//! Mechanical invariant enforcement for SafetyStrip.
//!
//! **Owner: CI/automation stream (D).** This is the single, portable enforcer of
//! the §5 invariants — no external cargo plugins required. Implement these
//! subcommands (each must fail with a remediation-oriented message):
//!
//!   check-core-deps    strict allowlist on `safetystrip-core`'s dependency tree
//!   check-no-network   workspace-wide banlist of network/OS-capable crates
//!   check-abi          regenerate the C header and diff against the checked-in one
//!   gen-header         (re)generate core-ffi/include/safetystrip.h via cbindgen
//!   check-unsafe-forbid assert `#![forbid(unsafe_code)]` is present in core
//!   check-entitlements assert the macOS entitlements file is minimal
//!   ci                 run fmt --check, clippy -D warnings, test, and all the above
//!
//! Until implemented this is a stub so the workspace builds.

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // TODO(D): dispatch to the checks above.
    eprintln!(
        "xtask: not yet implemented (args: {:?}). See xtask/src/main.rs.",
        args
    );
    std::process::ExitCode::SUCCESS
}
