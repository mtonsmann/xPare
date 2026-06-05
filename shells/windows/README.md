# SafetyStrip — Windows shell (reserved)

This directory is a **reserved sibling**. There is no Windows shell yet; this
note records what adding one entails. The macOS shell (`shells/macos`) is the
reference implementation.

## Adding this platform = implement the shell contract

A platform shell owns all OS integration and calls the Rust core through the
**same frozen C ABI** (`core-ffi/include/safetystrip.h`). **Zero core changes
are required** — feature selection crosses the boundary as a JSON config string,
so a new transform is never an ABI or shell-contract change.

Implement the full per-platform checklist (see
`docs/guardrails/shell-contract.md`):

- **Clipboard read/write, including rich → plain extraction.** Read the best
  available representation; prefer HTML (`CF_HTML`) and hand it to the core's
  `strip_html`, else RTF→plain, else plain Unicode text (`CF_UNICODETEXT`).
  Write the result back **in place** — no paste simulation.
- **Change detection / continuous mode.** Watch for clipboard changes
  (e.g. `AddClipboardFormatListener` / `WM_CLIPBOARDUPDATE`, or polling
  `GetClipboardSequenceNumber`). Continuous mode is opt-in and off by default;
  when disabled, no listener/timer remains active.
- **Tray UI.** A notification-area (system tray) icon with a menu: mode toggle,
  operation toggles, "strip now", quit.
- **Global hotkey.** Register a system-wide hotkey (e.g. `RegisterHotKey`) using
  a mechanism that does not require elevated or invasive input permissions.
- **Settings.** Persist preferences (mode, ordered operations, hotkey, poll
  interval) locally. **Never** persist clipboard content.
- **Call the core via the C ABI.** Link the core and call `ss_transform` /
  `ss_buffer_free` / `ss_abi_version` / `ss_capabilities_json`. Build the
  config JSON to the schema in `core/src/config.rs`.

## Linking the core

Build the FFI crate for the Windows target and link the resulting library:

```sh
cargo build -p safetystrip-ffi --release --target x86_64-pc-windows-msvc
# -> target/x86_64-pc-windows-msvc/release/safetystrip_ffi.lib (static)
#    or the matching .dll for dynamic linking
```

## Posture (must hold on every platform)

No network anywhere. No persistence or logging of clipboard content; in-memory
only, and the core zeroizes returned buffers in `ss_buffer_free`. Output is
deterministic for a given `(input, config)`. Keep transform logic in the core,
not the shell.
