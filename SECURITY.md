# Security & privacy posture

SafetyStrip handles the **clipboard** — passwords, API tokens, private keys, PII,
and proprietary source pass through it routinely, alongside untrusted HTML/Markdown
markup. The entire design exists to make one promise credible: **your clipboard
content never leaves the process, never gets persisted, and cannot corrupt the tool
that touches it.**

This document states the posture and — crucially — how each part is **enforced**,
not merely intended. For the rationale and threat model see [`DESIGN.md`](DESIGN.md);
for the boundary and module map see [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Posture at a glance

| Property | Statement | Enforced by |
|---|---|---|
| **No network** | No code path can open a socket — not in the core, not in any crate that could be linked or run at build time | `cargo xtask check-no-network` (banlist over the whole workspace tree); macOS App Sandbox grants **no** network entitlement |
| **No persistence of content** | Clipboard text is never written to a file, database, or any durable store | No filesystem dependency in the core (`check-core-deps`); the only disk I/O anywhere is the CLI reading a *config* file (never content); `check-no-content-logging` scans for content persistence, and `check-clipboard-safety` keeps real-clipboard exercise out of default targets |
| **No logging of content** | Clipboard text can never reach a log/console sink | Core denies `print!`/`println!`/`eprint*`/`dbg!` at compile time (`#![deny(...)]`); no logging crate is a dependency; the CLI sends *diagnostics* to stderr and *only transformed text* to stdout; `cargo xtask check-no-content-logging` scans the shipped Rust + Swift source for logging calls on clipboard-derived content |
| **In-memory only + wipe** | Content lives in memory only for the transform; pipeline intermediates, transform-local scratch storage, and the buffer that crosses the boundary are wiped before release | The core holds each pipeline intermediate in a `Zeroizing` buffer (wiped on drop); fused pipeline scratch storage is wiped before capacity growth can release old storage and on drop; `ss_buffer_free` zeroizes the returned buffer before freeing it (`zeroize` crate); `cargo xtask check-pipeline-zeroization` blocks the current fused-scratch regression class |
| **Memory safety** | The untrusted-input parser cannot be memory-unsafe | `#![forbid(unsafe_code)]` in the core + `cargo xtask check-unsafe-forbid`; all `unsafe` is isolated to the tiny `core-ffi` shim, which uses `catch_unwind` so a panic is never UB across FFI |
| **No telemetry / analytics** | The tool phones home to no one | Same mechanisms as "no network" + "no persistence" — there is no code that could |
| **Minimal OS privilege** | The macOS shell requests the least it can | App Sandbox + Hardened Runtime; entitlements file is *only* `app-sandbox = true`, verified by `cargo xtask check-entitlements`; official Developer ID releases sign with that file and verify the signed entitlement payload is still minimal; no Accessibility / Input Monitoring (hotkey uses Carbon `RegisterEventHotKey`); in-place clipboard rewrite only, never paste simulation |
| **Stable, auditable boundary** | The core/shell contract is small and frozen | Checked-in C header + `cargo xtask check-abi` (drift fails CI) |

Every check above is part of `cargo xtask ci`, which CI runs verbatim
(`.github/workflows/ci.yml`). A regression in any property fails the build.

## The trust boundary

The privacy guarantee rests on a single architectural line: **the core is the only
thing that parses untrusted content, and it has no way to talk to the outside
world.**

- The **core** (`safetystrip-core`) has no OS, filesystem, network, logging, or
  global mutable state. It cannot exfiltrate, persist, or log — there is no API in
  its dependency tree that could. This is enforced by a strict transitive
  dependency allowlist (`check-core-deps`), not by convention.
- The **shell** is the only component with OS access (clipboard, hotkey, UI). It is
  small, platform-specific, and sandboxed, and it owns the read → call-core →
  write-back-in-place flow.
- The **FFI shim** (`core-ffi`) is the only crate with `unsafe`. It is intentionally
  tiny so it can be audited in one sitting; it validates every pointer, lossy-decodes
  input so adversarial bytes can never make it fail, wraps the core call in
  `catch_unwind`, and zeroizes freed buffers.

See the boundary diagram in [`ARCHITECTURE.md`](ARCHITECTURE.md#the-trust-boundary).

## Threat model boundaries

SafetyStrip protects users from **SafetyStrip itself** becoming a leak, persistence
sink, over-privileged clipboard tool, or memory-unsafe parser. It does not claim to
protect the clipboard from every same-user local process. On macOS, other local
apps may be able to read or write the general pasteboard; a malicious or buggy local
pasteboard writer can replace content before SafetyStrip reads it, race a rewrite,
or feed oversized/rich malformed data. SafetyStrip treats that as a local
pasteboard race or denial-of-service condition, not as a confidentiality boundary it
can enforce. The enforced guarantees are that SafetyStrip will not exfiltrate,
persist, log, or memory-unsafely parse the content it is handed.

## Where zeroization matters

Zeroization is a best-effort **persistence** control. It reduces the chance that
clipboard-derived bytes remain recoverable from SafetyStrip-owned heap storage
after that storage is no longer needed. It is not an exfiltration control and does
not defend against an attacker who can read live process memory during a transform.

SafetyStrip enforces zeroization at ownership and last-use boundaries:

- Full pipeline intermediates are wiped when the next operation supersedes them or
  when the transform returns.
- Transform-local scratch storage is wiped before capacity growth can release old
  bytes to the allocator, and again on drop. Allocation-preserving reuse may clear
  logical length without a hot-path wipe because the storage is still owned by the
  same transform.
- The FFI output buffer is wiped when the shell calls `ss_buffer_free`, after the
  shell has written the transformed text back to the clipboard.
- The shell's input buffer and the OS clipboard are outside the core/FFI ownership
  boundary; the shell minimizes lifetime but cannot promise core-owned zeroization
  for memory it does not own.

## Handling of adversarial input

Clipboard markup is attacker-influenced, so the core treats all input as hostile:

- **Lossy UTF-8 decoding** — invalid bytes become U+FFFD instead of an error, so no
  input can make a transform fail.
- **Never panics, never hangs** — the hand-rolled HTML parser iterates by `char` on
  UTF-8 boundaries, runs in linear time with only bounded lookahead, and is proven
  panic-free by `cargo fuzz` targets, property tests, and a checked-in adversarial
  corpus. A panic, were one to occur, is caught at the FFI boundary and returned as
  `SS_STATUS_ERR_INTERNAL` rather than unwinding into the host (which would be UB).
- **Active content is neutralized by `StripHtml`** — `<script>`/`<style>` bodies are
  dropped entirely and all tags removed. The shell feeds the clipboard's HTML
  representation through `StripHtml`; the canonical sanitization order is
  `StripHtml` → `StripMarkdown`. See
  [the transform guardrail](docs/guardrails/transform-correctness-and-adversarial-input.md).
- **HTML-to-Markdown is explicit conversion, not sanitization policy** — the
  one-shot converter consumes raw HTML to preserve structure, but still drops
  `<script>`/`<style>` bodies and unsafe link schemes. Entity-decoded text is
  escaped where Markdown could reinterpret it as raw HTML, and copied code/pre
  content uses delimiters that cannot be closed by the copied backtick run. It is
  not injected into continuous mode or the canonical sanitization pipeline.
- **Size limits are enforced before the core transform** — the macOS shell refuses
  extracted text above its RAM-proportional limit, and the FFI rejects anything above
  `SS_MAX_INPUT_BYTES` before reading or allocating. The macOS shell also checks raw
  HTML/RTF representation bytes before decoding them when AppKit exposes those
  bytes. Rich-format extraction itself is still platform work, so the limit is a
  pre-core-transform guard, not a streaming rich-format parser for every native
  format.
- **Configs are envelope-bounded before transform** — `parse_config` rejects configs
  with too many operations (≤ 32), free-text parameters over 16 UTF-8 bytes, `\r`/`\n`
  inside prefix/suffix/join/split parameters, or a pipeline whose worst-case output
  growth exceeds `MAX_PIPELINE_GROWTH_FACTOR` (4096×). That growth bound is the
  product of each operation's conservative growth factor
  (`Operation::max_growth_factor`): it catches *composition* — e.g. a `SplitOn` that
  re-maximizes the line count so a following `PrefixLines`/`JoinWith` re-amplifies —
  which the per-operation caps alone do not, and which a fuzz run originally showed
  could expand a sub-KiB input past 2 GiB (a resource-exhaustion / DoS vector). The
  16-byte param ceiling caps any single affix's factor at 17, and the 4096× pipeline
  cap sits hundreds of times above a realistic config's product (~6) while bounding
  the worst accepted amplification far below any out-of-memory threshold; a Kani
  proof (`cargo xtask check-kani`) shows the saturating product can never wrap an
  amplifying pipeline into acceptance. This is a config compatibility tightening, not
  an ABI or privacy-posture change: invalid configs fail as `ErrInvalidConfig` at the
  FFI boundary instead of entering the infallible transform path.
- **Optional masking is an output rewrite, not a new data path** — `MaskIdentifiers`
  replaces selected email/IPv4/IPv6 tokens with fixed placeholders inside the same
  pure core pipeline. It adds no network, persistence, logging, entitlement, or
  telemetry capability and does not change the in-memory-only posture.

## Known limitations (security-relevant)

These are documented honestly in [`DESIGN.md`](DESIGN.md#known-limitations); the
security-relevant ones:

- **Zeroization is best-effort.** The core now holds each pipeline intermediate in
  `Zeroizing` storage, wipes transform-local scratch storage before capacity growth
  can release old clipboard-derived bytes, and the FFI wipes the output buffer on
  free. Clipboard-derived content is scrubbed from SafetyStrip-owned heap storage
  when that storage is no longer needed (at a measured throughput cost on very large
  inputs — see [`docs/performance.md`](docs/performance.md)). It remains
  *best-effort*: the caller's own input buffer (e.g. the shell's pasteboard read) and
  the OS clipboard itself are outside the core's control, and the allocator may retain
  freed pages briefly before reuse. The invalid-UTF-8 FFI path is covered by this:
  when lossy decoding needs an owned replacement string, that temporary copy is held
  in a `Zeroizing` buffer and wiped on drop; the caller's original byte buffer remains
  outside the FFI's ownership.
- **Continuous mode is best-effort under local races.** It polls the pasteboard
  `changeCount`; it does not lock the system pasteboard or prove that no other local
  writer changed it before SafetyStrip read or after it rewrote the pasteboard. The
  shell suppresses SafetyStrip self-write generations, coalesces continuous callbacks
  while a strip is running, and drops stale transform completions if `changeCount`
  moved in flight. SafetyStrip still performs content-free outcomes only and applies
  the same size limits to each attempted run.
- **Official release sandboxing is part of the release gate.** Unsigned/ad-hoc
  previews are for testing and are not official downloadable binaries. `make dist`
  and the release workflow require the checked-in App Sandbox entitlements for the
  Developer ID signature and verify the signed app's entitlement payload is still
  minimal.
- **`StripMarkdown` alone is not a sanitizer** for hostile `<script>` content — use
  `StripHtml` → `StripMarkdown`.
- **`HtmlToMarkdown` is not a browser-grade sanitizer or renderer** — it is a
  dependency-free clipboard converter for common copied-web fragments.
- **Privacy masking is not comprehensive anonymization or DLP.** It is a
  deterministic, heuristic, token-level rewrite for selected emails and IPs. It does
  not promise to find every form of PII or to protect against other same-user apps
  reading the system pasteboard before SafetyStrip rewrites it.

## Reporting a vulnerability

This repository is the system of record. If you find a security issue —
particularly anything that could cause clipboard content to leave the process, be
persisted/logged, or a way to make the core panic, hang, or read out of bounds —
open an issue (or, for sensitive reports, contact the maintainers privately) and
clearly mark it as a security report. A reproducing input for a core
panic/hang/OOB is the most valuable thing you can include; it becomes a regression
in the corpus.

A change that alters any property in the table above is a **posture change**: it
must be called out explicitly in the PR, justified, and reflected here and in the
relevant guardrail before it can land. The corresponding `xtask` check must be
updated to *match* the new posture — never weakened to hide a regression.

Security-finding fixes also follow
[`docs/guardrails/review-finding-closure.md`](docs/guardrails/review-finding-closure.md):
the issue class needs repeatable regression protection plus a short docs lesson
so future agents and humans know which invariant was violated and which check now
protects it.
