# Exec Plan 0005 — Canonical pipeline ordering

Status: **completed**. Started: 2026-06-05. Completed: 2026-06-09.

> Completion note: canonical-by-default ordering, `ordering: as_given`,
> `CONFIG_VERSION` 2, and the shell's "Manual order" drag-to-reorder mode all
> shipped; D3 was amended and D13 recorded in `DESIGN.md`.

## Goal

Make the operation pipeline **correct and efficient by default** so a user toggling
ops never has to reason about their order, while keeping deterministic, byte-exact
control for callers who need it. Bake a documented **canonical order** into the core,
on by default, with an explicit **`as_given`** override; surface it in the shell as
canonical-by-default with a "Manual order" mode (the drag-to-reorder UI) that flips to
`as_given`.

This **amends decision D3** (see below) — a deliberate, approved contract change.

## Decisions

### D-1 — Amend D3; canonical ordering is a core default

D3 currently says the core "never reorders — applies operations exactly as given."
We reword it: the core applies a **documented canonical order by default**, and
`ordering: as_given` runs operations exactly as given. The invariant's *value* —
determinism and the ability to get an exact, hand-specified order — is preserved
(canonical ordering is deterministic and documented, not silent; `as_given` recovers
exact control). Add a new decision **D13 — Canonical pipeline ordering** documenting
the rank and rationale.

### D-2 — `Config.ordering` + `CONFIG_VERSION` 1 → 2

`Config` gains `ordering: Ordering { Canonical, AsGiven }`, defaulting to `Canonical`.
This is a wire-contract change, so **bump `CONFIG_VERSION` to 2** (the mechanism the
design reserves for incompatible changes). The **C ABI is unchanged** — `ordering` is
just another field in the JSON that already crosses the boundary; the checked-in
header does not move. `capabilities()` reports `config_version: 2`.

### D-3 — Per-surface defaults

- **Core:** default `Canonical` (per D-1).
- **CLI:** the explicit/validation tool — defaults to **`as_given`** (preserves its
  "run exactly what I typed" contract and avoids surprising existing scripts), with
  `--canonical` to opt in.
- **Shell:** sends `Canonical`; a Settings "Manual order" switch sets `as_given` and
  reveals a drag-reorder list of the enabled ops.

## The canonical order (the rank)

Canonicalization is a **stable sort by a per-op rank** (ties keep input order, so any
truly-free pair is left as the user has it). Rank, with the *load-bearing* rows in
**bold** (order provably changes output, or is faster):

| Rank | Op | Constraint kind / why |
|--:|--|--|
| 1 | **StripHtml** | correctness — markup→text first; **StripHtml < StripMarkdown** (D6) |
| 2 | **StripMarkdown** | " |
| 3 | SplitOn | structural — expand delimiters into lines before line ops |
| 4 | **UnwrapLines** | correctness — **must precede RemoveBlankLines** (blank line = its paragraph delimiter) |
| 5 | CollapseWhitespace | normalize; **collapse < trim** (collapse can leave a trailing space) |
| 6 | **TrimTrailingWhitespace** | correctness — **must precede Dedupe** (so ws-only-different lines dedupe) |
| 7 | **CleanUrls** | correctness — **must precede Defang** (defang mangles the URL so CleanUrls can't match it) |
| 8 | **Defang** | " |
| 9 | Refang | adjacent to defang (pipelining both is degenerate; just needs a stable slot) |
| 10 | ExtractEmails | reduction — derive the subset after content is cleaned |
| 11 | ExtractUrls | " |
| 12 | RemoveBlankLines | line-set reduction |
| 13 | **DedupeLines** | efficiency — **dedupe < sort**: *output-identical* (dedupe is global, first-occurrence) but cheaper to sort fewer lines |
| 14 | SortLines | " |
| 15 | ChangeCase | *ambiguous* (see below) — default: case the body, then decorate |
| 16 | PrefixLines | decorate the finalized lines (prefix/suffix are a free pair) |
| 17 | SuffixLines | " |
| 18 | JoinWith | terminal — collapses lines; nothing line-based may follow |

In the shell's persistent pipeline only the *rewrite* ops appear (extraction/refang
are one-shot commands), so the everyday canonical question is rows 1–8, 12–18.

### The one genuinely user-controllable case
**ChangeCase placement** is the only spot where reordering changes output in a way a
user would plausibly want: `PrefixLines("Note: ")` + uppercase yields `NOTE: hello`
(case-then-prefix) vs `Note: HELLO` (prefix-then-case). Default = case the body first
(prefix text kept verbatim); the **`as_given` / Manual-order mode is the override**
that lets a user choose otherwise. No bespoke per-op control is needed — the global
manual mode covers it (and everything else).

## Properties to prove (the safety net)

- **Idempotent:** `canonicalize(canonicalize(x)) == canonicalize(x)` (stable sort).
- **Efficiency pairs are output-equivalent:** for any enabled set, `Canonical` and
  `AsGiven` produce identical output whenever they differ *only* by efficiency
  reorderings (the dedupe/sort case) — proven by a proptest comparing both modes.
- **Correctness orderings hold:** golden tests that the canonical result equals the
  documented-correct output regardless of the order the user enabled ops in (e.g.
  CleanUrls+Defang in either input order → same de-tracked, defanged result).
- **Determinism + panic-freedom** unchanged; total rank (every op a distinct rank)
  so canonical output never depends on input order except within intentional ties.

## Workstreams

1. **Core schema + canonicalize.** Add `Ordering` enum + `Config.ordering` (serde
   default `Canonical`); `CONFIG_VERSION` → 2; `canonical_rank(&Operation) -> u16` +
   `canonicalize(&mut Vec<Operation>)`; `transform` canonicalizes when
   `ordering == Canonical` before folding. Tests per § above + update the
   capabilities `config_version` assertion.
2. **Test migration.** Audit existing multi-op tests (`pipeline.rs`, parts of
   `golden.rs`, `determinism.rs`, `corpus_replay.rs`) that assert an as-given order;
   set `ordering: AsGiven` where they pin a hand order, or re-baseline under
   canonical. The output-equivalence proptest guards the efficiency cases.
3. **CLI.** `--canonical` flag (default `as_given`); update README CLI examples to
   `"version":2` and show the flag.
4. **FFI/ABI.** Confirm header unchanged (`check-abi` green); bump version assertions;
   capabilities reports v2.
5. **Swift core mirror.** Add `Ordering` + `TransformConfig.ordering` (default
   `.canonical`); `schemaVersion` → 2; encode/decode + round-trip tests.
6. **Shell UX.** Pipeline runs canonical; menu/Settings show ops in canonical order.
   Settings "Manual order" toggle → `as_given` + a drag-reorder `List` of enabled ops
   (this *is* the deferred reorder feature). Tests: ordering persists; manual reorders;
   canonical mode ignores stored order.
7. **Docs.** Reword D3, add D13 (rank + rationale) to `DESIGN.md`; update README
   (v2 examples, capability note); update `docs/deferred-work.md` (drag-reorder is now
   delivered here, not deferred).

## Migration / breaking-change callout

`CONFIG_VERSION` 1 → 2 means **existing v1 wire configs are rejected** and must move to
v2 (the README's `{"version":1,…}` examples, any hand-written CLI JSON). Persisted
**shell Settings are unaffected** — they store the ops list + mode, not a wire version;
`transformConfig()` builds a fresh v2 config. Call this out in the PR and README.

## Risks

- **Tests assuming as-given order** (workstream 2) — the main churn; mitigated by the
  output-equivalence proptest and explicit `AsGiven` in order-pinning tests.
- **Rank completeness** — every op needs a distinct rank for deterministic output;
  intentional ties (free pairs) fall back to stable input order, which is the desired
  "leave it as the user has it" behavior.
- **Two reductions in one canonical pipeline** (extract emails + urls) is degenerate
  (yields empty) — the GUI prevents it (they're one-shot commands); the CLI/`as_given`
  user owns that choice.
