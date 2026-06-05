//! End-to-end tests of the C ABI, driven through the actual `extern "C"` entry
//! points (reachable here because the crate is also built as an `rlib`).
//!
//! These prove the *boundary* contract — pointer validation, the buffer/ownership
//! protocol, the lossy-UTF-8 input handling, and the `SsStatus` error model — and
//! that none of it can panic. The transform *logic* itself is owned and tested by
//! `safetystrip-core`; where output correctness matters we compare against
//! `safetystrip_core::transform` rather than hardcoding a brittle expected string,
//! which simultaneously asserts the FFI faithfully relays `(input, config)`.
//!
//! Every `unsafe` call is funnelled through a tiny safe helper below with a SAFETY
//! comment, so the individual `#[test]`s stay readable and unsafe-free.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use safetystrip_ffi::{
    ss_abi_version, ss_buffer_free, ss_capabilities_json, ss_transform, SsStatus,
    SS_MAX_INPUT_BYTES,
};

// ---------------------------------------------------------------------------
// Safe helpers around the unsafe FFI surface.
// ---------------------------------------------------------------------------

/// Build a NUL-terminated config string for passing as `*const c_char`.
///
/// Panics in the *test* (not the library) if `json` contains an interior NUL,
/// which would only ever be a bug in the test inputs themselves.
fn config(json: &str) -> CString {
    CString::new(json).expect("test config must not contain an interior NUL")
}

/// Read the static capabilities C string as a Rust `&str`.
fn capabilities_str() -> &'static str {
    // SAFETY: `ss_capabilities_json` is documented to return a non-null pointer to
    // a static, process-lifetime, NUL-terminated UTF-8 string that must not be
    // freed. We borrow it as `'static` and never free it, matching that contract.
    let ptr = ss_capabilities_json();
    assert!(
        !ptr.is_null(),
        "ss_capabilities_json must never return null"
    );
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("capabilities JSON must be valid UTF-8")
}

/// Outcome of a successful `ss_transform`: the output bytes copied out of the
/// FFI-owned buffer, which has already been freed via `ss_buffer_free`.
struct TransformOk {
    status: SsStatus,
    bytes: Vec<u8>,
}

/// Drive `ss_transform` over the given input/config, copy the output buffer out,
/// then free it through `ss_buffer_free`. The returned `bytes` therefore outlive
/// the FFI allocation, so callers can assert on them safely.
///
/// `input` is passed exactly as given (including an empty slice), so the
/// `(ptr, len)` the library sees mirrors a real caller's.
fn transform(input: &[u8], cfg: &CStr) -> TransformOk {
    let input_ptr = input.as_ptr();
    let mut out: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 0;

    // SAFETY: `input_ptr`/`input.len()` describe a valid readable slice; `cfg` is a
    // valid NUL-terminated string for the duration of the call; `out`/`out_len` are
    // local, non-null, and valid for writes. All preconditions of `ss_transform`
    // hold, so the call is sound.
    let status =
        unsafe { ss_transform(input_ptr, input.len(), cfg.as_ptr(), &mut out, &mut out_len) };

    let bytes = if status == SsStatus::Ok {
        // On Ok the library guarantees `out`/`out_len` describe a heap buffer it
        // owns. Copy the bytes out *before* freeing so the returned Vec is
        // independent of the (about-to-be-freed) FFI allocation.
        let copied = copy_out(out, out_len);
        free(out, out_len);
        copied
    } else {
        // On any error the contract is `*out == null` and `*out_len == 0`.
        assert!(out.is_null(), "on error *out must be null");
        assert_eq!(out_len, 0, "on error *out_len must be 0");
        Vec::new()
    };

    TransformOk { status, bytes }
}

/// Copy `len` bytes out of an FFI-owned buffer into an owned `Vec<u8>`.
///
/// A zero-length output is represented as an empty `Vec` regardless of whether
/// `ptr` is null or a non-null (still-freeable) pointer.
fn copy_out(ptr: *const u8, len: usize) -> Vec<u8> {
    if len == 0 {
        return Vec::new();
    }
    assert!(
        !ptr.is_null(),
        "non-zero out_len must come with a non-null ptr"
    );
    // SAFETY: on `Ok` with `len != 0`, the library guarantees `ptr` points to `len`
    // initialized bytes it owns. We only read them (into a fresh Vec) and do not
    // alias or free here; freeing happens exactly once via `free`.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    slice.to_vec()
}

/// Free a buffer produced by `ss_transform` via the matching `ss_buffer_free`.
fn free(ptr: *mut u8, len: usize) {
    // SAFETY: `ptr`/`len` are exactly the pair `ss_transform` wrote into the
    // out-params for this call (or null/0, which `ss_buffer_free` treats as a
    // no-op). We free at most once per allocation and never use it afterwards.
    unsafe { ss_buffer_free(ptr, len) };
}

/// A config that performs no operations: the identity transform (echo, modulo the
/// lossy UTF-8 decode the boundary always applies to input).
const IDENTITY_CFG: &str = r#"{"version":1,"operations":[]}"#;

// ---------------------------------------------------------------------------
// 1. ABI version.
// ---------------------------------------------------------------------------

#[test]
fn abi_version_is_two() {
    assert_eq!(ss_abi_version(), 2);
    // Cross-check against the public constant so a bump can never silently pass.
    assert_eq!(ss_abi_version(), safetystrip_ffi::SS_ABI_VERSION);
}

// ---------------------------------------------------------------------------
// 1b. Input size ceiling (ABI v2).
// ---------------------------------------------------------------------------

#[test]
fn oversized_input_is_rejected_before_read_or_alloc() {
    // Do NOT allocate >2 GiB: the size check runs *before* `input` is read, so we pass
    // a tiny valid buffer with an over-limit `input_len`. `ss_transform` must return
    // ErrInputTooLarge without dereferencing the pointer or allocating.
    let tiny = [0u8; 1];
    let cfg = config(IDENTITY_CFG);
    let mut out: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 7; // sentinel — must be cleared to 0 on error
                                // SAFETY: `input_len` > SS_MAX_INPUT_BYTES, so the size check returns before
                                // `input` is read; the buffer is valid for 1 byte regardless, `cfg` is a valid
                                // NUL-terminated string, and the out-params are valid for writes.
    let status = unsafe {
        ss_transform(
            tiny.as_ptr(),
            SS_MAX_INPUT_BYTES + 1,
            cfg.as_ptr(),
            &mut out,
            &mut out_len,
        )
    };
    assert_eq!(status, SsStatus::ErrInputTooLarge);
    assert!(out.is_null(), "on error *out must be null");
    assert_eq!(out_len, 0, "on error *out_len must be reset to 0");
}

// ---------------------------------------------------------------------------
// 2. Capabilities JSON.
// ---------------------------------------------------------------------------

#[test]
fn capabilities_json_matches_core_and_describes_schema() {
    let caps = capabilities_str();

    // Simplest faithful check, per the brief: the C string is exactly the core's
    // self-description. No JSON dependency needed.
    assert_eq!(
        caps,
        safetystrip_core::capabilities(),
        "ss_capabilities_json must relay safetystrip_core::capabilities() verbatim"
    );

    // And it must actually be the capabilities document: an `operations` array and
    // a `config_version` matching the core's CONFIG_VERSION (1). Checked via string
    // search to avoid pulling in a JSON parser as a dependency.
    assert!(
        caps.contains("\"operations\":["),
        "capabilities must contain an operations array, got: {caps}"
    );
    let expected_version = format!("\"config_version\":{}", safetystrip_core::CONFIG_VERSION);
    assert!(
        caps.contains(&expected_version),
        "capabilities must report config_version {}, got: {caps}",
        safetystrip_core::CONFIG_VERSION
    );

    // Calling twice must yield the same (cached) static string, byte-for-byte.
    assert_eq!(caps, capabilities_str());
}

// ---------------------------------------------------------------------------
// 3. Happy path: a real transform round-trips through the buffer protocol.
// ---------------------------------------------------------------------------

#[test]
fn transform_happy_path_strip_html_then_collapse_whitespace() {
    let cfg_json =
        r#"{"version":1,"operations":[{"op":"strip_html"},{"op":"collapse_whitespace"}]}"#;
    let cfg = config(cfg_json);
    let input = b"<p>Hi   there</p>";

    let result = transform(input, &cfg);
    assert_eq!(result.status, SsStatus::Ok);

    let out = String::from_utf8(result.bytes).expect("output must be valid UTF-8");

    // Concrete, documented expectation: `<p>` is a block element (newline at start
    // and end), the whole-document leading/trailing whitespace is trimmed, then the
    // internal run of spaces collapses to one.
    assert_eq!(out, "Hi there");

    // Also assert the FFI relayed `(input, config)` faithfully by reproducing the
    // expected output directly from the core with the same parsed config.
    let core_cfg = safetystrip_core::parse_config(cfg_json).expect("config parses");
    let expected = safetystrip_core::transform(&String::from_utf8_lossy(input), &core_cfg);
    assert_eq!(out, expected);
}

// ---------------------------------------------------------------------------
// 4. Empty input (null pointer, zero length) is Ok with an empty, freeable output.
// ---------------------------------------------------------------------------

#[test]
fn transform_empty_null_input_is_ok_and_freeable() {
    let cfg = config(IDENTITY_CFG);

    let mut out: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 1; // deliberately non-zero to prove it gets reset.

    // SAFETY: `input` is null *with* `input_len == 0`, which the contract explicitly
    // permits (the library never reads the pointer). `cfg` is a valid NUL-terminated
    // string; `out`/`out_len` are valid for writes.
    let status = unsafe { ss_transform(std::ptr::null(), 0, cfg.as_ptr(), &mut out, &mut out_len) };

    assert_eq!(status, SsStatus::Ok);
    assert_eq!(out_len, 0, "empty input must yield out_len == 0");

    // `out` may be null or a non-null zero-length pointer; either way it must be
    // safe to hand to ss_buffer_free exactly once.
    let copied = copy_out(out, out_len);
    assert!(copied.is_empty());
    free(out, out_len);
}

// ---------------------------------------------------------------------------
// 5. Invalid UTF-8 input is decoded losslessly — never an error, never a panic.
// ---------------------------------------------------------------------------

#[test]
fn transform_lossy_utf8_input_does_not_panic() {
    let cfg = config(IDENTITY_CFG);
    // 0xff,0xfe are invalid UTF-8 start bytes; 0x41 is 'A'.
    let input = [0xffu8, 0xfe, 0x41];

    let result = transform(&input, &cfg);
    assert_eq!(result.status, SsStatus::Ok);

    // The identity transform echoes the lossily-decoded text. Two invalid bytes
    // become two U+FFFD replacement chars, followed by 'A'.
    let out = String::from_utf8(result.bytes).expect("lossy-decoded output is valid UTF-8");
    assert_eq!(out, "\u{fffd}\u{fffd}A");
}

// ---------------------------------------------------------------------------
// 6. Error paths. Each asserts the exact status AND that the out-params are
//    cleared (`*out == null`, `*out_len == 0`).
// ---------------------------------------------------------------------------

/// Invoke `ss_transform` with raw arguments and assert it returns `expected`,
/// leaving `*out`/`*out_len` cleared. Used for the null/invalid-arg cases where we
/// must control each pointer individually.
///
/// # Safety
/// `input`/`config_json` must satisfy `ss_transform`'s preconditions *for the case
/// under test*; the deliberately-invalid argument is the one being exercised.
unsafe fn assert_transform_err(
    input: *const u8,
    input_len: usize,
    config_json: *const c_char,
    expected: SsStatus,
) {
    let mut out: *mut u8 = (&mut 0u8) as *mut u8; // non-null sentinel; must be cleared.
    let mut out_len: usize = 7; // non-zero sentinel; must be cleared.

    // SAFETY: forwarded by the caller's contract (see this fn's # Safety). `out`/
    // `out_len` are valid for writes.
    let status = unsafe { ss_transform(input, input_len, config_json, &mut out, &mut out_len) };

    assert_eq!(status, expected, "unexpected status for error case");
    assert!(out.is_null(), "error path must set *out to null");
    assert_eq!(out_len, 0, "error path must set *out_len to 0");
}

#[test]
fn transform_null_config_is_null_arg() {
    // SAFETY: input is a valid (empty) slice; only `config_json` is null, which is
    // exactly the precondition violation under test. The fn validates and returns
    // before dereferencing the null config.
    unsafe {
        assert_transform_err(b"x".as_ptr(), 1, std::ptr::null(), SsStatus::ErrNullArg);
    }
}

#[test]
fn transform_null_out_pointer_is_null_arg() {
    let cfg = config(IDENTITY_CFG);
    let input = b"hello";
    let mut out_len: usize = 9;

    // SAFETY: `out` is null (the precondition violation under test); `input`/`cfg`
    // are valid; `out_len` is valid for writes. The fn checks `out.is_null()` first
    // and returns without dereferencing it.
    let status = unsafe {
        ss_transform(
            input.as_ptr(),
            input.len(),
            cfg.as_ptr(),
            std::ptr::null_mut(),
            &mut out_len,
        )
    };
    assert_eq!(status, SsStatus::ErrNullArg);
}

#[test]
fn transform_null_out_len_pointer_is_null_arg() {
    let cfg = config(IDENTITY_CFG);
    let input = b"hello";
    let mut out: *mut u8 = std::ptr::null_mut();

    // SAFETY: `out_len` is null (the precondition violation under test); `input`/
    // `cfg`/`out` are valid. The fn checks `out_len.is_null()` first and returns
    // without dereferencing it.
    let status = unsafe {
        ss_transform(
            input.as_ptr(),
            input.len(),
            cfg.as_ptr(),
            &mut out,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(status, SsStatus::ErrNullArg);
    assert!(out.is_null(), "out must be untouched / null");
}

#[test]
fn transform_null_input_with_nonzero_len_is_null_arg() {
    let cfg = config(IDENTITY_CFG);
    // SAFETY: input is null but input_len != 0 — the precondition violation under
    // test. `cfg` is valid. The fn detects this combination and returns before
    // constructing a slice from the null pointer.
    unsafe {
        assert_transform_err(std::ptr::null(), 4, cfg.as_ptr(), SsStatus::ErrNullArg);
    }
}

#[test]
fn transform_malformed_json_config_is_invalid_config() {
    let cfg = config("{ this is not json ");
    let result = transform(b"hello", &cfg);
    assert_eq!(result.status, SsStatus::ErrInvalidConfig);
    assert!(result.bytes.is_empty());
}

#[test]
fn transform_unsupported_version_is_invalid_config() {
    let cfg = config(r#"{"version":999,"operations":[]}"#);
    let result = transform(b"hello", &cfg);
    assert_eq!(result.status, SsStatus::ErrInvalidConfig);
    assert!(result.bytes.is_empty());
}

// ---------------------------------------------------------------------------
// 7. Fuzz-lite: many pseudo-random byte inputs through the boundary. Guards the
//    FFI shim itself (pointer/slice/buffer handling), complementing cargo-fuzz
//    which targets the core. Deterministic LCG/xorshift — no `rand` dependency.
// ---------------------------------------------------------------------------

/// A tiny deterministic xorshift64 PRNG. Seeded by a constant so the run is
/// reproducible; this is for input *coverage*, not cryptography.
struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        // xorshift64 is undefined for a zero state; force a non-zero seed.
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

#[test]
fn transform_fuzz_lite_boundary_never_panics() {
    // A non-trivial pipeline with two hand-rolled/parser-backed strippers, so the
    // random bytes exercise real work behind the boundary.
    let cfg = config(r#"{"version":1,"operations":[{"op":"strip_html"},{"op":"strip_markdown"}]}"#);

    let mut rng = XorShift64::new(0x5afe_5719_2026_0604);

    for _ in 0..1000 {
        // Length 0..=255, biased toward small but including empties.
        let len = (rng.next_u64() % 256) as usize;
        let mut buf = Vec::with_capacity(len);
        for _ in 0..len {
            buf.push((rng.next_u64() & 0xff) as u8);
        }

        // `transform` already copies the output out and frees the FFI buffer, so a
        // leak or double-free in the protocol would surface here (and under
        // sanitizers/Miri). We only require: status Ok, and no panic.
        let result = transform(&buf, &cfg);
        assert_eq!(
            result.status,
            SsStatus::Ok,
            "fuzz-lite input must always succeed, len={len}"
        );
        // Output must be valid UTF-8 (the boundary decodes input losslessly and the
        // core only ever produces valid UTF-8).
        assert!(std::str::from_utf8(&result.bytes).is_ok());
    }
}
