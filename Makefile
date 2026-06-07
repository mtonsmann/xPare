# SafetyStrip — convenience wrapper over the canonical commands.
#
# Every target DELEGATES to `cargo`, `cargo xtask`, or the shell scripts so there
# is never a second source of truth: `make ci` is exactly `cargo xtask ci` — the
# same gate CI runs — and `make app` just calls shells/macos/package-app.sh.
# This is ergonomics, not a build system; xtask remains authoritative.
#
# Run `make help` for the list.

CARGO ?= cargo
.DEFAULT_GOAL := help

# Throughput harness knobs (see core/tests/throughput.rs and docs/performance.md).
# PERF_MIN_MIB_PER_SEC is empty by default (report-only); set it only on a
# calibrated machine to turn the end-to-end scenarios into a hard floor.
PERF_MIB ?= 64
PERF_SAMPLES ?= 3
PERF_MIN_MIB_PER_SEC ?=
FUZZ_SMOKE_SECONDS ?= 30
FUZZ_HOURS ?= 8
FUZZ_TARGETS ?=

# Release packaging (see shells/macos/release.sh and docs/release-model.md).
# dist/github-release are gated and need Developer ID credentials + a vX.Y.Z tag.
# dist signs with shells/macos/SafetyStrip.entitlements by default. SIGN_ENTITLEMENTS
# exists so CI can pass the checked file as an absolute path; other paths are rejected.
VERSION ?=
CERT_NAME ?=
NOTARY_PROFILE ?=
SIGN_ENTITLEMENTS ?=

.PHONY: help build test lint fmt fmt-check ci checks supply-chain lint-actions lint-shell header bench bench-large perf fuzz fuzz-smoke fuzz-overnight zizmor app run preview dist github-release clean clean-release

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | \
		awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-11s\033[0m %s\n", $$1, $$2}'

build: ## Build the whole workspace (debug)
	$(CARGO) build --workspace

test: ## Run all workspace tests
	$(CARGO) test --workspace

lint: ## clippy across all targets, warnings denied
	$(CARGO) clippy --workspace --all-targets -- -D warnings

fmt: ## Format the whole workspace
	$(CARGO) fmt --all

fmt-check: ## Check formatting without changing files
	$(CARGO) fmt --all --check

ci: ## Full gate: fmt + clippy + tests + every invariant (identical to CI)
	$(CARGO) run -p xtask -- ci

checks: ## Run only the structural invariant checks (no build/test)
	$(CARGO) run -p xtask -- check-abi
	$(CARGO) run -p xtask -- check-unsafe-forbid
	$(CARGO) run -p xtask -- check-core-deps
	$(CARGO) run -p xtask -- check-no-network
	$(CARGO) run -p xtask -- check-entitlements
	$(CARGO) run -p xtask -- check-no-content-logging
	$(CARGO) run -p xtask -- check-pipeline-zeroization
	$(CARGO) run -p xtask -- check-clipboard-safety
	$(CARGO) run -p xtask -- check-c-ffi-surface
	$(CARGO) run -p xtask -- check-release-posture

supply-chain: ## cargo-deny: RustSec advisories + license allowlist + bans + sources
	$(CARGO) run -p xtask -- check-supply-chain

lint-actions: ## Lint workflows: actionlint (correctness) + zizmor (security)
	$(CARGO) run -p xtask -- check-workflows

lint-shell: ## shellcheck the shell scripts (build/release plumbing)
	$(CARGO) run -p xtask -- check-shell

header: ## Regenerate the frozen C ABI header
	$(CARGO) run -p xtask -- gen-header

bench: ## Run the quick (clipboard-scale) benchmarks
	$(CARGO) bench -p safetystrip-core --bench transform

bench-large: ## Run the heavy log-file benchmarks up to 256 MB (slow)
	$(CARGO) bench -p safetystrip-core --bench transform_large

perf: ## Throughput baseline (PERF_MIB=128 PERF_SAMPLES=7 [PERF_MIN_MIB_PER_SEC=N])
	SS_PERF_MIB=$(PERF_MIB) SS_PERF_SAMPLES=$(PERF_SAMPLES) SS_PERF_MIN_MIB_PER_SEC=$(PERF_MIN_MIB_PER_SEC) \
		$(CARGO) test -p safetystrip-core --release --test throughput -- --ignored --nocapture

fuzz: ## Build all fuzz targets (auto-installs nightly/cargo-fuzz if needed)
	$(CARGO) run -p xtask -- check-fuzz

fuzz-smoke: ## Build and briefly run all fuzz targets (FUZZ_SMOKE_SECONDS=30)
	SS_FUZZ_SMOKE_SECONDS=$(FUZZ_SMOKE_SECONDS) $(CARGO) run -p xtask -- check-fuzz

fuzz-overnight: ## Resource-sized local fuzz run across all targets (FUZZ_HOURS=8 [FUZZ_TARGETS=...])
	scripts/overnight-fuzz.sh $(FUZZ_HOURS) $(FUZZ_TARGETS)

zizmor: ## Audit the GitHub Actions config (workflows + dependabot) for security (needs zizmor)
	zizmor .github/

app: ## Build the macOS menu-bar .app bundle (dist/SafetyStrip.app)
	cd shells/macos && ./package-app.sh

run: ## Build and launch the macOS menu-bar app
	cd shells/macos && ./package-app.sh --run

preview: ## Unsigned/ad-hoc preview zip + checksum under dist/release (VERSION optional)
	cd shells/macos && VERSION="$(VERSION)" ./release.sh preview

dist: ## Gated Developer ID sign+notarize+staple release (needs CERT_NAME; uses checked entitlements)
	cd shells/macos && VERSION="$(VERSION)" CERT_NAME="$(CERT_NAME)" NOTARY_PROFILE="$(NOTARY_PROFILE)" SIGN_ENTITLEMENTS="$(SIGN_ENTITLEMENTS)" ./release.sh dist

github-release: ## Upload the signed release zip + checksum via gh (needs VERSION)
	cd shells/macos && VERSION="$(VERSION)" ./release.sh github-release

clean: ## Remove build artifacts (workspace, fuzz, macOS, release)
	$(CARGO) clean
	rm -rf shells/macos/.build shells/macos/dist fuzz/target dist/release

clean-release: ## Remove only staged release artifacts (dist/release)
	rm -rf dist/release
