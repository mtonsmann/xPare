# Guardrail: macOS posture

**When to consult:** anything in `shells/macos/` that touches the sandbox,
entitlements, the global hotkey, the pasteboard, or the continuous-mode poller.
Pair with [shell-contract](shell-contract.md) and
[privacy-and-data-handling](privacy-and-data-handling.md).

The macOS shell runs with the **least privilege a clipboard utility can**. A
clipboard tool asking for broad permissions is exactly the kind of thing a user
should distrust, so the posture is deliberately minimal and is checked mechanically.

## The rules

### Sandbox & Hardened Runtime

1. **App Sandbox is on.** The entitlements file must contain
   `com.apple.security.app-sandbox` set to `<true/>`.
2. **Hardened Runtime is on** for the shipped app.
3. **The entitlements file is minimal: ONLY `app-sandbox = true`.** Reading and
   writing the pasteboard needs **no** entitlement, so none is requested. The
   `check-entitlements` task rejects any extra entitlement key; the following
   classes are called out because they are especially dangerous:
   - any `com.apple.security.network.*` (no network — clipboard data must never be
     exfiltratable),
   - any `com.apple.security.device.*` (camera, mic, USB, input-monitoring, …),
   - any `com.apple.security.personal-information.*` (address book, calendar, …),
   - any `com.apple.security.files.*` (broad file access),
   - `com.apple.security.automation.apple-events` (no scripting other apps),
   - the code-signing-weakening entitlements
     (`cs.disable-library-validation`, `cs.allow-unsigned-executable-memory`,
     `cs.allow-dyld-environment-variables`),
   - anything Accessibility / input-monitoring / post-event related, however
     namespaced.

   The checked-in entitlements file lives at
   `shells/macos/xPare.entitlements` (the path `check-entitlements` reads).
4. **Official Developer ID releases must use that entitlements file.** Unsigned or
   ad-hoc preview builds are not official binaries. `release.sh dist` defaults to
   `shells/macos/xPare.entitlements`, rejects alternate resolved paths, signs
   the executable and bundle with it, and verifies both signed payloads with
   `codesign -d --entitlements :-`. `cargo xtask check-release-posture`
   mechanically checks that the release script still has those fail-closed guards.

### Hotkey

5. **Use Carbon `RegisterEventHotKey`** for the global hotkey (default **⌥⌘V**).
   **Do not** use `CGEventTap` or a global `NSEvent` monitor: those require the
   Accessibility or Input Monitoring TCC grants. `RegisterEventHotKey` registers one
   specific chord and needs neither, which is the whole point.

### Pasteboard

6. **In-place rewrite only.** Read `NSPasteboard.general`, extract the best text
   representation (prefer the HTML rep → core `StripHtml`), transform via the core,
   and write the result back to the same pasteboard. **Never** simulate a paste
   (synthesizing Cmd-V) — that needs Accessibility and can target the wrong app.
7. **No persistence or logging of pasteboard content** — see
   [privacy-and-data-handling](privacy-and-data-handling.md); the single sanctioned
   exception is the opt-in paste-as-file store (`PasteFileStore`, rule 2 there).
   Free the core's output buffer with `ss_buffer_free` (it is zeroized on free).
8. **Refuse oversized clipboards gracefully.** Before handing pasteboard text to the
   core, check it against a RAM-proportional ceiling
   (`StripController.defaultMaxInputBytes()` = `min(SS_MAX_INPUT_BYTES,
   physicalMemory / 10)`). A larger clipboard yields a content-free "too large"
   outcome and is **left untouched** — never risk an out-of-memory abort on a huge
   paste. This mirrors the OS clipboard's own memory-bound nature; the core's
   `SS_MAX_INPUT_BYTES` (ABI v2) is the hard backstop beneath it. See `DESIGN.md`
   → *Performance & large inputs → Input size ceiling*.
9. **Treat local pasteboard writers as a race/DoS boundary, not a confidentiality
   boundary.** Another same-user process can write the general pasteboard before a
   read, during a transform, or after the in-place rewrite. xPare must still
   avoid logging/persistence/exfiltration and must bound each transform, but it does
   not claim to lock the pasteboard against local writers.

### Continuous mode

10. **Owned poller on `changeCount`, fully torn down when off.** Continuous mode polls
   `NSPasteboard.general.changeCount` on a **500 ms** default interval. When the mode
   is disabled the timer/poller object must be invalidated **and** niled — no loop
   runs when the feature is off. On-demand mode (the default) does no polling at all.
11. **No stronger ordering is implied.** Polling is best-effort; it can miss
   intermediate values if multiple writes happen between ticks and it can race a
   writer before the read or after the rewrite. The shell suppresses xPare
   self-write generations, drops stale transform completions when `changeCount`
   moved in flight, and coalesces callbacks while a strip is running. Those controls
   belong in the shell and must not change the core ABI.

### Responsiveness

12. **Transform off the main thread; indicate only when it's slow.** `stripNow` runs
   the core transform on a background task — the menu-bar UI must never block, even on
   a large clipboard. It is **threshold-gated**: `onStrippingChange(true)` fires only
   if a run outlasts `busyThreshold` (default 400 ms), and `(false)` when it finishes,
   so the instant common case shows nothing and only a multi-second run surfaces a
   "Stripping…" state. The pasteboard read and the in-place write stay on the main
   actor (AppKit is main-affine); only the pure transform is backgrounded. The
   indicator is **indeterminate** by design — the FFI is one opaque call, so an honest
   percentage isn't available without a progress-callback ABI or the deferred
   streaming API.

## Why (short form)

Every avoided permission is a permission the user never has to grant and an attack
surface the app never has. The sandbox with no network entitlement is the OS-level
backstop for the "no exfiltration" promise; refusing Accessibility/Input-Monitoring
keeps xPare from being the kind of input-watching tool it is meant to protect
you from. Full rationale: [`DESIGN.md`](../../DESIGN.md) (D8, D9) and
[`SECURITY.md`](../../SECURITY.md).

## Enforcing checks

- `cargo xtask check-entitlements` — reads `shells/macos/xPare.entitlements`,
  **requires** `app-sandbox = true`, and **fails** on any extra key. A missing file
  is a failure (the entitlements file is a required deliverable). The check is a
  portable XML scan (no `plutil`), so it runs on the Linux CI gate too.
- `cargo xtask check-release-posture` — asserts the official signing path still
  defaults to the checked entitlements file, rejects alternate resolved
  `SIGN_ENTITLEMENTS` paths, signs executable and bundle with that file, and
  verifies both signed entitlement payloads are minimal.
- `shells/macos/release.sh dist` — resolves the same file by default, rejects
  alternate resolved paths, refuses to sign if it is missing, and verifies that the
  signed payload is still minimal after Developer ID signing.
- The macOS shell anti-slop tier (`cargo xtask check-swift`: swift-format lint +
  `swift test` + a Sources coverage floor, plus SwiftLint if present) runs best-effort
  on macOS CI (the `continue-on-error` `macos-shell` job), superseding the old bare
  `swift build` smoke.

## What a PR must call out

- **Any new entitlement** — this is a posture change; justify it, and update this
  guardrail and `SECURITY.md`. (Expect strong resistance: the intended file is
  *only* app-sandbox.)
- Any release-signing path that omits the checked-in entitlements, accepts alternate
  entitlement files, or disables the post-signing entitlement verification.
- Any change to the hotkey registration mechanism (must remain Accessibility-free).
- Any change to pasteboard read/write that could persist, log, or copy content, or
  that introduces paste simulation.
- Any change to the poller's lifecycle (it must stay fully torn down when off).
