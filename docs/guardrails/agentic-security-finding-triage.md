# Agentic Security Finding Triage

Use this before fixing a security finding reported by an agent, scanner, cloud
reviewer, fuzz run, CI security signal, manual audit, or generated fix PR. The
goal is to turn a finding report into a validated issue-class closure plan
before any agent writes a patch.

This guardrail is an intake layer. Once a finding is validated as a real issue
class, follow [`review-finding-closure.md`](review-finding-closure.md) for the
durable fix, blocker, docs lesson, and PR evidence.

## The rule

Treat every security finding report and suggested fix as candidate evidence, not
as the scope of work. A triage pass must answer:

1. **Is the finding real in this repo?** Validate against source, tests, workflow
   behavior, threat model, and existing blockers.
2. **What issue class is this?** Name the class, not just the line, file, or
   generated finding title.
3. **Which boundary owns the fix?** Pick the lowest layer that owns the broken
   control.
4. **Where else can the class appear?** Search sibling paths, equivalent syntax,
   alternate entry points, other platform shells, other workflow jobs, token
   scopes, release phases, or data paths.
5. **What repeatable blocker proves closure?** Choose the narrowest test,
   corpus entry, property, fuzz replay, structural check, or standard linter that
   fails if the class comes back.

Do not start by applying the generated patch. Do not let a finding be closed by
review memory, PR prose, or a tool reporting "no findings" after a narrow patch.

## Triage outcomes

- **True positive:** Implement the smallest durable fix and follow
  `review-finding-closure.md`.
- **False positive:** Record the repo-specific control that makes the finding
  invalid. Add docs or checks only when the same confusion is likely to recur.
- **Accepted risk:** State the threat-model reason, owner decision, and any
  compensating control. Do not disguise this as a fix.
- **Needs threat modeling:** Stop before patching. Clarify attacker, source,
  sink, trust boundary, impact, and acceptable residual risk.
- **Already blocked:** Identify the exact blocker and command that catches the
  class; add a short lesson only if future agents are likely to miss it.

## Intake note

Write this note before editing code:

```markdown
Finding:
- Source tool/reviewer:
- Finding id/title:
- Reported file/path:
- Reported source:
- Reported sink or broken control:
- Suggested fix, if any:

Validation:
- Status: true positive / false positive / accepted risk / needs threat modeling / already blocked
- Repo evidence:
- Reproducer or counterevidence:

Issue class:
- Class name:
- Owning boundary:
- Invariant:
- Sibling search:

Closure plan:
- Fix:
- Mechanical blocker:
- Docs lesson:
- Checks:
- Proof gaps:
```

Keep the note concise. It is a decision aid, not a second finding report.

## Boundary map

- **Core transform:** deterministic transform behavior, parsing, normalization,
  case/line ops, resource envelope. Prove with unit tests, property tests,
  reference-vs-production tests, fuzz regressions, or corpus replay.
- **FFI / ABI:** C ABI, generated bindings, config serialization, version and
  capabilities. Prove with ABI/header drift checks, pointer/size tests, panic
  containment tests, and zeroization checks.
- **Native shell:** clipboard integration, pasteboard handling, hotkeys,
  settings, menu behavior, shell-owned OS access. Prove with Swift tests,
  posture checks, and shell-contract docs.
- **Security/privacy posture:** logging, persistence, network, telemetry,
  entitlements, subprocesses, zeroization, data paths. Prove with structural
  checks, entitlement checks, content-logging checks, and posture docs.
- **Dependency / CI / automation:** workflows, scripts, `xtask`, `deny.toml`,
  dependency changes, CodeQL policy packs. Prove with `cargo-deny`, `zizmor`,
  `actionlint`, `shellcheck`, CodeQL where relevant, or focused `xtask` checks.
- **Release/signing:** signing/notarization, release assets, draft releases,
  attestations, SBOMs, artifact handoff, token scope. Prove with workflow
  posture checks, release-script checks, and release-model docs.
- **Docs only:** posture or process documentation without behavior change. Prove
  with formatting, link/path sanity, and a clear statement that no code gate was
  relevant.

## Sibling search

Before implementing, search for variants of the class:

- same source or sink in other files;
- same helper or wrapper called from another boundary;
- equivalent parser or workflow syntax;
- other platform shells or reserved platform directories;
- other token scopes, jobs, actions, artifacts, caches, environment files, or
  release phases;
- generated bindings, docs, fixtures, and tests that copied the same invariant;
- existing checks that claim to enforce the invariant but only match one spelling.

For workflow and release findings, be skeptical of path strings, job names,
draft-release state, and action boundaries as confinement controls. Verify who
can read or mutate the data at each phase.

## Blocker selection

Pick the lowest practical blocker:

- behavior bug -> focused unit/integration test;
- parser/input bug -> adversarial coverage, property test, fuzz replay, or
  committed corpus case;
- performance bug -> complexity guard or benchmark-flow update;
- posture/boundary bug -> always-on structural check, usually in `xtask`;
- dependency/workflow/script bug -> `cargo-deny`, `zizmor`, `actionlint`,
  `shellcheck`, CodeQL policy query, or focused `xtask` assertion;
- docs-only issue -> guardrail/posture doc update, plus a docs check only when
  the issue is mechanically expressible.

If no mechanical blocker is practical, state why and add the strongest
repeatable substitute. Never leave the only protection in the finding thread.

## Agent wrappers

The repository provides thin wrappers for common agents:

- Codex: `.agents/skills/security-finding-triage/`
- Claude: `.claude/skills/security-finding-triage/`

The wrappers must point back to this guardrail. Do not let tool-specific skill
text become a second source of truth.

## PR evidence

For true positives, the PR or final task report must include:

- finding status and issue class;
- source/sink/control summary;
- owning boundary and invariant;
- sibling search performed;
- blocker added, with file/path;
- docs/guardrail lesson added;
- exact checks run and results;
- compatibility/security posture impact;
- residual proof gaps.

For false positives or accepted risks, include the validation evidence and the
repo-specific reason no code fix was made.
