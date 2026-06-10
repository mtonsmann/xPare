# Guardrail: privacy & data handling

**When to consult:** anything touching how clipboard **content** is handled —
network, persistence, logging/telemetry, in-memory lifetime, entitlements, or any
new data path. This is the guardrail behind [`SECURITY.md`](../../SECURITY.md). The
mechanical content-logging / clipboard-safety checks have their own guardrail:
[content-logging-and-clipboard-safety](content-logging-and-clipboard-safety.md).

The one promise: **clipboard content never leaves the process, is never persisted,
and is never logged.** Content lives in memory only for the duration of a transform
and is wiped from the buffer that crosses the boundary. Persistence has exactly one
sanctioned, opt-in exception — the paste-as-file feature — bounded by rule 2 below.

## The rules

1. **No network. Anywhere.** Not in the core, not in any crate that could be linked
   into a shipped artifact or run at build time. There must be no code path that can
   open a socket. (Enforced by `check-no-network` over the whole workspace tree, and
   by the macOS sandbox granting no network entitlement.)
2. **No persistence of content — with one sanctioned, opt-in exception.** Clipboard
   text is never written to a file, database, cache, temp file, defaults/registry,
   or any durable store. The only disk I/O the project does is the CLI reading a
   **config** file (`--config <path>`), the shell persisting **settings**, and the
   **paste-as-file exception**: when the user has enabled *Paste large clipboards
   as a file* (off by default) and a transformed result exceeds their threshold,
   `PasteFileStore` — the single audited writer — persists that result so the
   pasteboard can hold a file reference. Its constraints (one file at a time,
   owner-only, sandbox-container temp dir, Spotlight/backup-excluded, deleted when
   the pasteboard moves on / on launch / on quit) are documented in `SECURITY.md`
   ("Opt-in paste-as-file exception") and enforced by `check-no-content-logging`,
   which honors the `xpare:allow-content-persistence` marker **only** in
   `PasteFileStore.swift`. No other content persistence is permitted, and the
   exception must never grow a second writer without repeating this whole
   posture-change exercise.
3. **No logging of content.** Clipboard text must never reach a log/console/telemetry
   sink. The core makes this a *compile error* (`#![deny(clippy::print_stdout,
   clippy::print_stderr, clippy::dbg_macro)]` and no logging dependency). In the CLI,
   diagnostics go to **stderr** and only transformed text goes to **stdout**, so the
   two streams never mix. Shells must not log content either.
4. **In-memory only + best-effort wipe.** Content exists only as in-memory strings
   during a transform. The core holds each pipeline intermediate in a `Zeroizing`
   buffer (wiped on drop) and `xp_buffer_free` zeroizes the returned buffer before
   freeing it. If the FFI has to allocate an owned lossy-UTF-8 replacement string,
   that temporary is also `Zeroizing` and wiped on drop. Minimize transient copies of
   content; do not stash it in a global, a cache, or a long-lived object. Private
   fused-operation scratch buffers that hold clipboard-derived bytes are covered by
   the same rule, but the boundary is storage lifetime rather than every logical
   reuse: keep them borrowed-only, wrap them in `Zeroizing`, or explicitly wipe them
   before capacity growth/reallocation and drop. Operation output accumulators
   follow the same lifetime rule: either pre-size them to a provably sufficient
   capacity (pinned by property tests) so they never reallocate mid-construction,
   or route growth through the wipe-on-grow append helper (`core/src/ops/wipe.rs`),
   which zeroizes a superseded allocation before the allocator reclaims it. The
   wipe remains **best-effort** — the residual gaps (returned values before
   wrapping, third-party parser internals, comparison keys, OS paging/registers)
   are enumerated in `core/src/pipeline.rs`'s module doc and `SECURITY.md`.
5. **No telemetry / analytics / "phone home."** There is nothing to add here — there
   is no code that could, and there must not be.
6. **Settings are configuration, not content.** Persisting the user's pipeline,
   hotkey, and interval is fine. Persisting what was on the clipboard is not.
7. **Minimal OS privilege.** The macOS shell ships only the App Sandbox entitlement;
   official Developer ID releases must be signed with the checked-in App Sandbox
   entitlements and verify the signed payload is still minimal; no network, device,
   personal-info, file, automation, or accessibility grants. See
   [macos-posture](macos-posture.md).
8. **Local pasteboard writers are out of the confidentiality boundary.** A same-user
   process that can write the system pasteboard can race xPare or feed huge
   rich data. That can cause missed intermediate states or local DoS pressure; it
   must not create exfiltration, persistence, logging, or memory-unsafety.
9. **Resource limits apply before the core transform, after platform extraction.**
   Native shells must refuse oversized extracted text before calling the core, and
   the FFI has its own hard backstop. When a platform exposes raw rich representation
   bytes, check them before decoding; still do not document the ceiling as a
   universal streaming pre-parse limit for every native format.

## Why each rule has teeth

The clipboard routinely holds passwords, tokens, private keys, PII, and source. The
rules above are chosen so that the *capability* to misuse that data does not exist in
the build — not so that the code merely chooses not to. The core literally cannot
reach the network or a log; the sandbox cannot reach the network; the surface that
could is the small, audited FFI shim and the shells, both covered by their own
guardrails.

## Enforcing checks

| Rule | Check |
|---|---|
| No network anywhere | `cargo xtask check-no-network` (banlist across the whole tree) + sandbox has no network entitlement |
| Core has no OS/filesystem/network deps | `cargo xtask check-core-deps` (strict transitive allowlist) |
| No log sink in the core | `#![deny(clippy::print_stdout, print_stderr, dbg_macro)]` + `clippy -D warnings` + no logging crate |
| No content logged/persisted (incl. shells) | `cargo xtask check-no-content-logging` (scans shipped Rust + Swift source; the paste-as-file allow-marker is honored only in `PasteFileStore.swift`) |
| Default checks avoid the real clipboard | `cargo xtask check-clipboard-safety` |
| In-memory only / wipe | pipeline intermediates in `Zeroizing`; fused pipeline scratch storage zeroized before release/reallocation and drop; output buffer zeroized in `xp_buffer_free`; covered by `cargo test` and `cargo xtask check-pipeline-zeroization` |
| Minimal entitlements | `cargo xtask check-entitlements`; `cargo xtask check-release-posture`; `release.sh dist` signs executable and bundle with the checked-in entitlements and verifies both signed payloads |

All of these are part of `cargo xtask ci`, which CI runs verbatim.

## What a PR must call out

Any of the following is a **posture change** — call it out explicitly, justify it,
update `SECURITY.md` and the relevant guardrail, and update the enforcing check to
*match* the new (justified) posture, never to hide a regression:

- a new dependency capable of network access (or that pulls one in),
- any new persistence of content, any new log/telemetry path, or a new data path
  that lets content escape the transform,
- a new entitlement,
- weakening the core's `print*`/`dbg!` denies, pipeline intermediate wiping, or
  fused scratch storage wipe-before-release posture.
