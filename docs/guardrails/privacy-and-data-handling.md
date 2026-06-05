# Guardrail: privacy & data handling

**When to consult:** anything touching how clipboard **content** is handled —
network, persistence, logging/telemetry, in-memory lifetime, entitlements, or any
new data path. This is the guardrail behind [`SECURITY.md`](../../SECURITY.md).

The one promise: **clipboard content never leaves the process, is never persisted,
and is never logged.** Content lives in memory only for the duration of a transform
and is wiped from the buffer that crosses the boundary.

## The rules

1. **No network. Anywhere.** Not in the core, not in any crate that could be linked
   into a shipped artifact or run at build time. There must be no code path that can
   open a socket. (Enforced by `check-no-network` over the whole workspace tree, and
   by the macOS sandbox granting no network entitlement.)
2. **No persistence of content.** Clipboard text is never written to a file,
   database, cache, temp file, defaults/registry, or any durable store. The only disk
   I/O the project does is the CLI reading a **config** file (`--config <path>`) and
   the shell persisting **settings** — never content.
3. **No logging of content.** Clipboard text must never reach a log/console/telemetry
   sink. The core makes this a *compile error* (`#![deny(clippy::print_stdout,
   clippy::print_stderr, clippy::dbg_macro)]` and no logging dependency). In the CLI,
   diagnostics go to **stderr** and only transformed text goes to **stdout**, so the
   two streams never mix. Shells must not log content either.
4. **In-memory only + best-effort wipe.** Content exists only as in-memory strings
   during a transform. `ss_buffer_free` zeroizes the returned buffer before freeing
   it. Minimize transient copies of content; do not stash it in a global, a cache, or
   a long-lived object.
5. **No telemetry / analytics / "phone home."** There is nothing to add here — there
   is no code that could, and there must not be.
6. **Settings are configuration, not content.** Persisting the user's pipeline,
   hotkey, and interval is fine. Persisting what was on the clipboard is not.
7. **Minimal OS privilege.** The macOS shell ships only the App Sandbox entitlement;
   no network/device/personal-info/file/automation/accessibility grants. See
   [macos-posture](macos-posture.md).

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
| In-memory only / wipe | buffer zeroized in `ss_buffer_free`; covered by `cargo test -p safetystrip-ffi` |
| Minimal entitlements | `cargo xtask check-entitlements` |

All of these are part of `cargo xtask ci`, which CI runs verbatim.

## What a PR must call out

Any of the following is a **posture change** — call it out explicitly, justify it,
update `SECURITY.md` and the relevant guardrail, and update the enforcing check to
*match* the new (justified) posture, never to hide a regression:

- a new dependency capable of network access (or that pulls one in),
- any new persistence of content, any new log/telemetry path, or a new data path
  that lets content escape the transform,
- a new entitlement,
- weakening the core's `print*`/`dbg!` denies or the zeroization.
