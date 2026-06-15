# Execution Plan 0017: Recover macOS Image OCR Onto Main

## Classification

- Change class: macOS shell plus privacy/data-handling posture.
- Boundary impact: no core transform change and no FFI/ABI change.
- Posture impact: adds a bounded local image-read path in the macOS shell and uses
  Apple's on-device Vision OCR. No network, no new entitlement, no content logging,
  and no content persistence.

## Problem

The image OCR feature and its continuous-mode follow-up were merged into the stale
`growth-envelope-tightening` branch instead of `main`. That branch is now far behind
`main` and carries unrelated commits, so merging it would reintroduce stale state.

## Invariants

- Image bytes and recognized text stay in process memory only.
- OCR is shell-owned OS integration; the Rust core and C ABI remain unchanged.
- Raw image representations and recognized text are bounded before expensive decode
  or pasteboard write.
- Vision recognition runs off the main actor and stale completions never overwrite
  newer pasteboard generations.
- Continuous OCR is separately opt-in, defaults off, and is visible in menu/settings.
- Continuous mode still honors do-not-process pasteboard markers before content read.

## Implementation Plan

1. Port the final stranded OCR behavior onto current `origin/main`, adapting
   `SafetyStrip*` names to `XPare*` and preserving newer paste-as-file and
   concealed-marker behavior.
2. Add Swift tests for image read bounds, Vision request configuration, OCR command
   behavior, stale-generation handling, continuous-mode opt-in, and Swift-only OCR
   orchestration performance.
3. Update architecture/security/shell docs for the new shell-owned OCR data path.
4. Run Swift-focused checks, the repo gate, and include performance results in the PR.

## Decision Log

- 2026-06-15: Recover by porting onto a fresh main-based branch instead of merging or
  rebasing `growth-envelope-tightening`; the stale branch has unrelated topic commits
  and a large main drift.
- 2026-06-15: Keep OCR output on the plain-string pasteboard rewrite path, not the
  paste-as-file exception; OCR is a bounded command/result path and should not grow
  the sanctioned persistence exception.
- 2026-06-15: Continuous OCR remains a separate setting because image-only clipboards
  are outside the ordinary text pipeline. The menu shows the configured state.

## Validation

- `swift format lint --strict --recursive Sources Tests` — passed.
- `cargo build -p xpare-ffi --release` — passed.
- `swift build` from `shells/macos` — passed.
- `swift test` from `shells/macos` — blocked locally by the environment missing the
  Swift `Testing` module used by the existing test targets.
- `cargo run -p xtask -- ci` — passed through Rust tests and structural checks; the
  final `cargo-deny` step was blocked by the sandboxed RustSec DB lock.
- `cargo deny check` outside the sandbox — passed.
- Temporary SwiftPM release benchmark against `XPareKit` fake-recognizer OCR paths:
  100 manual OCR orchestration iterations in 5.570 ms; 100 continuous OCR
  orchestration iterations in 3.925 ms. This excludes real Vision OCR and measures
  xPare-owned bounded read / detached recognizer / generation check / writeback
  overhead.
