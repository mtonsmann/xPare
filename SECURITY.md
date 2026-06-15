# Security & privacy posture

xPare handles the **clipboard** — passwords, API tokens, private keys, PII,
and proprietary source pass through it routinely, alongside untrusted HTML/Markdown
markup. The entire design exists to make one promise credible: **your clipboard
content never leaves the process, never gets persisted, and cannot corrupt the tool
that touches it.** Persistence has exactly one **opt-in** exception — the
paste-as-file feature — bounded and enforced as described
[below](#opt-in-paste-as-file-exception).

This document states the posture and — crucially — how each part is **enforced**,
not merely intended. For the rationale and threat model see [`DESIGN.md`](DESIGN.md);
for the boundary and module map see [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Posture at a glance

| Property | Statement | Enforced by |
|---|---|---|
| **No network** | No code path can open a socket or browser/auth callback surface — not in the core, not in any crate that could be linked or run at build time, and not through direct Swift platform APIs | `cargo xtask check-no-network` (banlist over the whole workspace tree); `cargo xtask check-swift-no-network-apis` (Swift network/browser/auth API tokens); macOS App Sandbox grants **no** network entitlement |
| **No persistence of content** | Clipboard text is never written to a file, database, or any durable store — with **one explicit, opt-in exception**: the paste-as-file feature (see [below](#opt-in-paste-as-file-exception)) | No filesystem dependency in the core (`check-core-deps`); the only disk I/O anywhere is the CLI reading a *config* file (never content) and the sanctioned `PasteFileStore`; `check-no-content-logging` scans for content persistence and honors its allow-marker **only** inside `PasteFileStore.swift`, and `check-clipboard-safety` keeps real-clipboard exercise out of default targets |
| **No logging of content** | Clipboard text can never reach a log/console sink | Core denies `print!`/`println!`/`eprint*`/`dbg!` at compile time (`#![deny(...)]`); no logging crate is a dependency; the CLI sends *diagnostics* to stderr and *only transformed text* to stdout; `cargo xtask check-no-content-logging` scans the shipped Rust + Swift source for logging calls on clipboard-derived content |
| **No command execution surface** | Clipboard handling cannot spawn subprocesses or delegate content to shell commands | `cargo xtask check-shipped-command-exec` scans shipped Rust and Swift sources; command execution is confined to `xtask` and reviewed release/build scripts |
| **In-memory only + wipe** | Content lives in memory only for the transform; pipeline intermediates, transform-local scratch and accumulator storage, and the buffer that crosses the boundary are wiped before release (best-effort gaps inventoried [below](#where-zeroization-matters)) | The core holds each pipeline intermediate in a `Zeroizing` buffer (wiped on drop); fused pipeline scratch storage and growing op accumulators are wiped before capacity growth can release old storage and on drop; `xp_buffer_free` zeroizes the returned buffer before freeing it (`zeroize` crate); `cargo xtask check-pipeline-zeroization` is a regression tripwire for the fused-scratch pattern (it does not prove every allocation is wiped) |
| **Memory safety** | The first-party untrusted-input parsing code has no `unsafe`; the few third-party parsing deps are a frozen, audited, pure-data allowlist carrying their own vetted internal `unsafe` | `#![forbid(unsafe_code)]` in the core + `cargo xtask check-unsafe-forbid`; the dep allowlist (`check-core-deps`); all first-party `unsafe` is isolated to the tiny `core-ffi` shim, which uses `catch_unwind` so a panic is never UB across FFI |
| **No telemetry / analytics** | The tool phones home to no one | Same mechanisms as "no network" + "no persistence" — there is no code that could |
| **Minimal OS privilege** | The macOS shell requests the least it can | App Sandbox + Hardened Runtime; entitlements file is *only* `app-sandbox = true`, verified by `cargo xtask check-entitlements`; official Developer ID releases sign with that file and verify the signed entitlement payload is still minimal; no Accessibility / Input Monitoring (hotkey uses Carbon `RegisterEventHotKey`); in-place clipboard rewrite only, never paste simulation; `cargo xtask check-pasteboard-write-shape` keeps the plain-string rewrite narrow while the opt-in paste-as-file path remains the documented exception |
| **Stable, auditable boundary** | The core/shell contract is small and frozen | Checked-in C header + `cargo xtask check-abi` (drift fails CI) |

Every check above is part of `cargo xtask ci`, which CI runs verbatim
(`.github/workflows/ci.yml`). A regression in any property fails the build.

## Opt-in paste-as-file exception

**Paste as file** (menu bar → "Paste as file", with a custom threshold in
Settings; **off by default**) is the one deliberate exception to "no persistence of content". When
the user enables it and a transformed result exceeds their size threshold, the
shell writes the result to a file and puts a *file reference* on the pasteboard
instead of the raw string — so pasting attaches a file rather than dumping a
huge text blob. A pasteboard file reference cannot exist without a real file
behind it, so the persistence is inherent to the feature, not incidental.

The exception is kept as small as it can be:

- **Strictly opt-in.** With the toggle off (the default), the code path never
  runs and nothing is ever written.
- **One audited writer.** All file I/O lives in `PasteFileStore`
  (`shells/macos/Sources/XPareKit/PasteFileStore.swift`). It is the only
  file in which `check-no-content-logging` honors the
  `xpare:allow-content-persistence` marker; the marker anywhere else is
  itself a CI failure, so the exception cannot quietly spread.
- **Contained location, minimal privilege.** The file lives in a dedicated
  `PasteAsFile.noindex` directory inside the App Sandbox container's own
  temporary directory — no new entitlement. `.noindex` keeps Spotlight from
  indexing it and it is excluded from backups. Directory `0700`, file `0600`.
- **At most one file, shortest practical lifetime.** Each write replaces the
  previous file. The file is deleted as soon as the pasteboard stops
  referencing it (checked on every strip), on every launch, and on quit. The
  file name is a timestamp, never derived from the content.
- **Residual risk, stated plainly.** While the file exists it is ordinary
  user-readable-by-owner disk content, and deletion does not scrub the
  underlying disk blocks (no userland tool can promise that on APFS). Apps you
  paste the file into may copy it. If your clipboard holds secrets, leave this
  feature off or set the threshold high; everything else in this document is
  unchanged and continues to apply when the feature is off or below threshold.

## The trust boundary

The privacy guarantee rests on a single architectural line: **the core is the only
thing that parses untrusted content, and it has no way to talk to the outside
world.**

- The **core** (`xpare-core`) has no OS, filesystem, network, logging, or
  global mutable state. It cannot exfiltrate, persist, or log — there is no API in
  its dependency tree that could. This is enforced by a strict transitive
  dependency allowlist (`check-core-deps`), not by convention.
- The **shell** is the only component with OS access (clipboard, hotkey, UI). It is
  small, platform-specific, and sandboxed, and it owns the read → call-core →
  write-back-in-place flow. On macOS it also owns image OCR: bounded image bytes are
  handed to Apple's on-device Vision framework locally, then recognized text is
  written back as a plain string.
- The **FFI shim** (`core-ffi`) is the only crate with `unsafe`. It is intentionally
  tiny so it can be audited in one sitting; it validates every pointer, lossy-decodes
  input so adversarial bytes can never make it fail, wraps the core call in
  `catch_unwind`, and zeroizes freed buffers.

See the boundary diagram in [`ARCHITECTURE.md`](ARCHITECTURE.md#the-trust-boundary).

## Threat model boundaries

xPare protects users from **xPare itself** becoming a leak, persistence
sink, over-privileged clipboard tool, or memory-unsafe parser. It does not claim to
protect the clipboard from every same-user local process. On macOS, other local
apps may be able to read or write the general pasteboard; a malicious or buggy local
pasteboard writer can replace content before xPare reads it, race a rewrite,
or feed oversized/rich malformed data. xPare treats that as a local
pasteboard race or denial-of-service condition, not as a confidentiality boundary it
can enforce. The enforced guarantees are that xPare will not exfiltrate,
persist, log, or memory-unsafely parse the content it is handed.

## Where zeroization matters

Zeroization is a best-effort **persistence** control. It reduces the chance that
clipboard-derived bytes remain recoverable from xPare-owned heap storage
after that storage is no longer needed. It is not an exfiltration control and does
not defend against an attacker who can read live process memory during a transform.

xPare wipes at ownership and last-use boundaries. What **is** wiped:

- Full pipeline intermediates: every op output that feeds another pass is held in
  `Zeroizing` storage and wiped when its pass completes or the transform returns.
- Transform-local scratch storage is wiped before capacity growth can release old
  bytes to the allocator, and again on drop. Allocation-preserving reuse may clear
  logical length without a hot-path wipe because the storage is still owned by the
  same transform.
- Op output accumulators cannot leak via mid-construction reallocation: each is
  either pre-sized to a provably sufficient capacity (so it never reallocates —
  the bounds are pinned by property tests) or appended through a wipe-on-grow
  helper (`core/src/ops/wipe.rs`) that zeroizes the superseded allocation before
  growth frees it. Transient content copies in the HTML-to-Markdown converter
  (decoded entities and attributes, link destinations, pre/code buffers) are
  `Zeroizing` as well.
- The FFI output buffer is wiped when the shell calls `xp_buffer_free`, after the
  shell has written the transformed text back to the clipboard.

What remains **best-effort** — the precise residual gaps:

- Each op's *return value* is a plain `String` until the pipeline wraps it (for
  next-pass intermediates) or the FFI frees it (for the final output); that
  window is inherent to the return-by-value design.
- `sort_lines` with `case_insensitive = true` allocates one fully case-folded
  copy of every line as a plain comparison key, dropped unwiped.
- `strip_markdown`'s third-party parser (`pulldown-cmark`) keeps internal,
  unwiped allocations (owned `CowStr` buffers for entity-unescaped text and
  parser state) holding clipboard-derived bytes.
- Process-level limits apply to all of it: registers, stack temporaries,
  allocator metadata, and OS paging/swap are not covered.
- The shell's own copies of the clipboard text — the pasteboard snapshot, the
  UTF-8 byte array handed across the FFI, and the output `String` written back —
  are ordinary Swift allocations and are not scrubbed.

`cargo xtask check-pipeline-zeroization` is a **regression tripwire** for the
fused-scratch wipe-before-release pattern and for the wipe-on-grow routing of
the growable op accumulators (`html_to_markdown`, the Unicode case mappings,
`strip_markdown`) — it catches those specific classes of regression
mechanically; it is not a proof that every allocation is wiped.

## Handling of adversarial input

Clipboard markup is attacker-influenced, so the core treats all input as hostile:

- **Lossy UTF-8 decoding** — invalid bytes become U+FFFD instead of an error, so no
  input can make a transform fail.
- **Never panics, never hangs** — the hand-rolled HTML parser iterates by `char` on
  UTF-8 boundaries, runs in linear time with only bounded lookahead, and is pinned
  down as panic-free by `cargo fuzz` targets, property tests, and a checked-in
  adversarial corpus. A panic, were one to occur, is caught at the FFI boundary and returned as
  `XP_STATUS_ERR_INTERNAL` rather than unwinding into the host (which would be UB).
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
  `XP_MAX_INPUT_BYTES` before reading or allocating. The macOS shell also checks raw
  HTML/RTF representation bytes before decoding them when AppKit exposes those
  bytes. Rich-format extraction itself is still platform work, so the limit is a
  pre-core-transform guard, not a streaming rich-format parser for every native
  format.
- **Image OCR is local and bounded** — the macOS shell reads only a bounded image
  representation, rejects oversized decoded dimensions before Vision creates a
  `CGImage`, runs Apple's on-device OCR off the main actor, and writes recognized
  text back as a single plain string. It does not add network, filesystem,
  entitlement, or core/ABI capability.
- **Configs are envelope-bounded before transform** — `parse_config` rejects configs
  with too many operations, overlong free-text parameters, `\r`/`\n` inside
  prefix/suffix/join/split parameters, or a pipeline whose worst-case output growth
  could amplify a small input without bound. That last bound is the product of each
  operation's conservative growth factor (`Operation::max_growth_factor`) against
  `MAX_PIPELINE_GROWTH_FACTOR`: it catches *composition* — e.g. a `SplitOn` that
  re-maximizes the line count so a following `PrefixLines`/`JoinWith` re-amplifies —
  which the per-operation caps alone do not, and which a fuzz run showed could expand
  a sub-KiB input past 2 GiB (a resource-exhaustion / DoS vector). This is a config
  compatibility tightening, not an ABI or privacy-posture change: invalid configs
  fail as `ErrInvalidConfig` at the FFI boundary instead of entering the infallible
  transform path.
- **Optional masking is an output rewrite, not a new data path** — `MaskIdentifiers`
  replaces selected email/IPv4/IPv6 tokens with fixed placeholders inside the same
  pure core pipeline. It adds no network, persistence, logging, entitlement, or
  telemetry capability and does not change the in-memory-only posture.

## Known limitations (security-relevant)

These are documented honestly in [`DESIGN.md`](DESIGN.md#known-limitations); the
security-relevant ones:

- **Zeroization is best-effort.** The core holds each pipeline intermediate in
  `Zeroizing` storage, wipes transform-local scratch and accumulator storage before
  capacity growth can release old clipboard-derived bytes, and the FFI wipes the
  output buffer on free (at a measured throughput cost on very large inputs — see
  [`docs/performance.md`](docs/performance.md)). The exact inventory — what is
  wiped and the specific residual gaps (plain-`String` return windows, the
  case-insensitive sort's folded keys, `pulldown-cmark`'s internal buffers,
  process-level limits, the shell's own unscrubbed copies) — is in
  [Where zeroization matters](#where-zeroization-matters). The caller's own input
  buffer (e.g. the shell's pasteboard read) and the OS clipboard itself are outside
  the core's control, and the allocator may retain freed pages briefly before
  reuse. The invalid-UTF-8 FFI path is covered by this: when lossy decoding needs
  an owned replacement string, that temporary copy is held in a `Zeroizing` buffer
  and wiped on drop; the caller's original byte buffer remains outside the FFI's
  ownership.
- **Continuous mode is best-effort under local races.** It polls the pasteboard
  `changeCount`; it does not lock the system pasteboard or prove that no other local
  writer changed it before xPare read or after it rewrote the pasteboard. The
  shell suppresses xPare self-write generations, coalesces continuous callbacks
  while a strip is running, and drops stale transform completions if `changeCount`
  moved in flight. xPare still performs content-free outcomes only and applies
  the same size limits to each attempted run.
- **Continuous mode skips concealed/transient pasteboard content.** Before reading
  anything, the continuous poller checks for the
  [nspasteboard.org](http://nspasteboard.org) marker types —
  `org.nspasteboard.ConcealedType`, `org.nspasteboard.TransientType`,
  `org.nspasteboard.AutoGeneratedType` — and leaves marked content untouched
  *without reading it* (password-manager etiquette). The user-initiated
  hotkey/menu path deliberately still processes marked content: an explicit
  strip request wins over the marker.
- **Continuous image OCR is separately opt-in.** With continuous monitoring on,
  xPare still follows the text pipeline first. Only image-only clipboards fall
  through to OCR, and only when the user has enabled the continuous OCR setting.
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
  reading the system pasteboard before xPare rewrites it.

## Reporting a vulnerability

This repository is the system of record. If you find a security issue —
particularly anything that could cause clipboard content to leave the process, be
persisted/logged, or a way to make the core panic, hang, or read out of bounds —
report it through **GitHub Private Vulnerability Reporting**:
<https://github.com/mtonsmann/xPare/security/advisories/new>. The report stays
private until a fix is released. A reproducing input for a core panic/hang/OOB is
the most valuable thing you can include; it becomes a regression in the corpus.

A change that alters any property in the table above is a **posture change**: it
must be called out explicitly in the PR, justified, and reflected here and in the
relevant guardrail before it can land. The corresponding `xtask` check must be
updated to *match* the new posture — never weakened to hide a regression.

Security-finding fixes also follow
[`docs/guardrails/review-finding-closure.md`](docs/guardrails/review-finding-closure.md):
the issue class needs repeatable regression protection plus a short docs lesson
so future agents and humans know which invariant was violated and which check now
protects it.
