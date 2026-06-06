# Exec Plan 0010 — Mask menu submenu

Status: **completed** · Started: 2026-06-06 · Completed: 2026-06-06

Based on `main` at `3f31c90` on 2026-06-06.

## Goal

Keep privacy masking easy to inspect and configure without crowding the macOS
menu. The current menu shows one row per masking target; the target state should
collapse into one "Mask identifiers: <state>" submenu, like `Sort lines`, with
child rows carrying native checkmarks.

## Change Class

Native shell UX + docs.

Compatibility/posture summary:

- **No C ABI change.** Masking still crosses as the existing serialized config op.
- **No core transform change.** Output and canonical ordering are unchanged.
- **No privacy posture change.** No new data path, persistence, logging, network,
  or entitlement.
- **No supported-transform change.** This is presentation only.

## Decisions

### D-1 — One row per feature family when options are bounded

Masking has three bounded boolean targets. Showing them as three sibling menu
rows makes the common Clean section scan worse as more features land. Keep the
top-level menu to feature families, with a compact state summary in the row title
and target toggles in the submenu.

### D-2 — Preserve native checkmark behavior

Use SwiftUI `Menu` plus child `Toggle` rows. This keeps the system checkmarks,
keyboard-driven AppKit menu behavior, and existing persistent settings plumbing.

### D-3 — Document the menu-density rule as a shell presentation contract

`DESIGN.md` D12 already distinguishes persistent toggles from one-shot commands
and sends bounded params to submenus. Tighten that into an explicit top-level menu
design rule so future transform features do not add one sibling row per flag.

## Workstreams

1. Add a derived masking summary label to `AppModel`.
2. Replace the three top-level mask toggles with one `Mask identifiers: …`
   submenu containing the same target toggles.
3. Update `DESIGN.md` D12 with the top-level menu-density rule.

## Verification Plan

Minimum checks before closing:

```sh
swift build --package-path shells/macos
python3 scripts/guardrails.py --repo .
git diff --check
```

If Swift build lacks the Rust staticlib, first run:

```sh
cargo build -p safetystrip-ffi --release
```

## Verification Result

Completed on 2026-06-06:

- `cargo build -p safetystrip-ffi --release`
- `swift build --package-path shells/macos`
- `cargo run -p xtask -- check-no-content-logging`
- `cargo run -p xtask -- check-clipboard-safety`
- `cargo run -p xtask -- check-entitlements`
- `git diff --check`

`python3 scripts/guardrails.py --repo .` from the original plan was not run
because `scripts/guardrails.py` is not present in this checkout. The relevant
current shell/privacy checks were run through `xtask` instead.

## PR Callouts

- Change class: native shell UX + docs.
- Compatibility/posture impact: no C ABI change, no core transform change, no
  privacy posture change, no new entitlement, and no supported-transform change.
