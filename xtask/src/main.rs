//! Mechanical invariant enforcement for SafetyStrip.
//!
//! The single, portable enforcer of the §5 invariants — no external cargo plugins
//! required, so the same checks run locally and in CI.
//!
//! Implemented here: `gen-header` and `check-abi` (the frozen C ABI).
//!
//! **Owner: CI/automation stream (D).** Extend with the remaining subcommands —
//! each must fail with a remediation-oriented message — and wire them into `ci`:
//!   check-core-deps    strict allowlist on `safetystrip-core`'s dependency tree
//!   check-no-network   workspace-wide banlist of network/OS-capable crates
//!   check-unsafe-forbid assert `#![forbid(unsafe_code)]` is present in core
//!   check-entitlements assert the macOS entitlements file is minimal
//!   ci                 run fmt --check, clippy -D warnings, test, and all the above

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("gen-header") => gen_header(false),
        Some("check-abi") => gen_header(true),
        // TODO(D): check-core-deps | check-no-network | check-unsafe-forbid |
        //          check-entitlements | ci
        Some(other) => {
            eprintln!("xtask: unknown subcommand '{other}'");
            eprintln!("usage: cargo xtask <gen-header|check-abi|...>");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("usage: cargo xtask <gen-header|check-abi|...>");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (xtask's manifest dir is `<root>/xtask`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live under the workspace root")
        .to_path_buf()
}

fn header_path() -> PathBuf {
    workspace_root().join("core-ffi/include/safetystrip.h")
}

/// Generate the C header from the `safetystrip-ffi` source using the pinned
/// cbindgen lib + the checked-in `cbindgen.toml`.
fn generate_header() -> String {
    let crate_dir = workspace_root().join("core-ffi");
    let config = cbindgen::Config::from_root_or_default(&crate_dir);
    let mut buf: Vec<u8> = Vec::new();
    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .expect("cbindgen failed to parse/generate the FFI crate")
        .write(&mut buf);
    String::from_utf8(buf).expect("cbindgen produced non-UTF-8 output")
}

/// `gen-header` writes the header; `check-abi` (check_only) fails on any drift.
fn gen_header(check_only: bool) -> ExitCode {
    let generated = generate_header();
    let path = header_path();

    if check_only {
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        if existing == generated {
            println!("check-abi: C header is in sync with the FFI source.");
            ExitCode::SUCCESS
        } else {
            eprintln!(
                "check-abi: FAIL — {} is out of sync with the FFI source.\n\
                 \n\
                 The C ABI is a frozen compatibility surface. If this change is\n\
                 intentional:\n\
                   1. bump SS_ABI_VERSION in core-ffi/src/lib.rs,\n\
                   2. run `cargo xtask gen-header` to regenerate the header,\n\
                   3. call out the ABI change in your PR and confirm a non-Swift\n\
                      shell could still consume the boundary.\n\
                 If unintentional, revert the signature/layout change.",
                path.display()
            );
            ExitCode::FAILURE
        }
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create include dir");
        }
        std::fs::write(&path, generated).expect("write header");
        println!("gen-header: wrote {}", path.display());
        ExitCode::SUCCESS
    }
}
