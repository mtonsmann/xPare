# Architecture

SafetyStrip is a memory-safe, plain-text clipboard utility. It strips formatting
and noise out of clipboard text — coerce rich text to plain, strip HTML/Markdown,
convert copied HTML to Markdown, normalize whitespace, change case, line
operations, IOC cleanup, and optional email/IP masking — without the clipboard
content ever leaving the process.

It is built as **two components separated by a frozen, language-neutral C ABI**:

- a portable Rust **transformation core** that is pure and `#![forbid(unsafe_code)]`, and
- native **shells** (macOS today; Windows/Linux reserved) that own all OS integration.

This file is the repository map: who owns what, where the trust boundary sits,
how data flows, and which invariants are enforced mechanically. For the *why*,
see [`DESIGN.md`](DESIGN.md); for the privacy posture see [`SECURITY.md`](SECURITY.md);
for change-class rules see [`docs/guardrails/`](docs/guardrails/).

## Crate & module map

```
core/        pure, deterministic transform core — #![forbid(unsafe_code)], no OS/IO/net
core-ffi/    thin C ABI shim over core — the ONLY crate that uses `unsafe`
cli/         headless harness over core (the project's own validation/fuzz driver)
xtask/       mechanical invariant enforcement (cargo xtask <check>)
fuzz/        cargo-fuzz targets (separate workspace; nightly + libFuzzer)
shells/macos Swift menu-bar shell over the C ABI (OS integration lives here)
shells/windows, shells/linux   reserved (empty) — a new platform implements the shell contract
docs/        ARCHITECTURE/DESIGN/SECURITY, guardrails, exec plans
```

### `core/` — `safetystrip-core`

The transformation engine. `String` in, `String` out, selected by a [`Config`].
Fed arbitrary, possibly adversarial text, so it carries the load-bearing safety
invariants.

| File | Responsibility |
|---|---|
| `core/src/lib.rs` | Crate root. Declares `#![forbid(unsafe_code)]` and the `print*` denies (the `dbg!` deny is workspace-wide), and inherits `[workspace.lints]`. Re-exports the public API and holds `CAPABILITIES_JSON` (the static self-description). |
| `core/src/config.rs` | The `Config` / `Operation` / `CaseKind` schema and `parse_config`. This is the data that crosses the FFI. `CONFIG_VERSION = 2`. The saturating growth-envelope arithmetic is factored into `saturating_growth_product` and carries `#[cfg(kani)]` bounded proofs (`cargo xtask check-kani`). |
| `core/src/pipeline.rs` | `transform(input, config)` — folds the ordered operations over the text. Infallible and deterministic; holds intermediates in `Zeroizing` storage and wipes fused scratch storage before release or reallocation. |
| `core/src/ops/mod.rs` | Operations module root; each op is a pure free function. |
| `core/src/ops/html.rs` | Hand-rolled, pure-safe-Rust HTML→text state machine + curated entity decoder. The rich→plain / script-neutralizing workhorse. |
| `core/src/ops/markdown.rs` | Markdown→text via `pulldown-cmark`; delegates embedded HTML to `html::strip_html`. |
| `core/src/ops/html_to_markdown.rs` | Dependency-free, pure-safe-Rust HTML→Markdown converter for common copied-web fragments. Drops active content and unsafe links; used as a one-shot command. |
| `core/src/ops/whitespace.rs` | `collapse_whitespace`, `trim_trailing_whitespace`. |
| `core/src/ops/lines.rs` | The line model plus the line ops and best-effort `extract_emails`/`extract_urls`. |
| `core/src/ops/case.rs` | `change_case`: upper / lower / title / sentence (full Unicode). |
| `core/src/ops/indicators.rs` | Shared token and email/URL/IP heuristics used by extraction, IOC cleanup, and masking. |
| `core/src/ops/mask.rs` | Token-level privacy masking for selected email, IPv4, and IPv6 identifiers. |

Public API: `transform`, `parse_config`, `capabilities`/`CAPABILITIES_JSON`,
and the `Config`, `Operation`, `CaseKind`, `ConfigError`, `CONFIG_VERSION` types.

### `core-ffi/` — `safetystrip-ffi`

The only crate permitted to use `unsafe`. A deliberately tiny C ABI over `core`
so it can be audited in one sitting. Built as `staticlib` + `cdylib` + `rlib`
(lib name `safetystrip_ffi`). Every entry point validates pointers, lossy-decodes
input UTF-8, and wraps the call to the core in `catch_unwind`, so a panic becomes
an error code instead of undefined behavior across the boundary. Returned buffers
are zeroized on free. Because this is the only `unsafe` in the tree, its boundary
tests run under Miri's undefined-behavior detector (`cargo xtask check-miri`;
nightly, best-effort). See [the FFI guardrail](docs/guardrails/ffi-boundary-and-abi-stability.md).

### `cli/` — `safetystrip-cli` (binary `safetystrip`)

A headless harness over `core` with **no** clipboard or OS integration. Reads
stdin, lossy-decodes it (mirroring the FFI), applies a JSON config, writes the
result to stdout; diagnostics go to stderr only. It is the project's own manual
testing / fuzz driver, intentionally dependency-light (hand-rolled arg parsing).
Subcommands: `capabilities`, `transform`.

### `xtask/` — `xtask`

The portable enforcer of the invariants (no external cargo plugins), so the same
checks run locally and in CI. Subcommands: `gen-header`, `check-abi`,
`check-unsafe-forbid`, `check-core-deps`, `check-no-network`, `check-entitlements`,
`check-no-content-logging`, `check-clipboard-safety`, `check-agent-workflow`,
`check-unused-deps` (cargo-machete: no declared-but-unused dependency),
`check-test-hygiene` (every ignored test has a reason; the count is ratcheted),
`check-docs` (docs build with `-D warnings`: no broken intra-doc links or invalid doc
HTML), `check-miri` (run the `core-ffi` boundary tests under Miri), `check-kani` (run
the bounded resource-envelope proofs), `check-coverage` (line-coverage floor ratchet via
cargo-llvm-cov, excluding the `xtask` harness) and `check-mutants` (mutation testing via
cargo-mutants) — those last four nightly/heavy, best-effort, and outside the required gate
— and `ci` (fmt + clippy + test + every structural check).
See [the dependency guardrail](docs/guardrails/dependency-posture.md) and [the code &
test hygiene guardrail](docs/guardrails/code-and-test-hygiene.md).

### `fuzz/` — `safetystrip-fuzz`

Its **own** workspace (so libFuzzer and the nightly toolchain never leak into the
stable build). cargo-fuzz targets prove the never-panics invariant on the
hand-rolled parsers: `strip_html`, `strip_markdown`, `transform_pipeline`.

### `shells/` — native OS integration

Shells own everything the core refuses to touch: clipboard read/write
(including rich→plain extraction), change detection, tray/menu-bar UI, the global
hotkey, settings, and calling the core over the C ABI. **No transform logic lives
in a shell.**

- `shells/macos/` — the Swift menu-bar shell. Links the FFI staticlib through the
  C ABI via the `CSafetyStrip` module map (`Sources/CSafetyStrip/include/`), which
  re-includes the single source-of-truth header at `core-ffi/include/safetystrip.h`
  rather than copying it. See [the macOS posture](docs/guardrails/macos-posture.md).
- `shells/windows/`, `shells/linux/` — reserved, empty. Adding a platform means
  implementing [the shell contract](docs/guardrails/shell-contract.md) and linking
  the same core — no ABI or core change required.

## The trust boundary

```
            UNTRUSTED INPUT (clipboard: passwords, tokens, PII, source, HTML, Markdown)
                                    |
   ┌───────────────────────────────┼─────────────────────────────────────────────┐
   │  SHELL (trusted with OS)       │                                              │
   │  • reads the pasteboard        │                                              │
   │  • extracts the best text rep  │   rich → plain                               │
   │  • owns hotkey / tray / poller │                                              │
   └───────────────────────────────┼─────────────────────────────────────────────┘
                                    │  ss_transform(input_bytes, config_json)   ← C ABI
   ┌───────────────────────────────┼─────────────────────────────────────────────┐
   │  CORE (no OS, no IO, no net)   │   #![forbid(unsafe_code)]                    │
   │  • lossy-UTF-8 decode          │   parses untrusted text                      │
   │  • run the ordered pipeline    │   pure, deterministic, never panics          │
   └───────────────────────────────┼─────────────────────────────────────────────┘
                                    │  (ptr, len)  →  ss_buffer_free zeroizes
   ┌───────────────────────────────┼─────────────────────────────────────────────┐
   │  SHELL writes the result back to the pasteboard IN PLACE (no paste simulation)│
   └─────────────────────────────────────────────────────────────────────────────┘
```

Two things make this boundary meaningful:

1. **The core is the untrusted-input parser, and it cannot be memory-unsafe.**
   `#![forbid(unsafe_code)]` makes that true by construction; the only residual
   risk for the hand-rolled parsers (panics, hangs) is pinned down by the fuzz +
   property + corpus suites.
2. **The core cannot leak data.** It has no OS, filesystem, network, logging, or
   global mutable state — enforced by the dependency allowlist and the
   no-network banlist, not just promised.

The `unsafe` needed to cross the C ABI is quarantined entirely in `core-ffi`,
which is small enough to read end to end.

## Data flow

1. The shell observes a clipboard change (on-demand via hotkey, or via the
   continuous poller on the platform change counter).
2. The shell reads the clipboard and **extracts the best plain representation**:
   it prefers the HTML representation and feeds it to the core's `StripHtml`,
   because that is the path that neutralizes `<script>`/`<style>` and tags.
   The one-shot `HtmlToMarkdown` command is the deliberate exception: it consumes
   the raw HTML representation directly so structure can be preserved as Markdown.
3. The shell calls `ss_transform(input, config_json)`. `config_json` is a
   versioned, ordered list of operations — feature selection is **data**, not API.
4. The core lossy-decodes the bytes, runs the pipeline left-to-right, and returns
   an owned `(ptr, len)` buffer.
5. The shell writes the transformed text back to the clipboard **in place**, then
   frees the buffer with `ss_buffer_free`, which zeroizes it first.

The canonical sanitization config is **`StripHtml` → `StripMarkdown`** (HTML first
to neutralize active content, then Markdown to remove residual formatting),
optionally followed by URL/IOC/masking, whitespace/case, and line ops. See
[the transform guardrail](docs/guardrails/transform-correctness-and-adversarial-input.md).

## Enforced invariants

These are not prose promises — each is a check that fails `cargo xtask ci` (and
therefore CI). Fix the code to satisfy the check; never weaken the check.

| Invariant | Mechanism | Where |
|---|---|---|
| No `unsafe` in the core | `#![forbid(unsafe_code)]` + `check-unsafe-forbid` | `core/src/lib.rs`, `xtask` |
| Core has no OS/IO/network deps | `check-core-deps` (strict transitive allowlist) | `xtask` `CORE_DEP_ALLOWLIST` |
| No network anywhere in the workspace | `check-no-network` (banlist over the whole tree) | `xtask` `NETWORK_BANLIST` |
| Frozen C ABI | checked-in `core-ffi/include/safetystrip.h` + `check-abi` (drift fails) | `xtask` (cbindgen) |
| Config is data (adding a transform ≠ ABI change) | serde round-trip + version tests | `core` tests |
| Never panics on input | cargo-fuzz targets + property tests + adversarial corpus | `fuzz/`, `core` tests |
| No log sink in the core | `#![deny(clippy::print_stdout, print_stderr)]` in core/core-ffi + workspace-wide `dbg_macro` deny + no logging deps | `core/src/lib.rs`, `[workspace.lints]` |
| No clipboard content logged or persisted | `check-no-content-logging` (scans shipped Rust + Swift source for sink calls on clipboard-derived content) | `xtask` |
| Default checks avoid the real clipboard | `check-clipboard-safety` (no default Make target depends on a real-clipboard smoke) | `xtask`, `Makefile` |
| Pipeline intermediates and fused scratch storage wiped before release | `Zeroizing` buffers in the pipeline + `check-pipeline-zeroization` + `ss_buffer_free` zeroizes output | `core/src/pipeline.rs`, `xtask`, `core-ffi` |
| Deterministic output | `transform(x,c) == transform(x,c)` property test | `core` tests |
| Optimized pipeline == reference semantics | differential property test: production `transform` equals a one-op-at-a-time reference interpreter (so every fused fast path stays byte-for-byte equal to sequential application, and canonical ordering equals an explicitly sorted `as_given` run) | `core/tests/reference_transform.rs` |
| AI-native workflow docs present & structured | `check-agent-workflow` (the workflow doc, brief/PR templates, and per-class task prompts exist with required headings) | `xtask`, `docs/agent-workflow.md` |
| Minimal macOS entitlements | checked-in entitlements file + `check-entitlements`; `release.sh dist` requires it for Developer ID signing and verifies the signed payload is still minimal | `xtask`, `shells/macos/` |
| No dead/dangling code | `unreachable_pub = "deny"` forces unexported `pub` to `pub(crate)`, after which `dead_code` (via `-D warnings`) flags the truly unused | `[workspace.lints]`, all crates |
| No tangled functions or scaffolding macros | `cognitive_complexity` + `too_many_arguments` thresholds; `clippy::todo`/`unimplemented`/`dbg_macro` denied | `[workspace.lints]`, `clippy.toml` |
| No declared-but-unused dependency | `check-unused-deps` (cargo-machete over the whole workspace) | `xtask`, `CARGO_MACHETE_VERSION` |
| Every ignored test justified, count ratcheted | `check-test-hygiene` (bare `#[ignore]` fails; total `#[ignore]`s ≤ ceiling) | `xtask` `MAX_IGNORED_TESTS` |
| No broken doc links or invalid doc HTML | `check-docs` (`cargo doc --no-deps` with `RUSTDOCFLAGS=-D warnings`) | `xtask` |
| Line coverage stays above a ratcheted floor | `check-coverage` (cargo-llvm-cov; `COVERAGE_FLOOR_PCT`, excludes the `xtask` harness) | `xtask`; best-effort, **outside** the `ci` gate (`hygiene.yml`) |
| No dead code or under-asserting "slop" tests | `check-mutants` (cargo-mutants; a surviving mutant means a test to strengthen) | `xtask`, `.cargo/mutants.toml`; best-effort, **outside** the `ci` gate (`hygiene.yml`) |

The single gate that runs all of the above is `cargo xtask ci`; CI runs the exact
same command (`.github/workflows/ci.yml`). See [`CONTRIBUTING.md`](CONTRIBUTING.md).
When a review discovers a new issue class, close it through
[`docs/guardrails/review-finding-closure.md`](docs/guardrails/review-finding-closure.md):
add the blocker to the owning test/check layer, and update this table if the
finding creates or changes an enforced invariant.
