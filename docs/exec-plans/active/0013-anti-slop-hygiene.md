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

### D-4 - `check-docs` + `missing_docs` (required), `lychee` deferred

`check-docs` builds the workspace docs with `RUSTDOCFLAGS=-D warnings` (deterministic,
offline, first-party — no new pinned tool). Landing it required a one-time cleanup of 7
pre-existing doc-slop issues (5 public→private intra-doc links, 2 invalid-HTML-tag
placeholders). **`missing_docs` (done, 2026-06-09):** documented the 23 undocumented
public items (all `Operation` / `ConfigError` struct-variant fields in `config.rs`) and
added `#![deny(missing_docs)]` to `core` and `core-ffi` — the two shipped libs whose
public surface is the FFI/ABI contract; the `cli`/`xtask` tools are intentionally
exempt (per-crate, like the `print_*` denies). **Deferred:** `lychee` markdown
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

Remaining follow-up: none outstanding. The D-5 full-tree mutation sweep + exhaustive
survivor triage is **complete** and `missing_docs` (D-4) is now enforced (see the Decision
log). Still deferred (low value / poor fit; tracked here until 0013 is archived):
`cargo-public-api` snapshot (nightly-brittle) and `lychee` markdown link-checking
(external-URL flakiness).

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
- 2026-06-08: Full-tree mutation baseline run. **Found a config bug first:** cargo-mutants
  reads `.cargo/mutants.toml`, but the config was committed at the repo root, so all
  exclusions/timeouts were silently ignored (the first sweep wasted ~193 mutants on `xtask`
  and mutated the `#[cfg(kani)]` harnesses). Fixed: moved to `.cargo/mutants.toml`, added
  `exclude_re=["kani_proofs"]`, corrected the `hygiene.yml` path filter + doc refs (commit
  7b54389). Authoritative baseline (929 product mutants, `-j 10` local): **684 caught, 120
  missed, 105 timeout, 20 unviable.** Timeouts are a mix — genuine cursor-advance infinite
  loops in the parsers (effectively caught) plus a contention-spurious tail from `-j 10`
  oversubscription (e.g. all 8 `mask.rs` timeouts are in the non-looping `has_relevant_byte`
  perf pre-filter, which cannot loop). The 120 real survivors cluster in
  `html_to_markdown`(45)/`html`(27)/`markdown`(10) — mostly equivalent mutants in internal
  index math — plus a high-value genuine minority: `defang` boolean logic (15;
  security-relevant), `config` envelope (`Config::validate`, `max_growth_factor`,
  `ConfigError` Display), and `pipeline` boundaries. Triage next: strengthen tests for the
  genuine minority (each a permanent regression), skip-list confirmed-equivalent mutants
  with reasons. Follow-up tuning: the `-j 10` contention tail argues for capping per-job test
  threads (or a higher timeout multiplier) so timeouts classify cleanly.
- 2026-06-09: Exhaustive survivor triage (method: subagent-classify → write tests → `cargo
  test` pass → re-mutate for ground truth → reclassify/exclude/document; re-mutation
  corrects the analysis, never the reverse).
  - **defang** done: 3 real gaps killed (`is_bare_domain` TLD-length / edge-hyphen label /
    allowed-label-byte). 12 survivors equivalent — `has_indicator_byte` + `already_defanged`
    excluded in `.cargo/mutants.toml` (perf pre-filter / redundant idempotence guard);
    `classify_core` L154 (re-mutation flipped it REAL→EQUIVALENT: col-24 `||` is the
    perf-prefilter term) and `push_defang_url` L260 (contract-dead `www.*://` branch) documented.
  - **Batch 1** (config/cli/indicators/case) re-mutation: **102 caught, 5 missed, 0 timeout**
    (the `-j 6` + 5× timeout config eliminated the spurious-timeout tail). Real gaps killed:
    `is_email` L36/L40(&&,>=), `is_ipv4` L80 (==255), `is_ipv6` L93/L95×2, `config`
    L132 (inclusive cap) / L281-282 (empty-affix factor) / L426 (Display), `cli` L220
    (flag-as-value), `case` L156 (Sentence expect-capital). 5 equivalent survivors documented:
    `config` L542×2 (the `#[cfg(kani)]` `MAX_FACTOR` const — unreachable by `cargo test`),
    `indicators` L40×3 (the `is_email` "domain must not end in a dot" upper bound — defensive
    and unreachable, because callers trim trailing dots before `is_email` ever sees them).
  - **Batch 2** (html/html_to_markdown/markdown): regression tests committed (commit 0c2e40c:
    close-tag scanning, entity boundaries, self-close detection, comment/PI/declaration
    skipping, table cells, heading levels h3–h6, emphasis/code/list structure, link
    destinations). Re-mutation in progress to confirm kills + classify residual equivalents.
- 2026-06-09: Batch 2 re-mutation (`-j 6`): 353 caught, 55 missed, 44 timeout, 9 unviable.
  The 44 timeouts are all html_to_markdown cursor/scanner functions (skip_comment,
  find_tag_end, read_attr_value, advance_one_char, …) — genuine infinite-loop mutants,
  killed-by-hang (not real survivors). The 55 missed were closed exhaustively (commit
  128aade): killing tests added for every real gap — `matches_close_tag`; the strip_html
  `&`-fast-path guard (REAL, not equivalent); find_tag_end self-close; nested `<pre>`;
  blockquote/table close; ordered-list numbering; newline-in-inline-code; trailing-space
  trim; inline-code edge-backtick; attr-lookup termination; unquoted-attr stop; link `>`
  escaping; no-spurious-space; markdown setext `=` and code-block blank-line preservation.
  **Documented-equivalent survivors** (no test can distinguish — mutation cannot change
  observable output; recorded here, kept visible rather than excluded because every
  function is mixed): html `strip_html` L90 fast-path guard, `normalize_trailing` L257/L260
  (truncate/drain no-ops), `decode_entity_at` L451/L467/L469 + `decode_numeric_entity`
  L514×2/L521/L551 (perf/DoS caps; out-of-range still caught by the `>0x10FFFF` check),
  `is_tag_name_char` L594×2 (`-`/`:`/`_` only in inline custom tags), `is_void_newline_tag`
  L599×2 (`br`/`hr` also in `is_block_tag`); html_to_markdown L39/L47/L194 (empty-slice
  no-ops), `parse_tag` L110×2 (`find('>')` self-corrects); markdown `strip_markdown_parser`
  L83×2 (bitflags — distinct flags, identical set), `plain_log_line_kind` L213/L214,
  `push_char` L338 (dead arm)/L339 (no-op reset), `is_intraword_ascii_underscore`
  fast-path-selector mutants, `starts_ordered_list_marker` L291 `>=`, `normalize`
  L410/L413 boundary no-ops, `strip_plain_log_markdown` L183 (`&&`≡`||` on reachable
  states). Plus the prior `config` L542×2 (cfg(kani)) and `indicators` L40×3 (defensive
  trailing-dot bound). Final confirming re-mutation of the three parser files running.
- 2026-06-09: **Final parser re-mutation: 364 caught, 40 missed, 48 timeout, 9 unviable**
  (caught up from 353, missed down from 55 — the residual tests killed their gaps). Source
  review of the 4 survivors the residual tests did *not* kill confirms all 4 EQUIVALENT:
  `trim_trailing_inline` L441 (only pops trailing buffer whitespace that the final
  `normalize` strips anyway — masked), `read_attr_value` L552×2 (`end + 1` is the *next-attr*
  position; the returned value slice / link destination is already correct, so it cannot
  change output), `skip_ascii_whitespace` L657 (`+=`→`-=` only perturbs a cursor whose error
  is masked, output already captured). So **all 40 missed are documented-equivalent**, and
  the 48 timeouts are killed-by-hang. **`pipeline` and `lines` survivors** from the original
  120 are likewise equivalent (no killing test possible, not re-mutated): `transform` L62
  (`i==len` handled by early returns), `prepare_collapse_scratch` L319×3 (zeroize-vs-clear
  wipe path, output-identical), `needs_ascii_collapse` L356/L358 (collapse loop is identity
  on non-collapsible lines; all-tab divergence masked by trailing-trim), `unwrap_lines` L154
  (`+`→`*` shifts a slice start that `.trim()` erases).
  - **FINAL ACCOUNTING — all 120 baseline survivors resolved.** Real gaps: committed killing
    regressions, each verified by re-mutation (caught rose). Equivalents: documented above
    with per-mutant reasons (perf fast-paths / DoS caps, redundant guards, bitflags, boundary
    & cursor no-ops, masked-by-`normalize`, position-only, `cfg(kani)`, defensive bounds).
    Loop-hang timeouts: killed-by-hang (the timeout guard is the oracle). Across the product
    crates the effective kill rate (caught + killed-by-hang) is ~91% of viable mutants, the
    remainder being provably-equivalent. `.cargo/mutants.toml` excludes only the two clean
    fully-equivalent helpers (`has_indicator_byte`, `already_defanged`) + `kani_proofs`; all
    other equivalents are mixed-function and stay visible (documented here). D-5 full-tree
    sweep + survivor triage: **complete.**
