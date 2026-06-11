# 0014 - CodeQL and mechanical-check coverage research

**Status:** completed

## Goal

Research whether enabling CodeQL would add useful signal for this repository,
where it overlaps existing `cargo xtask ci` checks, what additional deterministic
mechanical checks could cover gaps, and which custom CodeQL queries are worth
considering.

## Change class

Dependency / CI research plus docs-only evidence capture. No runtime code,
transform behavior, ABI surface, entitlement, or privacy posture change was made.

## Repo inventory

The checkout is already heavily guarded:

- Required local/CI gate: `cargo xtask ci`.
- Rust surface: 43 `.rs` files across `core`, `core-ffi`, `cli`, `xtask`, tests,
  benches, and fuzz targets.
- Swift surface: 18 `.swift` files in the macOS shell and tests.
- Tooling surface: 1 Python script, 4 shell scripts, 5 GitHub workflow/YAML files.
- Required checks already include fmt, clippy, tests, core `unsafe` forbid,
  core dependency allowlist, workspace no-network crate banlist, content logging
  scan, pipeline zeroization scan, agent workflow docs scan, clipboard-safety
  Makefile scan, C/FFI surface scan, C ABI drift check, entitlement minimality,
  release-signing posture, shellcheck, actionlint plus zizmor, and cargo-deny.
- Best-effort CI already includes the macOS Swift build, fuzz smoke, Miri over
  the unsafe FFI shim, and Kani over resource-envelope arithmetic.

Prior security-finding closure already added repo-specific checks and tests for
release entitlement posture, tiny C interop surface, HTML-first shell routing,
rich representation size checks, stale async transform writes, and continuous-mode
self-write handling. Those exact invariant checks are still higher signal than a
generic SAST rule for those classes.

## CodeQL coverage available now

Current GitHub/CodeQL docs show relevant support for this repository:

- Rust is supported for editions 2021 and 2024; the extractor requires `rustup`
  and `cargo`, and does not support nightly-only features.
- Swift is supported for Swift 5.4 through 6.3, but Swift analysis requires macOS.
- GitHub Actions workflows are supported directly.
- Python is supported, which would cover `shells/macos/generate-icon.py`.
- The maintained built-in suites are `default` and `security-extended`; the latter
  runs all default queries plus additional lower-precision/lower-severity queries.
- Built-in Rust queries include relevant classes such as invalid pointer access,
  use after lifetime, cleartext logging/storage/transmission, uncontrolled
  allocation size, uncontrolled path expression, log injection, SSRF, regex
  injection, and weak crypto.
- Built-in Swift queries include relevant classes such as cleartext logging,
  local database/preference storage, cleartext transmission, system command built
  from user-controlled sources, path expression, XXE, unsafe WebView fetch,
  JavaScript injection, regex issues, string length conflation, and crypto issues.
- Built-in Actions queries include cache/artifact poisoning, trusted-context
  checkout issues, code injection, untrusted environment/PATH values, excessive
  secrets exposure, missing permissions, vulnerable actions, and unpinned actions.
- GitHub's guidance is to start with default setup, then switch to advanced setup
  with manual builds for high-risk repositories after triaging initial alerts.

Sources:

- <https://codeql.github.com/docs/codeql-overview/supported-languages-and-frameworks/>
- <https://docs.github.com/en/code-security/concepts/code-scanning/codeql/codeql-query-suites>
- <https://docs.github.com/en/code-security/reference/code-scanning/codeql/codeql-queries/rust-built-in-queries>
- <https://docs.github.com/en/code-security/reference/code-scanning/codeql/codeql-queries/swift-built-in-queries>
- <https://docs.github.com/en/code-security/reference/code-scanning/codeql/codeql-queries/actions-built-in-queries>
- <https://docs.github.com/en/code-security/how-tos/find-and-fix-code-vulnerabilities/manage-your-configuration/codeql-for-compiled-languages>
- <https://docs.github.com/en/code-security/concepts/code-scanning/codeql/custom-queries>

## Recommendation

Enable CodeQL, but treat it as an additive review layer, not as a replacement for
`cargo xtask ci`.

The value is real but bounded:

- High value for the Swift shell, because that is where clipboard data meets OS
  APIs. CodeQL can reason about data/control flow in a way the current
  token-based `check-no-content-logging` cannot.
- Medium value for `core-ffi`, because the Rust built-in pointer/lifetime queries
  complement Miri and tests. The core remains `#![forbid(unsafe_code)]`, so CodeQL
  is less important there than fuzz/property/reference-model coverage.
- Medium value for GitHub Actions, mostly as another code-scanning UI signal on
  top of actionlint and zizmor. It should not replace `check-workflows`.
- Low-to-medium value for the Python icon generator. CodeQL may catch generic
  Python security issues, but this script is tiny and better guarded by a simple
  import/capability allowlist.

Suggested rollout:

1. Baseline CodeQL with `security-extended`, not `security-and-quality`. The repo
   already has strong style/quality gates through clippy, tests, and structural
   checks; the extra quality suite is likely noise.
2. Do not make CodeQL branch-protection-required until the initial alert baseline
   is triaged and false positives are understood. This repo has prior branch
   protection sensitivity around required checks, so avoid introducing a new hard
   gate before the signal is stable.
3. If default setup can be enabled with the desired language selection, start
   there. It is the lowest-maintenance way to get Rust, Actions, Python, and Swift
   signal.
4. If custom queries or exact build control are needed, switch to advanced setup:
   - Analyze Rust with `build-mode: none` unless a future build generates Rust
     source. Current Rust has no generated source requiring manual build capture.
   - Defer Swift CodeQL until the extractor can complete reliably in CI. Initial
     PR triage found the Swift job hanging inside the traced SwiftPM build even
     after pinning an Intel runner, prebuilding the Rust FFI library, and
     narrowing the traced command to the `XPareKit` target. Keep Swift covered by
     the existing `cargo xtask ci` shell posture checks while CodeQL starts with
     Rust, Python, and Actions.
   - Keep CodeQL outside `cargo xtask ci`. It is GitHub/SARIF-oriented and not the
     deterministic local gate this repo uses.
   - If adding a workflow, pin all `github/codeql-action/*` actions to full SHAs
     with version comments, and let Dependabot maintain them like the existing
     pinned actions.

## What CodeQL does not replace

CodeQL should not be used as the enforcement mechanism for these existing
invariants:

- Core has no `unsafe`: the compiler plus `check-unsafe-forbid` is stronger.
- Core has no OS/filesystem/network deps: `check-core-deps` is exact.
- No network-capable Rust crate anywhere: `check-no-network` is exact for the
  workspace dependency tree.
- Frozen C ABI and generated header drift: `check-abi` is exact.
- Minimal macOS entitlements and release signing posture: current plist/script
  checks are exact.
- Deterministic transform output, reference semantics, resource envelope,
  never-panics behavior: tests, properties, fuzz, Miri, and Kani are the right
  tools.
- Pipeline zeroization shape: the current exact structural check is intentionally
  narrow and low-noise.

## Additional mechanical checks to consider

These are better as deterministic `xtask` or linter checks than CodeQL alerts:

1. `check-swift-no-network-apis`

   Scan shipped Swift sources for network or browser-capable APIs such as
   `URLSession`, `URLRequest`, `NSURLConnection`, `Network.framework`,
   `NWConnection`, `WKWebView`, `SFSafariViewController`, and similar APIs. The
   Rust no-network banlist does not cover Swift/Foundation APIs, and entitlements
   are a runtime sandbox layer, not a source-level posture guard.

2. `check-shipped-command-exec`

   Ban process spawning from shipped app surfaces: Swift `Process`, Rust
   `std::process::Command` outside `xtask`, and Python `subprocess` in release
   tooling unless explicitly allowlisted. Built-in CodeQL catches some
   user-controlled command construction, but this project can use a stronger rule:
   shipped app code should not spawn commands at all.

3. `check-swift-package-deps`

   Keep `shells/macos/Package.swift` dependency-free unless a PR explicitly adds a
   reviewed Swift package dependency. The Rust dependency posture is strong, but
   SwiftPM dependency drift is not currently mirrored by cargo-deny.

4. `check-python-tooling-posture`

   Add `python3 -m py_compile shells/macos/generate-icon.py` and an AST/import
   allowlist for the icon generator. Allow only the expected pure stdlib modules
   (`argparse`, `math`, `struct`, `zlib`, `pathlib`, `__future__`). Ban
   `socket`, `urllib`, `http`, `requests`, `subprocess`, `os.system`, and similar
   capability imports/calls.

5. `check-real-clipboard-tests`

   Extend `check-clipboard-safety` beyond Makefile prerequisites by scanning tests
   for direct `NSPasteboard.general` use. Named pasteboards are fine; default tests
   should not read or mutate the user's real clipboard.

6. `check-pasteboard-write-shape`

   Enforce that shipped macOS code writes clipboard output through the
   `PasteboardProtocol.writePlain` path and that the concrete system implementation
   only does `clearContents()` plus `setString(_:forType: .string)`. This turns the
   shell contract's "plain-only in-place rewrite" into a source-level guard.

7. `check-codeql-workflow-posture` if advanced setup is added

   Verify the CodeQL workflow keeps least-privilege permissions, uses
   `security-extended`, has no privileged checkout of untrusted code, pins actions
   to SHAs, and keeps Swift analysis on macOS. This would complement, not replace,
   actionlint and zizmor.

## Custom CodeQL rules to consider

Custom CodeQL should be used only where data/control-flow analysis is materially
stronger than an exact text or AST check.

High priority:

1. Swift clipboard content to persistence/log/network sinks

   Sources: `PasteboardSnapshot.text`, `PasteboardRead.snapshot`, and
   `Transformer.transform` output.

   Sinks: `UserDefaults.set`, `FileManager`, `Data.write`, `String.write`,
   `NSKeyedArchiver`, `NSLog`, `os_log`, logger calls, `URLSession`, `URLRequest`,
   `NSWorkspace.open`, `WKWebView` load APIs, and `Process` arguments/environment.

   Why CodeQL: the current `check-no-content-logging` is token-based and can miss
   aliases or helper functions. This rule would encode xPare's actual
   clipboard-content source model.

2. Swift stale pasteboard write after async work

   Flag paths where data read with a pasteboard `changeCount` flows through an
   async transform operation and then reaches `writePlain` without a dominating
   `pasteboard.changeCount == read.changeCount` check.

   Why CodeQL: this is a control-flow invariant over async boundaries. Tests cover
   known paths, but a query could catch a new command path that bypasses the shared
   controller machinery.

3. Swift FFI output buffer ownership

   Flag `xp_transform` success paths where the returned `(outPtr, outLen)` is read
   without a guaranteed `xp_buffer_free(base, outLen)` on all exits, and flag
   direct `xp_transform` / `xp_buffer_free` calls outside the safe
   `Transformer.swift` wrapper.

   Why CodeQL: this is a classic ownership protocol and is a better fit for
   control-flow analysis than grep. Existing Swift integration tests should remain
   the primary executable proof.

Medium priority:

4. Rust FFI unsafe boundary protocol

   Flag new `unsafe` blocks in `core-ffi` that call pointer APIs before null/size
   checks, omit nearby `SAFETY:` comments, or call core transform logic outside
   `catch_unwind`.

   Why CodeQL: built-in Rust pointer queries may catch some issues, but a
   repo-specific query can encode the FFI protocol. `unsafe_op_in_unsafe_fn`,
   Miri, and tests remain stronger for concrete UB executions.

5. Swift reduction operation in continuous mode

   Flag paths from `.clipboardChanged` to `writePlain` where `TransformConfig`
   can contain reduction operations without the `config.operations.removeAll(where:
   { $0.isReduction })` guard.

   Why CodeQL: useful if the controller grows more entry points. Current tests are
   probably enough until that happens.

Low priority / probably not worth custom CodeQL:

- Workflow pinning and permissions. Existing actionlint/zizmor plus a possible
  `check-codeql-workflow-posture` are lower-noise.
- Transform determinism, canonical ordering, resource envelope, sanitizer
  semantics, and ABI drift. Existing tests, properties, fuzz, Kani, and `xtask`
  checks are the right enforcement layer.
- Generic Rust content logging. The core denies print/debug macros at compile
  time, and the CLI intentionally writes transformed text to stdout, so a generic
  dataflow query is likely to produce noise.

## Decision log

- Treat built-in CodeQL as worthwhile defense in depth, especially for Rust,
  workflow, and script review. Swift data-flow coverage remains desirable but is
  deferred until the Swift extractor completes reliably in CI.
- Prefer `security-extended` for security signal. Avoid `security-and-quality`
  initially because the repo already has strong quality gates and low tolerance
  for noisy required checks.
- Prefer default setup for the first baseline if it can select the desired
  languages. Prefer advanced setup only when custom queries or manual build
  capture are needed.
- Do not add CodeQL to branch protection until after alert triage.
- Add exact `xtask` checks for no-network Swift APIs, shipped command execution,
  SwiftPM dependency drift, Python helper posture, real clipboard test bans, and
  pasteboard write shape before investing in custom CodeQL.

## Evidence packet

- Created and used worktree: `/private/tmp/xPare-codeql-research` on branch
  `codex/codeql-research`.
- Read repo maps and guardrails: `ARCHITECTURE.md`,
  `docs/guardrails/dependency-posture.md`, `docs/agent-workflow.md`,
  `CONTRIBUTING.md`, `SECURITY.md`.
- Audited current enforcement in `xtask/src/main.rs`, `.github/workflows/ci.yml`,
  `.github/workflows/proofs.yml`, `.github/dependabot.yml`, `Makefile`,
  `Cargo.toml`, `shells/macos/Package.swift`, the Swift pasteboard/controller/
  transformer/settings code, `core-ffi/src/lib.rs`, and the Python icon generator.
- Counted analyzed language surfaces: 44 Rust files, 26 Swift files, 1 Python
  script, 4 shell scripts, and 8 workflow/YAML files after adding the CodeQL
  workflow.
- Checked current GitHub/CodeQL documentation for supported languages, query
  suites, built-in Rust/Swift/Actions query coverage, custom-query use cases, and
  compiled-language build modes.

## Proof gaps

This is research only. I did not enable CodeQL, run a CodeQL analysis, write
custom QL, or validate false-positive rates against a real SARIF result set.
Those require a follow-up implementation/baseline pass.
