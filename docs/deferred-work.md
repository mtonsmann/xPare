# Deferred work

Feature- and task-level work that was consciously deferred from **completed** exec
plans, collected here so it survives the plan being archived to
[`exec-plans/completed/`](exec-plans/completed/). Each item links back to the plan
that deferred it.

Scope split:

- **This file** is the tactical backlog — concrete features/improvements punted from
  a specific plan.
- **Strategic / architectural** deferrals (extra platform shells, a streaming
  transform path, signing/notarization automation, telemetry, doc-GC agents) live in
  [`DESIGN.md` → "Adopt if the project grows"](../DESIGN.md#adopt-if-the-project-grows).
- **In-progress** work and its not-yet-done items stay in the active plans under
  [`exec-plans/active/`](exec-plans/active/).

Nothing here is committed scope; it's a memory aid for the next maintainer.

## From exec-plan 0001 — initial re-architecture

- **Block-level embedded-HTML stripping in `strip_markdown`.** Accumulate consecutive
  raw-HTML *block* events and strip them as one unit, dropping block-level
  `<script>`/`<style>` in embedded HTML. Deferred because `StripHtml` already covers
  the security need and is the path the shell runs. (0001 → Follow-ups)
- **Nightly fuzz campaigns.** CI runs only a smoke `cargo fuzz` pass today; run longer
  campaigns as a scheduled job. (0001 → Follow-ups)
- **Package + run the real menu-bar `.app`.** Build and launch the signed/`LSUIElement`
  app on a full Xcode toolchain (this environment is Command-Line-Tools only, so the
  app is built but never launched). (0001 → Follow-ups)

## From exec-plan 0012 — AI-native, evidence-first workflow + reference semantics

- ~~**Bounded proof harness (Kani) over the resource envelope.**~~ Delivered: the
  saturating growth product is factored into `config::saturating_growth_product`, with
  `#[cfg(kani)]` harnesses that prove the gate accepts a pipeline iff its true
  worst-case growth is within `MAX_PIPELINE_GROWTH_FACTOR` (no saturation wrap can
  falsely accept). Run via `cargo xtask check-kani`; advisory CI cadence in
  `.github/workflows/proofs.yml`. The operation-count bound and canonical-rank
  total-order are covered by plain unit tests in `config.rs` (Kani adds nothing for
  constant data). The full text transformer is intentionally **not** Kani-proved.
  (0012 → Phase 3)

## From exec-plan 0004 — extraction, defang/refang, URL cleaning

- ~~**In-menu sort-flag submenu.**~~ Done — sort is a single "Sort lines: <mode>"
  submenu: one entry whose title shows the active mode, with the modes (Off / A→Z /
  Z→A / ±ignore-case) as an inline `Picker` so the active one gets the system ✓ (the
  native Finder "Sort By" idiom). Moved out of the Settings window so each control has
  one home.
- ~~**Drag-to-reorder pipeline.**~~ Delivered by
  [`0005-canonical-pipeline-ordering.md`](exec-plans/completed/0005-canonical-pipeline-ordering.md):
  the pipeline runs in a correct/efficient canonical order by default, and
  the Settings window's "Manual order" mode provides drag-to-reorder for exact control.
- ~~**Measured throughput for `defang` / `clean_urls`.**~~ Done — the throughput
  harness (`make perf`) now measures the new ops alongside the existing pipeline.

## From exec-plan 0013 — anti-slop code & test hygiene

- **`cargo-public-api` snapshot gate.** Freeze `core`'s public API surface and diff it in
  CI (like the frozen C header). Deferred because `cargo-public-api` needs a pinned
  *nightly* rustdoc and its output drifts across toolchains — a brittle, nightly-dependent
  required gate fights the repo's determinism ethos, and the concern (dangling `pub`
  surface) is already covered by `unreachable_pub` + `dead_code` + the frozen FFI
  `check-abi` + mutation testing. (0013 → D-2)
- **`lychee` markdown link-checking.** Catch dead links in `docs/`. Deferred because
  external URLs flake (network-dependent, non-deterministic); only internal/relative-link
  checking would be worth a gate, and its value is low next to `check-docs` (which already
  catches broken intra-doc links in Rust). (0013 → D-4)
- **Mutation-testing parallelism tuning.** `check-mutants` runs local at `-j <cores>`
  ("hammer the box"); the first full-tree run at `-j 10` produced contention-spurious
  timeouts. Mitigated by the `timeout_multiplier = 5` / `minimum_test_timeout = 60` in
  `.cargo/mutants.toml` (subsequent `-j 6` runs were clean). If `-j <cores>` ever
  spurious-times-out again, cap per-job test threads (so jobs × test-threads ≈ cores)
  rather than lowering `-j`. (0013 → D-5)
- **Hold the enforcement code (`xtask`) to the product's test bar.** The tier-2 review
  (D-6) found that the `xtask` checks ship with far less test coverage than the product —
  `xtask/**` is excluded from both `check-coverage` and `check-mutants` ("verified by being
  run in CI"), but "run in CI" exercises only the happy path, not the failure/parsing
  branches where a false-green hides. The highest-risk parser (`classify_ignore_line`) now
  has a unit test; the broader gap remains. Consider a scoped mutation/coverage pass over
  `xtask` (or at least unit tests for each check's failure branch). (0013 → D-6 review finding)
- ~~**Anti-slop parity for the Swift macOS shell.**~~ Largely delivered via
  `cargo xtask check-swift` (`make swift`): a **best-effort, macOS-only** tier that fronts
  `swift format lint --strict` (config in `shells/macos/.swift-format`), a `cargo build -p
  xpare-ffi --release` + `swift test`, and a Sources-only line-coverage floor via
  `llvm-cov` (`SWIFT_COVERAGE_FLOOR_PCT`; the floor and measured baseline are documented
  in `shells/macos/README.md`). It runs in the `continue-on-error` `macos-shell` CI job (replacing the
  old `swift build` smoke), so the shell's tests now run in CI rather than only locally, and
  skips cleanly where the Swift toolchain is absent. The Swift sources were normalized once
  with `swift format` so the strict lint passes; the coverage floor is a ratchet (raise,
  never lower). The OS-facing layers are tested headlessly — `SystemPasteboard` against an
  app-private `NSPasteboard(name:)`, the Carbon hot-key trampoline via a synthesized
  `kEventHotKeyPressed` event — leaving only the `XPareApp` SwiftUI executable
  unmeasured (it isn't linked into the test bundle; the analog of the Rust binary crates the
  workspace floor doesn't gate).
  SwiftLint (style/complexity, config in `shells/macos/.swiftlint.yml`) is wired as a
  **run-if-present** phase (non-`--strict`: warnings advise, `error`-severity fails) and CI
  installs it **SHA-pinned + checksum-verified** (the `portable_swiftlint.zip` release asset,
  hashed exactly like actionlint), so it runs in CI too — not just locally. **Still
  deferred:** `periphery` (dead code) — its binary is equally pinnable, but `periphery scan`
  needs a compiler **index store** (it drives a `swift build` first) and a curated retain-list
  config to avoid false positives on the SwiftUI/`@main` surface, neither of which is worth
  wiring until the Swift surface grows; add it (also run-if-present) then. The cross-language
  *security* posture was already enforced (`check-no-content-logging` /
  `check-clipboard-safety` scan `.swift`; `check-c-ffi-surface`; entitlements).
  (0013 → cross-language follow-up)

## From exec-plan 0011 — config resource envelope

- **Budgeted / fallible `transform`.** A
  `transform(input, config) -> Result<_, _>` with an output budget would be the
  strongest arbitrary-config defense, but it is a larger core/FFI contract change
  (and, post-1.0, a major-version ABI event). The accepted-config envelope blocks
  the amplification class for product-shaped configs; a fallible transform remains
  future hardening if arbitrary untrusted configs become in-scope. (0011 → D-2)
- **Shell mirror of the envelope limits.** Settings now validates empty op
  parameters inline; mirroring the core's envelope limits (param byte length,
  no-newline rule) as immediate UI feedback remains optional polish — the core
  stays authoritative either way. (0011 → D-4)

## From exec-plan 0003 — macOS release plumbing

Distribution-channel follow-ups, deliberately out of 1.0 scope (releases at 1.0 are
a notarized arm64-only `.app` zip on GitHub Releases — see
[`release-model.md`](release-model.md)):

- **DMG packaging.** Ship a `.dmg` alongside (or instead of) the zip. The zip is
  sufficient for Gatekeeper-stapled distribution; a DMG adds drag-to-Applications
  ergonomics and background art but also `create-dmg`-style tooling and its own
  signing/notarization step. (0003 → channels beyond zip)
- **x86_64 / universal binary.** 1.0 ships arm64-only. Add an x86_64 slice (or a
  `lipo` universal binary) only on demonstrated demand — it doubles build/notarize
  time and the Intel install base for a new utility is shrinking. Asset names
  already encode the architecture so this is additive. (0003 → follow-up)
- **Homebrew: personal tap, then `homebrew/cask` submission.** Start with a
  personal tap (`brew tap mtonsmann/xpare`) once notarized releases exist; casks
  require a notarized app, and unsigned casks are being removed from
  `homebrew/cask` by September 2026. A `homebrew/cask` self-submission also has a
  notability bar (~225 GitHub stars at time of writing), so the personal tap is the
  realistic first step. (0003 → follow-up)
- **Verify Gatekeeper acceptance on a clean macOS machine** after the first real
  notarized release — the signing path is fail-closed and mechanically checked,
  but it has never run end-to-end with real Developer ID credentials.
  (0003 → follow-up)
- **Mac App Store distribution.** A different signing/review/entitlement world
  (and the sandbox story would need re-review under App Store rules). Revisit only
  if GitHub-Release distribution proves insufficient. (0003 → channels)

## From the 1.0 release-prep review (2026-06)

Items consciously deferred while driving to 1.0.0:

- **Update-channel revisit.** xPare has **no auto-update by design** — Sparkle 2
  (the standard macOS updater) would add a network call and an appcast fetch,
  which the no-network posture rejects; updates are manual via GitHub Releases
  (see [`release-model.md`](release-model.md) → "Updates"). Revisit only with a
  design that preserves no-network (e.g. user-initiated check only), and treat it
  as a posture change.
- **Reproducible builds.** A bit-for-bit reproducible bundle would strengthen the
  provenance story beyond the build attestation. Needs pinned toolchains, stable
  zip timestamps, and codesigning determinism analysis — a project of its own.
- **crates.io publication of `xpare-core`.** The core is `publish = false` today;
  publication is deferred until the public-API surface is worth freezing for
  external consumers (semver then applies to the Rust API too, not just the C
  ABI/config/CLI surface).
- **README screenshot / demo GIF assets.** The README ships textual; record a
  short menu-bar demo and a Settings screenshot once the 1.0 UI is final, so the
  assets don't churn.
- **`cargo-auditable`.** Embed the dependency list in released binaries so
  `cargo audit bin` can scan shipped artifacts. Cheap to add to the release build;
  do it together with the next release-plumbing pass.
- **OpenSSF Best Practices badge application.** Scorecard already runs in CI; the
  Best Practices (formerly CII) badge is a questionnaire-driven complement worth
  filing once 1.0 is out and the docs stabilize.
- **Oversized-rich-clipboard plain-text fallback.** When rich clipboard content
  exceeds the size ceiling, xPare refuses and leaves it untouched rather than
  falling back to the (smaller) plain-text representation. The refusal is
  **deliberate**: silently transforming a different representation than the user
  sees invites surprising data loss, and reading an alternate representation of
  content we refused to read widens the touched surface. Revisit only with a
  privacy argument, not a convenience one.
- **End-to-end test for hotkey registration *failure*.** The success path, the
  state/callback funnel, and the handler-install result are tested, but driving
  `RegisterEventHotKey`/`InstallEventHandler` failure headlessly would require
  injecting a fake `HotkeyManager` factory into `StripController`, widening its
  API. Revisit if the registration code grows. (release-prep shell pass)
- **Automated tests for launch-at-login and the recorder/launch-at-login UI.**
  These live in the `XPareApp` executable target (no test target), and
  `SMAppService` talks to the real OS login-items service; the testable
  conversion logic (NSEvent flags → Carbon mask, display strings) is covered in
  `XPareKit`. (release-prep shell pass)
- **Nightly fuzz pass over the zeroization-hardened parsers.** The wipe-on-grow
  rework touched `html_to_markdown`'s hand-rolled parser without an accompanying
  nightly fuzz smoke (property suites are green). The ≥ 10 min/target Release Fuzz
  run required for the `v1.0.0-rc.1` tag covers this; no separate action needed
  unless that gate is skipped. (release-prep core pass)
