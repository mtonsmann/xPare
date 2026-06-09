# Exec Plan 0012 - Paste large buffers as files (opt-in)

Status: **completed** - Started: 2026-06-09 - Completed: 2026-06-09

## Goal

Add an opt-in macOS shell feature: when enabled and the transformed clipboard
result exceeds a user-configurable size, replace the pasteboard contents with a
**file reference** (a `.txt` file containing the result) instead of the raw
string, so pasting into Finder / mail / chat attaches a file rather than dumping
a huge text blob. Off by default; threshold user-configurable.

## Change Class

Shell (macOS) + **privacy posture** + enforcement tooling (`xtask`) + docs. No
core change, no C ABI change, no dependency change, no new entitlement (the file
lives in the App Sandbox container's own temporary directory).

## The posture question, answered head-on

The project promise is "clipboard content is never persisted." This feature
*requires* writing clipboard-derived content to disk — there is no way to put a
pasteboard file reference up without a real file behind it. Per
`docs/guardrails/privacy-and-data-handling.md` this is a **posture change**: it
must be explicitly justified, narrowly scoped, documented in `SECURITY.md` and
the guardrails, and the enforcing check updated to *match* the new posture —
never loosened in general.

## Decisions

### D-1 - The exception is opt-in, bounded, and owned by one type

Persistence happens only when the user enables **Paste large clipboards as a
file** *and* the transformed output exceeds their threshold. All file I/O lives
in a single audited type (`PasteFileStore`); nothing else in the tree may
persist content. Mitigations:

- the file lives in a dedicated `PasteAsFile.noindex` directory inside the app
  sandbox container's `temporaryDirectory` (no new entitlement; `.noindex`
  keeps Spotlight out; excluded from backups via `isExcludedFromBackup`);
- directory `0700`, file `0600` (owner-only);
- **at most one file exists at any time** — each write replaces the previous;
- best-effort lifetime minimization: the file is deleted when the pasteboard
  stops referencing it (checked on every strip), on app launch (leftovers from
  a previous run), and on controller deactivation/quit.

### D-2 - Threshold compares the transformed output, in user-facing KB

The threshold is what would actually be pasted: the transformed output's UTF-8
byte count, strictly greater than `pasteAsFileThresholdKB * 1024`. Stored in KB
(default 512) because that is the unit the settings field shows; clamped to a
minimum of 1 KB. The existing `maxInputBytes` refusal ceiling is unchanged and
still runs first — oversized clipboards are refused before the core ever runs.

### D-3 - The enforcing check gets an explicit allow-marker, not a hole

`check-no-content-logging` would (rightly) flag `PasteFileStore`. We do not
rename locals to dodge it — this is a true positive class, now sanctioned. The
check learns a literal marker, `safetystrip:allow-content-persistence`, which
exempts a line **only** inside an allowlisted file (`PasteFileStore.swift`);
the marker appearing anywhere else is itself a violation. Everyone else who
persists content still fails CI.

### D-4 - Failure degrades to the old behavior

If the file write fails, the controller falls back to the normal in-place plain
write. The user never loses the strip result because the disk was full.

### D-5 - File name carries no content

`Clipboard <timestamp>.txt` — operational metadata only, never derived from the
clipboard text.

## Work Items

1. `Settings`: `pasteLargeAsFile` (default false) + `pasteAsFileThresholdKB`
   (default 512), tolerant decode, tests.
2. `PasteFileStore` (+ `PasteFileWriting` protocol) with the mitigations above,
   tests.
3. `PasteboardProtocol.writeFileURL(_:)` + `SystemPasteboard` impl +
   `FakePasteboard` impl.
4. `StripController`: new `.strippedToFile` outcome; file branch in `perform`;
   stale-file cleanup on strip/activate/deactivate; tests.
5. App UI: settings section (toggle + KB field), menu status strings.
6. `xtask`: marker + allowlist in `check-no-content-logging`, unit tests.
7. Docs: `SECURITY.md`, `privacy-and-data-handling.md`,
   `content-logging-and-clipboard-safety.md`, `shell-contract.md`, `DESIGN.md`
   (D15), `README.md` blurb.

## Review follow-up (2026-06-09)

Manual review feedback: the feature must be menu-visible, not Settings-only.
Reworked as a `Paste as file: <mode>` menu row (Off + preset thresholds as radio
items + "Custom…" routing to Settings, which keeps only the typed threshold).
Two general rules were codified in DESIGN.md D12 alongside: **core functionality
is never Settings-only**, and **menu rows follow the canonical pipeline order**
(both the Clean section and the one-shot command section were reordered by
`Operation::canonical_rank`). The reorder exposed that `ChangeCase` had no shell
UI at all; it was given its D12 submenu (`Change case: Off/UPPERCASE/lowercase/
Title Case/Sentence case`, rank 17) in the same pass.

## Out of scope

- Simulating a paste (still forbidden); we only change what the pasteboard holds.
- Windows/Linux shells (reserved).
- Zeroizing the file's disk blocks — out of reach from userland; the posture
  docs state the residual risk plainly.
