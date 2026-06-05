# Design

This is the *why* behind SafetyStrip. [`ARCHITECTURE.md`](ARCHITECTURE.md) is the
map (what lives where, the boundary, the invariants); this document records the
settled decisions and their rationale, how the threat model shaped them, what the
known limitations are, and what is deliberately out of scope until the project
grows. The decision log mirrors `docs/exec-plans/active/0001-initial-rearch.md`.

## Threat model

SafetyStrip processes the **clipboard**, which is one of the most sensitive data
streams on a machine. At any moment it may contain passwords, API tokens, private
keys, PII, or proprietary source code. It also routinely contains **rich text**
(HTML/RTF) and Markdown copied from web pages, chat apps, and editors — i.e.
**untrusted, attacker-influenced markup** that the tool must parse.

Three threats follow directly, and each maps to a mechanism:

| Threat | Consequence if mishandled | Design response (mechanism) |
|---|---|---|
| **Data exfiltration** — clipboard content leaving the process | Catastrophic: silent theft of secrets | No network anywhere (`check-no-network`); core has no OS/IO/net deps (`check-core-deps`); no logging sink in the core (`deny(print*/dbg!)`); macOS App Sandbox with no network entitlement |
| **Data persistence** — content outliving its use | Secrets recoverable from disk/logs/freed memory | In-memory only; no files, no logs of content; freed FFI buffers are zeroized; no telemetry |
| **Untrusted-input parsing** — adversarial HTML/Markdown | Memory-unsafety, panics (DoS), hangs (DoS), or active content surviving into the "clean" output | Core is `#![forbid(unsafe_code)]` (no memory-unsafety by construction); fuzz + property + corpus prove no panic/hang; `strip_html` neutralizes `<script>`/`<style>` |

The boundary that enforces this is the core/shell split: the core is the untrusted
parser and cannot be memory-unsafe or leak; the shell is the only thing that talks
to the OS. See [`SECURITY.md`](SECURITY.md) for the posture as a checklist.

## Decision log

These are settled. Each entry is a decision, the choice, and why.

### D1 — Two-crate split with a C ABI

`core` is pure and `#![forbid(unsafe_code)]`; `core-ffi` is the only crate with
`unsafe`. **Why:** the core is the untrusted-input path, so memory-unsafety there
must be *impossible*, not merely unlikely — `forbid(unsafe_code)` makes the
compiler reject any `unsafe` block. But crossing a C ABI inherently needs `unsafe`
(raw pointers). Splitting the crates lets the core keep its guarantee while the
unavoidable `unsafe` is quarantined in a shim small enough to audit in one sitting.
The FFI shim adds `catch_unwind` so a panic in the core becomes an error code
rather than unwinding across the boundary (undefined behavior), and zeroizes freed
buffers as a best-effort wipe of clipboard-derived bytes.

### D2 — FFI = cbindgen + a narrow C ABI

A C ABI is language-neutral: a future Windows or Linux shell (Swift, C++, C#, Rust)
consumes it unchanged. **Why these four symbols** — `ss_abi_version`,
`ss_capabilities_json`, `ss_transform`, `ss_buffer_free` — and nothing more: the
surface stays narrow and data-driven. Feature selection crosses as a serialized
config string, so **adding a transform is a data change, never an ABI change**. The
checked-in header (`core-ffi/include/safetystrip.h`) is the source of truth and
`cargo xtask check-abi` fails CI if the code drifts from it, making any real ABI
change a deliberate, reviewable event (bump `SS_ABI_VERSION`, regenerate, call it
out). See [the FFI guardrail](docs/guardrails/ffi-boundary-and-abi-stability.md).

### D3 — Config is versioned JSON: an ordered list of operations

A `Config` is `{ "version": 1, "operations": [ ... ] }`, where each operation is an
internally-tagged object keyed on `op` (e.g. `{"op":"strip_html"}`,
`{"op":"change_case","case":"title"}`). **Why ordered and explicit:** transform
order is semantically significant (`StripHtml` then `StripMarkdown` is not the same
as the reverse), so the core never reorders — the pipeline applies operations
exactly as given. Versioning lets a shell detect a capability mismatch
deterministically (`parse_config` rejects any version other than `CONFIG_VERSION`).
Adding a transform is a new enum variant plus a pipeline arm — zero ABI change.

### D4 — Stateless `repr(C)` error model, lossy input decoding

Errors are a flat `repr(C)` status enum (`Ok`, `ErrNullArg`, `ErrInvalidConfig`,
`ErrInternal`) with **no global error state**. **Why:** a stateless, thread-safe,
trivially consumable contract — the caller reads the return code, no `errno`-style
hidden state to race on. Input bytes are decoded with lossy UTF-8 (invalid bytes
become U+FFFD) rather than rejected, so **adversarial bytes can never make
`ss_transform` fail** — it always produces *some* defined output. A caught panic
maps to `ErrInternal`, which should never occur (the core is fuzzed) but is handled
so a stray panic is never UB.

### D5 — HTML stripper: hand-rolled pure-safe-Rust state machine

`strip_html` is a hand-written `char`-by-`char` state machine plus a curated entity
table — **not** an upstream HTML parser. **Why reimplement a small subset:** a full
HTML5 parser is a large, opaque dependency with broad capability and its own attack
surface; the brief's guidance is to reimplement a small subset rather than depend
on opaque upstream. Because it is safe Rust, it is **memory-safe by construction**;
the only residual risks for a hand-rolled parser are panics and hangs, and those
are proven absent by an adversarial corpus, property tests, and `cargo fuzz`. The
scanner iterates by `char` and char-aligned byte offsets only (never slicing on a
non-UTF-8 boundary) and is strictly linear-time with only bounded lookahead.

**`strip_html` is the security workhorse.** It is the path that neutralizes active
content: `<script>` and `<style>` have their entire contents dropped (close tag
matched case-insensitively; unterminated raw text drops to end of input), and all
tags are removed. The shell hands the core the clipboard's **HTML representation**
and runs `StripHtml` on it — that is where script bodies die.

### D6 — Markdown stripper: pulldown-cmark

`strip_markdown` wraps `pulldown-cmark` (default features off). **Why not
hand-rolled:** CommonMark is too irregular to reimplement safely; `pulldown-cmark`
is the boring, well-audited standard (it powers rustdoc and mdBook), is itself
panic-free on arbitrary `&str`, and pulls in no OS/IO/network capability. Our event
handler keeps text content and drops formatting, and it is still fuzzed and
property-tested for panic freedom.

**Framing that matters for the threat model:** `strip_markdown` removes *Markdown*
formatting and, as a convenience, feeds any **embedded** raw-HTML fragments through
`strip_html` best-effort. But `strip_markdown` is **not itself the
script-neutralizing boundary** — the canonical, deliberate sanitization order is
**`StripHtml` → `StripMarkdown`**: run `StripHtml` on the HTML representation first
to kill active content, then `StripMarkdown` to clean residual formatting. Relying
on `StripMarkdown` *alone* to neutralize a script body is not the supported posture
(see Known limitations).

### D7 — Buffer ownership: leaked `Box<[u8]>`, freed + zeroized

`ss_transform` returns the output as a `(ptr, len)` pair over a leaked `Box<[u8]>`;
`ss_buffer_free` reclaims it. **Why `Box<[u8]>` and not the raw `String`/`Vec`:** a
boxed slice carries exactly `ptr + len` (no separate capacity), which is the minimal
thing the C side must track and round-trip. `ss_buffer_free` **zeroizes** the buffer
before dropping it, a best-effort wipe so clipboard-derived bytes do not linger in
freed memory. The only `unsafe` is reclaiming the box; producing it is safe Rust.

### D8 — Continuous mode: owned poller, fully torn down when off

The continuous (auto-clean) mode polls the platform clipboard change counter
(macOS `NSPasteboard.changeCount`) on a default **500 ms** interval. **Why a poller
that is fully invalidated and niled when disabled:** the requirement is that *no
loop runs when the feature is off* — not a paused timer, but no timer object at all.
On-demand mode (the default) does no polling: a hotkey triggers a single
read → transform → in-place rewrite.

### D9 — Global hotkey: Carbon `RegisterEventHotKey`

The macOS hotkey (default **⌥⌘V**) uses Carbon's `RegisterEventHotKey`. **Why not a
`CGEventTap` or a global `NSEvent` monitor:** those require the Accessibility or
Input Monitoring TCC permissions — broad, scary grants for a clipboard utility.
`RegisterEventHotKey` registers one specific chord and needs **neither**, which
keeps the privilege footprint minimal and avoids a permission prompt that would
undermine user trust.

### D10 — CLI has no dependencies

The `safetystrip` CLI parses its own arguments by hand. **Why:** it is a boring
validation/fuzz harness; an arg-parsing dependency would add surface for no benefit.
All config parsing lives in the core, so the CLI is a thin stdin→core→stdout pipe.

### D11 — Enforcement via a single portable `xtask`

All structural invariants are checked by the in-repo `xtask` crate, not external
cargo plugins. **Why:** the same `cargo xtask ci` runs identically locally and in
CI, so there is no CI-only logic to drift from and no extra tooling to install. Each
check prints a remediation-oriented message that teaches how to *fix* the violation,
not how to silence it.

### D12 — Operation taxonomy: rewrites vs reductions, toggles vs commands

Operations divide by **what they do to the buffer**, and the shell surface follows
from that — it is not a free UI choice.

- **Rewrites** preserve the text and edit it in place (`StripHtml`,
  `CollapseWhitespace`, `DedupeLines`, and the new `Defang` / `Refang` /
  `CleanUrls`). They compose additively — each is an independent stage refining the
  same buffer — and the idempotent ones are safe to run on every clipboard change.
- **Reductions** replace the buffer with a *derived subset* (`ExtractEmails`,
  `ExtractUrls`). They do **not** compose (extracting URLs from an email list yields
  nothing), they are terminal, and silently reducing every copy in continuous mode
  is never what the user wants.

This dictates two interaction models in the shell:

- **Persistent toggles** — rewrites that make sense always-on. Stored in the ordered
  `operations` pipeline, eligible for continuous mode. The menu's *Clean* section.
- **One-shot commands** — a transient single-op config run on demand, never
  persisted, never auto-run. *All reductions are commands;* so is `Refang`
  (re-activating received IOCs is a deliberate act, not a standing policy). The
  menu's *Extract* section, parallel to "Strip clipboard now".

The **core does not know about this split** — every op stays a plain pipeline entry,
so the CLI and power users can still compose extraction inside a pipeline. The
taxonomy is a *shell presentation* contract, with one hard rule: continuous mode
must refuse to run a reduction.

**Parameters follow Route A — a Settings window, not an expanded menu.**
`MenuBarExtra(.menu)` is a native AppKit menu and cannot host a text field, so the
free-text-parameterized ops (`PrefixLines`, `SuffixLines`, `JoinWith`, `SplitOn`)
and pipeline *ordering* live in a conventional SwiftUI `Settings` scene. Bounded,
enumerable params (`ChangeCase`'s case, `SortLines`'s two flags, `Defang`'s bracket
style) stay in the menu as **submenus with radio/checkmark items**. **Why not** make
the whole menu a `MenuBarExtra(.window)` panel: that buys inline text fields at the
cost of the crisp, keyboard-driven native-menu behavior on the common path; a
Settings window keeps the fast path fast and is where macOS users already expect
typed configuration to live.

### Other settled choices

- **macOS posture:** App Sandbox + Hardened Runtime, **minimal entitlements**. The
  only entitlement is `com.apple.security.app-sandbox` = true — reading and writing
  the pasteboard needs no entitlement. **In-place rewrite only:** SafetyStrip never
  simulates a paste (`Cmd-V`), which would need Accessibility and could fire into
  the wrong app; it only replaces the clipboard's own contents. See
  [the macOS posture guardrail](docs/guardrails/macos-posture.md).
- **Dependencies:** only boring, audited, API-stable crates with no OS/IO/network
  capability — `serde`/`serde_json` (config), `pulldown-cmark` (Markdown), `zeroize`
  (buffer wipe); `cbindgen` and `proptest` are tooling/dev-only; `libfuzzer-sys` and
  `arbitrary` are fuzz-only. The core's full transitive tree is frozen to an
  allowlist. See [the dependency guardrail](docs/guardrails/dependency-posture.md).

## Transform semantics (where the exact rules live)

The precise, frozen rules for each operation are documented as doc comments on the
implementing functions — that is the source of truth, kept next to the code and the
tests:

- **HTML** (tags, raw-text elements, block/inline whitespace, entities):
  `core/src/ops/html.rs`.
- **Markdown** (parser options, inline content, block structure, tables, embedded
  HTML): `core/src/ops/markdown.rs`.
- **Whitespace** (`collapse_whitespace`, `trim_trailing_whitespace`):
  `core/src/ops/whitespace.rs`.
- **Lines** (the shared line model, `unwrap_lines`, sort/dedupe/prefix/suffix/
  join/split, the email/URL extraction heuristics): `core/src/ops/lines.rs`.
- **Case** (title = capitalize the first char *by position* of each whitespace-
  delimited word; sentence = lowercase then capitalize after `.`/`!`/`?` + space):
  `core/src/ops/case.rs`.
- **IOC defang/refang** (`Defang`, `Refang` — neutralize/re-activate URLs,
  hostnames, IPv4/IPv6, and emails): `core/src/ops/defang.rs`.
- **URL cleaning** (`CleanUrls` — strip tracking/analytics query parameters):
  `core/src/ops/urls.rs`.

A few decisions worth surfacing here because they are easy to misread as bugs:

- **Title case is positional, not lexical.** `(HELLO)` → `(hello)` and `3RD` → `3rd`:
  the first *character* of a word is uppercased; if it is punctuation or a digit it
  has no uppercase mapping, so the rest of the word is just lowercased. We do not
  hunt for "the first letter."
- **`unwrap_lines` returns a clean paragraph block** with no trailing newline; it is
  intentionally not a line-list-preserving op.
- **`collapse_whitespace` only touches ASCII space and tab**, never `\n` and never
  other Unicode whitespace — "whitespace" here means what a wrapped clipboard paste
  actually produces.
- **CRLF handling:** the line model strips a trailing run of `\r` before a `\n`;
  `trim_trailing_whitespace` therefore normalizes CRLF→LF as a documented side
  effect.

### IOC defang/refang and URL cleaning (the agreed contract)

These three rewrites share the existing whitespace-tokenizer + URL/email heuristics
(see `ops/lines.rs`); they are deliberately not RFC parsers. The exact, frozen rule
for each lives in its implementing function's doc comment once built — this is the
design-level contract they must satisfy.

- **`Defang`** rewrites recognized network indicators so they are inert (not
  auto-linkified, not click-to-execute) while staying human-readable and
  reversible. Targets: URLs (`http`→`hxxp`, `https`→`hxxps`), hostnames/domains,
  IPv4, IPv6, and emails. Canonical substitutions follow the de-facto infosec
  convention — `.`→`[.]`, `://`→`[://]`, `@`→`[@]` — with the bracket style a
  bounded param (default `[.]`). It is **idempotent by construction**: it matches
  only *un*-defanged artifacts, so a second pass is a no-op (there are no bare dots
  left to bracket), which is what makes it safe under continuous mode.
- **`Refang`** is the documented textual inverse of that substitution set, applied
  globally — the analyst's "re-activate this received IOC" action. Because it is a
  global reverse-substitution, `refang(defang(x)) == x` **only when `x` contained no
  pre-existing defang tokens**; that caveat is documented, not a defect. Per D12 it
  is surfaced as a one-shot command, never a standing toggle.
- **`CleanUrls`** strips known tracking/analytics query parameters (`utm_*`,
  `fbclid`, `gclid`, `msclkid`, …) from URL tokens and reconstructs them, preserving
  every non-tracking parameter, their order, and any fragment. The denylist is a
  curated, **baked-in** constant — the core takes no network, so it is a
  point-in-time snapshot, not a live list. Idempotent (a cleaned URL has nothing
  left to strip) and order-significant only in that it should run after stripping.

All three are hand-rolled scanners over adversarial input, so they join the
panic-free regime: proptest (panic-freedom + idempotence + the round-trip property),
an adversarial corpus, a `cargo fuzz` target each, and the `perf_guard.rs`
linear-time budget.

## Performance & large inputs (log-file work)

The core is built to handle inputs far larger than a clipboard — e.g. log files in
the hundreds of MB. Every transform is **linear time** (an O(n²) in `strip_html`'s
newline collapsing was found by perf testing and fixed by bounding the backward
scan; `core/tests/perf_guard.rs` guards against regressions, and the fuzzer's 4 KB
input cap is why scaling bugs must be caught here, not there).

Performance is a methodical, measured track — criterion benches (`make bench`), an
opt-in roofline-calibrated throughput harness (`make perf`), and the `perf_guard.rs`
complexity gate. The method (ceiling model, optimization waves, accept /
diminishing-returns rules) and a current local baseline are in
[`docs/performance.md`](docs/performance.md) and
[exec-plan 0002](docs/exec-plans/active/0002-performance-ceiling-and-optimization-loop.md).

Measured on a 256 MB / 2.05 M-line synthetic log (release build, single op or the
noted pipeline); these single-op figures **predate intermediate zeroization** — see
[`docs/performance.md`](docs/performance.md) for current end-to-end throughput and the
measured cost of the wipe:

| Operation | Time | Peak RSS |
|---|---:|---:|
| `remove_blank_lines` | ~0.24 s | ~0.8 GB |
| `extract_urls` | ~0.38 s | ~0.5 GB |
| `sort_lines` (case-sensitive) | ~0.61 s | ~0.8 GB |
| `strip_html` | ~0.61 s | ~1.0 GB |
| `dedupe_lines` | ~0.90 s | ~0.9 GB |
| `collapse → trim → dedupe` | ~1.4 s | ~1.2 GB |
| `sort_lines` (case-insensitive) | ~2.0 s | ~1.3 GB |

**Memory model.** The pipeline is a fold — `text = op(text)` — so each operation
allocates a fresh output `String` and the previous one is wiped (`Zeroizing`) and
freed, giving a peak of ~2× the current text size per step; with the input buffer
also live, observed peak working set is ~3–5× the input. Two deliberate choices keep that bounded:

- `dedupe_lines` borrows line slices into a `HashSet<&str>` (membership only), so its
  extra memory is O(number of lines), not O(bytes).
- `sort_lines` sorts **borrowed slices in place** for the case-sensitive path (no
  per-line key allocation). Only the case-insensitive path materializes folded keys
  (~input size extra) — that is the unavoidable cost of Unicode case folding.

**Non-streaming, by contract.** The frozen FFI is `transform(input, config) →
output` (whole buffers), so the core is not a streaming/line-at-a-time API. For the
target sizes this is simpler and fast enough; true streaming (bounded memory
regardless of input size) is noted under *Adopt if the project grows*.

**Input size ceiling — memory-bound, like the OS clipboard.** macOS imposes no fixed
cap on *text* clipboard data (and Finder copies file *references*, not bytes), so
"match the platform" means scaling with available memory, not a small magic number.
Because a transform's peak working set is ~3–5× its input, the safe ceiling is a
fraction of RAM, and safety-first means **refusing gracefully rather than risking an
out-of-memory abort**. Three layers:

- **Core (pure):** unbounded — a plain memory-bound function.
- **C ABI (v2):** a fixed, generous backstop `SS_MAX_INPUT_BYTES` (2 GiB). A larger
  input returns `ErrInputTooLarge` *before* any read or allocation, so it can never
  abort or overflow at the boundary. It must be a constant because the
  platform-neutral core may not ask the OS about memory.
- **macOS shell:** the real, RAM-proportional policy —
  `min(SS_MAX_INPUT_BYTES, physicalMemory / 10)`, which keeps a worst-case strip under
  ~half of RAM and scales with the machine. An oversized clipboard yields a
  content-free "too large" status and is left untouched.
- **CLI:** intentionally uncapped — the right tool for multi-GB *file* work, where the
  caller manages its own memory.

This ceiling is what drove the v1 → v2 ABI bump (see the FFI guardrail).

**Shell responsiveness.** Because a strip can take a few seconds near the top of the
allowed range, the macOS shell runs the transform on a background task (the pasteboard
read and in-place write stay on the main actor) and shows a threshold-gated,
*indeterminate* "Stripping…" indicator only when a run exceeds ~400 ms — nothing for
the instant common case. A *determinate* progress bar was deliberately rejected: the
FFI is a single opaque call, so an honest percentage would need a progress-callback ABI
or the deferred streaming API, both heavier than the payoff for a delay that is
imperceptible on normal clipboards. See the macOS-posture guardrail.

Benchmarks for these sizes live in `core/benches/transform_large.rs`
(`make bench-large`); a pass/fail 256 MB scaling check is the `--ignored`
`handles_256mb_log_pipeline` test.

## Known limitations

These are accepted trade-offs, documented so they are not mistaken for defects.

- **Zeroization is best-effort.** The pipeline now holds each intermediate in a
  `Zeroizing` buffer (wiped on drop) and `ss_buffer_free` wipes the output buffer, so
  clipboard-derived bytes are scrubbed from the heap after use — at a measured
  throughput cost on very large inputs (see [`docs/performance.md`](docs/performance.md)).
  It remains best-effort: the caller's input buffer and the OS clipboard itself are
  outside the core's control, and the allocator may briefly retain freed pages before
  reuse.
- **`StripMarkdown` alone is not a script-neutralizing sanitizer.** It delegates
  *embedded* HTML to `strip_html` best-effort, but the supported path for hostile
  content is `StripHtml` → `StripMarkdown`. Do not rely on `StripMarkdown` by itself
  to scrub a `<script>` body.
- **The HTML entity table is a curated subset**, not the full WHATWG named-character
  reference. Unknown but well-formed `&name;` references are emitted **verbatim**
  (never dropped, never panicked on). Numeric references are fully supported, with
  out-of-range/surrogate values mapped to U+FFFD.
- **Email/URL extraction is heuristic, not a parser.** `extract_emails` /
  `extract_urls` tokenize on whitespace and apply a documented heuristic (see
  `ops/lines.rs`); they are deliberately not RFC 5322 / RFC 3986 compliant and may
  accept or reject edge cases a full parser would not. They are **reductions** (D12):
  the shell surfaces them as one-shot commands and continuous mode never runs them,
  but they remain valid pipeline ops for the CLI and power users.
- **Defang/refang and URL cleaning are heuristic too.** They reuse the same
  whitespace-tokenizer and URL/email heuristics, not RFC parsers. `Defang` is
  idempotent by construction; `Refang` is a global reverse-substitution, so
  `refang(defang(x)) == x` only when `x` held no pre-existing defang tokens; and
  `CleanUrls` matches a curated, baked-in tracking-parameter denylist that is a
  point-in-time snapshot (no network, so it cannot self-update). See the IOC
  contract under *Transform semantics*.
- **Rich→plain extraction is the shell's best-effort.** The core transforms whatever
  text the shell extracts; choosing the best clipboard representation (preferring
  HTML) is a shell responsibility and is itself heuristic per platform.
- **No full Xcode build in this environment.** The development environment is
  Command-Line-Tools-only, so `swift build` compiles the macOS shell sources but a
  signed, notarized `.app` is documented rather than produced. The C ABI, the FFI
  staticlib, and the shell sources are real; final packaging is a documented step.
- **Continuous mode is polling, not event-driven.** macOS exposes no clipboard-change
  notification, so the poller checks `changeCount` on an interval (default 500 ms);
  there is an inherent up-to-interval latency in continuous mode (on-demand mode is
  immediate).

## Adopt if the project grows

Explicitly **out of scope now** per the kickoff brief — listed so a future
maintainer knows they were considered and deferred, not forgotten:

- **Recurring documentation-GC agents** — automated passes that prune/refresh docs.
  The doc set is small and hand-maintained today.
- **An observability stack** — metrics/tracing/dashboards. A clipboard tool that
  must never exfiltrate data is the wrong place for telemetry; revisit only with a
  privacy-preserving, local-only design.
- **Quality-grade cadences** — scheduled audits, dependency-review rotations,
  fuzzing campaigns beyond the CI smoke. Today the CI gate + on-demand fuzzing
  suffice.
- **Per-worktree app booting / preview harnesses** — spinning up the full app per
  branch. Not warranted at this size.
- **Additional platform shells** (Windows/Linux), paste simulation, WASM/iOS, and
  signing/notarization automation — reserved, not built.
- **A streaming / bounded-memory transform path.** Today the core processes whole
  buffers (peak working set a small multiple of input — fine up to the ~256 MB log
  sizes we benchmark). For multi-GB inputs, a line-at-a-time streaming API (and a
  matching FFI variant) would cap memory regardless of input size. It would be an
  *additive* boundary (a new ABI entry point), not a change to the existing one, so
  it does not threaten the frozen contract — deferred until a real need appears.
