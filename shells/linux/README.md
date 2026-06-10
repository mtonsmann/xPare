# xPare — Linux shell (reserved)

This directory is a **reserved sibling**. There is no Linux shell yet; this note
records what adding one entails. The macOS shell (`shells/macos`) is the
reference implementation.

## Adding this platform = implement the shell contract

A platform shell owns all OS integration and calls the Rust core through the
**same frozen C ABI** (`core-ffi/include/xpare.h`). **Zero core changes
are required** — feature selection crosses the boundary as a JSON config string,
so a new transform is never an ABI or shell-contract change.

Implement the full per-platform checklist (see
`docs/guardrails/shell-contract.md`):

- **Clipboard read/write, including rich → plain extraction.** Talk to the X11
  selection (`CLIPBOARD`) or Wayland clipboard. Read the best available target;
  prefer `text/html` and hand it to the core's `strip_html`, else RTF→plain,
  else `UTF8_STRING`/`text/plain`. Write the result back **in place** — no paste
  simulation.
- **Change detection / continuous mode.** Observe selection-owner changes
  (e.g. `XFixesSelectionNotify`, or the Wayland data-device offers). Continuous
  mode is opt-in and off by default; when disabled, no watcher/timer remains
  active.
- **Tray UI.** A system-tray indicator (e.g. StatusNotifierItem / AppIndicator)
  with a menu: mode toggle, operation toggles, "strip now", quit.
- **Global hotkey.** Register a system-wide shortcut via the desktop environment
  / portal mechanism appropriate to the session (X11 grab or the
  `org.freedesktop.portal.GlobalShortcuts` portal on Wayland).
- **Settings.** Persist preferences (mode, ordered operations, hotkey, poll
  interval) locally. **Never** persist clipboard content.
- **Call the core via the C ABI.** Link the core and call `xp_transform` /
  `xp_buffer_free` / `xp_abi_version` / `xp_capabilities_json`. Build the
  config JSON to the schema in `core/src/config.rs`.

## Linking the core

Build the FFI crate and link the resulting library:

```sh
cargo build -p xpare-ffi --release
# -> target/release/libxpare_ffi.a  (static)
#    target/release/libxpare_ffi.so  (dynamic)
```

## Posture (must hold on every platform)

No network anywhere. No persistence or logging of clipboard content; in-memory
only, and the core zeroizes returned buffers in `xp_buffer_free`. Output is
deterministic for a given `(input, config)`. Keep transform logic in the core,
not the shell.
