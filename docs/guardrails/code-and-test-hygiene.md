# Guardrail: code & test hygiene (anti-slop)

**When to consult:** adding or editing any Rust code, dependency, doc comment, or
test — especially for large AI-authored changes, where dead code, unused deps, tangled
functions, broken doc links, and coverage-gaming tests accumulate fastest.

Heavily generated code rots in predictable ways: helpers nothing calls, dependencies
left behind after the code that used them is gone, functions that grow without bound,
doc links that dangle, and tests that execute code without asserting anything. The
project's answer is the same as everywhere else — *agents propose; deterministic tools
dispose*. Each kind of slop is a mechanical check that fails `cargo xtask ci`, not a
style note. Fix the code to satisfy the check; never weaken the check.

## The rules

1. Every `pub` item is reachable from outside its crate. `unreachable_pub` forces an
   over-exposed item down to `pub(crate)`; `dead_code` (escalated by `-D warnings`)
   then flags whatever is left with no callers. Delete dead code — do not `#[allow]` it.
2. No scaffolding macros ship: `todo!`, `unimplemented!`, and `dbg!` are denied
   workspace-wide. Finish the code or remove the stub.
3. Functions stay legible. `cognitive_complexity` and `too_many_arguments` cap tangle
   and parameter sprawl (thresholds in `clippy.toml`). Split the function or introduce a
   struct rather than raising a threshold.
4. Every declared dependency is actually used (`check-unused-deps`). Drop an unused
   dependency rather than leaving it "just in case".
5. Every `#[ignore]`d test states why (`#[ignore = "..."]`), and the total number of
   ignored tests never exceeds `MAX_IGNORED_TESTS` (`check-test-hygiene`). An ignored
   test is a disabled test; re-enable it rather than letting the count grow.
6. Docs build clean (`check-docs`): no broken intra-doc links, no public doc linking to
   a private item, no invalid inline HTML. Make the link resolve, drop the brackets to
   plain inline code, or fence a usage snippet — do not `#[allow]` the lint. Every public
   item in the shipped libs (`core`, `core-ffi`) is documented — `#![deny(missing_docs)]`
   makes a new undocumented `pub` item a build error; document it, do not `#[allow]` it.
7. New behavior earns its tests. Prefer a reference-interpreter clause + a property test
   over a single happy-path example. A test that runs code but asserts nothing is slop —
   it will not survive `check-mutants` (below), and it does not count as coverage.

## How the checks work

- **Lints (rules 1–3)** live in `[workspace.lints]` (`Cargo.toml`) and `clippy.toml`, so
  the policy is one source of truth that every crate inherits with `[lints] workspace = true`.
  The crate-specific `print_*` denies on `core`/`core-ffi` and `core`'s
  `#![forbid(unsafe_code)]` stay as source attributes on purpose — they are not
  workspace-wide.
- **`check-unused-deps`** runs `cargo-machete --with-metadata` over the whole workspace.
  It is orthogonal to `check-core-deps` (which constrains *what* the core may pull in)
  and `check-supply-chain` (advisories/licenses): this asks whether each declared crate
  is *used*.
- **`check-test-hygiene`** scans every `.rs` file for `#[ignore]` attributes, fails on a
  bare one, and fails if the count exceeds the `MAX_IGNORED_TESTS` ceiling. The ceiling
  is a constant in `xtask/src/main.rs` — raising it is a deliberate, reviewed edit, like
  the dependency allowlists.
- **`check-docs`** builds the workspace docs with `RUSTDOCFLAGS=-D warnings`. It is
  deterministic and offline (rustdoc on the pinned stable toolchain), so it stays in the
  required gate.
- **Coverage & mutation testing (`check-coverage`, `check-mutants`)** are the deepest
  signal for rules 1 and 7 — a surviving mutant is either dead code or an under-asserted
  test. They are **heavy and deterministic**, so (like Miri and Kani) they are *not* in
  the required `ci` gate: they run on demand, locally, and event-driven in
  [`hygiene.yml`](../../.github/workflows/hygiene.yml). A mutation finding is fixed by
  *strengthening a test*, and that new assertion becomes a permanent regression.

## Enforcing checks

- `cargo xtask ci` (runs rules 1–6 as part of the required gate)
- `cargo xtask check-unused-deps`
- `cargo xtask check-test-hygiene`
- `cargo xtask check-docs`
- `cargo xtask check-mutants` / `cargo xtask check-coverage` (best-effort; `XP_DIFF_BASE=<ref>` scopes to a diff)

## What a PR must call out

- Any new `#[ignore]`: the reason, and — if it raises `MAX_IGNORED_TESTS` — why the test
  cannot simply run.
- Any new dependency: that `check-unused-deps` passes (it is actually used).
- Any `#[allow(...)]` added to a hygiene lint: the specific justification, scoped to the
  smallest item — never a crate-level blanket allow.
- For new behavior: the reference/property/corpus coverage added (not just an example),
  and any mutation survivors knowingly left, with reasons.

## Tier-2 review (the residue)

Everything mechanizable is a gate above. What is left — naming, the wrong abstraction,
architectural drift from [`ARCHITECTURE.md`](../../ARCHITECTURE.md), golden tests that freeze
a bug as "correct", and security reasoning a rule cannot express — gets a tier-2 **agent
review** of the PR diff, never a cron: same convergence logic as the heavy gates, a quiet
repo pays nothing.

It runs as a connected **subscription cloud reviewer** (OpenAI Codex on a ChatGPT plan, or
Claude Code's cloud review) — deliberately NOT a pay-per-token CI action — so a side project
pays nothing beyond its existing subscription. The reviewer reads this repo's agent docs
([`AGENTS.md`](../../AGENTS.md) + these guardrails) as its rubric; the intended scope:

- **Anti-slop / repo-standards** on **every code PR**, reviewed against this guardrail.
- **Security** focus when the PR touches security-relevant surface — the unsafe FFI boundary
  (`core-ffi/`), the untrusted-input parsers / IOC-PII transforms
  (`core/src/ops/{html,markdown,html_to_markdown,defang,mask,indicators,urls}.rs`),
  config/pipeline validation, dependencies / `deny.toml`, macOS entitlements & signing, or
  CI workflows.
- **Advisory only**: the required signal stays `cargo xtask ci`. The reviewer is a
  *discovery* mechanism, not a merge gate — never block on a nondeterministic reviewer, and
  never let "the bot approved it" substitute for the deterministic invariants.

(Why a cloud reviewer, not a CI action? An API-keyed action bills per token; the
subscription OAuth token for CI currently expires ~daily with no refresh, and the dedicated
security-review action is API-only — so a connected cloud reviewer is the subscription-friendly
fit. One-time setup in CONTRIBUTING. If you ever want it in-repo and API-billed instead, a
two-track GitHub workflow is straightforward to add.)

Rules of the review:

- It emits **evidence**, not vibes — like a PR packet: `file:line` + the guardrail or
  invariant violated + a concrete fix. A finding the reviewer cannot ground that way is not
  a finding.
- A **recurring** finding graduates into a new deterministic `xtask` check: the agent
  discovers the *next* gate, then retires from that duty. Close it through
  [`review-finding-closure.md`](review-finding-closure.md) and, if it creates or changes an
  enforced invariant, update the `ARCHITECTURE.md` table. (Worked example: the review found
  an `html_to_markdown` bug a golden test had frozen as correct — the fix landed and the
  test now pins the correct output.)
