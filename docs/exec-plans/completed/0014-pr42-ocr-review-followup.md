# PR 42 OCR Review Follow-Up

## Correctness brief

### Change class

- [x] macOS shell (`shells/macos/*`)

### Intended behavior

Address the unresolved PR 42 OCR review findings without changing the core or
the C ABI. Image OCR rejects oversized decoded dimensions before creating a
`CGImage`, preserves literal OCR candidates by disabling Vision language
correction, honors image orientation metadata, and scans pasteboard image
representations only within a finite oversized-read budget.

### Must-preserve invariants

- Frozen core/FFI ABI; image OCR remains shell-owned OS extraction.
- No network, no telemetry, no content logging, and no content persistence.
- Minimal macOS entitlements; no new OS permission.
- Default tests must avoid `NSPasteboard.general`.
- Image OCR remains shell-owned and bounded before recognition/writeback.

### New invariants

- Vision OCR requires readable image dimensions and refuses images above the
  recognizer's decoded-pixel ceiling before decode.
- Vision language correction is disabled for this literal extraction path.
- Image orientation metadata is passed through to Vision.
- A single too-large pasteboard image representation does not block a later
  bounded representation.
- Repeated too-large pasteboard image representations stop the alternate scan
  before every advertised image type is materialized.

### Threats / bug classes considered

- Decode-time image expansion from highly compressed oversized images.
- Literal token corruption from OCR language correction.
- Sideways/upside-down OCR from dropped EXIF/TIFF orientation metadata.
- False `.tooLarge` outcomes when another advertised image type is safe.
- Unbounded materialization when several advertised image representations are
  all oversized.

### Test plan

- Add unit coverage for Vision recognizer metadata policy, language correction
  setup, and orientation mapping.
- Add controller coverage for decoded-dimension refusal staying a size failure.
- Add a named-pasteboard smoke that verifies `SystemPasteboard.readImage` skips
  an oversized first image representation and returns a later bounded one.
- Add a named-pasteboard regression that verifies repeated oversized image
  representations return `.tooLarge` before reaching a later bounded alternate.
- Update the macOS posture guardrail with the OCR review lessons.

### Fuzz / property plan

No core parser or transform changes; no new fuzz target applies.

### Performance plan

Use the existing Swift OCR orchestration performance guard. The new metadata
checks are constant-time ImageIO property reads before decode.

### Verification / proof plan

Run Swift macOS tests for the changed shell path, formatter, and focused
privacy/posture checks.

### Commands run

```sh
cargo build -p safetystrip-ffi --release
env XDG_CACHE_HOME=/private/tmp/xpare-pr42-cache CLANG_MODULE_CACHE_PATH=/private/tmp/xpare-pr42-cache/clang swift test --disable-sandbox --package-path shells/macos -Xswiftc -F -Xswiftc /Library/Developer/CommandLineTools/Library/Developer/Frameworks -Xlinker -rpath -Xlinker /Library/Developer/CommandLineTools/Library/Developer/Frameworks -Xlinker -rpath -Xlinker /Library/Developer/CommandLineTools/Library/Developer/usr/lib
cargo fmt --all --check
env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-pr42-xtask-target cargo run -p xtask -- check-no-network
env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-pr42-xtask-target cargo run -p xtask -- check-no-content-logging
env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-pr42-xtask-target cargo run -p xtask -- check-clipboard-safety
env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-pr42-xtask-target cargo run -p xtask -- check-entitlements
git diff --check
env XDG_CACHE_HOME=/private/tmp/xpare-pr42-cache CLANG_MODULE_CACHE_PATH=/private/tmp/xpare-pr42-cache/clang swift test --disable-sandbox --package-path shells/macos --filter ImageTextRecognizerTests -Xswiftc -F -Xswiftc /Library/Developer/CommandLineTools/Library/Developer/Frameworks -Xlinker -rpath -Xlinker /Library/Developer/CommandLineTools/Library/Developer/Frameworks -Xlinker -rpath -Xlinker /Library/Developer/CommandLineTools/Library/Developer/usr/lib
env XDG_CACHE_HOME=/private/tmp/xpare-pr42-cache CLANG_MODULE_CACHE_PATH=/private/tmp/xpare-pr42-cache/clang swift test --disable-sandbox --package-path shells/macos --filter systemPasteboardStopsAfterRepeatedOversizedImageRepresentations -Xswiftc -F -Xswiftc /Library/Developer/CommandLineTools/Library/Developer/Frameworks -Xlinker -rpath -Xlinker /Library/Developer/CommandLineTools/Library/Developer/Frameworks -Xlinker -rpath -Xlinker /Library/Developer/CommandLineTools/Library/Developer/usr/lib
cargo fmt --all --check
git diff --check
env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-pr46-xtask-ci cargo run -p xtask -- ci
env CARGO=cargo CARGO_TARGET_DIR=/private/tmp/xpare-pr46-xtask-ci cargo run -p xtask -- check-supply-chain
```

### Evidence packet

- `cargo build -p safetystrip-ffi --release` -> pass.
- `swift test --disable-sandbox --package-path shells/macos ...` with writable
  Swift/Clang caches and CommandLineTools framework/rpath flags -> pass after
  merging `origin/growth-envelope-tightening`, 80 tests in 6 suites. OCR
  orchestration guards reported 0.008s for manual OCR and 0.006s for continuous
  OCR.
- `cargo fmt --all --check` -> pass.
- `cargo run -p xtask -- check-no-network` -> pass.
- `cargo run -p xtask -- check-no-content-logging` -> pass.
- `cargo run -p xtask -- check-clipboard-safety` -> pass.
- `cargo run -p xtask -- check-entitlements` -> pass.
- `git diff --check` -> pass.
- `swift test --filter ImageTextRecognizerTests ...` after raising the decoded
  image cap to 30 MP -> pass, 4 tests in 1 suite.
- `swift test --filter systemPasteboardStopsAfterRepeatedOversizedImageRepresentations
  ...` -> pass, proving repeated oversized image representations stop the
  alternate scan before a later bounded representation.
- `cargo fmt --all --check` -> pass after resolving PR conflicts.
- `git diff --check` -> pass after resolving PR conflicts.
- `cargo run -p xtask -- ci` with fresh target dir -> passed formatting, clippy,
  Rust workspace tests, structural/privacy/ABI/entitlement/release/workflow
  checks, shellcheck, actionlint, and offline zizmor; failed only when sandboxed
  `cargo-deny` could not lock the read-only local advisory DB.
- Escalated `cargo run -p xtask -- check-supply-chain` with the same fresh target
  dir -> pass (`advisories ok, bans ok, licenses ok, sources ok`), with existing
  warnings about unmatched license allowances and duplicate `wit-bindgen`.
- `docs/guardrails/macos-posture.md` updated with the closure lesson for
  decoded-dimension caps, finite alternate representation scans, literal OCR,
  and orientation metadata.

### Proof gaps

Apple Vision's OCR quality and latency remain framework behavior, not proven by
SafetyStrip tests. The named-pasteboard smoke returns early if a headless agent
cannot populate a synthetic pasteboard; fake/controller tests still cover the
size and race behavior.

## Decision log

- 2026-06-11: Keep all fixes in the macOS shell. The findings are about local
  pasteboard/Vision handling and do not require core or ABI changes.
- 2026-06-11: Use a decoded-pixel ceiling rather than a decoded-byte API because
  ImageIO exposes dimensions before decode; map refusal back to the existing
  content-free `.tooLarge` controller outcome.
