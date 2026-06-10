# Guardrail — no content logging & clipboard safety

Two mechanical checks that protect the clipboard-privacy posture, ported from the
upstream FormatStripper `guardrails.py` and enforced here by the in-tree `xtask`
(so they run identically locally and in CI, with no extra toolchain).

## When to consult

- Adding or changing any logging, diagnostics, or persistence (files, `UserDefaults`).
- Touching the macOS shell's pasteboard read/write, settings, or smoke targets.
- Adding a Make target or CI step that exercises the clipboard.

## The rules

1. **Never log clipboard-derived content.** Diagnostics may record fixed
   operational states ("auto-cleaned", error codes, counts) but never the clipboard
   input, the transformed output, or any text derived from them. (The core also
   enforces this at compile time via `#![deny(clippy::print_stdout, print_stderr,
   dbg_macro)]`; this check extends the guarantee to the CLI and the Swift shell.)
2. **Never persist clipboard-derived content.** Persist only user *settings* —
   operation choices, the hotkey, window state. Never write clipboard input/output
   or derived text to disk, `UserDefaults`, or an archive. The single sanctioned
   exception is the opt-in **paste-as-file** store (`PasteFileStore.swift`): its
   sink lines carry the `xpare:allow-content-persistence` marker, which the
   check honors **only in that file** — the marker appearing anywhere else is
   itself a violation. See `SECURITY.md` ("Opt-in paste-as-file exception") and
   [privacy-and-data-handling](privacy-and-data-handling.md) rule 2.
3. **Default verification must not touch the real clipboard.** Any exercise of
   `NSPasteboard.general` stays behind an explicitly opt-in target. `make ci`,
   `make check`, `make build`, `make test`, `make app`, `make run`, `make preview`,
   and `make dist` must use synthetic pasteboards only, so the gate is safe to run
   anywhere and never reads or mutates the user's real clipboard.

## Enforcing checks

| Rule | Check | Command |
|------|-------|---------|
| 1, 2 | `check-no-content-logging` — scans shipped source (`core/src`, `cli/src`, `shells/macos/Sources`) for a line that both calls a log/persist sink **and** names clipboard-derived content; honors the paste-as-file allow-marker only inside `PasteFileStore.swift` and flags the marker anywhere else | `cargo xtask check-no-content-logging` |
| 3 | `check-clipboard-safety` — fails if a default Make target depends on a `*general*` (real-clipboard) smoke | `cargo xtask check-clipboard-safety` |

Both run inside `cargo xtask ci` (and `make checks`). The same gate also runs
`check-release-posture` for the official signing path and `check-c-ffi-surface`
for the C/SwiftPM bridge, because clipboard safety depends on those boundary
surfaces staying narrow and fail-closed.

### Heuristic scope (and why it is tuned)

`check-no-content-logging` flags a line only when a sink call
(`print*`/`eprintln!`/`dbg!`/`NSLog`/`os_log`/`logger.*`/`log::*`, or
`UserDefaults`/`FileManager.default`/`fs::write`/`File::create`/`write(to:`) appears
**on the same line** as a clipboard-derived-content word (`clipboard`, `pasteboard`,
`plaintext`, `payload`, `selection`, `transformed`, `stripped`, `clipboardText`).

It deliberately omits the generic `input`/`output`/`text` words the upstream regex
used: those would flag the CLI's *intentional* write of transformed output to
stdout (`stdout().write_all(output.as_bytes())`), which is the program's job, not a
leak. The trade-off is lower noise for a slightly narrower net; the core's
compile-time `print*` ban backstops the Rust side regardless. Tooling (`xtask`) and
tests are not scanned — they legitimately name these words.

## What a PR must call out

- Any new logging/persistence near pasteboard/transform code, with a one-line note
  on why it cannot contain clipboard content.
- Any new clipboard-touching Make/CI target, confirming it is opt-in (not a
  dependency of a default target).
- Never silence a finding by weakening the check — fix the code (log a state, not the
  content; persist a setting, not the payload; make the real-clipboard smoke opt-in).
