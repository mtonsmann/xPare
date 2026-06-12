# Agent task: security / privacy posture change

Prompt template for anything touching clipboard data handling, entitlements,
logging, network, telemetry, or in-memory lifetime / zeroization.

## Files to read

- [`docs/agent-workflow.md`](../agent-workflow.md).
- [`docs/guardrails/privacy-and-data-handling.md`](../guardrails/privacy-and-data-handling.md)
  and [`docs/guardrails/content-logging-and-clipboard-safety.md`](../guardrails/content-logging-and-clipboard-safety.md).
- `SECURITY.md`, `DESIGN.md` (threat model).
- The relevant source: `core/src/pipeline.rs` (zeroization), `core-ffi/src/lib.rs`
  (buffer free / lossy input), `shells/macos/xPare.entitlements`,
  `shells/macos/release.sh`.
- The enforcing `xtask` checks: `check-no-network`, `check-no-content-logging`,
  `check-pipeline-zeroization`, `check-clipboard-safety`, `check-entitlements`,
  `check-release-posture`, `check-swift-no-network-apis`,
  `check-shipped-command-exec`, `check-real-clipboard-tests`,
  `check-pasteboard-write-shape`.

## Hard constraints

- **No network anywhere** — not in any crate, build step, or entitlement.
- **No shipped command execution surface** — process spawning belongs in `xtask` or
  reviewed release shell scripts, not the app/core/CLI/helper surfaces.
- **No clipboard content logged or persisted.** Log fixed operational states only;
  persist user *settings*, never clipboard-derived text.
- **Default tests avoid the real clipboard.** Use fake or named pasteboards unless a
  real-clipboard exercise is behind an explicit opt-in target.
- **In-memory only.** Pipeline intermediates stay in `Zeroizing`; fused scratch is
  wiped before release/growth; `xp_buffer_free` zeroizes the output. Do not weaken
  any of these.
- **Minimal entitlements.** The macOS entitlements file is exactly
  `com.apple.security.app-sandbox = true`. No network/device/personal-info/automation/
  file-access/codesign-weakening/accessibility entitlement.
- Any new entitlement, network-capable dependency/API, subprocess path, data path,
  pasteboard representation, or weakened wipe is a **posture change** — justify it
  in the PR and update `SECURITY.md`.

## Implementation rules

- Prefer the lowest enforcement layer: a structural `xtask` check beats a prose
  promise. If you add a data path, add the check that keeps it from becoming a leak.
- A posture *weakening* is almost always wrong; default to "don't", and if it is
  genuinely required, treat it as a documented, reviewed decision.

## Required tests / checks

- The relevant `xtask` checks above, all passing under `cargo xtask ci`.
- If you added a data path or sink class, add/extend the `xtask` check (and its unit
  tests in `xtask/src/main.rs`) so the class cannot silently return.

## Required evidence

- The full `cargo xtask ci` run.
- An explicit posture statement: network (none), command execution (none in shipped
  app surfaces), content logging (none), default real-clipboard use (none),
  zeroization (preserved), entitlements (still minimal) — or the justified change.

## Proof gaps to report

- Zeroization is best-effort (the OS clipboard and the caller's input buffer are
  outside the core; the allocator may retain freed pages briefly).
- The confidentiality boundary does not cover same-user pasteboard races.
