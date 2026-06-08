# Exec Plan 0013 - Anti-slop code & test hygiene

Status: **active** - Started: 2026-06-08

## Goal

Extend the "agents propose; deterministic tools dispose" doctrine to *slop* — the
characteristic decay of heavily AI-generated code: dead/dangling code, unused
dependencies, tangled functions, broken doc links, and coverage-gaming tests. Each kind
of slop becomes a mechanical check, wired into the project as either a **required** cheap
check (in `cargo xtask ci`) or a **best-effort** heavy check (own subcommand + advisory
workflow, like Miri/Kani). The required tier must stay fast (it is `make ci`, run on every
change); heavy/deterministic tools run event-driven, never on a cron, so a stable repo
pays nothing. Ratcheted limits move one way only and live in code (reviewed), like the
dependency allowlists.

## Change Class

Dependencies, CI, and automation; docs. No C ABI change, no transform-output change, no
privacy-posture change. New `xtask` checks and `[workspace.lints]` policy; a small
doc-comment cleanup (broken intra-doc links / invalid doc HTML) with no behavior change.

## Decisions

### D-1 - Centralize lint policy in `[workspace.lints]`

Universal lints live in `[workspace.lints]`; members opt in with `[lints] workspace = true`.
Added `unreachable_pub = "deny"` (the dead-`pub` lever — forces unexported `pub` to
`pub(crate)`, after which `dead_code` flags the unused), `clippy::{todo, unimplemented,
dbg_macro}`, and `clippy::cognitive_complexity` (passes today with headroom; threshold in
`clippy.toml`). The `core`/`core-ffi` `print_*` denies and `core`'s `#![forbid(unsafe_code)]`
stay as source attributes — intentionally not workspace-wide. `too_many_lines` was **not**
enabled: it only flags long table-driven *tests*, which are not the slop we target.

### D-2 - `check-unused-deps` (required), public-API snapshot deferred

`check-unused-deps` runs `cargo-machete` (pinned `CARGO_MACHETE_VERSION`, mirrored into
`ci.yml`) — deterministic, offline, baseline clean — so it is required in `run_ci()`.
**Deferred:** a `cargo-public-api` snapshot gate. It needs a pinned *nightly* rustdoc and
its output drifts across nightlies; a brittle nightly-dependent required gate fights the
repo's determinism ethos, and the concern (dangling `pub` surface) is already covered by
`unreachable_pub` + `dead_code` + the frozen FFI `check-abi` + mutation testing.

### D-3 - `check-test-hygiene` (required), with an in-code ratchet

Every `#[ignore]` must carry a reason; the total count must not exceed `MAX_IGNORED_TESTS`
(currently 2 — the existing perf/throughput opt-ins). The ceiling is a constant in
`xtask/src/main.rs`: raising it is a deliberate, reviewed edit, not a blessable side file.
Assertion *quality* (tests that run but prove nothing) is out of scope here — it is the
job of `check-mutants` (D-5).

### D-4 - `check-docs` (required), `missing_docs` / `lychee` deferred

`check-docs` builds the workspace docs with `RUSTDOCFLAGS=-D warnings` (deterministic,
offline, first-party — no new pinned tool). Landing it required a one-time cleanup of 7
pre-existing doc-slop issues (5 public→private intra-doc links, 2 invalid-HTML-tag
placeholders). **Deferred:** `missing_docs` (23 undocumented public items — a larger
doc-writing effort, worth doing as a focused follow-up) and `lychee` markdown
link-checking (external URLs are inherently flaky; lower value).

### D-5 - Coverage & mutation testing are best-effort and event-driven (NOT cron)

`check-mutants` (`cargo-mutants`) and `check-coverage` (`cargo-llvm-cov`) are heavy but
**deterministic** — re-running them on unchanged code proves nothing new (exactly the
Kani argument in `proofs.yml`). So they are *not* in the required gate and are *not*
scheduled: they run on demand, locally, and event-driven (path-filtered) in
`hygiene.yml`, mirroring `proofs.yml`. A surviving mutant is either dead code or an
under-asserted test; the fix is to strengthen a test, and that assertion becomes a
permanent regression. `SS_DIFF_BASE=<ref>` scopes a run to a diff for fast PR feedback.

### D-6 - Tier-2 agent review is event-driven, not scheduled

The residue that cannot be mechanized (naming, wrong abstraction, architectural drift)
gets an event-driven PR-diff agent review, never a cron — same convergence logic: a quiet
repo should not pay. A recurring finding graduates into a deterministic check (the agent
discovers the *next* `xtask` gate, then retires from that duty). *(Status: pending.)*

## Status

Landed and green (required tier): D-1 (`[workspace.lints]` + `clippy.toml`), D-2
(`check-unused-deps`), D-3 (`check-test-hygiene`), D-4 (`check-docs` + doc cleanup). All
wired into `run_ci()` / `usage()` / module doc / `Makefile` / `ci.yml` lockstep, and
recorded in the `ARCHITECTURE.md` invariants table and `docs/guardrails/code-and-test-hygiene.md`.

Landed (best-effort tier, Phase 4): D-5 — `check-coverage` (cargo-llvm-cov, floor
`COVERAGE_FLOOR_PCT = 95`, product baseline ~95.6%, `xtask` excluded), `check-mutants`
(cargo-mutants, `.cargo/mutants.toml`), and the event-driven `hygiene.yml`. Outside the required
gate, mirroring `proofs.yml` (own subcommands + advisory workflow + `Makefile` targets),
and recorded in the `ARCHITECTURE.md` invariants table and the hygiene guardrail.

Landed (Phase 5): D-6 — the tier-2 agent-review doctrine is documented in
`docs/guardrails/code-and-test-hygiene.md` (`## Tier-2 review (the residue)`), cross-linked
to `review-finding-closure.md`.

Remaining follow-up: under D-5, the one-time **full-tree** mutation baseline sweep
(`SS_DIFF_BASE` unset) and survivor triage — the per-diff path is live and smoke-tested;
the full sweep is a focused pass that strengthens tests for any genuine survivors. Deferred
(tracked in `deferred-work.md`): `cargo-public-api` snapshot, `missing_docs`, `lychee`.

## Decision log

- 2026-06-08: D-1 through D-4 implemented and verified (`cargo xtask ci`-relevant checks
  green; `check-test-hygiene` negative-tested). Deferred public-api/missing_docs/lychee
  with rationale above.
- 2026-06-08: Phase 4 landed — `check-coverage` (cargo-llvm-cov 0.8.7, floor 95%, product
  baseline ~95.6%, `xtask` excluded), `check-mutants` (cargo-mutants 27.1.0, `.cargo/mutants.toml`),
  and the event-driven `hygiene.yml`, all best-effort and outside the required gate.
  Verified by reading: `check-coverage` passes; `hygiene.yml` passes `actionlint` + `zizmor`;
  the `check-mutants` plumbing (incl. `SS_DIFF_BASE` diff scoping) was smoke-tested. Phase 5
  (D-6) tier-2 agent-review doctrine documented in the hygiene guardrail. Remaining: the
  one-time full-tree mutation baseline sweep + survivor triage.
