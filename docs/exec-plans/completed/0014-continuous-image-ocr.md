# Continuous Image OCR

## Correctness brief

### Change class

- [x] macOS shell (`shells/macos/*`)
- [x] Security / privacy posture (clipboard image data path now has an opt-in
  continuous-mode policy)

### Intended behavior

Add a persisted macOS shell setting that lets users opt image OCR into continuous
mode. Continuous monitoring still defaults off, and continuous OCR also defaults
off. When both are enabled and a clipboard change has no text-like representation,
the shell reads a bounded image representation, runs local Vision OCR off the main
actor, and rewrites recognized plain text in place if the pasteboard generation is
unchanged. The dropdown menu must show the OCR-continuous setting directly.

### Must-preserve invariants

- Frozen core/FFI ABI; OCR remains shell-owned OS extraction.
- No network, telemetry, content logging, or content persistence.
- No new entitlement; Vision OCR remains local.
- Continuous monitor lifecycle stays owned and fully torn down when off.
- OCR input image bytes and recognized output text stay size-bounded.
- Stale OCR output must not overwrite newer clipboard content.
- Manual "Strip clipboard now" remains text-transform only; the explicit OCR
  command remains available.

### New invariants

- Continuous OCR is opt-in separately from continuous monitoring and defaults off.
- Continuous OCR only runs after the normal text pipeline finds no text-like
  clipboard representation.
- The Settings window owns user configuration, and the dropdown menu exposes the
  same setting state directly.

### Threats / bug classes considered

- Surprising automatic image replacement if enabled by default.
- OCR running for text clipboards that also contain image representations.
- Re-OCR loops after SafetyStrip writes recognized text.
- Expensive Vision work on the main actor.
- Persisting recognized text instead of only the boolean setting.
- Docs/guardrails continuing to describe OCR as explicit-only.

### Test plan

- Extend settings tests for the new persisted boolean default, codable round-trip,
  and tolerant decode of older settings blobs.
- Add controller tests proving continuous mode does not OCR unless the setting is
  enabled, does OCR image-only clipboards when enabled, does not OCR text clipboards,
  and preserves stale-generation protection.
- Compile the Settings window with the persisted toggle and duplicate menu toggle.
- Extend the Swift OCR orchestration performance guard to cover the continuous
  entry path.

### Fuzz / property plan

No core parser or transform changes; no new fuzz target applies.

### Performance plan

Use the existing CI-safe Swift OCR orchestration guard with a fast fake recognizer,
extended to exercise the continuous-mode entry path. This measures SafetyStrip-owned
shell overhead, not Apple's Vision latency.

### Verification / proof plan

Run focused Swift tests, full macOS Swift tests, Rust formatting, relevant xtask
privacy/posture checks, and `git diff --check`.

### Evidence packet

- `git fetch origin --prune` -> pass; merged OCR base is
  `origin/growth-envelope-tightening`.
- `swift test --filter continuousModeOCRsImageOnlyClipboardWhenEnabled` with local
  cache/runtime flags -> pass.
- `swift test --filter continuousImageTextCommandOverheadStaysBoundedWithFastRecognizer`
  with local cache/runtime flags -> pass, 0.003s for 100 synthetic cycles.
- `swift test --filter SettingsTests` with local cache/runtime flags -> pass, 7 tests.
- Full macOS Swift tests with local cache/runtime flags -> pass, 72 tests in 5
  suites. Continuous OCR performance guard reported 0.003s in-suite.
- `cargo build -p safetystrip-ffi --release` -> pass.
- `cargo fmt --all --check` -> pass.
- `cargo run -p xtask -- check-agent-workflow` with fresh target dir -> pass.
- `cargo run -p xtask -- check-no-network` with fresh target dir -> pass.
- `cargo run -p xtask -- check-no-content-logging` with fresh target dir -> pass.
- `cargo run -p xtask -- check-clipboard-safety` with fresh target dir -> pass.
- `cargo run -p xtask -- check-entitlements` with fresh target dir -> pass.
- `cargo test --workspace` with fresh target dir -> pass.
- `cargo clippy --workspace --all-targets -- -D warnings` with fresh target dir ->
  pass.
- `cargo run -p xtask -- ci` with fresh target dir -> all checks passed through
  `check-release-posture`, then `cargo-deny` failed to lock the read-only sandboxed
  advisory DB under `~/.cargo`.
- Escalated rerun of `cargo run -p xtask -- check-supply-chain` with the same fresh
  target dir -> pass (`advisories ok, bans ok, licenses ok, sources ok`).
- `git diff --check` -> pass.

### Proof gaps

- Real Vision OCR latency, OCR accuracy, language handling, and line ordering remain
  Apple Vision behavior and are not benchmarked or proven by these tests.
- The Swift performance guard measures SafetyStrip-owned orchestration around a fast
  fake recognizer, not the OS framework.

## Decision log

- 2026-06-11: Continuous OCR is an additional persisted shell boolean, not a core
  operation and not an ABI change. Keeping it separate from `mode` avoids turning
  all continuous users into automatic OCR users.
- 2026-06-11: The continuous path runs normal text stripping first and falls through
  to OCR only on `.empty`. This preserves text-clipboard behavior and avoids
  replacing mixed text/image clipboards with OCR output.
- 2026-06-11: The Settings window owns configuration, while the dropdown menu
  intentionally duplicates the same setting near "Continuous monitoring" so users
  can see when continuous OCR is enabled without opening settings.
