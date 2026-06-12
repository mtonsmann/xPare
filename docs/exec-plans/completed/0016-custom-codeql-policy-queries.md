# 0016 - Custom CodeQL policy queries

**Status:** completed

## Goal

Add low-noise custom CodeQL rules that encode xPare-specific security and
automation lessons not already covered by built-in CodeQL or deterministic
`xtask` checks.

## Change class

Dependency / CI / automation, with security/privacy posture documentation. This
adds QL source, wires it into GitHub Actions, and tightens the CodeQL posture
check. No runtime transform behavior, FFI ABI surface, entitlement, dependency,
clipboard persistence, logging, or network posture change is intended.

## Scope

- Add an in-repository Rust CodeQL query pack for shipped Rust capability drift.
- Add an in-repository Python CodeQL query pack for capability-light helper
  drift.
- Wire the custom packs into the CodeQL workflow while preserving the
  `security-extended` baseline.
- Extend `check-codeql-workflow-posture` so the custom packs and query IDs cannot
  be silently removed.
- Update docs and deferred work notes to distinguish implemented Rust/Python
  custom rules from Swift custom rules still blocked on reliable Swift CodeQL.

## Out of scope

- Swift CodeQL analysis or Swift custom QL rules.
- Branch-protection changes.
- Replacing `cargo xtask ci`, actionlint, zizmor, cargo-deny, Miri, Kani,
  fuzzing, or exact structural checks.
- Broad generic style, quality, or maintainability queries.

## Decision log

- 2026-06-12: Keep custom QL narrow. Official GitHub docs describe custom
  queries as useful for architecture-specific vulnerabilities and coding
  standards, but this repo already has a strong deterministic gate, so custom QL
  should only encode repo policy where CodeQL alerts are useful review signal.
- 2026-06-12: Implement Rust and Python query packs first because CodeQL is
  already green for those languages on main. Split GitHub Actions into its own
  CodeQL job so the Python custom pack is only attached to a Python database.
- 2026-06-12: Do not add custom Actions QL in this phase. Built-in Actions
  CodeQL plus actionlint, zizmor, and `check-codeql-workflow-posture` already
  enforce the relevant workflow lessons with lower noise.
- 2026-06-12: Keep the high-value Swift custom QL ideas deferred: clipboard
  content to persistence/log/network sinks, stale pasteboard writes after async
  transforms, and FFI output ownership. They are still the right long-term rules,
  but adding them before Swift CodeQL completes reliably would create dead
  automation.

## Acceptance

- `cargo fmt --all --check`
- `cargo test -p xtask`
- `cargo run -p xtask -- check-codeql-workflow-posture`
- `cargo run -p xtask -- check-workflows`
- If the CodeQL CLI is available, compile the custom query packs locally; if not,
  record that GitHub CodeQL CI is the first QL compiler run.

## Evidence packet

- `cargo fmt --all --check` passed.
- `cargo test -p xtask` passed: 62 tests.
- `cargo run -p xtask -- check-codeql-workflow-posture` passed and verified the
  custom query packs are still present and wired by language.
- `cargo run -p xtask -- check-workflows` passed: `actionlint` clean and
  `zizmor --offline .github/workflows` reported no findings.
- Sandboxed `cargo run --locked -p xtask -- ci` passed through Rust tests,
  structural checks, docs, shellcheck, actionlint, zizmor, and cargo-machete, then
  failed only because `cargo-deny` could not lock the read-only sandbox path
  `/Users/marcus/.cargo/advisory-dbs/db.lock`.
- Escalated `cargo run --locked -p xtask -- ci` passed end-to-end. `cargo-deny`
  emitted the existing informational warnings about unmatched license allowances
  and duplicate `wit-bindgen`, then reported `advisories ok, bans ok, licenses
  ok, sources ok`.
- `codeql` is not on `PATH` locally, so local QL compilation was not run; the
  GitHub CodeQL workflow is the first QL compiler/analyzer run for these packs.
- GitHub CodeQL's first compiler pass rejected the Rust and Python helper
  predicates that used `.matches()` on strings without an explicit binding set;
  the custom queries now annotate those predicates with `bindingset[...]`, and
  `cargo run -p xtask -- check-codeql-workflow-posture` passes with the fix.
- PR review found that the Rust rule only checked resolved call targets, which
  would miss `std::path`/`std::fs` in core type signatures, fields, or imports.
  The Rust query now also checks `PathTypeRepr` and `UseTree` references in
  `core`/`core-ffi`, and the posture check requires those query snippets.
- Follow-up PR review found two more source-reference gaps: grouped use-tree
  imports such as `use std::{path::PathBuf, fs};`, and `std::net` imports/types
  before any call site exists. The Rust query now resolves nested `UseTree`
  prefixes through `getUseTreeList().getAUseTree()` and applies
  source-reference checks to network imports/types across shipped Rust.

## Outcome

Rust and Python now have in-repository custom CodeQL packs wired alongside
`security-extended`. The Rust rule watches shipped Rust surfaces for process or
network capability drift and keeps the pure core/FFI boundary filesystem-free,
including grouped source-level path/filesystem imports and type references.
Network imports and type references are also flagged across shipped Rust before
they can become a resolved call target.
The Python rules keep the macOS helper script stdlib-only and capability-light by
flagging banned imports and dynamic/process/network calls. `xtask` now fails if
the workflow drops those packs, collapses Python and Actions into one CodeQL
category, or removes the expected query IDs.

Swift custom CodeQL remains deferred until the Swift extractor completes
reliably in CI; the specific Swift rule ideas are recorded in
`docs/deferred-work.md`.
