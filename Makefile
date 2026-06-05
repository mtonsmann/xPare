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

.PHONY: help build test lint fmt fmt-check ci checks header bench bench-large fuzz app run clean

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

header: ## Regenerate the frozen C ABI header
	$(CARGO) run -p xtask -- gen-header

bench: ## Run the quick (clipboard-scale) benchmarks
	$(CARGO) bench -p safetystrip-core --bench transform

bench-large: ## Run the heavy log-file benchmarks up to 256 MB (slow)
	$(CARGO) bench -p safetystrip-core --bench transform_large

fuzz: ## Build the fuzz targets (then: cargo +nightly fuzz run <target>)
	cd fuzz && $(CARGO) +nightly fuzz build

app: ## Build the macOS menu-bar .app bundle (dist/SafetyStrip.app)
	cd shells/macos && ./package-app.sh

run: ## Build and launch the macOS menu-bar app
	cd shells/macos && ./package-app.sh --run

clean: ## Remove build artifacts (workspace, fuzz, macOS)
	$(CARGO) clean
	rm -rf shells/macos/.build shells/macos/dist fuzz/target
