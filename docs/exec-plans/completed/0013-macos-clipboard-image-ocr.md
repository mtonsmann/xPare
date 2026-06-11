# macOS Clipboard Image OCR

> Follow-up note: `docs/exec-plans/completed/0014-continuous-image-ocr.md` supersedes
> the original explicit-only continuous-mode decision by adding a separate,
> user-enabled continuous image OCR setting. The other safety decisions here still
> apply: local Vision only, bounded image input, bounded recognized output,
> off-main recognition, no ABI change, and stale-generation suppression.

## Correctness brief

### Change class

- [x] macOS shell (`shells/macos/*`)
- [x] Security / privacy posture (new clipboard image data path)

### Intended behavior

Add an explicit macOS menu command that extracts text from an image currently on
the clipboard using Apple's local Vision text-recognition APIs, then rewrites the
same pasteboard in place with the recognized plain text. This is a one-shot
command, not a persistent transform or continuous-mode policy.

### Must-preserve invariants

- Frozen core/FFI ABI; image OCR is shell-owned OS extraction.
- No network, no telemetry, no content logging, and no content persistence.
- Minimal macOS entitlements; Vision OCR must require no new entitlement.
- Default checks must avoid `NSPasteboard.general`.
- Oversized image representations are refused before OCR decode/recognition, and
  oversized recognized text is refused before pasteboard writeback.
- Pasteboard generation races must not let stale OCR output overwrite newer
  clipboard content.

### New invariants

- Original invariant, superseded by 0014: image OCR was explicit-only and
  continuous mode never OCRed image clipboards.
- OCR recognition work runs off the main actor.
- Empty/no-text OCR output is content-free `.notApplicable`, not a pasteboard
  rewrite.

### Threats / bug classes considered

- Accidentally broadening privacy posture with network/device entitlements.
- Running expensive Vision recognition on the UI thread.
- Reading oversized image bytes into OCR and creating memory pressure.
- Overwriting a newer clipboard generation after slow OCR finishes.
- Treating OCR as a core transform and changing the ABI.
- Logging recognized text or storing it in settings/defaults.

### Test plan

- Add `SafetyStripKit` tests with fake pasteboard and fake recognizer coverage for
  success, no-image/not-applicable, oversized image refusal before recognition,
  oversized OCR output refusal, off-main recognition, no rewrite for empty OCR, and
  stale-generation suppression.
- Add a CI-safe Swift performance guard for the OCR command orchestration path
  using a fast fake recognizer. This does not benchmark Apple's Vision framework;
  it catches accidental slow paths in SafetyStrip-owned pasteboard/controller logic.
- Add a named-pasteboard smoke for `SystemPasteboard.readImage` using synthetic
  pasteboard data only, never `NSPasteboard.general`.

### Fuzz / property plan

No core parser or transform changes; no new fuzz target applies.

### Verification / proof plan

Run macOS shell build/tests plus structural privacy/posture checks:

```sh
cargo build -p safetystrip-ffi --release
swift test --package-path shells/macos
cargo fmt --all --check
cargo xtask check-no-network
cargo xtask check-no-content-logging
cargo xtask check-clipboard-safety
cargo xtask check-entitlements
```

Before PR, run `cargo xtask ci` if toolchain/system linters are available.

### Evidence packet

- `git pull --ff-only` -> already up to date.
- `cargo build -p safetystrip-ffi --release` -> pass.
- `env XDG_CACHE_HOME=/Users/marcus/Dev/xPare/.build-cache CLANG_MODULE_CACHE_PATH=/Users/marcus/Dev/xPare/.build-cache/clang swift test --disable-sandbox --package-path shells/macos -Xswiftc -F -Xswiftc /Library/Developer/CommandLineTools/Library/Developer/Frameworks -Xlinker -rpath -Xlinker /Library/Developer/CommandLineTools/Library/Developer/Frameworks -Xlinker -rpath -Xlinker /Library/Developer/CommandLineTools/Library/Developer/usr/lib` -> pass, 66 tests in 5 suites.
- `cargo fmt --all --check` -> pass.
- `env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-xtask-target cargo run -p xtask -- check-no-network` -> pass.
- `env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-xtask-target cargo run -p xtask -- check-no-content-logging` -> pass.
- `env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-xtask-target cargo run -p xtask -- check-clipboard-safety` -> pass.
- `env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-xtask-target cargo run -p xtask -- check-entitlements` -> pass.
- `env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-xtask-target cargo run -p xtask -- check-release-posture` -> pass.
- `env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-xtask-target cargo run -p xtask -- check-unsafe-forbid` -> pass.
- `env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-xtask-target cargo run -p xtask -- check-pipeline-zeroization` -> pass.
- `env CARGO_TARGET_DIR=/private/tmp/xpare-cargo-test cargo test --workspace` -> pass.
- `env CARGO_TARGET_DIR=/private/tmp/xpare-cargo-test cargo clippy --workspace --all-targets -- -D warnings` -> pass.
- `git diff --check` -> pass.

### Proof gaps

- OCR quality, line order, language handling, and true Vision latency are provided
  by Apple's Vision framework and are not proven by SafetyStrip tests.
- The named-pasteboard smoke returns early in headless/sandboxed agents when the
  environment cannot populate a synthetic `NSPasteboard`; fake pasteboard tests
  still enforce the controller's OCR behavior and race/size invariants.

## Decision log

- 2026-06-11: Implement OCR as a one-shot menu command under "Extract / convert"
  instead of automatic `stripNow` behavior. This avoids surprising image
  clipboard rewrites in continuous mode and matches the existing reductions /
  conversions taxonomy.
- 2026-06-11: Keep OCR in the macOS shell. Vision is OS integration; the core
  remains text-in/text-out and the C ABI remains unchanged.
- 2026-06-11: Read bounded image bytes from the pasteboard, then run Vision on a
  detached task. The pasteboard read/write stay main-actor/AppKit-affine.
- 2026-06-11: Also bound recognized OCR text before writing it back, so both input
  image bytes and output text respect the shell ceiling.
- 2026-06-11: Add a Swift-only OCR command overhead guard over the
  SafetyStrip-owned orchestration path. Keep real Vision benchmarking out of the
  required suite because it varies by OS, hardware, languages, and image content.
