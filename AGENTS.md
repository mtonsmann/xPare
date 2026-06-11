# Agent Instructions

You are working in a memory-safe plain-text clipboard utility: a platform-neutral
Rust transformation **core** (`core/`) driven by native **shells**
(`shells/macos/` in Swift today; `shells/windows/` and `shells/linux/` reserved).
The core transforms text (coerce rich → plain, strip HTML/Markdown, normalize
whitespace, change case, line ops). Shells own all OS integration.

Treat these as compatibility and safety surfaces. Do not change them without an
explicit instruction and a PR call-out:

- The **FFI ABI** between core and shells — narrow, stable, language-neutral.
- `#![forbid(unsafe_code)]` in the core.
- The **privacy posture**: no network anywhere, no persistence or logging of
  clipboard content, in-memory only. (Persistence has exactly one sanctioned,
  opt-in exception — paste-as-file via `PasteFileStore`; see SECURITY.md.)
- The core's freedom from OS, filesystem, and network dependencies.
- **Deterministic** transform output for a given `(input, config)`.

`ARCHITECTURE.md` is the repository map. Detailed rules live in
`docs/guardrails/`. This file is only a map — keep it short and route to the
guardrails rather than inlining their content.

## Knowledge base

The knowledge base is live. Start from the map and the guardrail for your change
class:

- `ARCHITECTURE.md` — repository map, the core/shell trust boundary, data flow,
  and the enforced-invariants table.
- `DESIGN.md` — every settled decision with rationale, the threat model, known
  limitations, and what is deferred until the project grows.
- `SECURITY.md` — the privacy/data-handling posture and how each property is
  enforced.
- `docs/guardrails/` — focused rules per change class (linked from each workflow
  section below).
- `docs/guardrails/review-finding-closure.md` — what to add when a review finds
  a class of bug that should not come back.
- `docs/agent-workflow.md` — the evidence-first engineering loop (classify → brief →
  invariants → tests/properties/fuzz → smallest patch → checks → evidence packet).
- `docs/templates/correctness-brief.md` — the brief to fill in before non-trivial
  work; the PR template asks for the resulting evidence packet.
- `docs/agent-tasks/` — copy-paste-ready prompt templates per change class
  (core transform, FFI/ABI, security/privacy, dependency/CI, review-finding closure).

The invariants named above are enforced by `cargo xtask ci` (see
`CONTRIBUTING.md`), which CI runs verbatim. Keep checks green by fixing the code,
not by weakening the check.

## Operating Loop

1. Classify the change before editing (core transform / FFI boundary / shell /
   security posture / dependencies & CI / docs).
2. Open the matching workflow section below for the guardrails to consult.
3. Use `ARCHITECTURE.md` for module responsibilities, the core/shell boundary,
   and the enforced invariants.
4. Keep the diff narrow. Do not mix transform logic, ABI changes, shell code,
   dependency posture, and formatting unless the task requires it.
5. Add or update focused tests for behavior changes — especially anything that
   affects transform output, the ABI, or the privacy posture. Core changes must
   include adversarial-input coverage.
6. If a review, scan, fuzz run, or performance pass found an issue class, follow
   `docs/guardrails/review-finding-closure.md`: add repeatable regression
   protection and the relevant docs lesson before closing it.
7. Run checks that match the risk of the change (see `CONTRIBUTING.md`). If you
   skip a relevant check, explain why in the PR.
8. Update `ARCHITECTURE.md`, `DESIGN.md`, the relevant guardrail, and the shell
   contract when the boundary, invariants, posture, or supported transforms
   change.

For non-trivial work, write an execution plan under `docs/exec-plans/active/`
with a decision log before you start; move it to `completed/` when done.

## Core Transformations

Use for changes to transform logic (HTML/Markdown strip, whitespace, case, line
ops) or the pipeline.

Consult:

- `docs/guardrails/transform-correctness-and-adversarial-input.md`
- `docs/guardrails/memory-safety.md`
- `docs/guardrails/code-and-test-hygiene.md` (dead code, test/doc hygiene, mutation testing)

The core stays `#![forbid(unsafe_code)]`, never panics on input, and has no OS,
I/O, network, or global mutable state. Every behavior change gets regression and
adversarial coverage. Output must remain deterministic for a given config.

## FFI Boundary And ABI

Use for the C ABI surface, config serialization, generated bindings, or the
version/capabilities query.

Consult:

- `docs/guardrails/ffi-boundary-and-abi-stability.md`
- `ARCHITECTURE.md` (boundary contract)

Keep the surface narrow and data-driven: `transform`, a matching free, and a
version/capabilities query. **Adding or changing a transform must not change the
ABI** — feature selection crosses as serialized config data. Any ABI change is a
compatibility event: bump the version, call it out in the PR, and confirm a
non-Swift shell could still consume the boundary unchanged. The checked-in C
header is the source of truth; a test fails if it drifts.

## Native Shells

Use for the Swift macOS shell, or scaffolding a new platform shell.

Consult:

- `docs/guardrails/shell-contract.md` (per-platform responsibility checklist)
- `docs/guardrails/macos-posture.md` (sandbox, entitlements, hotkey, pasteboard)

Shells own clipboard read/write, rich→plain extraction, change detection, tray
UI, hotkey, settings, and calling the core. No transform logic lives in a shell.
macOS: App Sandbox + Hardened Runtime, minimal entitlements, no Accessibility or
Input Monitoring, in-place pasteboard rewrite only. A new platform = implement
the shell contract checklist and link the core.

## Security And Privacy Posture

Use for anything touching clipboard data handling, entitlements, logging,
network, or telemetry.

Consult:

- `docs/guardrails/privacy-and-data-handling.md`
- `docs/guardrails/content-logging-and-clipboard-safety.md`
- `SECURITY.md`

No network anywhere. No persistence or logging of clipboard content. In-memory
only: the core holds pipeline intermediates in `Zeroizing` buffers and the FFI
zeroizes the output buffer, so clipboard-derived bytes are wiped after use. The
mechanical checks in `cargo xtask ci` enforce no content logging/persistence, no
Swift network/browser API surface, no shipped subprocess spawning, no default
real-clipboard tests, and plain-string-only clipboard rewrites. Any new entitlement,
any dependency or API capable of network access, any shipped command-exec path, or
any new data path is a posture change — call it out and justify it in the PR.

## Dependencies, CI, And Automation

Use for crate/dependency ranges, lints, CI, structural tests, and automation.

Consult:

- `docs/guardrails/dependency-posture.md`
- `docs/guardrails/code-and-test-hygiene.md` (unused-dependency, lint, and doc gates)

Favor boring, API-stable, well-audited crates; justify any new dependency,
especially anything pulling in `unsafe` or network capability. Keep mechanical
dependency and automation updates separate from behavior changes. The invariants
(no-unsafe core, frozen ABI surface, no-network, core-has-no-OS-deps,
determinism) are enforced by CI lints and structural tests. Supply-chain auditing
(`cargo-deny`: advisories, licenses, bans, sources), workflow linting (`actionlint`
+ `zizmor`), and shell linting (`shellcheck`) also run inside `cargo xtask ci`, so
the one gate stays a complete superset of CI. CodeQL runs separately as an additive,
SHA-pinned `security-extended` signal, not as branch protection until its baseline is
triaged. Keep all gates green by fixing the code, not by weakening the check.

## Performance And Releases

Use for transform performance work or macOS release packaging.

Consult:

- `docs/exec-plans/active/0002-performance-ceiling-and-optimization-loop.md`
  (ceiling model, optimization waves, acceptance rules) and `docs/performance.md`
  (what we measure + the local baseline).
- `docs/release-model.md` and
  `docs/exec-plans/completed/0003-macos-release-plumbing.md` (source / unsigned-preview /
  Developer ID releases).

Performance is *measured* by criterion (`make bench`) and the opt-in `make perf`
throughput harness, and *gated* in CI only for complexity (`perf_guard.rs`), never
absolute speed. Releases: `make preview` is unsigned and needs no Apple account;
`make dist` / `make github-release` are gated on Developer ID credentials. An
optimization or release change may never weaken a guardrail.

## Documentation-Only Changes

Use for README, `ARCHITECTURE.md`, `DESIGN.md`, guardrails, runbooks, and
process docs.

Consult `ARCHITECTURE.md` and the guardrail for the topic being documented.
Docs-only PRs may skip core tests when the PR explains why. Still run the
formatter.

If the docs-only change captures a review lesson, also consult
`docs/guardrails/review-finding-closure.md`.

## Pull Requests

- State the change class and any compatibility or posture impact (ABI, privacy,
  entitlements, supported transforms).
- Keep diffs narrow and single-purpose.
- Update the relevant guardrail and `ARCHITECTURE.md` when invariants or the
  boundary move.
- For any fixed review finding class, state the mechanical regression protection
  and the docs lesson added.
- Automated review: a subscription cloud reviewer (Codex / Claude Code cloud) reviews PRs
  against [`docs/guardrails/code-and-test-hygiene.md`](docs/guardrails/code-and-test-hygiene.md)
  ("Tier-2 review") — anti-slop on every code PR, security focus on the security-relevant
  surface listed there. Advisory only; the required gate is `cargo xtask ci`.
- GitHub integration: use the `gh` CLI for GitHub operations (PRs, issues,
  releases).
