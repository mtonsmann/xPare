# 0012 — AI-native, evidence-first engineering workflow + Cedar-style reference semantics

**Status:** completed

## Goal

Make xPare a repository where an agent cannot submit a plausible patch
without also submitting the evidence needed to trust it. Encode the
verification-guided development loop (correctness brief → reference model /
tests / properties / fuzz → implementation → deterministic gates → evidence
packet) as repo-native docs, and add a Cedar-*style* (not Cedar-the-dependency)
executable reference interpreter that the optimized production pipeline is
differentially tested against.

## Phase 0 — whole-repo review (done before editing)

### Currently enforced invariants (mechanical, via `cargo xtask ci`)

- No `unsafe` in the core (`#![forbid(unsafe_code)]` + `check-unsafe-forbid`).
- Core has no OS/IO/network deps (`check-core-deps` allowlist).
- No network-capable crate anywhere (`check-no-network` banlist).
- Frozen C ABI (checked-in header + `check-abi` drift fail).
- Config is data, not ABI (serde round-trip + version tests).
- Never panics on input (fuzz + proptest + adversarial corpus).
- No log sink in the core (`deny(print*/dbg!)`); no content logging/persistence
  (`check-no-content-logging`).
- Pipeline intermediates + fused scratch wiped (`Zeroizing` + `check-pipeline-zeroization`
  + `xp_buffer_free` zeroizes output).
- Deterministic output (property test).
- Minimal macOS entitlements + release posture (`check-entitlements`, `check-release-posture`).
- Supply chain / workflow / shell lint (`cargo-deny`, `actionlint`+`zizmor`, `shellcheck`).

### Most load-bearing correctness/security surfaces

1. `pipeline::transform` — determinism, canonical ordering = explicitly sorted
   `as_given`, and the five fused fast paths being byte-for-byte equal to
   sequential application.
2. `config::parse_config` / `Config::validate` — version gate, op-count cap,
   free-text param bounds, single-line params, saturating multiplicative growth
   envelope.
3. Sanitizers — `strip_html` (the script-neutralizing workhorse), `html_to_markdown`
   (unsafe-scheme dropping, backtick-fence escaping), defang/refang, `clean_urls`,
   `mask_identifiers`.
4. FFI boundary — pointer validation, lossy UTF-8, config-parse failure, oversized
   input rejection before alloc, panic containment, buffer ownership, zeroize-on-free.

### What this pass changes

- **Phase 1:** add `docs/agent-workflow.md`, `docs/templates/correctness-brief.md`,
  five `docs/agent-tasks/*.md` prompt templates, and `.github/pull_request_template.md`;
  add routing links to `AGENTS.md`.
- **Phase 2:** add `core/tests/reference_transform.rs` — a self-contained reference
  interpreter (resolves ordering, applies ops one at a time, never fused, never calls
  production `transform`) and differential properties: production == reference,
  canonical == reference-presorted as_given, determinism, and explicit configs that
  trigger every fusion in `pipeline.rs`.
- **Phase 3:** add a growth-envelope property (every accepted config keeps
  `out.len() <= in.len() * MAX_PIPELINE_GROWTH_FACTOR`).
- **Phase 5:** add the one missing FFI test — `xp_buffer_free(null, len)` is a no-op.
- **Phase 6:** add `cargo xtask check-agent-workflow` (the workflow files exist and
  carry required headings) and wire it into `cargo xtask ci`.
- **Phase 7:** doc updates — `AGENTS.md` links, `CONTRIBUTING.md` note, `DESIGN.md`
  decision entry (D14, Cedar-style VGD, why not Cedar), `ARCHITECTURE.md` invariant row.

### What this pass intentionally does NOT change

- **No runtime behavior change.** No edit to `core/src/**` transform logic, the
  pipeline, the config schema, or `core-ffi`. Reference semantics is test-only.
- **No new dependency.** Reference model and properties use the existing dev
  `proptest`; no Cedar, no new crates.
- **No ABI change, no new entitlement, no network, no weakened guardrail.**
- **No redundant Phase 3/4/5 tests.** `determinism.rs` already proves all five
  fusion equivalences and idempotence; `config_roundtrip.rs` already covers the
  version/op-count/param/growth envelope; `abi_roundtrip.rs` already covers the
  null/UTF-8/oversize/config-failure paths and buffer ownership. We add only the
  genuinely-missing reference interpreter, growth-envelope property, and null-free
  no-op test rather than restating existing coverage.

## Decision log

- **Reference interpreter, not a second production path.** The reference lives only
  under `core/tests/`, applies operations strictly one at a time via the public
  `ops::*` free functions, and resolves `Ordering` with the public
  `Operation::canonical_rank`. Differentially testing `transform == reference` makes
  the production fusions provably equivalent to naive sequential application for any
  generated config, and the explicit fusion configs guarantee each fast path is hit.
- **Growth envelope asserted against the public `MAX_PIPELINE_GROWTH_FACTOR`.** The
  per-op factors are private; asserting `out.len() <= in.len() * MAX` is the strongest
  bound expressible with the public surface and is sound because an accepted config's
  factor product is `<= MAX` by construction.

## Proof gaps (carried into the PR)

This is verification-*guided* development, not formal verification. It does not
prove the whole core correct, does not prove sanitizers correct against browser
semantics, does not formally prove FFI memory behavior, and does not add Kani in
this pass (recorded as a future bounded-proof track for the arithmetic envelope).
