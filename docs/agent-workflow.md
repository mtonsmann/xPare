# Agent workflow — evidence-first engineering

SafetyStrip is an AI-native, correctness-oriented repository. The goal is not to
generate code faster; it is to make **correctness evidence** the artifact a change
is judged by. Code is cheap. The evidence that a change is safe, deterministic, and
posture-preserving is what takes review time — so the workflow front-loads it.

> **Agents propose; deterministic tools dispose.** A diff is a proposal. The
> property tests, the reference interpreter, the fuzz targets, `perf_guard`, and the
> `cargo xtask ci` structural checks are what actually decide whether a change is
> correct and safe. Your job is to make those tools say yes for the right reasons.

This file is the loop. The per-change-class detail lives in
[`docs/guardrails/`](guardrails/); the templates that capture the evidence live in
[`docs/templates/`](templates/) and [`docs/agent-tasks/`](agent-tasks/).

## The loop

### 1. Classify the change

Pick the change class — it selects the guardrail, the minimum checks, and the agent
task template:

| Class | Touches | Guardrail | Task template |
|---|---|---|---|
| **Core transform** | `core/src/ops/*`, `core/src/pipeline.rs`, `core/src/config.rs` | [transform-correctness](guardrails/transform-correctness-and-adversarial-input.md), [memory-safety](guardrails/memory-safety.md) | [core-transform](agent-tasks/core-transform.md) |
| **FFI / ABI** | `core-ffi/*`, `cbindgen.toml`, the C header | [ffi-boundary-and-abi-stability](guardrails/ffi-boundary-and-abi-stability.md) | [ffi-boundary](agent-tasks/ffi-boundary.md) |
| **macOS shell** | `shells/macos/*` | [shell-contract](guardrails/shell-contract.md), [macos-posture](guardrails/macos-posture.md) | — |
| **Security / privacy posture** | entitlements, logging, data paths, zeroization | [privacy-and-data-handling](guardrails/privacy-and-data-handling.md), [content-logging-and-clipboard-safety](guardrails/content-logging-and-clipboard-safety.md) | [security-privacy](agent-tasks/security-privacy.md) |
| **Dependency / CI** | `Cargo.toml`/`Cargo.lock`, `xtask`, `.github/workflows/*`, scripts | [dependency-posture](guardrails/dependency-posture.md) | [dependency-ci](agent-tasks/dependency-ci.md) |
| **Docs only** | `README`, `ARCHITECTURE.md`, `DESIGN.md`, `docs/` | the guardrail for the topic | — |

A review/scan/fuzz/CI finding is its own flow:
[review-finding-closure](agent-tasks/review-finding-closure.md).

### 2. Write a correctness brief

Before editing, fill in [`docs/templates/correctness-brief.md`](templates/correctness-brief.md).
The brief is short and states *what behavior you intend, what invariants must
survive, what bug classes you considered, how you will prove it, and what
performance surface the change owns*. Writing it first is what turns "plausible
diff" into "verified change": it forces you to name the property before you write
code that might violate it. Paste the filled brief into the PR (or link it).

### 3. Identify invariants

List the invariants the change must preserve and any it newly introduces. Pull from
the [enforced-invariants table](../ARCHITECTURE.md#enforced-invariants) and the
relevant guardrail. The load-bearing ones for this repo:

- `transform(input, config)` is deterministic and never panics.
- Canonical ordering equals an explicitly sorted `as_given` run.
- Fused optimized paths are byte-for-byte equal to sequential application.
- Accepted configs stay inside the resource envelope (op count, param bytes,
  single-line params, multiplicative growth factor; saturating arithmetic, no wrap).
- `strip_html` neutralizes `<script>`/`<style>` and removes tags;
  `html_to_markdown` drops unsafe link schemes and cannot break out of code fences.
- The FFI validates pointers, lossy-decodes UTF-8, rejects oversized input before
  allocation, contains panics, and zeroizes freed buffers.
- No network anywhere; no clipboard content logged or persisted; no `unsafe` in the
  core; minimal entitlements.

### 4. Add or update tests / properties / fuzz coverage *first*

Encode each invariant as an executable check before (or alongside) the
implementation, at the lowest practical layer:

- **Reference semantics** — for pipeline/op behavior, extend the test-only reference
  interpreter and the differential property in
  [`core/tests/reference_transform.rs`](../core/tests/reference_transform.rs)
  (`transform == reference`). This is the Cedar-style executable-semantics check:
  the optimized production pipeline must match a simple, auditable interpreter.
- **Property tests** — when the law is crisp (determinism, idempotence, ordering
  equivalence, round-trips, envelope bounds): proptest under `core/tests/`.
- **Regression / corpus tests** — when full formalization would be vague: a focused
  case or a corpus file under `core/tests/corpus/<area>/`.
- **Fuzz** — run the target covering any hand-rolled parser you touch; commit any
  crashing input under `fuzz/regressions/<target>/`.

### 5. Add performance evidence for feature work

Every new feature needs a repeatable performance signal before PR. Name the
feature's performance surface in the correctness brief, add or extend the narrowest
practical guard/measurement at the owning layer, and include the result in the PR:

- **Core behavior** — extend `core/tests/perf_guard.rs` for complexity/DoS shape
  and run the relevant benchmark flow from [`docs/performance.md`](performance.md)
  (`make perf`, `make bench`, or `make bench-large`) when throughput changes.
- **Shell-owned behavior** — add a Swift shell performance guard or measured smoke
  for SafetyStrip-owned orchestration, pasteboard handling, or UI responsiveness.
  OS framework latency that depends on hardware/content (for example Vision OCR)
  may be a proof gap, but the SafetyStrip-owned overhead still needs a local signal.
- **Docs-only or process-only work** — mark performance "not applicable" only when
  there is no runtime behavior, and say that explicitly in the PR.

### 6. Implement the smallest patch

Make the narrowest change that satisfies the brief. Do not mix transform logic, ABI
changes, shell code, dependency posture, and formatting. Match the surrounding
code's idiom and comment density. If you change a documented op rule, change its doc
comment in the same diff.

### 7. Run risk-matched checks

Run the checks for your change class (see [`CONTRIBUTING.md`](../CONTRIBUTING.md)),
then the full gate before opening the PR. For feature work, also run the
performance command named in the brief:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask ci
```

If the change touches a hand-rolled parser, also run the relevant fuzz smoke
(`make fuzz-smoke FUZZ_SMOKE_SECONDS=60`, or one target with
`cargo +nightly fuzz run <target> -- -max_total_time=60`). If you skip a relevant
check, say why in the PR.

### 8. Produce an evidence packet

The evidence packet is the part of the PR that lets a reviewer trust the change
without re-deriving it. It is required (the
[PR template](../.github/pull_request_template.md) asks for it):

- The change class and a brief link/summary.
- The invariants preserved, and any new ones.
- The exact commands you ran and their results (pass/fail, not "looks good").
- The tests / properties / fuzz coverage added or updated, named.
- The performance guard or measurement run for every feature, with the result
  and any residual gap (or an explicit "not applicable" for no runtime behavior).
- Compatibility / privacy / security posture impact (ABI, entitlements, network,
  zeroization, supported transforms) — or an explicit "none".
- Any skipped check, with the reason.

### 9. Preserve discovered bug classes as permanent regressions

If anything — a property failure, a fuzz crash, a review note, a reference/production
mismatch — surfaced a bug *class*, do not close it with only the one-off fix. Follow
[review-finding-closure](guardrails/review-finding-closure.md): name the class, add
the narrowest repeatable blocker at the owning layer (test, corpus entry, fuzz
regression, `perf_guard`, or `xtask` check), and record the lesson in the right
guardrail/posture doc.

### 10. Document proof gaps

Be honest about what is *not* proven. This repo does verification-guided
development, property-based testing, differential random testing, reference
semantics, and fuzz/regression hardening — **not** full formal verification. State
the residual gap (e.g. "HTML is not parsed per browser semantics", "FFI memory
behavior is exercised, not formally proven") in the brief and the PR so a reviewer
knows the boundary of the evidence.

## North star

> SafetyStrip should make it hard for an agent to submit a plausible patch without
> also submitting the evidence needed to trust it.
>
> Code is cheap. Correctness evidence is the artifact.
