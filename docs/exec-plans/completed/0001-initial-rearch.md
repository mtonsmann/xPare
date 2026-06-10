# Exec Plan 0001 — Initial re-architecture & first working cut

Status: **completed** · Started: 2026-06-04 · Completed: 2026-06-04

## Goal

Stand up the memory-safe plain-text clipboard utility from an empty repo per the
kickoff brief: a portable Rust transformation **core**, a thin language-neutral
**C ABI**, a headless **CLI**, a Swift **macOS shell**, the **knowledge base**
(`ARCHITECTURE`/`DESIGN`/`SECURITY` + guardrails), and the **mechanically-enforced
invariants** wired into CI. Prefer a smaller, fully-working, fully-enforced core
over breadth.

## Environment constraints (discovered)

- Rust stable installed (1.96). Swift 6.3 present but **Command-Line-Tools only —
  no full Xcode**, so `swift build` works while `xcodebuild`/signed `.app` does
  not. The Swift shell is therefore real, compiling source with the UI entry
  wired; final `.app` packaging is documented, not produced (the brief's
  "UI may be stubbed" path).

## Approach: scaffold-then-fan-out

Phase 0 freezes the contracts so work parallelizes without merge conflicts:

1. **Phase 0 (done):** workspace, pinned deps, and compiling **frozen interfaces**
   — the `Config`/`Operation` schema, the `ops::*` function signatures, the C ABI
   (`ss_transform`/`ss_buffer_free`/`ss_abi_version`/`ss_capabilities_json`), and a
   green `build`/`test`/`clippy -D warnings`/`fmt` baseline.
2. **Phase 1 (parallel agents, disjoint file ownership):**
   - A1 — HTML + Markdown strippers, adversarial corpus, stripper tests
   - A2 — whitespace/case/lines ops + pipeline tests (determinism, round-trip, golden, unwrap rule)
   - B  — `core-ffi` header generation (cbindgen), ABI round-trip tests, FFI review
   - C  — CLI hardening + integration tests
   - D  — `xtask` checks + GitHub Actions CI + `CONTRIBUTING.md`
   - E  — cargo-fuzz targets (incl. `arbitrary`-derived configs)
   - F  — Swift macOS shell + reserved windows/linux siblings + entitlements
   - G  — docs: ARCHITECTURE, DESIGN, SECURITY, README, guardrails, extend AGENTS.md
3. **Phase 2 (integration):** full build/test/`cargo xtask ci`/`swift build`, fix
   cross-cutting gaps, self-review to green, move this plan to `completed/`.

## Decision log (full rationale in DESIGN.md)

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| D1 | FFI mechanism | cbindgen + C ABI; **two crates** (`core` pure + `core-ffi` shim) | Language-neutral C ABI is the brief's safe default; the split keeps `#![forbid(unsafe_code)]` true for the core while isolating all `unsafe` to a tiny, auditable shim |
| D2 | ABI surface | `ss_abi_version`, `ss_capabilities_json`, `ss_transform`, `ss_buffer_free` | Narrow + data-driven; a new transform never changes the ABI |
| D3 | Config format | JSON, versioned, **ordered list of internally-tagged operations** | Brief's default; order explicit; new transform = new enum variant, zero ABI change |
| D4 | Error model | `repr(C)` status enum; no global error state; input lossy-UTF-8 decoded; `catch_unwind` at the boundary | Stateless + deterministic + trivially consumable; robust on adversarial bytes; a panic becomes an error code, never UB |
| D5 | HTML stripper | **Hand-rolled** pure-safe-Rust state machine + curated entities | "Reimplement a small subset rather than depend on opaque upstream"; safe Rust ⇒ memory-safe by construction; fuzz proves no panic/hang |
| D6 | Markdown stripper | **pulldown-cmark** (default-features off) | CommonMark is too irregular to reimplement safely; boring, well-audited standard |
| D7 | Buffer ownership | `Box<[u8]>` leaked as `(ptr,len)`; freed + **zeroized** by `ss_buffer_free` | Only needs ptr+len (no capacity); zeroization best-effort wipes clipboard bytes |
| D8 | Continuous mode | owned poller on `changeCount`, fully invalidated+niled when off, 500 ms default | Satisfies "no loop runs when disabled" |
| D9 | Global hotkey | Carbon `RegisterEventHotKey` (default ⌥⌘V) | Needs neither Accessibility nor Input Monitoring |
| D10 | CLI deps | none (hand-rolled arg parsing) | Keep the harness boring and the dependency surface minimal |
| D11 | Enforcement | single portable `xtask` (+ `#![forbid]`, clippy denies, proptest, corpus, fuzz) | No external cargo plugins required; same checks locally and in CI |

## Invariant → mechanism map (§5)

- No unsafe in core → `#![forbid(unsafe_code)]` + `xtask check-unsafe-forbid`
- No OS/IO/net in core → `xtask check-core-deps` (strict allowlist)
- No network anywhere → `xtask check-no-network` (workspace banlist)
- Frozen ABI → checked-in `xpare.h` + `xtask check-abi` (drift fails CI)
- Config is data → serde round-trip + version proptests
- Never panics → fuzz targets + property tests + checked-in adversarial corpus replay
- No log sink → core denies `print*`/`dbg!`; no logging deps
- Determinism → `transform(x,c)==transform(x,c)` proptest
- Minimal entitlements → checked-in entitlements + `xtask check-entitlements`

## Out of scope (this plan)

Paste simulation, Windows/Linux shells (reserved only), WASM/iOS, signing/notarization,
and the "adopt-if-it-grows" harness pieces (doc-GC agents, observability, quality
cadences) — noted in DESIGN.md.

## Outcome

Delivered and green. Built scaffold-first (frozen `Config` schema + `ops::*`
signatures + C ABI), then fanned out the implementation across parallel agents
with disjoint file ownership, then integrated.

- **Core** (`#![forbid(unsafe_code)]`): hand-rolled HTML stripper + pulldown-cmark
  Markdown stripper + whitespace/case/line ops + best-effort extraction. ~115
  tests: stripper regressions, golden, pipeline, config round-trip, and
  determinism/idempotence proptests, plus a checked-in adversarial corpus replay.
- **core-ffi**: narrow C ABI with `catch_unwind` + buffer zeroization; 12
  round-trip tests; checked-in header with a drift check.
- **CLI**: dependency-free headless harness; 22 integration tests.
- **xtask**: `check-unsafe-forbid` / `check-core-deps` / `check-no-network` /
  `check-abi` / `check-entitlements` / `ci`; GitHub Actions runs the gate.
- **fuzz**: three nightly cargo-fuzz targets (incl. arbitrary-derived configs);
  smoke runs found no crashes.
- **Swift macOS shell**: compiles and statically links the core; 30 tests pass
  including a real FFI integration test and continuous-mode teardown; minimal
  app-sandbox entitlements. Signed `.app` packaging documented, not produced
  (Command-Line-Tools-only environment — no full Xcode).
- **Knowledge base**: ARCHITECTURE / DESIGN / SECURITY / README + 7 guardrails;
  AGENTS.md promoted from bootstrap to a live router.

`cargo xtask ci` passes end-to-end (fmt, clippy `-D warnings`, all workspace
tests, every structural invariant). `swift test` passes.

### Bugs caught by the harness during integration

- **Non-idempotent line ops** (`dedupe_lines("\r\r\n")` → `"\r\n"` → `"\n"`): the
  shared `split_lines` stripped only one trailing `\r`, so a content line could
  re-form a CRLF and re-strip. The idempotence proptest caught it; fixed by
  stripping the whole trailing `\r` run. Regression seed checked in.
- **Over-strong stripper test**: an initial test expected `strip_markdown` alone to
  drop a `<script>` body embedded in raw HTML. pulldown-cmark reparses the body as
  Markdown text, so this is neither achievable nor correct for `strip_markdown` —
  `strip_html` is the script-neutralizing workhorse. Reframed the test and the docs
  around the canonical `StripHtml → StripMarkdown` order.

### Follow-ups (not blocking)

- `strip_markdown` could accumulate consecutive raw-HTML *block* events and strip
  them as one unit (dropping block-level `<script>`/`<style>` in embedded HTML);
  deferred — `strip_html` already covers the security need and is what the shell runs.
- Run longer fuzz campaigns as a nightly CI job (currently smoke only).
- Package and run the real menu-bar `.app` once a full Xcode toolchain is available.
