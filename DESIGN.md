# Design

This is the *why* behind xPare. [`ARCHITECTURE.md`](ARCHITECTURE.md) is the
map (what lives where, the boundary, the invariants); this document records the
settled decisions and their rationale, how the threat model shaped them, what the
known limitations are, and what is deliberately out of scope until the project
grows. The decision log mirrors `docs/exec-plans/completed/0001-initial-rearch.md`.

## Threat model

xPare processes the **clipboard**, which is one of the most sensitive data
streams on a machine. At any moment it may contain passwords, API tokens, private
keys, PII, or proprietary source code. It also routinely contains **rich text**
(HTML/RTF) and Markdown copied from web pages, chat apps, and editors — i.e.
**untrusted, attacker-influenced markup** that the tool must parse.

Three threats follow directly, and each maps to a mechanism:

| Threat | Consequence if mishandled | Design response (mechanism) |
|---|---|---|
| **Data exfiltration** — clipboard content leaving the process | Catastrophic: silent theft of secrets | No network anywhere (`check-no-network`); core has no OS/IO/net deps (`check-core-deps`); no logging sink in the core (`deny(print*/dbg!)`); macOS App Sandbox with no network entitlement |
| **Data persistence** — content outliving its use | Secrets recoverable from disk/logs/freed memory | In-memory only; no files, no logs of content; freed FFI buffers are zeroized; no telemetry |
| **Untrusted-input parsing** — adversarial HTML/Markdown | Memory-unsafety, panics (DoS), hangs (DoS), or active content surviving into the "clean" output | First-party core code has no `unsafe` (`#![forbid(unsafe_code)]`; the audited pure-data parsing deps carry their own vetted internal `unsafe`); fuzz + property + corpus pin down no panic/hang; `strip_html` neutralizes `<script>`/`<style>` |

The boundary that enforces this is the core/shell split: the core is the untrusted
parser, its first-party code cannot contain `unsafe`, and it cannot leak; the shell
is the only thing that talks to the OS. See [`SECURITY.md`](SECURITY.md) for the
posture as a checklist.

The explicit non-goal is defending the system pasteboard from every same-user local
process. Another local app may write the pasteboard before xPare reads it,
replace it while a transform is in flight, or feed rich/oversized data to consume
resources. xPare's security promise is narrower and enforceable: when it is
the component handling clipboard content, it does not exfiltrate, persist, log, or
memory-unsafely parse that content, and it refuses oversized text before calling the
core.

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
consumes it unchanged. **Why these four symbols** — `xp_abi_version`,
`xp_capabilities_json`, `xp_transform`, `xp_buffer_free` — and nothing more: the
surface stays narrow and data-driven. Feature selection crosses as a serialized
config string, so **adding a transform is a data change, never an ABI change**. The
checked-in header (`core-ffi/include/xpare.h`) is the source of truth and
`cargo xtask check-abi` fails CI if the code drifts from it, making any real ABI
change a deliberate, reviewable event (bump `XP_ABI_VERSION`, regenerate, call it
out). See [the FFI guardrail](docs/guardrails/ffi-boundary-and-abi-stability.md)
and [D16](#d16--the-one-coordinated-pre-10-abi-rename-v3) for the v2 → v3 rename.

### D3 — Config is versioned JSON: an ordered list of operations

A `Config` is `{ "version": 3, "operations": [ ... ], "ordering": "canonical" }`,
where each operation is an internally-tagged object keyed on `op` (e.g.
`{"op":"strip_html"}`, `{"op":"change_case","case":"title"}`). **Why ordered and
explicit:** transform order is semantically significant (`StripHtml` then
`StripMarkdown` is not the same as the reverse). By default the core applies a
**documented canonical order** (`ordering: "canonical"`, see [D13](#d13--canonical-pipeline-ordering));
`ordering: "as_given"` runs the operations in exactly the order provided. Either way
the core is deterministic and never *silently* reorders — the order it uses is fully
specified by the config. Versioning lets a shell detect a capability mismatch
deterministically (`parse_config` rejects any version other than `CONFIG_VERSION`);
**v2** added the `ordering` field; **v3** tightened the resource envelope below
from the original broad v2 values. `parse_config` also enforces a resource envelope:
at most 32 operations, free-text parameters (`prefix`, `suffix`, `separator`,
`delimiter`) capped at 16 UTF-8 bytes (real tokens are 1–2 bytes — `"> "`, `", "` —
so 16 is generous while keeping the per-op growth factor and the FFI free-text surface
small), those parameters single-line (`\r`/`\n` rejected), and a whole-pipeline
worst-case growth product capped at `MAX_PIPELINE_GROWTH_FACTOR` (4096×). That blocks
configs that could otherwise turn tiny inputs into runaway intermediates before a
transform runs; the growth gate carries a Kani proof that it cannot wrap an amplifying
pipeline into acceptance (see D14). Adding a transform is a new enum variant plus a
pipeline arm — zero ABI change.

**Config compatibility is deliberately strict — fail-closed.** `parse_config`
rejects unknown fields (`deny_unknown_fields`) and any version other than the
exact `CONFIG_VERSION`; there is no silent tolerance of "mostly valid" configs
and no forward-compatibility guessing. **Why:** the config crosses a trust
boundary as data, and a half-understood config silently dropping or reinterpreting
an operation is worse than a clean, distinguishable error (the FFI surfaces a
version mismatch as `XP_STATUS_ERR_UNSUPPORTED_CONFIG_VERSION`, separate from
malformed JSON). Schema evolution happens only through explicit `CONFIG_VERSION`
bumps. From 1.0, a breaking change to the config schema — like one to the C ABI
or the CLI flags — requires a major version.

### D4 — Stateless `repr(C)` error model, lossy input decoding

Errors are a flat `repr(C)` status enum (`XpStatus`, ABI v3) with **no global
error state**: `XP_STATUS_OK` (0), `XP_STATUS_ERR_NULL_ARG` (1),
`XP_STATUS_ERR_INVALID_CONFIG` (2), `XP_STATUS_ERR_INTERNAL` (3),
`XP_STATUS_ERR_INPUT_TOO_LARGE` (4), and `XP_STATUS_ERR_UNSUPPORTED_CONFIG_VERSION`
(5 — the core's `ConfigError::UnsupportedVersion`, so a shell can distinguish a
config-version mismatch from malformed JSON). **Why:** a stateless, thread-safe,
trivially consumable contract — the caller reads the return code, no `errno`-style
hidden state to race on. Input bytes are decoded with lossy UTF-8 (invalid bytes
become U+FFFD) rather than rejected, so **adversarial bytes can never make
`xp_transform` fail** — it always produces *some* defined output. An input above
`XP_MAX_INPUT_BYTES` returns `XP_STATUS_ERR_INPUT_TOO_LARGE` before anything is
read or allocated. A caught panic maps to `XP_STATUS_ERR_INTERNAL`, which should
never occur (the core is fuzzed) but is handled so a stray panic is never UB.

### D5 — HTML stripper: hand-rolled pure-safe-Rust state machine

`strip_html` is a hand-written `char`-by-`char` state machine plus a curated entity
table — **not** an upstream HTML parser. **Why reimplement a small subset:** a full
HTML5 parser is a large, opaque dependency with broad capability and its own attack
surface; the brief's guidance is to reimplement a small subset rather than depend
on opaque upstream. Because it is safe Rust, it is **memory-safe by construction**;
the only residual risks for a hand-rolled parser are panics and hangs, and those
are pinned down by an adversarial corpus, property tests, and `cargo fuzz`. The
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

### D6a — HTML-to-Markdown conversion is one-shot and dependency-free

`html_to_markdown` is a small safe-Rust converter for common copied-web fragments:
headings, paragraphs, links, lists, blockquotes, inline emphasis/code, preformatted
code blocks, line breaks, and simple table rows. **Why not a parser/converter
dependency:** this is a convenience command, not a browser-grade import pipeline,
and the current project posture favors a tiny auditable core dependency tree. The
converter keeps the existing `strip_html` sanitizer unchanged and reuses its
bounded curated entity decoder.

Security framing: the converter drops comments, declarations, processing
instructions, and `<script>`/`<style>` raw-text bodies. It emits link destinations
only for inert schemes (`http`, `https`, `mailto`) and relative/hash URLs; unsafe
schemes such as `javascript:`, `data:`, `vbscript:`, and `file:` are dropped while
link text survives. Entity-decoded text is escaped so raw HTML stays inert in the
Markdown source, and inline/pre code delimiters are chosen longer than any copied
backtick run so code content cannot break out of its Markdown wrapper. It is
surfaced by the shell as an explicit one-shot command, so continuous mode never
silently converts every copied web fragment to Markdown.

### D7 — Buffer ownership: leaked `Box<[u8]>`, freed + zeroized

`xp_transform` returns the output as a `(ptr, len)` pair over a leaked `Box<[u8]>`;
`xp_buffer_free` reclaims it. **Why `Box<[u8]>` and not the raw `String`/`Vec`:** a
boxed slice carries exactly `ptr + len` (no separate capacity), which is the minimal
thing the C side must track and round-trip. `xp_buffer_free` **zeroizes** the buffer
before dropping it, a best-effort wipe so clipboard-derived bytes do not linger in
freed memory. The only `unsafe` is reclaiming the box; producing it is safe Rust.

### D8 — Continuous mode: owned poller, fully torn down when off

The continuous (auto-clean) mode polls the platform clipboard change counter
(macOS `NSPasteboard.changeCount`) on a default **500 ms** interval. **Why a poller
that is fully invalidated and niled when disabled:** the requirement is that *no
loop runs when the feature is off* — not a paused timer, but no timer object at all.
On-demand mode (the default) does no polling: a hotkey triggers a single
read → transform → in-place rewrite.

This is a best-effort convenience, not a pasteboard lock or an inter-process
compare-and-swap. The shell suppresses xPare self-write generations, drops a
transform completion if `changeCount` moved while it was running, and coalesces
continuous callbacks while a strip is already in flight. A local pasteboard writer
can still race before the read or after the rewrite; those controls are shell-only
and do not change the core ABI.

### D9 — Global hotkey: Carbon `RegisterEventHotKey`

The macOS hotkey (default **⌃⌥⌘V**, user-configurable via the in-app recorder in
Settings) uses Carbon's `RegisterEventHotKey`. **Why not a
`CGEventTap` or a global `NSEvent` monitor:** those require the Accessibility or
Input Monitoring TCC permissions — broad, scary grants for a clipboard utility.
`RegisterEventHotKey` registers one specific chord and needs **neither**, which
keeps the privilege footprint minimal and avoids a permission prompt that would
undermine user trust. The Settings recorder captures the replacement chord with a
**local** `NSEvent` monitor (the app's own key events only), which also needs no
permission — it is not the forbidden global monitor.

### D10 — CLI has no dependencies

The `xpare` CLI parses its own arguments by hand. **Why:** it is a boring
validation/fuzz harness; an arg-parsing dependency would add surface for no benefit.
All config parsing lives in the core, so the CLI is a thin stdin→core→stdout pipe.

### D11 — Enforcement via a single portable `xtask`

All structural invariants are checked by the in-repo `xtask` crate, not external
cargo plugins. **Why:** the same `cargo xtask ci` runs identically locally and in
CI, so there is no CI-only logic to drift from and no extra tooling to install. Each
check prints a remediation-oriented message that teaches how to *fix* the violation,
not how to silence it.

When a review finds a new class of security, correctness, or performance issue,
the fix also adds a repeatable blocker and a short guardrail lesson. Security
findings first pass through
[`docs/guardrails/agentic-security-finding-triage.md`](docs/guardrails/agentic-security-finding-triage.md)
to validate the source/sink/control, owning boundary, and sibling search; true
positives then close through
[`docs/guardrails/review-finding-closure.md`](docs/guardrails/review-finding-closure.md).
**Why:** the project treats review findings as new knowledge about an invariant,
not as one-off cleanup.

### D12 — Operation taxonomy: rewrites vs reductions, toggles vs commands

Operations divide by **what they do to the buffer**, and the shell surface follows
from that — it is not a free UI choice.

- **Rewrites** preserve the text and edit it in place (`StripHtml`,
  `CollapseWhitespace`, `DedupeLines`, and the new `Defang` / `Refang` /
  `CleanUrls` / `MaskIdentifiers`). They compose additively — each is an
  independent stage refining the same buffer — and the idempotent ones are safe to
  run on every clipboard change.
- **Reductions/conversions** replace the buffer with a derived subset or
  representation (`ExtractEmails`, `ExtractUrls`, `HtmlToMarkdown`). They do **not**
  compose predictably as always-on policy, they are terminal user commands, and
  silently reducing or converting every copy in continuous mode is never what the
  user wants.

This dictates two interaction models in the shell:

- **Persistent toggles** — rewrites that make sense always-on. Stored in the ordered
  `operations` pipeline, eligible for continuous mode. The menu's *Clean* section.
- **One-shot commands** — a transient single-op config run on demand, never
  persisted, never auto-run. *All reductions/conversions are commands;* so is
  `Refang` (re-activating received IOCs is a deliberate act, not a standing
  policy). The menu's *Extract / convert* section, parallel to "Strip clipboard now".
  macOS image OCR also lives here: it is shell-owned OS integration, not a core
  transform, and the manual "Extract text from image" command remains available
  without changing the saved text pipeline.

Continuous OCR is a separate opt-in because image-only clipboards are not part of
the ordinary rich→plain text pipeline. When enabled, continuous mode first tries the
text pipeline exactly as before; only an empty text read may fall through to bounded,
local Vision OCR. Marked concealed/transient/auto-generated pasteboard content still
short-circuits before either text or image bytes are read.

The **core does not know about this split** — every op stays a plain pipeline entry,
so the CLI and power users can still compose extraction inside a pipeline. The
taxonomy is a *shell presentation* contract, with one hard rule: continuous mode
must refuse to run a reduction.

**The top-level menu stays one row per feature family.** A standalone,
zero-parameter rewrite can be a simple top-level toggle, but a feature with
bounded options must expose a single status-bearing row plus a submenu of native
radio/checkmark items. Examples: `Sort lines: Off` owns the sort mode choices,
`Mask identifiers: Emails, IPv4` owns the selected masking targets, and
`Paste as file: > 512 KB` owns the threshold presets. Do not add one
top-level sibling row per flag unless each row is a truly independent workflow;
menu scanability is part of the product contract, not garnish.

**Core functionality is never Settings-only.** Every feature the user can turn on
or off must be visible — and switchable — at a glance in the menu. The Settings
window holds only what a native menu *cannot* host (typed/free-text input); when a
menu-surfaced feature also has a typed parameter, its submenu ends with a
**"Custom…"** item that opens Settings, so the menu remains the single point of
discovery and Settings is the continuation, never the hiding place.

**Menu rows follow the canonical pipeline order ([D13](#d13--canonical-pipeline-ordering)).**
The *Clean* section (and likewise the one-shot command section) lists entries by
`Operation::canonical_rank`, so the menu reads top-to-bottom in the order the
default pipeline actually runs — the menu *is* the pipeline, visually.

**Parameters follow Route A — a Settings window, not an expanded menu.**
`MenuBarExtra(.menu)` is a native AppKit menu and cannot host a text field, so the
free-text-parameterized ops (`PrefixLines`, `SuffixLines`, `JoinWith`, `SplitOn`),
pipeline *ordering*, and the paste-as-file *custom threshold* live in a
conventional SwiftUI `Settings` scene. Bounded,
enumerable params (`ChangeCase`'s case, `SortLines`'s two flags, `Defang`'s bracket
style, `MaskIdentifiers`' selected identifier classes, paste-as-file's preset
thresholds) stay in the menu as
**submenus with radio/checkmark items**. **Why not** make the whole menu a
`MenuBarExtra(.window)` panel: that buys inline text fields at the cost of the
crisp, keyboard-driven native-menu behavior on the common path; a Settings window
keeps the fast path fast and is where macOS users already expect typed
configuration to live.

The Rust core is the authoritative validator for the free-text parameter envelope
(single-line, 16-byte maximum); shells should mirror that in Settings for immediate
feedback, but never rely on UI validation as the only guard.

### D13 — Canonical pipeline ordering

By default the core **reorders** the operations into a documented canonical order
before running them (`Config.ordering = Canonical`), so a UI that simply toggles ops
on/off always gets a correct, efficient pipeline without the user reasoning about
order. `Ordering::AsGiven` runs them exactly as listed. **Why this doesn't betray
[D3](#d3--config-is-versioned-json-an-ordered-list-of-operations):** the order is
still fully determined by the config and deterministic — the core never *silently*
reorders; canonical ordering is explicit, documented, and overridable. This refines,
rather than reverses, D3, and it bumped `CONFIG_VERSION` to 2 (additive field, no ABI
change).

The order is a stable sort by a per-op rank (`Operation::canonical_rank`), so any
genuinely-free pair keeps the user's order. The rank encodes two kinds of rule:

- **Correctness** (order changes output): `StripHtml` < `StripMarkdown` (D6);
  strippers before everything; `TrimTrailingWhitespace` before `DedupeLines` (so
  whitespace-only-different lines dedupe); **`CleanUrls` before `MaskIdentifiers`
  before `Defang`/extraction** (`CleanUrls` needs intact URLs, then masking removes
  selected live identifiers before later stages can preserve or derive them);
  `UnwrapLines` before `RemoveBlankLines` (blank lines are its paragraph delimiter);
  `JoinWith` last (it collapses line structure).
- **Efficiency** (output-identical, one is cheaper): `DedupeLines` before `SortLines`
  — deduping first shrinks the set the sort must order.

**Where it lives.** The policy is a pure function in the core (not invoked by any op,
so it can't recurse into `transform`), exercised by a property test that canonical
output equals manually pre-sorting then running `as_given`. The **CLI** stays the
explicit tool — it defaults to `as_given` (with `--canonical` to opt in) so existing,
order-sensitive pipelines are unsurprising — while the **macOS shell** uses canonical
by default and exposes a "Manual order" (`as_given`) mode with a drag-reorder list for
the rare case a user wants to place an ambiguous op (e.g. `ChangeCase`) themselves.

### D14 — Cedar-style verification-guided development (without Cedar)

The repo's engineering loop is **evidence-first**: a change is judged by the
correctness evidence it ships, not by the plausibility of the diff. The loop —
classify → correctness brief → invariants → tests/properties/fuzz → smallest patch →
deterministic gates → evidence packet — is encoded as repo-native docs
([`CONTRIBUTING.md`](CONTRIBUTING.md), [`docs/agent-workflow.md`](docs/agent-workflow.md),
the brief and PR templates, the security-finding triage guardrail,
per-change-class task prompts under `docs/agent-tasks/`, and the thin
Codex/Claude security-triage skill wrappers) and kept from rotting by the
`check-agent-workflow` structural check. **Why:** agents make producing a
plausible patch cheap; what stays expensive — and is the actual product — is
trustworthy evidence. Making the evidence a required, mechanically-checked
artifact is what keeps quality from regressing as authoring gets faster. "Agents
propose; deterministic tools dispose."

The technical centerpiece is an **executable reference interpreter** for the
pipeline. Production `transform` fuses adjacent operations and folds intermediates
through `Zeroizing` storage; a test-only reference
([`core/tests/reference_transform.rs`](core/tests/reference_transform.rs)) resolves
the ordering and applies operations strictly one at a time via the public `ops::*`
functions, with no fusion. A differential property (`transform == reference`, 1024
cases) makes every fused fast path provably equal to naive sequential application for
any config that triggers it; companion properties pin canonical ordering to an
explicitly sorted `as_given` run, re-assert determinism, and bound an accepted
config's output growth by the per-op factor product.

**Cedar is the inspiration, not a dependency.** AWS's Cedar pairs its production
authorization engine with a simple executable specification and proves them
equivalent by differential random testing — the discipline this borrows. xPare
is **not** an authorization-policy engine and has no need for a policy language, so it
does **not** add Cedar (or any policy/DSL crate); doing so would import a large
capability surface for a problem the project does not have. What it adopts is the
*method*: executable reference semantics, property-based and differential random
testing, reference-vs-production equivalence, and repo-native evidence requirements.

This is verification-*guided* development, not formal verification. It does not prove
the whole core correct, does not prove the sanitizers correct against browser/RFC
semantics, and does not formally prove FFI memory behavior — though two narrow,
heavy-tool tracks tighten the highest-value gaps (both advisory, both outside the
required `cargo xtask ci` gate, both runnable locally):

- **Miri** runs the `core-ffi` boundary tests — `core-ffi` is the only crate with
  `unsafe` — under an undefined-behavior detector (`cargo xtask check-miri`), so UB on
  the tested executions is caught.
- **Kani** model-checks the crisp resource-envelope arithmetic
  (`cargo xtask check-kani`). The saturating growth-product is factored into
  `saturating_growth_product`, and `#[cfg(kani)]` harnesses in `config.rs` prove —
  for all symbolic factors within the validated range, over a full-length pipeline —
  that the gate accepts a config **iff** its true, arbitrary-precision worst-case
  growth is within `MAX_PIPELINE_GROWTH_FACTOR`. So no saturation wrap can turn an
  amplifying pipeline into an accepted one. This is bounded, not whole-program,
  verification: it deliberately proves the integer arithmetic only, not the
  `String`-bearing config or the text transformer. Kani harnesses are `#[cfg(kani)]`,
  so the `kani` crate never enters the dependency tree `check-core-deps` guards.

### D15 — Paste-as-file: a single sanctioned, opt-in persistence exception

**Paste large clipboards as a file** (off by default, threshold user-configurable
in KB) replaces the pasteboard's contents with a *file reference* when the
transformed result exceeds the threshold, so pasting attaches a `.txt` file
instead of dumping a huge string. Per [D12](#d12--operation-taxonomy-rewrites-vs-reductions-toggles-vs-commands)'s
menu-first rule it is surfaced as a status-bearing menu row (`Paste as file: …`)
with preset thresholds and a "Custom…" item routing to Settings for a typed value. **Why this doesn't betray the "no persistence"
promise:** a pasteboard file reference is impossible without a real file behind
it, so the persistence is inherent to the user-requested behavior, not a side
channel — and the project's own posture rules allow a posture change that is
explicit, justified, documented, and enforced. The exception is engineered to
stay singular:

- one audited writer (`PasteFileStore`), the only file where
  `check-no-content-logging` honors the `xpare:allow-content-persistence`
  marker — the marker anywhere else fails CI, and an xtask unit test pins the
  allowlist to exactly that file;
- the file lives in the sandbox container's temp directory
  (`PasteAsFile.noindex/`, no new entitlement, Spotlight- and backup-excluded),
  directory `0700` / file `0600`, name is a timestamp (never content-derived);
- at most one file at a time; deleted when the pasteboard stops referencing it,
  on launch, and on quit; a failed write degrades to the normal in-place plain
  write so the strip result is never lost;
- still **in-place only** — the feature changes *what* the pasteboard holds,
  never simulates a paste (the D-"Other settled choices" macOS posture stands).

Rejected alternatives: promoting the file write into the core (the core stays
free of OS/IO — this is pure shell/OS integration, per the shell contract); a
content-named or user-chosen file location (worse privacy, needs entitlements);
writing the string *and* the file URL together (receiving text editors would
paste the raw blob, defeating the feature). See SECURITY.md ("Opt-in
paste-as-file exception") and
[`docs/exec-plans/completed/0012-paste-large-buffers-as-files.md`](docs/exec-plans/completed/0012-paste-large-buffers-as-files.md).

### D16 — The one coordinated pre-1.0 ABI rename (v3)

*Decided 2026-06, for 1.0.0-rc.1.* The repository rename SafetyStrip → xPare
left the C ABI carrying the old prefix (`ss_*`, `SsStatus`, `SS_STATUS_*`). The
surface was renamed to `xp_*` / `XpStatus` / `XP_STATUS_*` in **one coordinated
compatibility event**, bumping `XP_ABI_VERSION` 2 → 3 and adding
`XP_STATUS_ERR_UNSUPPORTED_CONFIG_VERSION` (see [D4](#d4--stateless-reprc-error-model-lossy-input-decoding))
in the same bump. **Why now and why once:** there are zero external ABI
consumers — the macOS shell in this repo is the only caller — so this is the
last point at which the rename costs nothing; carrying a dead product name in a
frozen ABI forever was the alternative. The header guard (`XPARE_FFI_H`) and
`XP_MAX_INPUT_BYTES` were already on the new name and did not change. **The ABI
is frozen from 1.0 onward:** any breaking change to the C ABI — like one to the
config schema or the CLI flags — requires a major version.

### D17 — CodeQL is additive security review signal

CodeQL runs over Rust, Python, and GitHub Actions with the `security-extended`
query suite, plus repo-specific Rust/Python policy packs under
`.github/codeql/queries/`. It is **not** the required gate and should not be
branch-protection-required until the alert baseline has been triaged. Swift
CodeQL is deferred until the extractor completes reliably in CI; the Swift shell
remains covered by deterministic `cargo xtask ci` posture checks.
The required local/CI gate remains `cargo xtask ci`; CodeQL is defense in depth
for flow-sensitive or API-resolution issues that deterministic repo checks do
not model well. The workflow is deliberately SHA-pinned and least-privilege, with
`check-codeql-workflow-posture` preventing it from drifting into a moving-tag,
broad-permission, or custom-pack-disconnected setup while baseline noise is still
unknown.

Rejected alternatives: `security-and-quality` as the initial suite (too noisy
for a privacy utility with strong deterministic gates), custom Actions QL while
actionlint/zizmor/posture checks already cover workflow lessons with lower noise,
and making CodeQL a merge requirement on day one (could block unrelated work on
untriaged false positives).

### Other settled choices

- **macOS posture:** App Sandbox + Hardened Runtime, **minimal entitlements**. The
  only entitlement is `com.apple.security.app-sandbox` = true — reading and writing
  the pasteboard needs no entitlement. **In-place rewrite only:** xPare never
  simulates a paste (`Cmd-V`), which would need Accessibility and could fire into
  the wrong app; it only replaces the clipboard's own contents. See
  [the macOS posture guardrail](docs/guardrails/macos-posture.md).
- **Release posture:** unsigned/ad-hoc preview archives are test artifacts, not
  official binaries. Official `make dist` / release-workflow assets must be
  Developer ID signed with the checked-in App Sandbox entitlements, then notarized
  and stapled; the release script verifies the signed entitlement payload is still
  minimal. Releases are **arm64-only (Apple Silicon)** at 1.0 and are created as
  **draft** GitHub releases — a human publishes. There is **no auto-update
  mechanism by design** (it would need network access); updates are manual via
  GitHub Releases, and the app shows its version in the menu.
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
- **HTML-to-Markdown** (common copied-web structure, dropped active content,
  safe-link filtering): `core/src/ops/html_to_markdown.rs`.
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
- **Privacy masking** (`MaskIdentifiers` — replace selected email, IPv4, and IPv6
  tokens with fixed placeholders): `core/src/ops/mask.rs`.

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

### IOC defang/refang, URL cleaning, and privacy masking (the agreed contract)

These rewrites share the existing whitespace-tokenizer and indicator heuristics
(see `ops/indicators.rs`); they are deliberately not RFC parsers. The exact, frozen
rule for each lives in its implementing function's doc comment once built — this is
the design-level contract they must satisfy.

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
- **`MaskIdentifiers`** replaces selected email, IPv4, and IPv6 tokens with fixed
  placeholders (`[email]`, `[ipv4]`, `[ipv6]`). It is a rewrite, not a reduction, so
  it is safe as a persistent toggle and in continuous mode. It is deliberately
  deterministic and idempotent: no random values, hashes, counters, partial masks, or
  persistent pseudonym maps. Canonical ordering runs URL cleaning first, then masking,
  then defang/refang and extraction, so the privacy-preserving default masks live
  identifiers before another op can preserve or derive them.

All four are hand-rolled scanners over adversarial input, so they join the
panic-free regime: proptest (panic-freedom + idempotence, plus the defang/refang
round-trip property), an adversarial corpus, a `cargo fuzz` target each, and the
`perf_guard.rs` linear-time budget.

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
- **C ABI:** a fixed, generous backstop `XP_MAX_INPUT_BYTES` (2 GiB). A larger
  input returns `ErrInputTooLarge` *before* any read or allocation, so it can never
  abort or overflow at the boundary. It must be a constant because the
  platform-neutral core may not ask the OS about memory.
- **macOS shell:** the real, RAM-proportional policy —
  `min(XP_MAX_INPUT_BYTES, physicalMemory / 10)`, which keeps a worst-case strip under
  ~half of RAM and scales with the machine. An oversized clipboard yields a
  content-free "too large" status and is left untouched.
  The same ceiling bounds image representations read for OCR, and Vision rejects
  oversized decoded pixel dimensions before creating a `CGImage`.
- **CLI:** intentionally uncapped — the right tool for multi-GB *file* work, where the
  caller manages its own memory.

This ceiling is what drove the v1 → v2 ABI bump (see the FFI guardrail); the
v2 → v3 bump was the coordinated rename, [D16](#d16--the-one-coordinated-pre-10-abi-rename-v3).

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
  `Zeroizing` buffer (wiped on drop) and `xp_buffer_free` wipes the output buffer, so
  clipboard-derived bytes are scrubbed from the heap after use — at a measured
  throughput cost on very large inputs (see [`docs/performance.md`](docs/performance.md)).
  It remains best-effort: the caller's input buffer and the OS clipboard itself are
  outside the core's control, and the allocator may briefly retain freed pages before
  reuse. The invalid-UTF-8 FFI path is fixed within that ownership model: if lossy
  decoding creates an owned replacement string, that temporary is `Zeroizing` and is
  wiped on drop; the original caller-owned byte buffer remains outside the boundary.
  The precise inventory — what is wiped, and the remaining best-effort gaps — lives
  in [`SECURITY.md`](SECURITY.md#where-zeroization-matters).
- **`StripMarkdown` alone is not a script-neutralizing sanitizer.** It delegates
  *embedded* HTML to `strip_html` best-effort, but the supported path for hostile
  content is `StripHtml` → `StripMarkdown`. Do not rely on `StripMarkdown` by itself
  to scrub a `<script>` body.
- **The HTML entity table is a curated subset**, not the full WHATWG named-character
  reference. Unknown but well-formed `&name;` references are emitted **verbatim**
  (never dropped, never panicked on). Numeric references are fully supported, with
  out-of-range/surrogate values mapped to U+FFFD.
- **HTML-to-Markdown is a small clipboard converter, not full HTML5 import.** It
  preserves common copied-web structure and safe links, but it is not a DOM builder,
  CSS-aware renderer, or full table/list normalizer. It intentionally drops unsafe
  link schemes and active-content bodies while keeping visible text.
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
- **Privacy masking is heuristic token-level masking, not DLP.** `MaskIdentifiers`
  uses the shared whitespace-token model and fixed email/IPv4/IPv6 classifiers. It
  masks common clipboard/log shapes, but it does not detect names, phone numbers,
  postal addresses, secrets, every possible email/IP spelling, or identifiers
  embedded inside larger non-whitespace strings. It rewrites output only; the
  original OS clipboard value and same-user pasteboard races remain outside
  xPare's confidentiality boundary.
- **Rich→plain extraction is the shell's best-effort.** The core transforms whatever
  text the shell extracts; choosing the best clipboard representation (preferring
  HTML) is a shell responsibility and is itself heuristic per platform. The macOS
  shell checks raw HTML/RTF representation bytes before decoding them when AppKit
  exposes those bytes, then re-checks extracted UTF-8 before calling the core. This
  is still not a streaming rich-format parser: platform formats without raw byte
  access may require materialization before the extracted-text ceiling can apply.
- **No full Xcode build in this environment.** The development environment is
  Command-Line-Tools-only, so `swift build` compiles the macOS shell sources but a
  signed, notarized `.app` is documented rather than produced. The C ABI, the FFI
  staticlib, and the shell sources are real; final packaging is a documented step.
- **Continuous mode is polling, not event-driven.** macOS exposes no clipboard-change
  notification, so the poller checks `changeCount` on an interval (default 500 ms);
  there is an inherent up-to-interval latency in continuous mode (on-demand mode is
  immediate). It suppresses xPare self-writes and drops stale transform
  completions, but it does not serialize every same-user pasteboard race: a local
  writer can still change the pasteboard before xPare reads or after it writes.

## Adopt if the project grows

Explicitly **out of scope now** per the kickoff brief — listed so a future
maintainer knows they were considered and deferred, not forgotten. (These are the
*strategic* deferrals; smaller feature/task-level items punted from individual exec
plans are collected in [`docs/deferred-work.md`](docs/deferred-work.md).)

- **Recurring documentation-GC agents** — automated passes that prune/refresh docs.
  The doc set is small and hand-maintained today.
- **An observability stack** — metrics/tracing/dashboards. A clipboard tool that
  must never exfiltrate data is the wrong place for telemetry; revisit only with a
  privacy-preserving, local-only design.
- **Quality-grade cadences** — scheduled audits, dependency-review rotations,
  fuzzing campaigns beyond the CI smoke. Today the CI gate + on-demand fuzzing
  suffice.
- **Swift (shell) fuzzing.** Swift supports libFuzzer (`-sanitize=fuzzer`), but the
  shell has nothing fuzz-worthy *by construction*: the hard rule above ("no transform
  logic in a shell") keeps every untrusted-markup parser in the already-fuzzed core,
  and the shell's only byte-level work is bounded encoding sniffing
  (`SystemPasteboard.decodeHtml`, size-ceilinged before decode) plus Apple's own RTF
  decoder. The shell's realistic crash class is arithmetic traps on settings values,
  which the shell-contract clamp rule + extreme-value unit tests cover deterministically
  (cheaper and more targeted than a fuzz harness for fixed scalar math). Revisit only
  if a shell ever grows real parsing — which the contract forbids — or the
  platform-decode wrapper surface grows.
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
