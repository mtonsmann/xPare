# Exec Plan 0019 — Continuous mode menu-bar icon

Status: **completed** · Started: 2026-06-16 · Completed: 2026-06-16

Based on `main` at `e75da93` on 2026-06-16.

## Goal

Make continuous monitoring visible directly in the macOS menu bar so users can
tell when xPare is armed to modify new clipboard contents without opening the
menu.

## Change Class

Native shell UI + docs.

Compatibility/posture summary:

- **No C ABI change.** The shell still selects behavior through existing settings
  and serialized config.
- **No core transform change.** Clipboard output remains unchanged.
- **No privacy posture change.** No new data path, pasteboard access, logging,
  persistence, network use, or entitlement.
- **No watcher lifecycle change.** Continuous mode still starts/stops through the
  existing controller settings path.

## Correctness Brief

### Intended behavior

When `Settings.mode == .continuous`, the menu-bar extra uses the emphasized
`scissors.circle.fill` SF Symbol and a matching accessibility title. When mode is
on-demand, it uses the existing `scissors` SF Symbol and the plain `xPare` title.
The visual cue communicates that automatic clipboard cleanup is armed, not that a
transform is currently running.

### Must-preserve invariants

- The macOS shell remains the only changed boundary; no transform logic moves into
  Swift.
- The frozen C ABI, core determinism, and supported transform set are untouched.
- No new OS permission, entitlement, network API, persistence, or content logging.
- The menu's existing "Stripping..." row remains the slow-operation indicator.

### New invariants

- The menu-bar icon state is derived solely from persisted shell mode:
  continuous mode maps to `scissors.circle.fill`, on-demand mode maps to
  `scissors`.
- The menu-bar accessibility title names the continuous state when it is armed.

### Threats / bug classes considered

- A color-only cue would be inaccessible; use a shape/fill symbol variant instead.
- A circular-arrow icon could imply sync/network activity; keep the scissors
  identity to avoid weakening the no-network mental model.
- A busy animation would conflate "armed" with "currently transforming"; keep
  busy indication in the existing threshold-gated menu row.
- A new settings field would create migration and persistence surface; derive
  presentation from existing `Settings.mode`.

## Decisions

### D-1 — Use a shape-changing SF Symbol pair

Use `scissors` for on-demand and `scissors.circle.fill` for continuous mode. The
pair preserves xPare's existing identity, remains monochrome/system-rendered, and
does not rely on color alone.

### D-2 — Change the accessibility title with the visual state

Use `xPare` in on-demand mode and `xPare, continuous monitoring on` in continuous
mode so assistive technology exposes the same state that sighted users get from
the icon.

### D-3 — Keep transient work status separate

Do not animate or temporarily replace the menu-bar symbol while stripping. The
existing `Stripping...` menu row is threshold-gated and remains the honest
long-running-work indicator.

## Workstreams

1. Add small derived `AppModel` presentation helpers for the menu-bar title and
   SF Symbol.
2. Wire `XPareApp`'s `MenuBarExtra` initializer to those helpers.
3. Update macOS user docs to describe the two menu-bar icon states.

## Verification Plan

Minimum checks before closing:

```sh
cargo build -p xpare-ffi --release
cargo run -p xtask -- check-swift
cargo run -p xtask -- check-swift-no-network-apis
cargo run -p xtask -- check-entitlements
git diff --check
```

If the full Swift anti-slop tier is blocked by host tooling, run the narrow Swift
build/test path available on the host and record the gap.

## Evidence Packet

Completed on 2026-06-16:

- `cargo build -p xpare-ffi --release` — passed.
- `git diff --check` — passed.
- `cargo run -p xtask -- check-swift` — initial sandboxed run reached
  `swift-format` and the FFI build, then failed when SwiftPM tried to write the
  normal Clang module cache. Rerun with filesystem approval passed:
  `swift-format` clean, 165 Swift tests passed, Sources line coverage 95.71%
  against the 95.0% floor, optional SwiftLint skipped because it is not installed
  locally.
- `cargo run -p xtask -- check-swift-no-network-apis` — passed; no
  network/browser API tokens found.
- `cargo run -p xtask -- check-shipped-command-exec` — passed; no shipped command
  execution surface found.
- `cargo run -p xtask -- check-entitlements` — passed; macOS entitlements remain
  minimal.
- `cargo run -p xtask -- check-no-content-logging` — passed; no clipboard-content
  logging or persistence outside the sanctioned paste-as-file store.

## Proof Gaps

The exact rendered menu-bar appearance is not pixel-verified in this plan; the
implementation uses system SF Symbols available on the target macOS floor and
keeps the change to native `MenuBarExtra` presentation.
