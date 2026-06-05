# Exec Plan 0004 — Extraction commands, defang/refang, URL cleaning

Status: **active** · Started: 2026-06-05

## Progress / handoff (2026-06-05)

Phases 1, 2, 4 merged to `main` via #4. Phase 3 (macOS shell UX) is on
`claude/phase3-macos-shell-ux`:

- **Phase 3 — macOS shell UX (DONE on the phase3 branch):** menu restructured into a
  *Clean* section (the zero-param toggles + `Clean URL trackers`, a `Sort lines`
  toggle, a `Defang IOCs` toggle, and a `Defang bracket style` inline-`Picker`
  submenu) and an *Extract / convert* section of one-shot **commands** (Extract
  emails/URLs, Refang) wired to `StripController.runOnce(operations:)` — a transient
  config that is never persisted. Continuous mode now drops reductions
  (`Operation.isReduction`) so it can't silently reduce a copied buffer. A `Settings`
  scene (`SettingsView.swift`, Route A) hosts the free-text params (prefix/suffix/
  join/split) + the sort flags. Tests: `runOnceDoesNotPersistToSettings` and
  `continuousModeSkipsReductions` in `StripControllerTests`. `swift build` passes.
  Deferred-by-choice: in-menu sort-flag submenu and a drag-to-reorder pipeline list
  in Settings (noted in the Settings window), plus real `docs/performance.md`
  numbers and the external-only CI checks (cargo-deny / actionlint / shellcheck).

### Earlier phases (merged in #4), all green:

- **Phase 1 — core ops:** `Defang`/`Refang` (`core/src/ops/defang.rs`) and `CleanUrls`
  (`core/src/ops/urls.rs`) implemented TDD-first, wired into the `Operation` enum,
  pipeline, and capabilities JSON. `BracketStyle` lives in `config.rs` and is
  re-exported from `ops::defang` (one type, shared by the wire schema and the impl).
  Tests: `core/tests/defang.rs`, `core/tests/clean_urls.rs`, corpus fixtures under
  `core/tests/corpus/{defang,clean_urls}/`, plus `determinism.rs`/`perf_guard.rs`
  entries. `cargo test --workspace`, `clippy`, `fmt`, and `check-abi` all pass; the
  C ABI is **unchanged** (additive enum variants only, `CONFIG_VERSION` still 1).
- **Phase 2 — Swift mirror:** `shells/macos/.../TransformConfig.swift` has
  `BracketStyle` + `defang`/`refang`/`cleanUrls` cases with hand-written Codable;
  `swift build` passes (`swift test` can't run here — no `Testing` module in CLT).
- **Phase 4 — integration:** fuzz targets (`fuzz/fuzz_targets/{defang,clean_urls}.rs`
  + `transform_pipeline` `LocalOp` mirror), perf-guard cases, README capability line.

**All four phases now implemented** (Phase 3 details above). Deferred-by-choice and
not blocking: in-menu sort-flag submenu, a drag-to-reorder pipeline list in Settings,
real `docs/performance.md` throughput numbers (re-measure, don't fabricate), and the
external-only CI checks (cargo-deny / actionlint / shellcheck), which these changes
don't touch.

Process note: the parallel-subagent step did **not** auto-merge into this worktree —
work that must land here has to target `$CLAUDE_PROJECT_DIR` (or be committed by the
subagent and merged). User-level PreToolUse guardrail hooks now enforce this.

## Goal

Ship three capabilities and the shell surface to use them, **test-first**:

1. **Defang / Refang** — neutralize and re-activate network IOCs (URLs, hostnames,
   IPv4/IPv6, emails). New core ops.
2. **URL cleaning** — strip tracking/analytics query parameters from URLs. New core op.
3. **Extraction as a first-class shell feature** — surface the *existing*
   `ExtractEmails` / `ExtractUrls` ops (and `SortLines`, `Defang`) in the macOS menu,
   plus a **Settings window** (Route A) for the free-text-parameterized ops.

All of this lands **without weakening any invariant**: the frozen C ABI is untouched
(feature selection is data, per D3), the core stays pure / `#![forbid(unsafe_code)]`
/ no-OS-IO-net, output stays deterministic, and the new hand-rolled scanners join the
panic-free regime (proptest + corpus + fuzz + linear-time guard).

The design rationale is settled in [`DESIGN.md`](../../../DESIGN.md) decision **D12**
(operation taxonomy: rewrites vs reductions, toggles vs commands; Route A for params)
and the *IOC defang/refang and URL cleaning* contract under Transform semantics. This
plan is the *execution* mirror of those decisions.

## Non-goals

- No ABI change. If any workstream finds it "needs" one, stop — it doesn't (D3).
- No live/updatable tracker list, no network (`CleanUrls` denylist is baked-in).
- No RFC-grade URL/email parsing — heuristic, consistent with `extract_*`.
- Not switching the menu to `MenuBarExtra(.window)` — Route A keeps `.menu` (D12).
- No signed `.app` (CLT-only env, per the standing limitation).

## The frozen wire contract (decide once, up front — everything keys off this)

New `Operation` variants and their JSON (internally tagged on `op`, snake_case —
matches `core/src/config.rs`). **Agree these names/params before any workstream
starts**; the Swift mirror and shell depend on them verbatim.

| Variant | JSON | Family (D12) | Shell surface |
|---|---|---|---|
| `Defang { style }` | `{"op":"defang","style":"square"}` | rewrite | toggle (+ style submenu) |
| `Refang` | `{"op":"refang"}` | rewrite | one-shot command |
| `CleanUrls` | `{"op":"clean_urls"}` | rewrite | toggle |

- `style`: bounded enum, default `square` (`[.]`). Reserve `round` (`(.)`) as a second
  value so the param is real but minimal. Serde `#[serde(default)]` → defaults to
  `square` when absent, so older configs stay valid without a version bump.
- `CONFIG_VERSION` stays **1** — these are additive variants, and `deny_unknown_fields`
  is on the struct, not the enum, so adding variants is backward compatible.
- Capabilities JSON (`core/src/lib.rs`) gains:
  `{"op":"defang","params":["style"],"styles":["square","round"]}`,
  `{"op":"refang"}`, `{"op":"clean_urls"}`. The existing
  `capabilities_is_valid_json_and_version_consistent` test must still pass.

## TDD method (every core op follows this exact cycle)

These ops are described as a textbook TDD case for a reason — the spec *is* the test.

1. **Write the doc-comment spec first** on the (empty) implementing fn — the frozen,
   word-for-word rule (like the `extract_emails` doc at `core/src/ops/lines.rs:304`).
2. **Author the failing tests** against that spec, before any logic:
   - **Golden/table unit tests** — input → exact expected output, including the empty
     string, no-match, NUL/control bytes, and multi-`@`/multi-dot edge cases.
   - **Corpus fixtures** under `core/tests/corpus/` — these auto-flow through
     `corpus_replay.rs` (panic-freedom + per-file time bound); add IOC-flavored
     fixtures (defanged-already, mixed IPv4/IPv6, tracker-laden URLs).
   - **Property tests** (`core/tests/determinism.rs` style, proptest):
     - panic-freedom on arbitrary `&str`,
     - **determinism** (same input → same output),
     - **idempotence**: `defang(defang(x)) == defang(x)`, `clean_urls∘clean_urls`
       fixed point,
     - **round-trip**: `refang(defang(x)) == x` *for inputs with no pre-existing
       defang tokens* (constrain the proptest generator accordingly — see D12 caveat).
3. **Run → red.** Confirm the tests fail for the right reason.
4. **Implement minimally** to green. Char/byte-aligned scanning only (never slice on a
   non-UTF-8 boundary), strictly linear-time with bounded lookahead.
5. **Refactor** for clarity; keep allocation obvious.
6. **Add a `cargo fuzz` target** (`fuzz/fuzz_targets/`) and a `perf_guard.rs` entry so
   a superlinear regression fails CI.
7. `cargo xtask ci` green (no-network / core-deps / abi checks unaffected).

## Workstreams (subagent fan-out) and dependency graph

```
        ┌─────────────────────────────────────────────┐
Phase 1 │  WS-A defang/refang core   WS-B clean_urls   │  (parallel, isolated worktrees)
        └───────────────────┬──────────────┬──────────┘
                            ▼              ▼
Phase 2          WS-C Swift core mirror (Operation cases + Codable + round-trip tests)
                            │
                            ▼
Phase 3          WS-D macOS shell UX (menu sections + enum submenus + commands + Settings window)
                            │
                            ▼
Phase 4          WS-E integration: capabilities/ABI assertions, perf note, docs, full xtask ci
```

Phase 1 is two independent agents (worktree isolation — both touch `ops/mod.rs`,
`config.rs`, `pipeline.rs`, `lib.rs`, so serialize the *merge* or have one agent own
the shared-file edits). Phases 2→3→4 are gated because the Swift mirror needs the
final wire names and the shell needs the mirror.

### WS-A — Defang / Refang (core)
- New module `core/src/ops/defang.rs`; register in `ops/mod.rs`.
- `pub fn defang(input: &str, style: BracketStyle) -> String` and `pub fn refang(input: &str) -> String`.
- Targets & substitutions per the DESIGN contract (`http`→`hxxp`, `.`→`[.]`,
  `://`→`[://]`, `@`→`[@]`; IPv4/IPv6 dot/colon bracketing). Reuse the
  `trim_token_punct` / whitespace-tokenizer approach from `lines.rs`.
- `Operation::Defang { style }` + `Operation::Refang` in `config.rs`; pipeline arms in
  `pipeline.rs`; capabilities entries in `lib.rs`.
- Tests per the TDD cycle, with explicit focus on **idempotence** and the documented
  **round-trip caveat**. Fuzz target `fuzz/fuzz_targets/defang.rs`.

### WS-B — CleanUrls (core)
- New module `core/src/ops/urls.rs`; register in `ops/mod.rs`.
- `pub fn clean_urls(input: &str) -> String`. Baked-in `const` denylist
  (`utm_source/medium/campaign/term/content`, `fbclid`, `gclid`, `dclid`, `gbraid`,
  `wbraid`, `msclkid`, `mc_eid`, `igshid`, `yclid`, `_hsenc`, `_hsmi`, `vero_id`,
  `oly_*`, `ref` is **not** a tracker — leave it). Preserve non-tracking params,
  order, and `#fragment`. Drop the `?` entirely if no params remain.
- `Operation::CleanUrls` + pipeline arm + capabilities entry.
- Tests: idempotence, "non-URL text untouched", "preserves order + fragment + kept
  params", percent-encoding left intact. Fuzz target `fuzz/fuzz_targets/clean_urls.rs`.

### WS-C — Swift core mirror
- Add `defang(style:)`, `refang`, `cleanUrls` cases to `Operation` in
  `shells/macos/Sources/SafetyStripCore/TransformConfig.swift` + `opTag` + hand-written
  Codable encode/decode arms + a `BracketStyle` enum mirroring the Rust one.
- Extend `TransformConfigTests.swift`: encode each new variant and assert the JSON
  matches the wire contract byte-for-structure; decode round-trip.

### WS-D — macOS shell UX
- Restructure `MenuContent` (`SafetyStripApp.swift`) into the D12 sections: **Clean**
  (toggles — add `SortLines`, `Defang`), **Extract** (one-shot *commands* —
  `Extract emails`, `Extract URLs`, `Refang clipboard`).
- Enum params as submenus (SwiftUI `Menu`): `Defang` bracket style; (optional now)
  `ChangeCase`, `SortLines` flags.
- `AppModel`: add a `runOnce(_ op:)` path that builds a **transient** one-op
  `TransformConfig` and runs it via the controller *without* mutating
  `settings.operations` (parallels `stripNow`). **Guard:** continuous mode must not
  run reductions — assert/skip in `StripController`.
- **Settings window** (`Settings` scene): text fields for `PrefixLines` /
  `SuffixLines` / `JoinWith` / `SplitOn`, and a reorderable list binding to
  `settings.operations` (pipeline order). Persists through existing `Settings`
  Codable.
- Tests in `SafetyStripKitTests`: transient-config command path doesn't persist;
  continuous mode refuses reductions; Settings round-trips the new params.

### WS-E — Integration & docs
- Confirm **ABI unchanged**: `cargo xtask check-abi` green; add an assertion/comment
  that the header did not move.
- `core/tests/perf_guard.rs`: linear-time entries for `defang`/`clean_urls`.
- `docs/performance.md`: add the two ops to the measured table (re-measure locally,
  do not copy numbers).
- Verify `DESIGN.md` D12 + IOC contract match what shipped; update the decision-log
  mirror note. Update any capabilities/feature listing in `README.md`.
- Full `cargo xtask ci` + `swift build` + `swift test` green.

## Verification checklist (definition of done)

- [ ] `cargo test` (unit + golden + corpus_replay + determinism/proptest) green.
- [ ] `cargo fuzz run defang` / `clean_urls` survive a short smoke run (no crash).
- [ ] `cargo xtask ci` green — **no-network, core-deps, and ABI checks unaffected**.
- [ ] `swift build && swift test` green; menu shows Clean/Extract sections; defang
      style submenu works; extraction + refang run as one-shot and do **not** persist.
- [ ] Continuous mode provably refuses to run a reduction.
- [ ] `CONFIG_VERSION` still 1; old persisted settings still load.

## Risks / watch-items

- **Shared-file merge contention** (WS-A and WS-B both edit `config.rs` / `pipeline.rs`
  / `lib.rs` / `ops/mod.rs`). Mitigation: worktree isolation + one agent owns the
  shared edits, or sequence the shared-file patch.
- **Refang over-reach** — global reverse-substitution can touch text that merely
  *looks* defanged. Documented caveat (D12); the proptest generator must exclude
  pre-existing defang tokens to keep the round-trip property honest.
- **`CleanUrls` eating legitimate params** — keep the denylist conservative; only
  well-known trackers. A test fixture must prove non-tracker params survive.
- **Idempotence under continuous mode** — both new toggles must be fixed points;
  proven by property test, not assumed.
