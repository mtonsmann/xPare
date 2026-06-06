# Guardrail: shell contract

**When to consult:** you are working on the macOS shell (`shells/macos/`) or
scaffolding a new platform shell (`shells/windows/`, `shells/linux/`, both reserved
and empty today). For macOS-specific posture (sandbox/entitlements/hotkey/pasteboard)
also read [macos-posture](macos-posture.md).

A shell owns **all** OS integration. The core owns **all** transform logic. The line
between them is the C ABI. **Adding a platform = implement this checklist and link
the core** — no new core code, no ABI change.

## The hard rule

**No transform logic lives in a shell.** A shell never strips HTML, normalizes
whitespace, changes case, etc. itself. It extracts text, calls `ss_transform`, and
writes the result back. If you find yourself parsing markup in shell code, stop —
that belongs in the core as an operation.

## Per-platform responsibility checklist

Every shell must implement all of these. Use it as the acceptance checklist for a
new platform.

1. **Clipboard read (incl. rich → plain).** Read the clipboard and **extract the best
   plain representation**. Prefer the HTML representation and feed it to the core's
   `StripHtml` — that is the path that neutralizes `<script>`/`<style>` and tags.
   Falling back to a plain-text representation is fine when no rich form exists. This
   extraction is the shell's (best-effort, platform-specific) job. The shell's size
   ceiling applies to raw rich representation bytes when the platform exposes them,
   and to extracted text before calling the core. Do not rely on this as a universal
   streaming pre-parse limit for every native format.
2. **Clipboard write (in place).** Write the transformed text **back to the clipboard
   in place**. Never simulate a paste (e.g. synthesizing Cmd-V/Ctrl-V) — that needs
   intrusive input permissions and can fire into the wrong app. Replace the
   clipboard's own contents only. This is not a lock against same-user local
   pasteboard writers; another process may race the read/transform/rewrite window.
3. **Change detection.** Support a **continuous mode** that watches the platform
   clipboard change signal and auto-cleans. It must be an **owned watcher that is
   fully torn down (stopped and released) when the mode is off** — no loop/timer runs
   when disabled. Default poll interval: **500 ms** where the platform has no change
   event. On-demand mode (the default) does no watching. Polling is best-effort:
   repeated writes can collapse between ticks, and a write can happen while a
   transform of an older snapshot is still running. Shells should suppress their own
   write generations, drop stale completions when the clipboard generation changed,
   and coalesce callbacks while a strip is already running; those controls are shell
   concerns, not ABI changes.
4. **Tray / menu-bar UI.** A lightweight status-area UI: toggle continuous mode,
   trigger an on-demand clean, open settings, quit. No main window required.
5. **Global hotkey.** A configurable hotkey for the on-demand clean (default
   **⌥⌘V** on macOS). Choose a registration mechanism that needs the **least**
   privilege — specifically, **not** one requiring Accessibility or Input Monitoring
   (on macOS, Carbon `RegisterEventHotKey`; pick the equivalent low-privilege API per
   platform).
6. **Settings.** Persist user preferences (hotkey, continuous on/off + interval, the
   operation pipeline / chosen config). Settings are *configuration*, never clipboard
   *content* — persisting content is forbidden (see
   [privacy-and-data-handling](privacy-and-data-handling.md)).
7. **Calling the core.** Link the FFI staticlib and call the four C symbols:
   `ss_abi_version` (negotiate), `ss_capabilities_json` (discover supported ops —
   don't hardcode them), `ss_transform` (read → transform), and `ss_buffer_free`
   (release the result; it is zeroized on free). Build the `config_json` from the
   user's chosen pipeline. The canonical sanitization config is
   **`StripHtml` → `StripMarkdown`**.
   One-shot conversion commands may intentionally choose a different representation:
   `HtmlToMarkdown` consumes the raw HTML representation directly so structure is not
   destroyed before conversion.
8. **Off-thread transform (UI responsiveness).** `ss_transform` is synchronous and,
   on large inputs (e.g. a multi-hundred-MB log pasted onto the clipboard), can take
   ~1 s or more — far too long to run on the UI/event thread. Run the transform **off
   the UI thread** and marshal the result back to the UI thread to apply it. This is
   an **inherently per-shell** responsibility: each platform's threading/UI model
   differs, so it cannot be shared, and the C ABI stays synchronous on purpose — the
   shell owns concurrency exactly as it owns clipboard I/O and the hotkey. (The core
   also zeroizes pipeline intermediates, which adds cost on very large inputs — a
   further reason to keep big transforms off the UI thread; see
   [`docs/performance.md`](../performance.md).)

## Wiring to the core (how the macOS shell does it)

The macOS shell links `safetystrip-ffi` (build with
`cargo build -p safetystrip-ffi --release`) and exposes the C ABI to Swift via a
module map at `shells/macos/Sources/CSafetyStrip/include/`:

- `module.modulemap` declares a `CSafetyStrip` module whose header is `shim.h`.
- `shim.h` **re-includes** the single source-of-truth header at
  `core-ffi/include/safetystrip.h` by relative path rather than copying it, so the
  shell can never drift from the frozen ABI.

A new platform should follow the same principle: consume the one checked-in header,
never a copy. (`Package.swift`, the Swift app/kit sources, and the entitlements file
are owned by the shell stream and may still be landing; this guardrail documents the
contract they implement.)

## Enforcing checks

- **Build smoke:** `cargo build -p safetystrip-ffi --release` then
  `swift build --package-path shells/macos`. CI runs this best-effort on macOS
  (`continue-on-error`, since the image may lack full Xcode).
- **Entitlements (macOS):** `cargo xtask check-entitlements` — see
  [macos-posture](macos-posture.md).
- The ABI the shell links against is frozen by `cargo xtask check-abi`.

## What a PR must call out

- Which checklist items the change touches (clipboard read/write, change detection,
  UI, hotkey, settings, core call).
- Any new OS permission/entitlement the shell now requires — that is a **posture
  change** (justify it; update [macos-posture](macos-posture.md) and
  [privacy-and-data-handling](privacy-and-data-handling.md)).
- For a new platform: confirmation that it links the core unchanged (no core/ABI
  edits) and implements every checklist item.
