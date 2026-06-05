# SafetyStrip — macOS shell

The Swift/SwiftUI native shell for SafetyStrip. It owns all OS integration —
clipboard read/write, rich→plain extraction, change detection, the global
hotkey, settings, and the menu-bar UI — and calls the platform-neutral Rust
**core** through the frozen C ABI (`core-ffi/include/safetystrip.h`). No
transform logic lives here; feature selection crosses the boundary as a JSON
config string.

## Package layout

A SwiftPM package (`Package.swift`, Swift 6 language mode, macOS 14+) with four
library/exe targets plus tests:

| Target            | Kind        | Responsibility |
|-------------------|-------------|----------------|
| `CSafetyStrip`    | C interop   | Exposes the frozen C ABI to Swift via a `module.modulemap`. Does **not** copy the header — `shim.h` `#include`s the single source of truth at `core-ffi/include/safetystrip.h`. |
| `SafetyStripCore` | Swift       | Safe `Transformer` wrapping the ABI (`transform`, `capabilities`, `abiVersion`), plus `TransformConfig`/`Operation`/`CaseKind` as `Codable` types that encode **exactly** the Rust JSON schema. The only target that links the staticlib. |
| `SafetyStripKit`  | Swift, no UI| The testable shell contract: `Pasteboard`, `ClipboardMonitor`, `HotkeyManager`, `Settings`, `StripController`. |
| `SafetyStripApp`  | executable  | SwiftUI `MenuBarExtra` app wiring a `StripController`. |
| `SafetyStripCoreTests`, `SafetyStripKitTests` | test | swift-testing suites (see *Testing* below). |

## Building

The Swift package links a **prebuilt** Rust staticlib, so build the core first:

```sh
# from the repo root
cargo build -p safetystrip-ffi --release      # -> target/release/libsafetystrip_ffi.a
cd shells/macos
swift build                                    # compiles + links the app
```

Or use the helper, which does both (and wires up the test runtime paths):

```sh
cd shells/macos
./build.sh            # build core + swift build
./build.sh test       # build core + swift build + swift test
./build.sh release    # build core + swift build -c release
```

### Link-path assumption

`Package.swift` links the staticlib with:

```swift
.unsafeFlags(["-L../../target/release", "-lsafetystrip_ffi"])
```

`-L../../target/release` is **relative to the package root** (`shells/macos`),
which resolves to the workspace's `target/release`. The build script asserts the
archive exists there before invoking `swift build`. If you build the core to a
different location, override at the command line:

```sh
swift build -Xlinker -L/abs/path/to/dir
```

The staticlib's own system dependencies (`-lSystem -lc -lm`) are provided
automatically by the macOS SDK that SwiftPM already links — no extra system
libraries are declared here.

## Run it on your menu bar

`package-app.sh` assembles a runnable, ad-hoc-signed menu-bar `.app` from the
SwiftPM build — **no full Xcode required**:

```sh
cd shells/macos
./package-app.sh --run        # build core + app, bundle it, sign, and open it
# or build without launching:
./package-app.sh              # -> dist/SafetyStrip.app
open dist/SafetyStrip.app
```

A ✂ **scissors** icon appears in the menu bar (no Dock icon — it's an
`LSUIElement` agent). Using it:

- **Strip on demand (default):** copy some rich text (e.g. a styled web
  selection), then press the global hotkey **⌥⌘V** — the clipboard is rewritten
  in place as clean plain text. The same action is the menu's "Strip clipboard
  now".
- **Default pipeline:** strip HTML → collapse whitespace → trim trailing
  whitespace. Toggle individual operations from the menu.
- **Continuous mode (opt-in):** enable "Continuous monitoring" to strip
  automatically whenever the clipboard changes (500 ms poll). Off by default.
- **Quit:** the menu's "Quit SafetyStrip" (⌘Q).

Notes:

- Ad-hoc signing runs the app **unsandboxed** — the most reliable local test.
  `./package-app.sh --sandbox` signs with the minimal App Sandbox entitlement
  instead; a *distributable* sandboxed build additionally needs a Developer ID
  identity + notarization (see *Packaging & distribution*).
- The global hotkey uses Carbon `RegisterEventHotKey`, so it needs **no**
  Accessibility or Input-Monitoring permission — macOS should not prompt.

## Testing

```sh
./build.sh test
```

The tests use **swift-testing** (`import Testing`), not XCTest — a deliberate
choice forced by the environment (see *What is real vs. stubbed* below). The
suite (30 tests, 5 suites) covers:

- **FFI integration** through the *real linked core*: `strip_html` on
  `"<p>hi  there</p>"` + `collapse_whitespace` → `"hi there"`, ABI version,
  capabilities JSON, error mapping, and a 500-iteration alloc/free loop that
  exercises the `(ptr,len)` buffer protocol and `ss_buffer_free`. Passing these
  proves the static link and buffer ownership work end to end from Swift.
- **`TransformConfig` JSON encoding** matches the exact Rust wire schema for
  sample pipelines (internally-tagged on `op`, snake_case).
- **`ClipboardMonitor` teardown**: `start()` creates a timer; `stop()`
  invalidates *and* nils it, and no further polls fire afterward.
- **`Settings`** Codable + `UserDefaults` round-trip, with graceful fallback to
  defaults on absent/corrupt data.
- **`StripController`** end-to-end: HTML is stripped and written back in place;
  HTML sources force `strip_html` even if unset; unchanged plain text is not
  rewritten.

## Packaging & distribution

`package-app.sh` (above) already produces a runnable, ad-hoc-signed bundle
locally — no full Xcode needed. For a **distributable** app (one other Macs will
run without Gatekeeper friction), reproduce the same structure under full Xcode,
or extend the script, with a real signing identity:

1. **Bundle + Info.plist.** Create an app bundle and set `LSUIElement` (a.k.a.
   "Application is agent") to `true` in `Info.plist` so it runs as a menu-bar
   accessory with no Dock icon or main window. The `MenuBarExtra` scene is the
   entire UI.
2. **Embed entitlements.** Use the checked-in `SafetyStrip.entitlements` as the
   target's *Code Signing Entitlements*. Its contents are intentionally minimal
   (see below) and a CI check (`cargo xtask check-entitlements`) fails on any
   additional key.
3. **Enable App Sandbox.** Turn on *App Sandbox* (the `com.apple.security.app-sandbox`
   entitlement) for the target.
4. **Enable Hardened Runtime with no exceptions.** Hardened Runtime is a
   signing/build setting, *not* an entitlement, so it does not appear in the
   entitlements file. Enable it on the target and add **no** runtime exceptions
   (no JIT, no unsigned-memory, no library-validation disabling, etc.).
5. **Sign + notarize** with a Developer ID for distribution.

## Entitlements — minimal by design

`SafetyStrip.entitlements` contains **only**:

```xml
<key>com.apple.security.app-sandbox</key>
<true/>
```

No network, file, device, Accessibility, or Input-Monitoring entitlements. This
is enforced: `cargo xtask check-entitlements` (and CI) fails if the file is
absent, if app-sandbox is missing/false, or if any banned key appears.

## macOS posture notes

- **In-place pasteboard rewrite only.** `SystemPasteboard.writePlain` does
  `clearContents()` then `setString(_:forType:.string)` — it replaces the
  current item with plain text. There is **no paste simulation** (no synthetic
  ⌘V), which would require Accessibility.
- **Rich → plain extraction** reads the best representation: prefer
  `public.html` (handed to the core's `strip_html`), else RTF flattened to its
  plain attributed-string value, else a plain string.
- **Global hotkey via Carbon.** `HotkeyManager` uses `RegisterEventHotKey` /
  `InstallEventHandler` (default ⌥⌘V). This is the one global-hotkey mechanism
  that needs **neither** Accessibility **nor** Input Monitoring. `CGEventTap`
  and `NSEvent.addGlobalMonitorForEvents` are deliberately *not* used because
  they require those forbidden permissions.
- **Continuous mode is opt-in** and off by default. The poller watches
  `NSPasteboard.general.changeCount` on a `Timer` (500 ms default); turning it
  off fully invalidates and drops the timer so nothing runs.
- **No clipboard content is ever logged or persisted.** `StripController`
  surfaces only content-free outcomes; `Settings` persists preferences only.

## What is real vs. stubbed in this headless environment

**Real (compiles, links, and — where runnable — tested):**

- The entire Swift source: C interop, the safe `Transformer`, the full shell
  contract in `SafetyStripKit`, and the `MenuBarExtra` app. `swift build`
  produces a working linked executable.
- The FFI link against the real Rust staticlib, verified by passing integration
  tests that call the core and round-trip buffers.
- `swift test`: 30 tests green (using swift-testing).

**Adapted to the environment:**

- **Test framework.** Command-Line-Tools ships swift-testing's
  `Testing.framework` but **not** XCTest, so the suites use `import Testing`
  rather than `import XCTest`. `build.sh test` passes the `-F`/`-rpath` flags
  needed to locate `Testing.framework` and `lib_TestingInterop.dylib` at
  runtime; with full Xcode those flags are harmless.

**Produced locally (no full Xcode):**

- A runnable menu-bar `.app` (`LSUIElement`, ad-hoc signed) via `package-app.sh`
  — see *Run it on your menu bar*. It launches as a true accessory app and was
  smoke-tested to start cleanly.

**Out of scope (needs full Xcode + a Developer account):**

- A Developer-ID-signed, **notarized** build for distribution to other Macs, and
  a fully enforced App Sandbox in a shipped artifact — see *Packaging &
  distribution*.
