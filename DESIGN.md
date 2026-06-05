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

## Known limitations

These are accepted trade-offs, documented so they are not mistaken for defects.

- **Zeroization is best-effort.** `ss_buffer_free` zeroizes the final output buffer,
  but Rust may reallocate intermediate `String`s during a multi-step pipeline, and
  those interim allocations are not individually wiped. We minimize copies and wipe
  the buffer that crosses the boundary; we cannot guarantee no transient copy ever
  touched the allocator. The OS clipboard itself is also outside our control once
  the shell writes back.
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
  accept or reject edge cases a full parser would not.
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
