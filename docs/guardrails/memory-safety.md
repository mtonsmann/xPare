# Guardrail: memory safety

**When to consult:** any change to the core (`core/`), and any change to the FFI
shim (`core-ffi/`). Memory safety is the invariant the whole "untrusted-input
parser you can trust" story rests on.

The model is split deliberately:

- **The core cannot be memory-unsafe.** `core/src/lib.rs` declares
  `#![forbid(unsafe_code)]`, so the compiler rejects *any* `unsafe` block. There is
  nothing to audit — memory-unsafety is impossible by construction. The only
  residual risks for the hand-rolled parsers are **panics** and **hangs**, which are
  pinned down by the fuzz/property/corpus suites (see
  [transform-correctness-and-adversarial-input](transform-correctness-and-adversarial-input.md)).
- **All `unsafe` lives in `core-ffi`,** which is tiny on purpose so it can be audited
  in one sitting. It is the boundary; it cannot `forbid(unsafe_code)`. Instead it
  uses `#![deny(unsafe_op_in_unsafe_fn)]` so every unsafe operation is spelled out
  with a `SAFETY:` justification.

## The rules

### In the core

1. **Never add `unsafe`.** Not a block, not a function, not a downgrade to
   `#![deny(unsafe_code)]` (which can be locally overridden). If you think you need
   `unsafe` for performance, you do not — the core is text processing; keep it safe.
   Any genuine FFI/pointer work belongs in `core-ffi`, never here.
2. **Keep the lint that makes it real.** `#![forbid(unsafe_code)]` must remain the
   crate-level attribute in `core/src/lib.rs`.

### In `core-ffi`

3. **Validate every pointer before use.** Null checks first; initialize all
   out-params to defined values on every return path (the existing `ss_transform`
   sets `*out = null` / `*out_len = 0` before doing anything else).
4. **Never let a panic cross the boundary.** Unwinding across the C ABI is undefined
   behavior. Every call into the core is wrapped in `catch_unwind` and converted to
   `SsStatus::ErrInternal`. Keep it that way; do not call core functions outside the
   guarded region. (`panic = "unwind"` is kept on purpose so `catch_unwind` works —
   see `Cargo.toml`.)
5. **Round-trip ownership exactly.** A buffer handed out by `ss_transform` is a
   leaked `Box<[u8]>` reconstructed only by `ss_buffer_free` with the matching
   `(ptr, len)`. Do not change the allocation scheme on one side only.
6. **Zeroize freed buffers.** `ss_buffer_free` zeroizes before dropping, a
   best-effort wipe of clipboard-derived bytes. Keep that, and prefer minimizing
   transient copies of content elsewhere.
7. **Every `unsafe` op carries a `SAFETY:` comment** explaining why it is sound. No
   silent unsafe.

## Enforcing checks

- `cargo xtask check-unsafe-forbid` — fails if `#![forbid(unsafe_code)]` is missing
  from `core/src/lib.rs` (matched as a trimmed line, so reformatting cannot defeat
  it, but a downgrade to `deny` does).
- `cargo clippy --workspace --all-targets -- -D warnings` — the FFI's
  `deny(unsafe_op_in_unsafe_fn)` and the workspace `-D warnings` catch unjustified or
  sloppy unsafe.
- `cargo test -p safetystrip-ffi` — ABI round-trip and ownership tests.
- The fuzz suite (`fuzz/`) backs the "no panic to catch in the first place" half.

## What a PR must call out

- **Any new `unsafe`** (only ever in `core-ffi`): why it is necessary, the `SAFETY:`
  reasoning, and the test that exercises it. Adding `unsafe` to the core is not a
  PR — it is a non-starter; redesign instead.
- Any change to the buffer ownership/zeroization contract — this is also an
  [FFI/ABI](ffi-boundary-and-abi-stability.md) and
  [privacy](privacy-and-data-handling.md) concern; cross-reference and justify.
