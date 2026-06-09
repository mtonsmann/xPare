//! Thin, language-neutral C ABI over `xpare-core`.
//!
//! This is the **only** crate permitted to use `unsafe`, and the surface is kept
//! deliberately tiny so it can be audited in one sitting:
//!
//! * [`ss_abi_version`]      — integer ABI version for capability negotiation.
//! * [`ss_capabilities_json`] — static JSON describing supported transforms.
//! * [`ss_transform`]        — `transform(input, config) -> output`.
//! * [`ss_buffer_free`]      — free (and zeroize) a buffer returned by `ss_transform`.
//!
//! Adding or changing a *transform* never changes this ABI: feature selection
//! crosses as the `config_json` string. Any change to the signatures or the enum
//! below is a compatibility event — bump [`XP_ABI_VERSION`], regenerate the header
//! (`cargo xtask gen-header`), and call it out in the PR.
//!
//! Safety model: every entry point validates its pointers, decodes input UTF-8
//! losslessly (so adversarial bytes can never make it fail), and wraps the call to
//! the core in `catch_unwind` so a panic becomes [`SsStatus::ErrInternal`] instead
//! of unwinding across the FFI boundary (which is undefined behavior).

// We cannot `forbid(unsafe_code)` here — this is the boundary. Instead we force
// every unsafe operation to be spelled out explicitly with a SAFETY justification.
#![deny(unsafe_op_in_unsafe_fn)]
// (`clippy::dbg_macro` is denied workspace-wide; `print_*` stay FFI-specific here.)
#![deny(clippy::print_stdout, clippy::print_stderr)]
// The C ABI is the audited surface; every exported item must be documented.
#![deny(missing_docs)]

use std::borrow::Cow;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::OnceLock;

use zeroize::{Zeroize, Zeroizing};

/// Version of this C ABI. Bump on **any** change to the function signatures,
/// struct/enum layouts, or memory-ownership contract below. Adding a transform is
/// NOT an ABI change and must NOT bump this.
///
/// History: v1 → v2 added [`XP_MAX_INPUT_BYTES`] and a new trailing status,
/// [`SsStatus::ErrInputTooLarge`]. Existing status values are unchanged, so a v1
/// caller still interprets `Ok`/`ErrNullArg`/`ErrInvalidConfig`/`ErrInternal`
/// correctly; only the new size ceiling is added.
pub const XP_ABI_VERSION: u32 = 2;

/// Hard upper bound, in bytes, on the input [`ss_transform`] will accept. A larger
/// input returns [`SsStatus::ErrInputTooLarge`] *before* anything is read or
/// allocated, so a pathological size can never cause an out-of-memory abort or an
/// allocation-size overflow at the boundary.
///
/// This is a **generous, platform-neutral backstop, not the everyday limit.** A
/// transform's peak working set is a few times its input, so the real ceiling is
/// memory-bound: the native shell enforces a tighter, RAM-proportional limit (it is
/// the only layer permitted to ask the OS how much memory exists) and refuses an
/// oversized clipboard gracefully. The headless CLI is intentionally uncapped, for
/// large file/log work where the caller manages its own memory.
///
/// Written as a plain decimal literal (not `2 * 1024 * 1024 * 1024`) so the
/// cbindgen-generated `#define` is a single 64-bit-typed C constant: the expression
/// form would be evaluated in 32-bit `int` by a C compiler and overflow (2^31 >
/// INT_MAX), corrupting the value for C/C++ consumers and the Swift macro importer.
pub const XP_MAX_INPUT_BYTES: usize = 2_147_483_648; // 2 GiB (2 * 1024^3)

/// Result status for [`ss_transform`]. `repr(C)` so it is a plain C enum.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsStatus {
    /// Success. `*out` / `*out_len` describe a buffer the caller must free.
    Ok = 0,
    /// A required pointer argument was null.
    ErrNullArg = 1,
    /// `config_json` was not valid UTF-8, not valid JSON, or an unsupported version.
    ErrInvalidConfig = 2,
    /// An unexpected internal error (e.g. a caught panic). Should never happen.
    ErrInternal = 3,
    /// `input_len` exceeded [`XP_MAX_INPUT_BYTES`]; nothing was read, allocated, or
    /// transformed. (Added in ABI v2.)
    ErrInputTooLarge = 4,
}

/// Returns the integer ABI version. See [`XP_ABI_VERSION`].
#[no_mangle]
pub extern "C" fn ss_abi_version() -> u32 {
    XP_ABI_VERSION
}

/// Returns a pointer to a static, NUL-terminated UTF-8 JSON string describing this
/// core's capabilities (name, version, config schema version, supported operations).
///
/// The returned pointer is valid for the lifetime of the process and **must not be
/// freed**. Never returns null.
#[no_mangle]
pub extern "C" fn ss_capabilities_json() -> *const c_char {
    static CAPS: OnceLock<CString> = OnceLock::new();
    // The capabilities JSON is ASCII and contains no interior NUL, so `CString::new`
    // cannot fail in practice; `unwrap_or_default` keeps this panic-free regardless.
    CAPS.get_or_init(|| CString::new(xpare_core::capabilities()).unwrap_or_default())
        .as_ptr()
}

/// Transform `input` according to `config_json`.
///
/// * `input` / `input_len` — UTF-8 text. Invalid UTF-8 is decoded losslessly
///   (replacement characters) rather than rejected. `input` may be null only if
///   `input_len` is 0.
/// * `config_json` — NUL-terminated UTF-8 JSON config. Must not be null.
/// * `out` / `out_len` — on `Ok`, `*out` receives a heap buffer of `*out_len` UTF-8
///   bytes (not NUL-terminated) that the caller must release with [`ss_buffer_free`].
///   On any error, `*out` is set to null and `*out_len` to 0. Both must not be null.
///
/// Inputs larger than [`XP_MAX_INPUT_BYTES`] return [`SsStatus::ErrInputTooLarge`]
/// without reading `input` or allocating.
///
/// # Safety
/// `input` must be valid for reads of `input_len` bytes (or null with `input_len`
/// 0); `config_json` must point to a valid NUL-terminated string; `out` and
/// `out_len` must be valid for writes.
#[no_mangle]
pub unsafe extern "C" fn ss_transform(
    input: *const u8,
    input_len: usize,
    config_json: *const c_char,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> SsStatus {
    // Out-params must be writable to report anything at all.
    if out.is_null() || out_len.is_null() {
        return SsStatus::ErrNullArg;
    }
    // SAFETY: `out`/`out_len` are non-null and the caller guarantees they are valid
    // for writes. Initialize them so the caller has defined values on every path.
    unsafe {
        *out = std::ptr::null_mut();
        *out_len = 0;
    }

    if config_json.is_null() || (input.is_null() && input_len != 0) {
        return SsStatus::ErrNullArg;
    }

    // Reject pathologically large inputs BEFORE reading `input` or allocating, so an
    // absurd size cannot trigger an out-of-memory abort or an allocation overflow.
    if input_len > XP_MAX_INPUT_BYTES {
        return SsStatus::ErrInputTooLarge;
    }

    // SAFETY: `input` is non-null and valid for `input_len` bytes, or `input_len`
    // is 0 (in which case we use an empty slice and never read `input`).
    let input_bytes: &[u8] = if input_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(input, input_len) }
    };
    let input_text = LossyInput::new(input_bytes);

    // SAFETY: `config_json` is non-null and the caller guarantees it is a valid
    // NUL-terminated string.
    let config_cstr = unsafe { CStr::from_ptr(config_json) };
    let config_str = match config_cstr.to_str() {
        Ok(s) => s,
        Err(_) => return SsStatus::ErrInvalidConfig,
    };
    let config = match xpare_core::parse_config(config_str) {
        Ok(c) => c,
        Err(_) => return SsStatus::ErrInvalidConfig,
    };

    // Defense in depth: the core is fuzzed to never panic, but a panic must never
    // unwind across the FFI boundary. Convert any panic to ErrInternal.
    let output = match catch_unwind(AssertUnwindSafe(|| {
        xpare_core::transform(input_text.as_str(), &config)
    })) {
        Ok(text) => text,
        Err(_) => return SsStatus::ErrInternal,
    };

    let (ptr, len) = into_c_buffer(output);
    // SAFETY: `out`/`out_len` validated non-null above and guaranteed writable.
    unsafe {
        *out = ptr;
        *out_len = len;
    }
    SsStatus::Ok
}

/// Free a buffer returned by [`ss_transform`], zeroizing it first so clipboard-derived
/// bytes do not linger in freed memory.
///
/// `ptr`/`len` must be exactly the values produced by a single `ss_transform` call,
/// and must be freed at most once. A null `ptr` is a no-op.
///
/// # Safety
/// `ptr` must originate from `ss_transform`'s `*out` with the matching `len`, not be
/// used afterwards, and not be freed more than once.
#[no_mangle]
pub unsafe extern "C" fn ss_buffer_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    let slice_ptr: *mut [u8] = std::ptr::slice_from_raw_parts_mut(ptr, len);
    // SAFETY: `slice_ptr` reconstructs the exact `Box<[u8]>` that `into_c_buffer`
    // leaked via `Box::into_raw`. The caller guarantees the pointer/len pair is
    // unmodified and freed only once.
    let mut boxed: Box<[u8]> = unsafe { Box::from_raw(slice_ptr) };
    boxed.zeroize();
    drop(boxed);
}

/// Convert an owned `String` into a raw `(ptr, len)` over a leaked `Box<[u8]>`.
/// Pure safe Rust: the only `unsafe` is reclaiming this in [`ss_buffer_free`].
fn into_c_buffer(s: String) -> (*mut u8, usize) {
    let boxed: Box<[u8]> = s.into_bytes().into_boxed_slice();
    let len = boxed.len();
    let ptr = Box::into_raw(boxed).cast::<u8>();
    (ptr, len)
}

enum LossyInput<'a> {
    Borrowed(&'a str),
    Owned(Zeroizing<String>),
}

impl<'a> LossyInput<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        match String::from_utf8_lossy(bytes) {
            Cow::Borrowed(text) => Self::Borrowed(text),
            Cow::Owned(text) => Self::Owned(Zeroizing::new(text)),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(text) => text,
            Self::Owned(text) => text.as_str(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LossyInput;

    #[test]
    fn valid_utf8_input_stays_borrowed() {
        let input = LossyInput::new("valid UTF-8".as_bytes());
        assert!(matches!(&input, LossyInput::Borrowed("valid UTF-8")));
    }

    #[test]
    fn invalid_utf8_input_uses_zeroizing_owned_copy() {
        let input = LossyInput::new(&[0xff, b'A']);
        assert!(matches!(&input, LossyInput::Owned(_)));
        assert_eq!(input.as_str(), "\u{fffd}A");
    }
}
