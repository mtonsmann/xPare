//! Mechanical invariant enforcement for xPare.
//!
//! The single, portable enforcer of the §5 invariants — no external cargo plugins
//! required, so the same checks run locally and in CI.
//!
//! Subcommands:
//!   gen-header          (re)write the frozen C header from the FFI source
//!   check-abi           fail if the checked-in C header has drifted
//!   check-unsafe-forbid assert `#![forbid(unsafe_code)]` is present in core
//!   check-core-deps     strict allowlist on `xpare-core`'s dependency tree
//!   check-no-network    workspace-wide banlist of network/OS-capable crates
//!   check-entitlements  assert the macOS entitlements file is minimal
//!   check-no-content-logging  assert no clipboard content is logged/persisted
//!   check-clipboard-safety    assert default targets avoid the real clipboard
//!   check-pipeline-zeroization assert fused core scratch storage is wiped before release
//!   check-agent-workflow      assert AI-native workflow docs/skills stay wired
//!   check-dependabot-policy   assert Dependabot update PRs stay reviewable and security-first
//!   check-c-ffi-surface       assert C/SwiftPM interop stays header-only and tiny
//!   check-test-hygiene        assert every ignored test has a reason and the count is ratcheted
//!   check-swift-no-network-apis assert shipped Swift cannot introduce network/browser APIs
//!   check-shipped-command-exec assert shipped app surfaces cannot spawn commands
//!   check-swift-package-deps assert the Swift package has no external dependencies
//!   check-python-tooling-posture assert Python helper imports/calls stay capability-light
//!   check-real-clipboard-tests assert default tests do not touch NSPasteboard.general
//!   check-pasteboard-write-shape assert clipboard writes stay plain-string only
//!   check-codeql-workflow-posture assert CodeQL stays additive, pinned, and custom packs stay wired
//!   check-release-posture     assert official signing cannot broaden entitlements
//!   check-supply-chain  cargo-deny: advisories + licenses + bans + sources
//!   check-unused-deps   cargo-machete: fail on a declared-but-unused dependency
//!   check-docs          build docs with -D warnings (broken intra-doc links, bad HTML)
//!   check-workflows     lint GitHub Actions workflows (actionlint + zizmor)
//!   check-shell         shellcheck the shell scripts
//!   check-fuzz          build cargo-fuzz targets; optionally smoke-run all targets
//!   check-miri          run the core-ffi boundary tests under Miri (UB detection)
//!   check-kani          run the bounded Kani proofs over the resource-envelope arithmetic
//!   check-coverage      line-coverage floor (best-effort; heavy; outside `ci`)
//!   check-mutants       cargo-mutants; `XP_DIFF_BASE` scopes to a diff (best-effort; outside `ci`)
//!   check-swift         macOS shell anti-slop: swift-format lint + swift test + coverage floor
//!                       (+ SwiftLint if present); best-effort, macOS-only, outside `ci`
//!   ci                  run fmt --check, clippy -D warnings, test, and all the above
//!
//! Every check exits nonzero on violation with a remediation-oriented message so a
//! future agent learns how to fix it rather than how to silence it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("gen-header") => gen_header(false),
        Some("check-abi") => gen_header(true),
        Some("check-unsafe-forbid") => report(check_unsafe_forbid()),
        Some("check-core-deps") => report(check_core_deps()),
        Some("check-no-network") => report(check_no_network()),
        Some("check-entitlements") => report(check_entitlements()),
        Some("check-no-content-logging") => report(check_no_content_logging()),
        Some("check-clipboard-safety") => report(check_clipboard_safety()),
        Some("check-pipeline-zeroization") => report(check_pipeline_zeroization()),
        Some("check-agent-workflow") => report(check_agent_workflow()),
        Some("check-dependabot-policy") => report(check_dependabot_policy()),
        Some("check-c-ffi-surface") => report(check_c_ffi_surface()),
        Some("check-test-hygiene") => report(check_test_hygiene()),
        Some("check-swift-no-network-apis") => report(check_swift_no_network_apis()),
        Some("check-shipped-command-exec") => report(check_shipped_command_exec()),
        Some("check-swift-package-deps") => report(check_swift_package_deps()),
        Some("check-python-tooling-posture") => report(check_python_tooling_posture()),
        Some("check-real-clipboard-tests") => report(check_real_clipboard_tests()),
        Some("check-pasteboard-write-shape") => report(check_pasteboard_write_shape()),
        Some("check-codeql-workflow-posture") => report(check_codeql_workflow_posture()),
        Some("check-release-posture") => report(check_release_posture()),
        Some("check-supply-chain") => report(check_supply_chain()),
        Some("check-unused-deps") => report(check_unused_deps()),
        Some("check-docs") => report(check_docs()),
        Some("check-workflows") => report(check_workflows()),
        Some("check-shell") => report(check_shell()),
        Some("check-fuzz") => report(check_fuzz()),
        Some("check-miri") => report(check_miri()),
        Some("check-kani") => report(check_kani()),
        Some("check-coverage") => report(check_coverage()),
        Some("check-mutants") => report(check_mutants()),
        Some("check-swift") => report(check_swift()),
        Some("ci") => run_ci(),
        Some(other) => {
            eprintln!("xtask: unknown subcommand '{other}'");
            usage();
            ExitCode::FAILURE
        }
        None => {
            usage();
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!(
        "usage: cargo xtask <subcommand>\n\
         subcommands:\n\
         \x20 gen-header           (re)write the frozen C header from the FFI source\n\
         \x20 check-abi            fail if the checked-in C header has drifted\n\
         \x20 check-unsafe-forbid  assert core forbids unsafe code\n\
         \x20 check-core-deps      assert core's dep tree is on the strict allowlist\n\
         \x20 check-no-network     assert no network/OS crate is anywhere in the tree\n\
         \x20 check-entitlements   assert the macOS entitlements file is minimal\n\
         \x20 check-no-content-logging  assert no clipboard content is logged/persisted\n\
         \x20 check-clipboard-safety     assert default targets avoid the real clipboard\n\
         \x20 check-pipeline-zeroization assert fused core scratch storage is wiped before release\n\
         \x20 check-agent-workflow       assert AI-native workflow docs/skills stay wired\n\
         \x20 check-dependabot-policy    assert Dependabot PRs stay reviewable and security-first\n\
         \x20 check-c-ffi-surface        assert C/SwiftPM interop stays header-only and tiny\n\
         \x20 check-test-hygiene         assert every #[ignore] has a reason and the count is ratcheted\n\
         \x20 check-swift-no-network-apis assert shipped Swift has no network/browser API surface\n\
         \x20 check-shipped-command-exec assert shipped app surfaces cannot spawn commands\n\
         \x20 check-swift-package-deps   assert SwiftPM has no external dependencies\n\
         \x20 check-python-tooling-posture assert Python helper imports/calls stay capability-light\n\
         \x20 check-real-clipboard-tests assert default tests do not touch NSPasteboard.general\n\
         \x20 check-pasteboard-write-shape assert clipboard writes stay plain-string only\n\
         \x20 check-codeql-workflow-posture assert CodeQL stays additive, pinned, and custom packs stay wired\n\
         \x20 check-release-posture      assert official signing cannot broaden entitlements\n\
         \x20 check-supply-chain   cargo-deny: advisories + licenses + bans + sources\n\
         \x20 check-unused-deps    cargo-machete: fail on a declared-but-unused dependency\n\
         \x20 check-docs           build docs with -D warnings (broken intra-doc links, bad HTML)\n\
         \x20 check-workflows      lint GitHub Actions workflows (actionlint + zizmor)\n\
         \x20 check-shell          shellcheck the shell scripts\n\
         \x20 check-fuzz           build fuzz targets; set XP_FUZZ_SMOKE_SECONDS=N to run them\n\
         \x20 check-miri           run core-ffi boundary tests under Miri (UB detection in the unsafe shim)\n\
         \x20 check-kani           run the bounded Kani proofs over the resource-envelope arithmetic\n\
         \x20 check-coverage       line-coverage floor (best-effort; heavy; outside `ci`)\n\
         \x20 check-mutants        cargo-mutants; XP_DIFF_BASE=<ref> scopes to a diff (best-effort; outside `ci`)\n\
         \x20 check-swift          macOS shell: swift-format lint + swift test + coverage (+ SwiftLint if present)\n\
         \x20 ci                   fmt + clippy + test + every structural & external check"
    );
}

/// Turn a check result into a process exit code, printing the failure message to
/// stderr so it is visible in CI logs.
fn report(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("{msg}");
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
    workspace_root().join("core-ffi/include/xpare.h")
}

/// Generate the C header from the `xpare-ffi` source using the pinned
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
                   1. bump XP_ABI_VERSION in core-ffi/src/lib.rs,\n\
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

// ---------------------------------------------------------------------------
// check-unsafe-forbid
// ---------------------------------------------------------------------------

/// The exact crate-level attribute that makes memory-unsafety impossible by
/// construction in the core. Matched as a trimmed line so reformatting (leading
/// whitespace) does not defeat it, but a weakening to `#![deny(...)]` does.
const FORBID_UNSAFE_ATTR: &str = "#![forbid(unsafe_code)]";

/// Assert that the core still forbids `unsafe`. This is the load-bearing memory
/// safety invariant: without it the fuzz/property suites lose their meaning.
fn check_unsafe_forbid() -> Result<(), String> {
    let path = workspace_root().join("core/src/lib.rs");
    let src = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "check-unsafe-forbid: FAIL — could not read {}: {e}\n\
             The core crate must exist and declare `{FORBID_UNSAFE_ATTR}` at its top.",
            path.display()
        )
    })?;

    if src.lines().any(|l| l.trim() == FORBID_UNSAFE_ATTR) {
        println!("check-unsafe-forbid: core declares `{FORBID_UNSAFE_ATTR}`.");
        Ok(())
    } else {
        Err(format!(
            "check-unsafe-forbid: FAIL — `{FORBID_UNSAFE_ATTR}` is missing from {}.\n\
             \n\
             The core is the untrusted-input path; forbidding unsafe is what makes\n\
             memory-unsafety impossible by construction. Restore the crate-level\n\
             attribute as the first non-doc line of core/src/lib.rs. Do NOT downgrade\n\
             it to `#![deny(unsafe_code)]` (which can be locally overridden) and do NOT\n\
             move the unsafe into core — all `unsafe` belongs in the core-ffi shim.",
            path.display()
        ))
    }
}

// ---------------------------------------------------------------------------
// cargo metadata model (only the fields we read)
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    resolve: Resolve,
    workspace_members: Vec<String>,
}

#[derive(serde::Deserialize)]
struct Package {
    id: String,
    name: String,
}

#[derive(serde::Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(serde::Deserialize)]
struct Node {
    id: String,
    deps: Vec<NodeDep>,
}

#[derive(serde::Deserialize)]
struct NodeDep {
    pkg: String,
    dep_kinds: Vec<DepKind>,
}

#[derive(serde::Deserialize)]
struct DepKind {
    /// `null` = normal dependency; otherwise "dev" or "build".
    kind: Option<String>,
}

/// Run `cargo metadata --locked --format-version 1` and parse it. We invoke the
/// same `cargo` that launched xtask (via `$CARGO`, falling back to `cargo`) so the
/// pinned toolchain is honored. `--locked` matters: the dependency checks must
/// audit the *committed* `Cargo.lock` — a silent re-resolve here would let the
/// checks pass against a tree that is not what CI builds or a release ships.
fn cargo_metadata() -> Result<Metadata, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(&cargo)
        .args(["metadata", "--locked", "--format-version", "1"])
        .current_dir(workspace_root())
        .output()
        .map_err(|e| format!("failed to run `{cargo} metadata`: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "`{cargo} metadata --locked` exited with {}:\n{}\n\
             If Cargo.lock is out of sync with the manifests, regenerate it (any cargo\n\
             build without `--locked`, e.g. `cargo metadata --format-version 1`), review\n\
             the lockfile diff, and commit it.",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("failed to parse `cargo metadata` JSON: {e}"))
}

impl Metadata {
    fn name_of(&self) -> HashMap<&str, &str> {
        self.packages
            .iter()
            .map(|p| (p.id.as_str(), p.name.as_str()))
            .collect()
    }

    fn nodes_by_id(&self) -> HashMap<&str, &Node> {
        self.resolve
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n))
            .collect()
    }

    fn package_ids_named(&self, name: &str) -> Vec<&str> {
        self.packages
            .iter()
            .filter(|p| p.name == name)
            .map(|p| p.id.as_str())
            .collect()
    }
}

/// Walk the transitive dependency closure from `start` ids, following *normal*
/// (`kind: null`) and *build* (`kind: "build"`) edges. Both kinds matter to the
/// posture checks: normal deps link into shipped artifacts, and build deps
/// execute on the build machine via build scripts. Dev-dependencies are skipped
/// (tests/benches neither ship nor run during a plain build), so e.g. a crate's
/// `proptest`/`criterion` tree does not pollute the result.
fn normal_and_build_dep_closure<'a>(meta: &'a Metadata, start: &[&'a str]) -> BTreeSet<&'a str> {
    let nodes = meta.nodes_by_id();
    let name_of = meta.name_of();

    let mut seen_ids: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = start.to_vec();
    let mut names: BTreeSet<&str> = BTreeSet::new();

    while let Some(id) = stack.pop() {
        if !seen_ids.insert(id) {
            continue;
        }
        if let Some(name) = name_of.get(id) {
            names.insert(name);
        }
        if let Some(node) = nodes.get(id) {
            for dep in &node.deps {
                let followed = dep
                    .dep_kinds
                    .iter()
                    .any(|k| matches!(k.kind.as_deref(), None | Some("build")));
                if followed && !seen_ids.contains(dep.pkg.as_str()) {
                    stack.push(dep.pkg.as_str());
                }
            }
        }
    }
    names
}

// ---------------------------------------------------------------------------
// check-core-deps
// ---------------------------------------------------------------------------

/// Explicit allowlist for `xpare-core`'s full transitive dependency tree —
/// *normal* and *build* dependencies; dev-dependencies (tests/benches) excluded.
///
/// This is the mechanical form of "the core has no OS, filesystem, or network
/// dependencies". The set is intentionally tiny and consists only of pure-data /
/// text / proc-macro crates:
///
/// * `serde`, `serde_core`, `serde_derive`, `serde_json` — config (de)serialization.
/// * `pulldown-cmark` — CommonMark → events for the Markdown stripper.
/// * proc-macro toolchain pulled in by `serde_derive`: `proc-macro2`, `quote`,
///   `syn`, `unicode-ident`.
/// * pure formatting / data helpers: `itoa` (integer formatting), `zmij` (float
///   formatting), `memchr`, `bitflags`, `unicase`.
/// * `zeroize` — best-effort wiping of clipboard-derived pipeline intermediates
///   (alloc feature only; no transitive crates, no OS/IO/network surface).
///
/// Derived by running `cargo metadata` against the pinned dependency ranges and
/// then frozen here. If `cargo update` legitimately introduces a new *pure-data*
/// transitive crate, add it here in its own PR with justification. If the new
/// crate touches the OS/filesystem/network, the right fix is to drop the
/// dependency that pulled it in — not to widen this list.
const CORE_DEP_ALLOWLIST: &[&str] = &[
    // workspace member itself (closure includes the root)
    "xpare-core",
    // direct config / markdown deps
    "serde",
    "serde_core",
    "serde_derive",
    "serde_json",
    "pulldown-cmark",
    // proc-macro toolchain (via serde_derive)
    "proc-macro2",
    "quote",
    "syn",
    "unicode-ident",
    // pure formatting / data helpers
    "itoa",
    "zmij",
    "memchr",
    "bitflags",
    "unicase",
    // best-effort heap zeroization of clipboard-derived pipeline intermediates
    // (alloc feature only; no transitive crates, no OS/IO/network surface).
    "zeroize",
];

/// Assert that every crate in the core's transitive normal+build dependency tree
/// is on [`CORE_DEP_ALLOWLIST`] (dev-dependencies excluded). This is how a future
/// OS/IO/network dependency sneaking into the core — whether linked in or run as
/// a build script — gets caught at CI time.
fn check_core_deps() -> Result<(), String> {
    let meta = cargo_metadata().map_err(|e| format!("check-core-deps: FAIL — {e}"))?;

    let core_ids = meta.package_ids_named("xpare-core");
    if core_ids.is_empty() {
        return Err(
            "check-core-deps: FAIL — `xpare-core` not found in `cargo metadata`. \
             Did the core crate get renamed or removed?"
                .to_string(),
        );
    }

    let allow: HashSet<&str> = CORE_DEP_ALLOWLIST.iter().copied().collect();
    let closure = normal_and_build_dep_closure(&meta, &core_ids);

    let mut offenders: Vec<&str> = closure
        .iter()
        .copied()
        .filter(|name| !allow.contains(name))
        .collect();
    offenders.sort_unstable();

    if offenders.is_empty() {
        println!(
            "check-core-deps: core's {} transitive normal+build deps are all on the allowlist.",
            closure.len()
        );
        Ok(())
    } else {
        Err(format!(
            "check-core-deps: FAIL — `xpare-core` depends (transitively) on crate(s) \
             not on the allowlist:\n\
             \x20 {}\n\
             \n\
             The core is platform-neutral and pure: it must have NO OS, filesystem, or\n\
             network dependencies — that freedom is a load-bearing privacy/safety\n\
             invariant. To fix:\n\
             \x20 * If you added a dependency to core that pulled this in, remove it and\n\
             \x20   keep OS/IO/net concerns in the shells (which own all OS integration).\n\
             \x20 * If `cargo update` introduced a new *pure-data* transitive crate that is\n\
             \x20   genuinely free of OS/IO/net, add it to CORE_DEP_ALLOWLIST in\n\
             \x20   xtask/src/main.rs in its own PR, with justification.\n\
             Never widen the allowlist to admit a crate with OS/IO/network capability.",
            offenders.join("\n  ")
        ))
    }
}

// ---------------------------------------------------------------------------
// check-no-network
// ---------------------------------------------------------------------------

/// Banlist of crates that provide network or broad OS/event-loop capability.
///
/// The privacy posture is **no network anywhere** — not just in the core, but in
/// every crate that could end up linked into a shipped artifact or run during a
/// build. Concretely, the check walks the *normal* and *build* dependency closure
/// of every workspace member; dev-dependencies (tests/benches) are deliberately
/// excluded so the larger `proptest`/`criterion` trees stay out (see
/// `docs/guardrails/dependency-posture.md`). If any of these appears in that
/// closure, that is a posture change that must be caught, explained, and
/// justified (or, far more likely, reverted).
///
/// This is a name banlist, not an exhaustive audit; it targets the common async
/// runtimes, HTTP/TLS stacks, websocket/RPC libraries, and the low-level
/// socket/event-loop crates they are built on.
const NETWORK_BANLIST: &[&str] = &[
    // async runtimes / executors
    "tokio",
    "tokio-util",
    "async-std",
    "smol",
    "async-io",
    "async-global-executor",
    // low-level event loops / sockets
    "mio",
    "polling",
    "socket2",
    "nix",
    // HTTP clients / servers
    "reqwest",
    "hyper",
    "hyper-util",
    "h2",
    "h3",
    "isahc",
    "ureq",
    "attohttpc",
    "curl",
    "curl-sys",
    "surf",
    "actix-web",
    "warp",
    "axum",
    "tiny_http",
    // TLS
    "native-tls",
    "openssl",
    "openssl-sys",
    "rustls",
    "tokio-rustls",
    "hyper-tls",
    "hyper-rustls",
    "schannel",
    "security-framework",
    // websockets / RPC / gRPC
    "tungstenite",
    "tokio-tungstenite",
    "tonic",
    "tower",
    "tower-http",
    // DNS / URL fetching helpers commonly paired with network IO
    "trust-dns-resolver",
    "hickory-resolver",
    "dns-lookup",
];

/// Walk the workspace's normal+build dependency closure (dev-deps excluded) and
/// fail if any banned network/OS crate is present anywhere in it.
fn check_no_network() -> Result<(), String> {
    let meta = cargo_metadata().map_err(|e| format!("check-no-network: FAIL — {e}"))?;

    // Start from every workspace member so the closure spans core, core-ffi, cli,
    // and xtask (and thus catches a network dep introduced into any of them).
    let members: Vec<&str> = meta.workspace_members.iter().map(String::as_str).collect();
    let closure = normal_and_build_dep_closure(&meta, &members);

    let banned: HashSet<&str> = NETWORK_BANLIST.iter().copied().collect();
    let mut offenders: Vec<&str> = closure
        .iter()
        .copied()
        .filter(|name| banned.contains(name))
        .collect();
    offenders.sort_unstable();

    if offenders.is_empty() {
        println!(
            "check-no-network: scanned {} crates; no network/OS-capable crate present.",
            closure.len()
        );
        Ok(())
    } else {
        Err(format!(
            "check-no-network: FAIL — banned network/OS-capable crate(s) found in the \
             workspace dependency tree:\n\
             \x20 {}\n\
             \n\
             xPare's privacy posture is no-network-anywhere: a plain-text clipboard\n\
             utility must never be able to exfiltrate clipboard content, and no shipped or\n\
             build-time crate should grant that capability. Remove the dependency that\n\
             pulled this in. If a network capability is somehow genuinely required, it is a\n\
             posture change that must be called out and justified in the PR and SECURITY.md\n\
             before this banlist could be revisited.",
            offenders.join("\n  ")
        ))
    }
}

// ---------------------------------------------------------------------------
// check-entitlements
// ---------------------------------------------------------------------------

fn entitlements_path() -> PathBuf {
    workspace_root().join("shells/macos/xPare.entitlements")
}

/// Validate the *text* of a macOS entitlements plist (a portable XML string scan;
/// we never shell out to `plutil`, since CI runs on Linux).
///
/// Policy, mirroring `docs/guardrails/macos-posture.md`:
/// * REQUIRE `com.apple.security.app-sandbox` = true.
/// * FORBID any networking/device/personal-info/automation/file-access/codesign-
///   weakening/accessibility entitlement. The intended file is *only*
///   app-sandbox=true.
fn validate_entitlements(text: &str) -> Result<(), String> {
    // 1. app-sandbox must be present AND set to true.
    if !key_present(text, "com.apple.security.app-sandbox") {
        return Err(
            "missing required entitlement `com.apple.security.app-sandbox`. The macOS shell \
             must run under the App Sandbox; add the key set to <true/>."
                .to_string(),
        );
    }
    if !key_set_true(text, "com.apple.security.app-sandbox") {
        return Err(
            "`com.apple.security.app-sandbox` is present but not set to <true/>. The App \
             Sandbox must be enabled; the value must be the boolean true."
                .to_string(),
        );
    }

    // 2. No banned key may appear. We check both exact keys and dangerous prefixes.
    let keys = entitlement_keys(text);
    let mut hits: Vec<String> = Vec::new();
    for key in &keys {
        if is_banned_entitlement(key) {
            hits.push(key.clone());
        }
    }
    if !hits.is_empty() {
        hits.sort();
        hits.dedup();
        return Err(format!(
            "banned entitlement key(s) present: {}. These grant network access, device \
             access, personal-information access, Apple-events automation, broad file \
             access, code-signing weakening, or accessibility/input monitoring — all \
             forbidden by the macOS posture. The intended entitlements file contains ONLY \
             `com.apple.security.app-sandbox` = true.",
            hits.join(", ")
        ));
    }

    let mut extras: Vec<String> = keys
        .into_iter()
        .filter(|key| key != "com.apple.security.app-sandbox")
        .collect();
    extras.sort();
    extras.dedup();
    if !extras.is_empty() {
        return Err(format!(
            "extra entitlement key(s) present: {}. The intended entitlements file \
             contains exactly one key: `com.apple.security.app-sandbox` = true.",
            extras.join(", ")
        ));
    }

    Ok(())
}

/// Return true if `key` appears as a `<key>…</key>` element in the plist text.
fn key_present(text: &str, key: &str) -> bool {
    entitlement_keys(text).iter().any(|k| k == key)
}

/// Extract every `<key>NAME</key>` value from the plist text (whitespace-tolerant,
/// case-sensitive on the key element name itself).
fn entitlement_keys(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("<key>") {
        let after = &rest[open + "<key>".len()..];
        if let Some(close) = after.find("</key>") {
            keys.push(after[..close].trim().to_string());
            rest = &after[close + "</key>".len()..];
        } else {
            break;
        }
    }
    keys
}

/// Return true if, after the `<key>NAME</key>` element for `key`, the next plist
/// value element is `<true/>` (allowing self-closing or `<true></true>` and
/// surrounding whitespace/newlines).
fn key_set_true(text: &str, key: &str) -> bool {
    let needle = format!("<key>{key}</key>");
    let Some(pos) = text.find(&needle) else {
        return false;
    };
    let after = text[pos + needle.len()..].trim_start();
    after.starts_with("<true/>") || after.starts_with("<true>")
}

/// Classify a single entitlement key as banned per the posture.
fn is_banned_entitlement(key: &str) -> bool {
    const BANNED_PREFIXES: &[&str] = &[
        "com.apple.security.network.",
        "com.apple.security.device.",
        "com.apple.security.personal-information.",
        "com.apple.security.files.",
    ];
    const BANNED_EXACT: &[&str] = &[
        "com.apple.security.automation.apple-events",
        "com.apple.security.cs.disable-library-validation",
        "com.apple.security.cs.allow-unsigned-executable-memory",
        "com.apple.security.cs.allow-dyld-environment-variables",
    ];

    if BANNED_EXACT.contains(&key) {
        return true;
    }
    if BANNED_PREFIXES.iter().any(|p| key.starts_with(p)) {
        return true;
    }
    // Accessibility / input-monitoring related entitlements, however namespaced.
    let lower = key.to_ascii_lowercase();
    lower.contains("accessibility")
        || lower.contains("input-monitoring")
        || lower.contains("postevent")
}

/// `check-entitlements` CLI entry: read the real file (FAIL if absent) and validate.
fn check_entitlements() -> Result<(), String> {
    let path = entitlements_path();
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "check-entitlements: FAIL — entitlements file not found at {} — the macOS shell \
             must ship a minimal, checked-in entitlements file (only \
             `com.apple.security.app-sandbox` = true).",
            path.display()
        )
    })?;

    validate_entitlements(&text).map_err(|e| {
        format!(
            "check-entitlements: FAIL — {} is not a minimal entitlements file: {e}",
            path.display()
        )
    })?;

    println!(
        "check-entitlements: {} is minimal (app-sandbox=true, no banned keys).",
        path.display()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// check-release-posture
// ---------------------------------------------------------------------------

fn release_script_path() -> PathBuf {
    workspace_root().join("shells/macos/release.sh")
}

fn require_script_snippet(text: &str, snippet: &str, why: &str, missing: &mut Vec<String>) {
    if !text.contains(snippet) {
        missing.push(format!("{why}: missing `{snippet}`"));
    }
}

fn release_script_case_slice<'a>(
    text: &'a str,
    case_label: &str,
    next_case_label: &str,
) -> Result<&'a str, String> {
    let marker = format!("    {case_label})\n");
    let start = text
        .find(&marker)
        .ok_or_else(|| format!("release script is missing `{case_label})` case"))?;
    let next_marker = format!("\n    {next_case_label})");
    let end = text[start + marker.len()..]
        .find(&next_marker)
        .map(|idx| start + marker.len() + idx)
        .ok_or_else(|| {
            format!("release script is missing `{next_case_label})` after `{case_label})`")
        })?;
    Ok(&text[start..end])
}

/// Validate the release script's load-bearing entitlement controls by exact
/// textual assertions. This is intentionally strict: if release signing is
/// refactored, the guard must be updated in the same PR after proving the new
/// code still rejects alternate or broader entitlement payloads.
fn validate_release_posture(release_text: &str, entitlements_text: &str) -> Result<(), String> {
    validate_entitlements(entitlements_text).map_err(|e| {
        format!(
            "checked-in signing entitlements are not exactly minimal, so official \
             release posture is not enforceable: {e}"
        )
    })?;

    let mut missing = Vec::new();
    require_script_snippet(
        release_text,
        r#"DEFAULT_SIGN_ENTITLEMENTS="${SCRIPT_DIR}/xPare.entitlements""#,
        "default signing entitlements must be the checked-in plist",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        "resolve_sign_entitlements()",
        "dist must resolve and validate the signing entitlement path",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#"resolved="$(canonical_path "${path}")""#,
        "dist must canonicalize the requested signing entitlement path",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#"default_resolved="$(canonical_path "${DEFAULT_SIGN_ENTITLEMENTS}")""#,
        "dist must canonicalize the checked signing entitlement path",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#"[[ "${resolved}" == "${default_resolved}" ]] || die"#,
        "dist must reject alternate SIGN_ENTITLEMENTS paths",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#"require_minimal_entitlements "${resolved}" "signing entitlements ${resolved}""#,
        "dist must verify source entitlements are minimal before signing",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        "/usr/libexec/PlistBuddy -c 'Print :com.apple.security.app-sandbox'",
        "minimal entitlement verification must check app-sandbox=true",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#"must contain only com.apple.security.app-sandbox=true"#,
        "minimal entitlement verification must reject extra entitlement keys",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#"--entitlements "${sign_entitlements}" --sign "${CERT_NAME}" "${EXE}""#,
        "dist must sign the inner executable with the checked entitlements",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#"--entitlements "${sign_entitlements}" --sign "${CERT_NAME}" "${APP}""#,
        "dist must sign the app bundle with the checked entitlements",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#"verify_signed_entitlements "${EXE}""#,
        "dist must verify signed entitlements on the executable",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#"verify_signed_entitlements "${APP}""#,
        "dist must verify signed entitlements on the app bundle",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        "NOTARY_KEYCHAIN=path",
        "official CI must be able to bind notary credentials to the temporary keychain",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#"notary_keychain_args=(--keychain "${NOTARY_KEYCHAIN}")"#,
        "notarytool must support an explicit keychain for the stored profile",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#"xcrun notarytool submit "${notary_zip}" --keychain-profile "${NOTARY_PROFILE}" \"#,
        "dist must pass the keychain-bound notary profile to notarytool submit",
        &mut missing,
    );
    require_script_snippet(
        release_text,
        r#""${notary_keychain_args[@]}" --wait --output-format json"#,
        "notarytool submit must consume the explicit keychain argument",
        &mut missing,
    );

    let github_release_case =
        release_script_case_slice(release_text, "github-release", "\"\"|-h|--help")?;
    let release_view = r#"gh release view "v${version}" >/dev/null 2>&1"#;
    let release_view_line = r#"if gh release view "v${version}" >/dev/null 2>&1; then"#;
    let release_refusal_prefix =
        r#"die "release v${version} already exists; refusing to replace release assets"#;
    let release_create = r#"gh release create "v${version}""#;
    for (snippet, why) in [
        (
            release_view_line,
            "github-release must check for an existing release before creating one",
        ),
        (
            release_refusal_prefix,
            "github-release must fail closed before replacing existing release assets",
        ),
        (
            release_create,
            "github-release must create the release only after the existing-release guard",
        ),
        (
            r#"sbom="${RELEASE_DIR}/${APP_NAME}-v${version}-sbom.spdx.json""#,
            "github-release must stage the release SBOM path",
        ),
        (
            r#"[ -f "${sbom}" ] || die"#,
            "github-release must fail closed when the release SBOM is missing",
        ),
    ] {
        if !non_comment_line_contains(github_release_case, snippet) {
            missing.push(format!("{why}: missing `{snippet}`"));
        }
    }
    if !non_comment_line_starts_with(github_release_case, release_view_line) {
        missing.push(
            "github-release must execute the existing-release guard directly, not echo or comment it"
                .to_string(),
        );
    }
    if let Err(err) = require_non_comment_lines_in_order(
        github_release_case,
        &[
            release_view_line,
            release_refusal_prefix,
            "else",
            release_create,
        ],
        "github-release existing-release guard",
    ) {
        missing.push(err);
    }
    let release_create_command =
        continued_shell_command_starting_with(github_release_case, release_create);
    match release_create_command {
        Some(command) if command.contains(r#""${sbom}""#) => {}
        Some(_) => missing.push(
            "github-release must include the staged SBOM in the one-shot release creation command"
                .to_string(),
        ),
        None => missing.push(
            "github-release must contain an executable `gh release create` command".to_string(),
        ),
    }
    if let (Some(view_pos), Some(create_pos)) = (
        github_release_case.find(release_view),
        github_release_case.find(release_create),
    ) {
        if view_pos >= create_pos {
            missing.push(
                "github-release must check for an existing release before release creation"
                    .to_string(),
            );
        }
    }
    for banned in [
        "gh release upload",
        "--clobber",
        "gh release delete",
        "gh release delete-asset",
        "uploads/releases/assets",
    ] {
        if non_comment_line_contains(release_text, banned) {
            missing.push(format!(
                "github-release must never replace existing release assets: banned `{banned}`"
            ));
        }
    }

    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "release entitlement posture guard(s) missing:\n  {}\n\
             \n\
             Official Developer ID releases must sign with the checked \
             app-sandbox-only entitlement file, reject alternate SIGN_ENTITLEMENTS \
             paths, verify the signed entitlement payload remains minimal, and \
             never replace assets on an existing GitHub Release. \
             Restore these controls or update this check in the same PR with an \
             equivalent fail-closed proof.",
            missing.join("\n  ")
        ))
    }
}

fn check_release_posture() -> Result<(), String> {
    let release_path = release_script_path();
    let release_text = std::fs::read_to_string(&release_path).map_err(|e| {
        format!(
            "check-release-posture: FAIL — could not read {}: {e}",
            release_path.display()
        )
    })?;
    let entitlements_path = entitlements_path();
    let entitlements_text = std::fs::read_to_string(&entitlements_path).map_err(|e| {
        format!(
            "check-release-posture: FAIL — could not read {}: {e}",
            entitlements_path.display()
        )
    })?;

    validate_release_posture(&release_text, &entitlements_text).map_err(|e| {
        format!(
            "check-release-posture: FAIL — {} no longer mechanically preserves \
             official release posture:\n{e}",
            release_path.display()
        )
    })?;

    println!(
        "check-release-posture: official signing path rejects alternate entitlements, verifies minimal signed payloads, and refuses to replace existing release assets."
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// check-c-ffi-surface
// ---------------------------------------------------------------------------

const ALLOWED_C_FFI_SURFACE: &[&str] = &[
    "core-ffi/include/xpare.h",
    "shells/macos/Sources/CXPare/dummy.c",
    "shells/macos/Sources/CXPare/include/module.modulemap",
    "shells/macos/Sources/CXPare/include/shim.h",
];

fn slash_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn c_ffi_surface_files(root: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_source_files(
        root,
        &["c", "cc", "cpp", "cxx", "h", "hpp", "m", "mm", "modulemap"],
        &mut files,
    );
    files.sort();
    files
}

fn strip_c_like_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_block = false;
    while let Some(ch) = chars.next() {
        if in_block {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            } else if ch == '\n' {
                out.push('\n');
            }
            continue;
        }

        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            in_block = true;
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn noncomment_lines(text: &str) -> Vec<String> {
    strip_c_like_comments(text)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn check_file_lines(
    root: &std::path::Path,
    rel: &str,
    expected: &[&str],
    errors: &mut Vec<String>,
) {
    let path = root.join(rel);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            errors.push(format!("could not read {rel}: {e}"));
            return;
        }
    };
    let lines = noncomment_lines(&text);
    let expected: Vec<String> = expected.iter().map(|line| (*line).to_string()).collect();
    if lines != expected {
        errors.push(format!(
            "{rel} changed executable/non-comment content.\n  expected: {:?}\n  actual:   {:?}",
            expected, lines
        ));
    }
}

fn check_generated_header_shape(root: &std::path::Path, errors: &mut Vec<String>) {
    let rel = "core-ffi/include/xpare.h";
    let path = root.join(rel);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            errors.push(format!("could not read {rel}: {e}"));
            return;
        }
    };
    for snippet in [
        "GENERATED by cbindgen",
        "#ifndef XPARE_FFI_H",
        "uint32_t xp_abi_version(void);",
        "const char *xp_capabilities_json(void);",
        "enum XpStatus xp_transform(",
        "void xp_buffer_free(uint8_t *ptr, size_t len);",
        "XP_STATUS_ERR_UNSUPPORTED_CONFIG_VERSION = 5,",
    ] {
        if !text.contains(snippet) {
            errors.push(format!(
                "{rel} is missing expected generated ABI snippet `{snippet}`"
            ));
        }
    }
}

fn check_c_ffi_surface() -> Result<(), String> {
    let root = workspace_root();
    let expected: BTreeSet<String> = ALLOWED_C_FFI_SURFACE
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    let actual: BTreeSet<String> = c_ffi_surface_files(&root)
        .into_iter()
        .filter_map(|path| path.strip_prefix(&root).ok().map(slash_path))
        .collect();

    let mut errors = Vec::new();
    let unexpected: Vec<_> = actual.difference(&expected).cloned().collect();
    let missing: Vec<_> = expected.difference(&actual).cloned().collect();
    if !unexpected.is_empty() {
        errors.push(format!(
            "unexpected C/C++/Objective-C/modulemap file(s): {}",
            unexpected.join(", ")
        ));
    }
    if !missing.is_empty() {
        errors.push(format!(
            "expected C/FFI bridge file(s) missing: {}",
            missing.join(", ")
        ));
    }

    check_file_lines(
        &root,
        "shells/macos/Sources/CXPare/dummy.c",
        &[],
        &mut errors,
    );
    check_file_lines(
        &root,
        "shells/macos/Sources/CXPare/include/shim.h",
        &[r#"#include "../../../../../core-ffi/include/xpare.h""#],
        &mut errors,
    );
    check_file_lines(
        &root,
        "shells/macos/Sources/CXPare/include/module.modulemap",
        &["module CXPare {", r#"header "shim.h""#, "export *", "}"],
        &mut errors,
    );
    check_generated_header_shape(&root, &mut errors);

    if errors.is_empty() {
        println!(
            "check-c-ffi-surface: C/SwiftPM interop is limited to the generated header, shim header, module map, and empty dummy source."
        );
        Ok(())
    } else {
        Err(format!(
            "check-c-ffi-surface: FAIL —\n  {}\n\
             \n\
             xPare keeps handwritten C logic out of the project. The only allowed \
             C-adjacent surface is the cbindgen-generated ABI header, a SwiftPM shim \
             that includes that header, the module map, and an empty dummy translation \
             unit required by SwiftPM. Do not add C/C++/Objective-C implementation code \
             without an explicit compatibility/security review.",
            errors.join("\n  ")
        ))
    }
}

// ---------------------------------------------------------------------------
// source posture checks (Swift, Python, pasteboard shape, CodeQL workflow)
// ---------------------------------------------------------------------------

/// The CodeQL action release currently pinned in `.github/workflows/codeql.yml`.
/// Keep init/analyze on the same audited commit so the workflow cannot drift to a
/// moving tag while still looking superficially "pinned". This must be the peeled
/// release commit SHA, not the annotated tag object SHA; GitHub Advanced Security
/// reports `zizmor/ref-version-mismatch` for annotated tag object pins.
const CODEQL_ACTION_PIN: &str = "8aad20d150bbac5944a9f9d289da16a4b0d87c1e";
const CODEQL_ACTION_VERSION_COMMENT: &str = "v4.36.2";
const CODEQL_ACTION_ANNOTATED_TAG_OBJECT_PINS: &[&str] = &[
    // refs/tags/v4 -> tag object -> CODEQL_ACTION_PIN
    "411bbbe57033eedfc1a82d68c01345aa96c737d7",
    // refs/tags/v4.36.2 -> tag object -> CODEQL_ACTION_PIN
    "1a818fd5f97ed0ee9a823421bd5b171add01227f",
];

/// Shipped Swift and Rust source roots scanned by the app-surface posture checks.
const SHIPPED_SWIFT_ROOTS: &[&str] = &["shells/macos/Sources"];
const SHIPPED_RUST_ROOTS: &[&str] = &["core/src", "core-ffi/src", "cli/src"];

/// Network/browser/authentication API tokens that are incompatible with xPare's
/// no-network privacy posture in the shipped macOS shell. The dependency check
/// catches linked network crates; this catches direct platform APIs that do not
/// show up in Cargo metadata.
const SWIFT_NETWORK_API_TOKENS: &[&str] = &[
    "URLSession",
    "URLRequest",
    "NSURLConnection",
    "import Network",
    "NWConnection",
    "NWListener",
    "NWBrowser",
    "NWPathMonitor",
    "import WebKit",
    "WKWebView",
    "import SafariServices",
    "SFSafariViewController",
    "import AuthenticationServices",
    "ASWebAuthenticationSession",
    "NSWorkspace.shared.open",
];

/// Process-spawning tokens banned from shipped Swift and Rust surfaces. Release
/// scripts and xtask may invoke tools; the app/core/CLI must not gain a command
/// execution surface as part of clipboard handling.
const SWIFT_COMMAND_EXEC_TOKENS: &[&str] = &[
    "Process(",
    "Process.run",
    "NSTask",
    "posix_spawn",
    "system(",
    "popen(",
];
const RUST_COMMAND_EXEC_TOKENS: &[&str] = &[
    "std::process::Command",
    "process::Command",
    "Command::new(",
    ".spawn(",
    "std::os::unix::process",
];

/// The icon helper is intentionally stdlib-only and capability-light.
const PYTHON_ALLOWED_IMPORTS: &[&str] = &[
    "__future__",
    "argparse",
    "math",
    "pathlib",
    "struct",
    "zlib",
];
const PYTHON_BANNED_TOKENS: &[&str] = &[
    "subprocess",
    "socket",
    "urllib",
    "http.client",
    "requests",
    "ftplib",
    "telnetlib",
    "asyncio",
    "multiprocessing",
    "os.system(",
    "os.popen(",
    "eval(",
    "exec(",
];

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn collect_files_under(root: &Path, rel_roots: &[&str], exts: &[&str]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for rel in rel_roots {
        let dir = root.join(rel);
        if dir.is_dir() {
            collect_source_files(&dir, exts, &mut files);
        }
    }
    files.sort();
    files
}

fn token_hits_in_files(root: &Path, files: &[PathBuf], tokens: &[&str]) -> Vec<String> {
    let mut hits = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let rel = rel_path(root, path);
        for (line_no, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*") {
                continue;
            }
            for token in tokens {
                if line.contains(token) {
                    hits.push(format!(
                        "{rel}:{}: banned token `{token}`: {}",
                        line_no + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
    hits
}

/// Assert the shipped Swift app surface has no direct network/browser API usage.
fn check_swift_no_network_apis() -> Result<(), String> {
    let root = workspace_root();
    let files = collect_files_under(&root, SHIPPED_SWIFT_ROOTS, &["swift"]);
    let hits = token_hits_in_files(&root, &files, SWIFT_NETWORK_API_TOKENS);
    if hits.is_empty() {
        println!(
            "check-swift-no-network-apis: scanned {} shipped Swift file(s); no network/browser API tokens found.",
            files.len()
        );
        Ok(())
    } else {
        Err(format!(
            "check-swift-no-network-apis: FAIL — shipped Swift source references network, browser, \
             or authentication API token(s):\n  {}\n\
             \n\
             xPare's privacy posture is no network anywhere and no browser/auth callback surface. \
             Keep those APIs out of the macOS shell; if this is a deliberate posture change, update \
             SECURITY.md, the guardrail, entitlements, and this check in the same PR.",
            hits.join("\n  ")
        ))
    }
}

/// Assert shipped app/core/CLI surfaces do not spawn subprocesses.
fn check_shipped_command_exec() -> Result<(), String> {
    let root = workspace_root();
    let swift_files = collect_files_under(&root, SHIPPED_SWIFT_ROOTS, &["swift"]);
    let rust_files = collect_files_under(&root, SHIPPED_RUST_ROOTS, &["rs"]);
    let mut hits = token_hits_in_files(&root, &swift_files, SWIFT_COMMAND_EXEC_TOKENS);
    hits.extend(token_hits_in_files(
        &root,
        &rust_files,
        RUST_COMMAND_EXEC_TOKENS,
    ));
    if hits.is_empty() {
        println!(
            "check-shipped-command-exec: scanned {} Swift and {} Rust shipped file(s); no command execution surface found.",
            swift_files.len(),
            rust_files.len()
        );
        Ok(())
    } else {
        Err(format!(
            "check-shipped-command-exec: FAIL — shipped source references command execution token(s):\n  {}\n\
             \n\
             xPare may run tools in release scripts and xtask, but shipped clipboard handling must not \
             spawn commands. Remove the command execution surface or make the compatibility/security \
             posture change explicit in docs and this guard.",
            hits.join("\n  ")
        ))
    }
}

fn validate_swift_package_deps(text: &str) -> Result<(), String> {
    let mut hits = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        for token in [
            ".package(",
            ".product(",
            ".binaryTarget(",
            ".systemLibrary(",
        ] {
            if trimmed.contains(token) {
                hits.push(format!("line {}: `{}`", line_no + 1, trimmed));
            }
        }
    }
    if hits.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "external SwiftPM dependency declaration(s):\n  {}",
            hits.join("\n  ")
        ))
    }
}

/// Assert the macOS Swift package remains dependency-free except for local targets.
fn check_swift_package_deps() -> Result<(), String> {
    let path = workspace_root().join("shells/macos/Package.swift");
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "check-swift-package-deps: FAIL — could not read {}: {e}",
            path.display()
        )
    })?;
    validate_swift_package_deps(&text).map_err(|e| {
        format!(
            "check-swift-package-deps: FAIL — {e}\n\
             \n\
             The shipped macOS shell intentionally has no external SwiftPM packages. If a new \
             package is truly needed, justify it in docs/guardrails/dependency-posture.md and \
             update this check in the same PR; otherwise keep dependencies local."
        )
    })?;
    println!("check-swift-package-deps: SwiftPM package declares only local targets.");
    Ok(())
}

fn python_import_roots(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Vec::new();
    }
    if let Some(rest) = trimmed.strip_prefix("import ") {
        return rest
            .split(',')
            .filter_map(|part| {
                part.split_whitespace()
                    .next()
                    .and_then(|name| name.split('.').next())
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
            })
            .collect();
    }
    if let Some(rest) = trimmed.strip_prefix("from ") {
        return rest
            .split_whitespace()
            .next()
            .and_then(|name| name.split('.').next())
            .filter(|name| !name.is_empty())
            .map(|name| vec![name.to_string()])
            .unwrap_or_default();
    }
    Vec::new()
}

fn validate_python_helper_posture(text: &str) -> Result<(), String> {
    let allowed: HashSet<&str> = PYTHON_ALLOWED_IMPORTS.iter().copied().collect();
    let mut hits = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        for root in python_import_roots(line) {
            if !allowed.contains(root.as_str()) {
                hits.push(format!(
                    "line {}: import `{root}` is outside the stdlib helper allowlist",
                    line_no + 1
                ));
            }
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        for token in PYTHON_BANNED_TOKENS {
            if line.contains(token) {
                hits.push(format!(
                    "line {}: banned capability token `{token}`: {}",
                    line_no + 1,
                    line.trim()
                ));
            }
        }
    }
    if hits.is_empty() {
        Ok(())
    } else {
        Err(hits.join("\n  "))
    }
}

fn python_syntax_check(files: &[PathBuf]) -> Result<(), String> {
    if files.is_empty() {
        return Ok(());
    }
    let python = resolve_tool("python3").ok_or_else(|| {
        "check-python-tooling-posture: FAIL — `python3` is not on PATH; install Python 3 to syntax-check helper scripts."
            .to_string()
    })?;
    let pycache = workspace_root().join("target").join("python-pycache");
    std::fs::create_dir_all(&pycache).map_err(|e| {
        format!(
            "check-python-tooling-posture: FAIL — could not create {}: {e}",
            pycache.display()
        )
    })?;
    let mut command = Command::new(&python);
    command.args(["-m", "py_compile"]).args(files);
    let status = command
        .env("PYTHONPYCACHEPREFIX", &pycache)
        .current_dir(workspace_root())
        .status()
        .map_err(|e| format!("check-python-tooling-posture: FAIL — could not run python3: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "check-python-tooling-posture: FAIL — `python3 -m py_compile` reported a syntax error (exited {status})."
        ))
    }
}

/// Assert Python helpers stay stdlib-only and capability-light.
fn check_python_tooling_posture() -> Result<(), String> {
    let root = workspace_root();
    let files = collect_files_under(&root, &["shells/macos", "scripts"], &["py"]);
    let mut hits = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path).map_err(|e| {
            format!(
                "check-python-tooling-posture: FAIL — could not read {}: {e}",
                path.display()
            )
        })?;
        if let Err(err) = validate_python_helper_posture(&text) {
            hits.push(format!("{}:\n  {err}", rel_path(&root, path)));
        }
    }
    if !hits.is_empty() {
        return Err(format!(
            "check-python-tooling-posture: FAIL — Python helper script(s) gained disallowed imports/capabilities:\n  {}\n\
             \n\
             Helper scripts should stay pure-stdlib and capability-light: no network, subprocess, \
             multiprocessing, or dynamic code execution. If a real tooling need appears, justify it \
             in the dependency guardrail and update the allowlist deliberately.",
            hits.join("\n  ")
        ));
    }
    python_syntax_check(&files)?;
    println!(
        "check-python-tooling-posture: {} Python helper file(s) are syntax-valid and capability-light.",
        files.len()
    );
    Ok(())
}

/// Assert default Swift tests do not touch the user's real clipboard. Comments may
/// explain the invariant; executable code may not reference `NSPasteboard.general`.
fn check_real_clipboard_tests() -> Result<(), String> {
    let root = workspace_root();
    let files = collect_files_under(&root, &["shells/macos/Tests"], &["swift"]);
    let mut hits = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let rel = rel_path(&root, path);
        for (line_no, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            if line.contains("NSPasteboard.general") {
                hits.push(format!("{rel}:{}: {}", line_no + 1, line.trim()));
            }
        }
    }
    if hits.is_empty() {
        println!(
            "check-real-clipboard-tests: scanned {} Swift test file(s); default tests avoid NSPasteboard.general.",
            files.len()
        );
        Ok(())
    } else {
        Err(format!(
            "check-real-clipboard-tests: FAIL — default tests reference the real clipboard:\n  {}\n\
             \n\
             Default tests must use named/synthetic pasteboards so local and CI runs never read or \
             mutate the user's clipboard. Move real NSPasteboard.general exercise behind an explicit \
             opt-in target.",
            hits.join("\n  ")
        ))
    }
}

fn extract_swift_function_body<'a>(text: &'a str, signature: &str) -> Option<&'a str> {
    let start = text.find(signature)?;
    let after_signature = &text[start..];
    let open_rel = after_signature.find('{')?;
    let open = start + open_rel;
    let mut depth = 0usize;
    let mut body_start = None;
    for (offset, ch) in text[open..].char_indices() {
        match ch {
            '{' => {
                depth += 1;
                if depth == 1 {
                    body_start = Some(open + offset + ch.len_utf8());
                }
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return body_start.map(|body| &text[body..open + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

fn validate_pasteboard_write_shape(files: &[(String, String)]) -> Result<(), String> {
    let Some((rel, text)) = files
        .iter()
        .find(|(rel, _)| rel == "shells/macos/Sources/XPareKit/Pasteboard.swift")
    else {
        return Err("shells/macos/Sources/XPareKit/Pasteboard.swift is missing".to_string());
    };
    let Some(body) =
        extract_swift_function_body(text, "public func writePlain(_ text: String) -> Int?")
    else {
        return Err(format!(
            "{rel}: could not find `SystemPasteboard.writePlain(_:)`"
        ));
    };

    let clear_count = body.matches("pasteboard.clearContents()").count();
    let set_string_count = body.matches("pasteboard.setString(").count();
    let mut errors = Vec::new();
    if clear_count != 1 {
        errors.push(format!(
            "`writePlain(_:)` must clear pasteboard contents exactly once before rewriting; found {clear_count}"
        ));
    }
    if set_string_count != 1 {
        errors.push(format!(
            "`writePlain(_:)` must write exactly one plain string; found {set_string_count} `setString` call(s)"
        ));
    }
    if !body.contains("pasteboard.setString(text, forType: .string)") {
        errors.push(
            "`writePlain(_:)` must write the transformed text as NSPasteboard.PasteboardType.string"
                .to_string(),
        );
    }
    for token in [
        "writeObjects(",
        "setData(",
        "setPropertyList(",
        "declareTypes(",
        "writeFileURL(",
    ] {
        if body.contains(token) {
            errors.push(format!(
                "`writePlain(_:)` contains non-string pasteboard write token `{token}`"
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n  "))
    }
}

/// Assert the plain-string clipboard rewrite stays narrow. The opt-in paste-as-file
/// path is covered separately by the content-persistence allow marker; this guard
/// is only about `writePlain(_:)`.
fn check_pasteboard_write_shape() -> Result<(), String> {
    let root = workspace_root();
    let rel = "shells/macos/Sources/XPareKit/Pasteboard.swift";
    let text = std::fs::read_to_string(root.join(rel))
        .map_err(|e| format!("check-pasteboard-write-shape: FAIL — could not read {rel}: {e}"))?;
    validate_pasteboard_write_shape(&[(rel.to_string(), text)]).map_err(|e| {
        format!(
            "check-pasteboard-write-shape: FAIL —\n  {e}\n\
             \n\
             `SystemPasteboard.writePlain(_:)` is the in-place plain-string rewrite path. It must \
             clear the pasteboard once and write exactly one `.string` payload. The separate \
             opt-in paste-as-file exception may write a file URL through `writeFileURL(_:)`, but \
             broadening the plain rewrite path is a clipboard-safety posture change."
        )
    })?;
    println!(
        "check-pasteboard-write-shape: SystemPasteboard.writePlain clears once and writes exactly one plain string."
    );
    Ok(())
}

fn validate_codeql_workflow_posture(text: &str) -> Result<(), String> {
    let mut errors = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        let uses = trimmed
            .strip_prefix("uses: ")
            .or_else(|| trimmed.strip_prefix("- uses: "));
        if let Some(rest) = uses {
            if !rest.contains('@') {
                errors.push(format!(
                    "line {}: action is not versioned: {trimmed}",
                    line_no + 1
                ));
            }
            if rest.starts_with("github/codeql-action/") {
                validate_codeql_action_pin(line_no + 1, rest, trimmed, &mut errors);
            }
            if rest.starts_with("actions/checkout@v") {
                errors.push(format!(
                    "line {}: checkout must be pinned to a commit SHA, not a moving tag: {trimmed}",
                    line_no + 1
                ));
            }
            if rest.starts_with("dtolnay/rust-toolchain@stable") {
                errors.push(format!(
                    "line {}: rust-toolchain action must be pinned to a commit SHA: {trimmed}",
                    line_no + 1
                ));
            }
        }
        for broad_perm in [
            "contents: write",
            "pull-requests: write",
            "issues: write",
            "id-token: write",
            "actions: write",
        ] {
            if trimmed == broad_perm {
                errors.push(format!(
                    "line {}: CodeQL workflow grants disallowed permission `{broad_perm}`",
                    line_no + 1
                ));
            }
        }
    }

    for required in [
        "Additive signal only",
        "Keep this workflow out of branch protection",
        "permissions:\n  contents: read",
        "security-events: write",
        "languages: rust",
        "languages: python",
        "languages: actions",
        "build-mode: none",
        "queries: security-extended",
        "queries: security-extended,./.github/codeql/queries/rust",
        "queries: security-extended,./.github/codeql/queries/python",
        "./.github/codeql/queries/rust",
        "./.github/codeql/queries/python",
        "dependency-caching: true",
        "category: \"/language:rust\"",
        "category: \"/language:python\"",
        "category: \"/language:actions\"",
    ] {
        if !text.contains(required) {
            errors.push(format!(
                "missing required CodeQL workflow snippet `{required}`"
            ));
        }
    }
    if text.contains("languages: python, actions") {
        errors.push(
            "analyze Python and GitHub Actions in separate CodeQL jobs so custom Python \
             query packs attach only to a Python database"
                .into(),
        );
    }
    if text.contains("category: \"/language:python-actions\"") {
        errors.push("use separate CodeQL categories for Python and GitHub Actions".into());
    }
    if text.contains("github/codeql-action/autobuild") {
        errors.push("CodeQL autobuild is disabled; keep analysis explicit and reviewable".into());
    }
    if text.contains("safetystrip") || text.contains("SafetyStrip") {
        errors.push("workflow contains stale SafetyStrip naming".into());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n  "))
    }
}

fn validate_codeql_action_pin(
    line_no: usize,
    action_ref: &str,
    trimmed_line: &str,
    errors: &mut Vec<String>,
) {
    let (action_and_pin, comment) = match action_ref.split_once('#') {
        Some((left, right)) => (left.trim(), Some(right.trim())),
        None => (action_ref.trim(), None),
    };
    let pin = match action_and_pin.rsplit_once('@') {
        Some((_, pin)) => pin.trim(),
        None => {
            errors.push(format!(
                "line {line_no}: CodeQL action is missing an @ pin: {trimmed_line}"
            ));
            return;
        }
    };

    if CODEQL_ACTION_ANNOTATED_TAG_OBJECT_PINS.contains(&pin) {
        errors.push(format!(
            "line {line_no}: CodeQL action pin uses an annotated tag object SHA; \
             use peeled release commit {CODEQL_ACTION_PIN} with \
             `# {CODEQL_ACTION_VERSION_COMMENT}`: {trimmed_line}"
        ));
    }
    if pin != CODEQL_ACTION_PIN {
        errors.push(format!(
            "line {line_no}: CodeQL action must be pinned to peeled release commit \
             {CODEQL_ACTION_PIN}: {trimmed_line}"
        ));
    }
    if comment != Some(CODEQL_ACTION_VERSION_COMMENT) {
        errors.push(format!(
            "line {line_no}: CodeQL action pin must carry exact version comment \
             `# {CODEQL_ACTION_VERSION_COMMENT}`: {trimmed_line}"
        ));
    }
}

fn validate_codeql_custom_queries(root: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    let required_files: [(&str, &[&str]); 5] = [
        (
            ".github/codeql/queries/rust/qlpack.yml",
            &["name: xpare/rust-policy-queries", "codeql/rust-all"],
        ),
        (
            ".github/codeql/queries/rust/src/ShippedRustCapabilitySurface.ql",
            &[
                "@id xpare/rust-shipped-capability-surface",
                "import rust",
                "processExecutionTarget",
                "coreFilesystemCallTarget",
                "coreFilesystemSourceTarget",
                "networkSourceTarget",
                "resolvedUseTreePath",
                "getUseTreeList",
                "getAUseTree",
                "PathTypeRepr",
                "UseTree",
            ],
        ),
        (
            ".github/codeql/queries/python/qlpack.yml",
            &["name: xpare/python-policy-queries", "codeql/python-all"],
        ),
        (
            ".github/codeql/queries/python/src/PythonHelperBannedImports.ql",
            &[
                "@id xpare/python-helper-banned-import",
                "ImportingStmt",
                "bannedImportRoot",
            ],
        ),
        (
            ".github/codeql/queries/python/src/PythonHelperBannedCalls.ql",
            &[
                "@id xpare/python-helper-banned-call",
                "semmle.python.ApiGraphs",
                "bannedBuiltinCall",
                "bannedModuleCall",
            ],
        ),
    ];

    for (rel, snippets) in required_files {
        match std::fs::read_to_string(root.join(rel)) {
            Ok(text) => {
                for snippet in snippets {
                    if !text.contains(snippet) {
                        errors.push(format!(
                            "{rel} is missing required CodeQL query snippet `{snippet}`"
                        ));
                    }
                }
            }
            Err(e) => errors.push(format!(
                "could not read required CodeQL query file {rel}: {e}"
            )),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n  "))
    }
}

/// Assert CodeQL remains additive, least-privilege, commit-pinned, and custom-pack wired.
fn check_codeql_workflow_posture() -> Result<(), String> {
    let root = workspace_root();
    let rel = ".github/workflows/codeql.yml";
    let text = std::fs::read_to_string(root.join(rel))
        .map_err(|e| format!("check-codeql-workflow-posture: FAIL — could not read {rel}: {e}"))?;
    validate_codeql_workflow_posture(&text).map_err(|e| {
        format!(
            "check-codeql-workflow-posture: FAIL —\n  {e}\n\
             \n\
             CodeQL is being introduced as additive security-review signal, not as a required \
             merge gate yet. Keep actions pinned, permissions minimal, queries set to \
             `security-extended`, custom query packs wired by language, and Python/Actions \
             split so first-triage noise can be understood before branch protection changes."
        )
    })?;
    validate_codeql_custom_queries(&root).map_err(|e| {
        format!(
            "check-codeql-workflow-posture: FAIL —\n  {e}\n\
             \n\
             The custom CodeQL packs encode repo-specific review lessons that are awkward to \
             enforce with built-in CodeQL alone. Keep these query packs checked in and wired \
             unless the replacement mechanical guard is called out in the PR."
        )
    })?;
    println!(
        "check-codeql-workflow-posture: CodeQL workflow is additive, pinned, least-privilege, and custom packs are wired."
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// check-no-content-logging
// ---------------------------------------------------------------------------
//
// Ported from the upstream FormatStripper `guardrails.py` content-logging check and
// tuned for low noise on this tree: the trigger words are clipboard-specific or
// transform-result names (not the generic input/output/text), so the CLI's
// intentional write of transformed output to stdout is NOT flagged, while logging or
// persisting actual clipboard-derived content is. Scans shipped source only; `xtask`
// (this tooling) and tests are excluded — they legitimately name these words.

/// Shipped source roots scanned for clipboard-content logging/persistence.
const CONTENT_SCAN_ROOTS: &[&str] = &["core/src", "cli/src", "shells/macos/Sources"];

/// Call tokens that emit to a log/diagnostic sink (Rust + Swift idioms).
const LOGGING_TOKENS: &[&str] = &[
    "print!",
    "print(",
    "println!",
    "println(",
    "eprint!",
    "eprint(",
    "eprintln!",
    "eprintln(",
    "dbg!",
    "NSLog(",
    "os_log(",
    "logger.debug",
    "logger.info",
    "logger.trace",
    "logger.warning",
    "logger.error",
    "log::debug",
    "log::info",
    "log::trace",
    "log::warn",
    "log::error",
];

/// Call tokens that persist data to disk / user defaults.
const PERSISTENCE_TOKENS: &[&str] = &[
    "UserDefaults",
    "FileManager.default",
    "fs::write",
    "File::create",
    "write(to:",
    "NSKeyedArchiver",
];

/// Words that name clipboard-derived / transform-result content (matched
/// case-insensitively). Deliberately excludes the generic `input`/`output`/`text`
/// the upstream regex used, which would flag the CLI's legitimate stdout write.
const CONTENT_WORDS: &[&str] = &[
    "clipboard",
    "pasteboard",
    "plaintext",
    "plain_text",
    "payload",
    "selection",
    "transformed",
    "stripped",
    "clipboardtext",
];

/// True if the line calls a logging or persistence sink.
fn line_logs_or_persists(line: &str) -> bool {
    LOGGING_TOKENS.iter().any(|t| line.contains(t))
        || PERSISTENCE_TOKENS.iter().any(|t| line.contains(t))
}

/// True if the (already-lowercased) line names clipboard-derived content.
fn line_names_content(line_lower: &str) -> bool {
    CONTENT_WORDS.iter().any(|w| line_lower.contains(w))
}

/// A line is a violation iff it both routes to a sink AND names clipboard content.
fn flags_content_logging(line: &str) -> bool {
    line_logs_or_persists(line) && line_names_content(&line.to_ascii_lowercase())
}

/// The explicit, audited exemption for the **opt-in paste-as-file feature** — the
/// one sanctioned place that persists clipboard-derived content (SECURITY.md,
/// "Opt-in paste-as-file exception"). A line carrying this marker is exempt from
/// the content scan, but only inside [`CONTENT_PERSISTENCE_ALLOWED_FILES`];
/// anywhere else the marker's presence is itself a violation, so the exemption
/// cannot quietly spread.
const CONTENT_PERSISTENCE_ALLOW_MARKER: &str = "xpare:allow-content-persistence";

/// The only shipped source files permitted to carry the allow marker.
const CONTENT_PERSISTENCE_ALLOWED_FILES: &[&str] =
    &["shells/macos/Sources/XPareKit/PasteFileStore.swift"];

/// Per-line verdict for `check-no-content-logging`, marker-aware. Returns a short
/// reason when the line is a violation; `None` when it is clean or exempted.
/// Pure (no I/O) so it is unit-tested directly.
fn content_line_violation(line: &str, marker_allowed_here: bool) -> Option<&'static str> {
    if line.contains(CONTENT_PERSISTENCE_ALLOW_MARKER) {
        return if marker_allowed_here {
            None
        } else {
            Some("carries the allow-content-persistence marker outside the allowlisted file")
        };
    }
    flags_content_logging(line).then_some("appears to log or persist clipboard-derived content")
}

// ---------------------------------------------------------------------------
// check-test-hygiene
// ---------------------------------------------------------------------------

/// Ceiling on `#[ignore]`d tests across the workspace. An ignored test is a
/// *disabled* test — a quiet way for the suite to look green while coverage rots —
/// so the count is ratcheted. Raising it is a deliberate, reviewed edit to this
/// constant (with the reason in the PR), exactly like the `core` dependency
/// allowlist: the ratchet lives in code, not in a blessable side file.
const MAX_IGNORED_TESTS: usize = 2;

/// check-test-hygiene: a deterministic guard on *test slop*. Every `#[ignore]`
/// attribute must carry a reason (`#[ignore = "why"]`), and the total number of
/// ignored tests must not exceed [`MAX_IGNORED_TESTS`]. A bare `#[ignore]` hides
/// *why* a test is off; unbounded growth lets disabled tests accumulate behind a
/// green suite. (Assertion *quality* — tests that execute code but assert nothing —
/// is the other half of test slop, caught separately by `cargo xtask check-mutants`.)
/// Classify an already-`trim_start`ed source line as a test `#[ignore]` attribute.
///
/// * `None` — not an `#[ignore]` attribute: a different attribute that merely starts
///   with `ignore` (e.g. `#[ignored_x]`), or a doc-comment / prose / string mention.
/// * `Some(true)` — carries a reason: `#[ignore = "..."]` (spacing-insensitive).
/// * `Some(false)` — a bare `#[ignore]` (a silent skip).
///
/// Scope: a `cfg_attr`-gated ignore (`#[cfg_attr(<cond>, ignore)]`) is intentionally NOT
/// matched — it cannot carry a `= "reason"`, and none exist in the tree. Pure (no I/O) so
/// it is unit-tested directly.
fn classify_ignore_line(trimmed: &str) -> Option<bool> {
    let rest = trimmed.strip_prefix("#[ignore")?.trim_start();
    // The next char must be `]` (bare) or `=` (reason); anything else means the token was
    // a longer identifier (e.g. `ignored`), not the std `#[ignore]` attribute.
    match rest.as_bytes().first() {
        Some(b'=') => Some(true),
        Some(b']') => Some(false),
        _ => None,
    }
}

fn check_test_hygiene() -> Result<(), String> {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    collect_source_files(&root, &["rs"], &mut files);
    files.sort();

    let mut bare: Vec<String> = Vec::new();
    let mut ignored = 0usize;
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            match classify_ignore_line(trimmed) {
                None => continue,
                Some(has_reason) => {
                    ignored += 1;
                    if !has_reason {
                        let shown = path.strip_prefix(&root).unwrap_or(path.as_path());
                        bare.push(format!("{}:{}: {}", shown.display(), i + 1, trimmed));
                    }
                }
            }
        }
    }

    if !bare.is_empty() {
        return Err(format!(
            "check-test-hygiene: FAIL — {} `#[ignore]` attribute(s) without a reason:\n  {}\n\
             \n\
             A disabled test must say WHY. Use `#[ignore = \"...\"]` so the next reader\n\
             knows whether it is a slow opt-in, an environment gap, or a known failure —\n\
             never a silent skip. Add a reason; do not delete the test.",
            bare.len(),
            bare.join("\n  ")
        ));
    }
    if ignored > MAX_IGNORED_TESTS {
        return Err(format!(
            "check-test-hygiene: FAIL — {ignored} ignored test(s), but the ceiling is \
             MAX_IGNORED_TESTS = {MAX_IGNORED_TESTS}.\n\
             \n\
             Ignored tests are disabled tests; letting them accumulate rots coverage behind\n\
             a green suite. Re-enable a test (preferred), or — if a new opt-in is genuinely\n\
             warranted — raise MAX_IGNORED_TESTS in xtask/src/main.rs in THIS PR with the\n\
             reason. The ratchet only moves with a deliberate, reviewed edit."
        ));
    }
    println!(
        "check-test-hygiene: {ignored} ignored test(s) (≤ {MAX_IGNORED_TESTS}), each with a reason."
    );
    Ok(())
}

/// Recursively collect files under `root` with one of `exts`, skipping build/VCS dirs.
fn collect_source_files(root: &std::path::Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(
                name.as_ref(),
                "target" | ".build" | ".git" | ".swiftpm" | ".claude"
            ) {
                continue;
            }
            collect_source_files(&path, exts, out);
        } else if path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| exts.contains(&x))
        {
            out.push(path);
        }
    }
}

/// Assert no shipped source line logs or persists clipboard-derived content.
fn check_no_content_logging() -> Result<(), String> {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    for r in CONTENT_SCAN_ROOTS {
        collect_source_files(&root.join(r), &["rs", "swift"], &mut files);
    }
    files.sort();

    let mut hits: Vec<String> = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let shown = path.strip_prefix(&root).unwrap_or(path.as_path());
        let rel = shown.display().to_string().replace('\\', "/");
        let marker_allowed = CONTENT_PERSISTENCE_ALLOWED_FILES.contains(&rel.as_str());
        for (i, line) in text.lines().enumerate() {
            if let Some(reason) = content_line_violation(line, marker_allowed) {
                hits.push(format!("{rel}:{}: {reason}: {}", i + 1, line.trim()));
            }
        }
    }

    if hits.is_empty() {
        println!(
            "check-no-content-logging: scanned {} shipped source file(s); no clipboard-content \
             logging or persistence outside the sanctioned paste-as-file store.",
            files.len()
        );
        Ok(())
    } else {
        Err(format!(
            "check-no-content-logging: FAIL — line(s) appear to log or persist clipboard-derived \
             content:\n\x20 {}\n\
             \n\
             xPare must never write clipboard content to a log sink, to disk, or to user\n\
             defaults. Log fixed operational states only; persist user *settings* (operation\n\
             choices, shortcuts), never clipboard input/output/derived text. If this is a false\n\
             positive, rename the local so the line no longer reads as logging real content.\n\
             The ONE sanctioned exception is the opt-in paste-as-file store\n\
             (PasteFileStore.swift), whose sink lines carry the\n\
             `xpare:allow-content-persistence` marker; that marker is honored nowhere\n\
             else, and never silences a finding by being copied around.",
            hits.join("\n  ")
        ))
    }
}

// ---------------------------------------------------------------------------
// check-pipeline-zeroization
// ---------------------------------------------------------------------------
//
// Security finding class: fused pipeline scratch buffers — and op output
// accumulators that can outgrow any cheap pre-size bound — hold
// clipboard-derived bytes just like full pipeline intermediates. They must be
// wiped before their storage is released or reallocated and on drop; otherwise
// an optimization silently weakens the documented in-memory hygiene posture.
//
// This is a TRIPWIRE, not a proof: it asserts the exact load-bearing source
// constructs the posture depends on, so deleting or renaming them fails loudly
// and forces a re-review. It cannot see every allocation (op return values,
// dev-only paths, third-party parser internals stay best-effort — see
// core/src/pipeline.rs's module doc and SECURITY.md for the honest gap list).

fn pipeline_path() -> PathBuf {
    workspace_root().join("core/src/pipeline.rs")
}

/// Load-bearing wipe-on-grow markers for the op accumulators whose output can
/// outgrow any cheap up-front capacity bound (`html_to_markdown`, the Unicode
/// case mappings, `strip_markdown`). Each entry is
/// `(workspace-relative file, exact source marker, remediation reason)`.
/// Pre-sized ops are NOT listed here: their posture is the documented
/// `with_capacity` bound at each site, pinned by capacity property tests in
/// `core/tests/`.
const OP_ACCUMULATOR_WIPE_MARKERS: &[(&str, &str, &str)] = &[
    (
        "core/src/ops/wipe.rs",
        "let retired = std::mem::replace(buf, grown);",
        "wipe-on-grow must retire the outgrown allocation by hand (a plain `String` \
         realloc frees it unwiped)",
    ),
    (
        "core/src/ops/wipe.rs",
        "drop(Zeroizing::new(retired));",
        "wipe-on-grow must zeroize the retired allocation before the allocator reclaims it",
    ),
    (
        "core/src/ops/html_to_markdown.rs",
        "use super::wipe::{push_char_wiping, push_str_wiping};",
        "html_to_markdown accumulator appends must route through `ops::wipe`",
    ),
    (
        "core/src/ops/html_to_markdown.rs",
        "text: Zeroizing<String>,",
        "the html_to_markdown accumulator must live in `Zeroizing` storage so drop wipes it",
    ),
    (
        "core/src/ops/markdown.rs",
        "use crate::ops::wipe::{push_char_wiping, push_str_wiping};",
        "strip_markdown accumulator appends must route through `ops::wipe`",
    ),
    (
        "core/src/ops/case.rs",
        "use crate::ops::wipe::push_char_wiping;",
        "Unicode case-mapping appends must route through `ops::wipe`",
    ),
];

/// Check the wipe-on-grow markers for one file's text; returns the missing
/// markers' reasons. Split from the IO so tests can run it on doctored sources.
fn missing_accumulator_wipe_markers(rel: &str, text: &str) -> Vec<&'static str> {
    OP_ACCUMULATOR_WIPE_MARKERS
        .iter()
        .filter(|(file, marker, _)| *file == rel && !text.contains(marker))
        .map(|(_, _, reason)| *reason)
        .collect()
}

fn validate_pipeline_zeroization(text: &str) -> Result<(), String> {
    let mut missing = Vec::new();
    if !text.contains("let mut collapsed = Zeroizing::new(Vec::new());") {
        missing.push("W3b collapsed-line scratch must be `Zeroizing::new(Vec::new())`");
    }
    if !text.contains("fn prepare_collapse_scratch(scratch: &mut Vec<u8>, needed: usize)") {
        missing.push("W3b collapsed-line scratch must use the prepare helper");
    }
    if !text.contains("if needed > scratch.capacity() {") {
        missing.push("W3b collapsed-line scratch must check capacity before growth");
    }
    if !text.contains("scratch.zeroize();") {
        missing.push("W3b collapsed-line scratch must call `scratch.zeroize()` before growth");
    }
    if !text.contains("prepare_collapse_scratch(scratch, line.len());") {
        missing.push("W3b collapse must prepare scratch before writing clipboard-derived bytes");
    }
    if !text.contains("scratch.reserve(needed);") {
        missing.push("W3b collapsed-line scratch must reserve only after the growth wipe check");
    }
    if text.contains("let mut collapsed = Vec::new();") {
        missing.push("plain `Vec::new()` scratch reintroduces heap remanence");
    }

    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing.join("\n  "))
    }
}

fn check_pipeline_zeroization() -> Result<(), String> {
    let path = pipeline_path();
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "check-pipeline-zeroization: FAIL — could not read {}: {e}",
            path.display()
        )
    })?;

    validate_pipeline_zeroization(&text).map_err(|e| {
        format!(
            "check-pipeline-zeroization: FAIL — core pipeline fused scratch buffers \
             are not mechanically covered by the wipe posture:\n  {e}\n\
             \n\
             Fused transform scratch buffers hold clipboard-derived bytes. Keep them \
             wrapped in `Zeroizing` and wipe them before capacity growth can release \
             old storage; allocation-preserving reuse may use `clear()` because drop \
             still wipes the transform-owned allocation.",
        )
    })?;

    // Same tripwire posture for the growable op output accumulators.
    let root = workspace_root();
    let mut missing = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (rel, _, _) in OP_ACCUMULATOR_WIPE_MARKERS {
        if !seen.insert(*rel) {
            continue;
        }
        let path = root.join(rel);
        let text = std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "check-pipeline-zeroization: FAIL — could not read {}: {e}",
                path.display()
            )
        })?;
        for reason in missing_accumulator_wipe_markers(rel, &text) {
            missing.push(format!("{rel}: {reason}"));
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "check-pipeline-zeroization: FAIL — growable op accumulators are not \
             mechanically covered by the wipe-on-grow posture:\n  {}\n\
             \n\
             Op outputs that can outgrow any cheap pre-size bound must keep their \
             accumulators in `Zeroizing` storage and route appends through \
             `core/src/ops/wipe.rs`, which zeroizes a superseded allocation before \
             the allocator reclaims it. If the implementation legitimately changed, \
             update OP_ACCUMULATOR_WIPE_MARKERS to the new load-bearing constructs \
             in the same PR — do not delete the coverage.",
            missing.join("\n  ")
        ));
    }

    println!(
        "check-pipeline-zeroization: fused pipeline scratch storage is zeroized before release \
         and on drop; growable op accumulators route appends through the wipe-on-grow helpers."
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// check-clipboard-safety
// ---------------------------------------------------------------------------
//
// The default verification targets must never touch the user's REAL clipboard.
// Real `NSPasteboard.general` exercise stays behind an explicitly opt-in target, so
// `make ci` / `make check` can run safely in any environment.

/// Default (non-opt-in) Make targets that must not depend on a real-clipboard smoke.
const DEFAULT_MAKE_TARGETS: &[&str] = &[
    "check", "checks", "ci", "all", "build", "test", "app", "run", "preview", "dist",
];

/// Parse a Makefile rule `target: prereqs` into its parts, or `None` if the line is a
/// recipe (leading tab), a variable assignment, or not a rule.
fn parse_make_rule(line: &str) -> Option<(&str, &str)> {
    if line.starts_with('\t') {
        return None;
    }
    let colon = line.find(':')?;
    let before = &line[..colon];
    let after = &line[colon + 1..];
    // Skip `X := y` / `X ?= y` / `X = y` / `X ::= y` assignments.
    if before.contains('=') || after.starts_with('=') {
        return None;
    }
    let prereqs = after.split('#').next().unwrap_or("").trim();
    Some((before.trim(), prereqs))
}

/// Assert no default Make target depends on a real-clipboard (`*general*`) smoke.
fn check_clipboard_safety() -> Result<(), String> {
    let path = workspace_root().join("Makefile");
    let Ok(text) = std::fs::read_to_string(&path) else {
        println!("check-clipboard-safety: no Makefile present; nothing to check.");
        return Ok(());
    };

    let mut hits: Vec<String> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some((target, prereqs)) = parse_make_rule(line) {
            if DEFAULT_MAKE_TARGETS.contains(&target) {
                for prereq in prereqs.split_whitespace() {
                    if prereq.contains("general") {
                        hits.push(format!(
                            "Makefile:{}: default target `{target}` depends on `{prereq}`",
                            i + 1
                        ));
                    }
                }
            }
        }
    }

    if hits.is_empty() {
        println!(
            "check-clipboard-safety: default Make targets do not exercise the real clipboard."
        );
        Ok(())
    } else {
        Err(format!(
            "check-clipboard-safety: FAIL —\n\x20 {}\n\
             \n\
             Real NSPasteboard.general verification must stay OPT-IN. Default targets must use\n\
             synthetic pasteboards only, so `make ci`/`make check` never read or mutate the\n\
             user's real clipboard.",
            hits.join("\n  ")
        ))
    }
}

// ---------------------------------------------------------------------------
// external linters and dev tools (supply-chain, workflows, shell, fuzz)
// ---------------------------------------------------------------------------
//
// Unlike the structural checks above (pure Rust, zero external deps), these shell
// out to third-party linters. They are still part of `cargo xtask ci` so the one
// gate stays a true SUPERSET of CI — a green local run means a green PR, with no
// second command to remember. Pinned versions keep local and CI byte-identical;
// the cargo-installable tools auto-install on first local use, while the system
// tools fail with the exact install commands (CI installs all of them as a pinned
// step). Bump these in lockstep with the "Install lint tools" step in
// .github/workflows/ci.yml.
const CARGO_DENY_VERSION: &str = "0.19.8";
const ZIZMOR_VERSION: &str = "1.25.2";
const CARGO_FUZZ_VERSION: &str = "0.13.1";
const KANI_VERSION: &str = "0.67.0";
const CARGO_MACHETE_VERSION: &str = "0.9.2";
const CARGO_MUTANTS_VERSION: &str = "27.1.0";
const CARGO_LLVM_COV_VERSION: &str = "0.8.7";

const SHELLCHECK_INSTALL_HINT: &str = "\x20 macOS:  brew install shellcheck\n\
     \x20 Debian: sudo apt-get install -y shellcheck\n\
     \x20 Pinned: https://github.com/koalaman/shellcheck/releases/tag/v0.11.0";

const ACTIONLINT_INSTALL_HINT: &str = "\x20 macOS:  brew install actionlint\n\
     \x20 Go:     go install github.com/rhysd/actionlint/cmd/actionlint@v1.7.12\n\
     \x20 Pinned: https://github.com/rhysd/actionlint/releases/tag/v1.7.12";

/// The cargo bin dir (`$CARGO_HOME/bin`, else `~/.cargo/bin`) where `cargo install`
/// places executables — not always on `$PATH` in minimal agent/CI shells, so we
/// search it explicitly.
fn cargo_bin_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("CARGO_HOME") {
        return Some(PathBuf::from(home).join("bin"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo").join("bin"))
}

/// Resolve an executable to an absolute path by searching `$PATH` then the cargo
/// bin dir. `None` if it is not found anywhere we look. (Unix layout: xtask runs on
/// the macOS/Linux dev/CI hosts; the reserved Windows shell is not built here.)
fn resolve_tool(bin: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(cargo_bin_dir());
    dirs.into_iter().map(|d| d.join(bin)).find(|c| c.is_file())
}

/// Return a PATH that definitely contains `tool`'s directory. Useful for cargo
/// subcommands: `cargo +nightly fuzz ...` discovers `cargo-fuzz` via PATH, but
/// minimal agent shells do not always include `$CARGO_HOME/bin`.
fn path_with_tool_dir(tool: &std::path::Path) -> Option<std::ffi::OsString> {
    let parent = tool.parent()?;
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if !dirs.iter().any(|d| d == parent) {
        dirs.insert(0, parent.to_path_buf());
    }
    std::env::join_paths(dirs).ok()
}

/// Run an external linter (resolved to an absolute path) from the workspace root,
/// inheriting stdio so its diagnostics stream straight to the user. Same contract
/// as `run_cargo`: Ok on success, a remediation-oriented message otherwise.
fn run_tool(label: &str, program: &std::path::Path, args: &[&str]) -> Result<(), String> {
    println!("ci: $ {label} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(workspace_root())
        .status()
        .map_err(|e| format!("ci: FAIL — could not launch `{label}`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "ci: FAIL — `{label}` reported problems (exited {status}). Fix the reported \
             issues; do not weaken the check."
        ))
    }
}

/// Ensure a cargo-installable linter is present, auto-installing the pinned version
/// on first local use (it lands in the cargo bin dir). In CI the tool is
/// pre-installed, so this is a no-op there.
fn ensure_cargo_tool(bin: &str, crate_name: &str, version: &str) -> Result<PathBuf, String> {
    if let Some(path) = resolve_tool(bin) {
        return Ok(path);
    }
    println!(
        "ci: `{bin}` not found — installing {crate_name}@{version} via cargo \
         (one-time; CI pre-installs it)…"
    );
    let installed = Command::new("cargo")
        .args(["install", "--locked", &format!("{crate_name}@{version}")])
        .status()
        .map_err(|e| format!("ci: FAIL — could not launch `cargo install {crate_name}`: {e}"))?
        .success();
    if installed {
        if let Some(path) = resolve_tool(bin) {
            return Ok(path);
        }
    }
    Err(format!(
        "ci: FAIL — could not auto-install `{bin}`. Install it manually and re-run:\n\
         \x20 cargo install --locked {crate_name}@{version}"
    ))
}

/// Require a system linter (not a cargo crate). If missing, fail with the exact
/// install commands rather than silently skipping — a skip would let a local pass
/// hide a CI failure. CI installs these as a pinned step, so the gate is identical
/// locally and in CI.
fn require_system_tool(bin: &str, what: &str, install_hint: &str) -> Result<PathBuf, String> {
    resolve_tool(bin).ok_or_else(|| {
        format!(
            "ci: FAIL — `{bin}` is not installed (needed for {what}).\n\
             Install it, then re-run `cargo xtask ci`:\n{install_hint}"
        )
    })
}

/// All shell scripts under the workspace, skipping build/VCS/worktree dirs.
fn shell_scripts(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_source_files(root, &["sh"], &mut out);
    out.sort();
    out
}

/// check-shell: shellcheck every shell script in the tree. The macOS build/release
/// plumbing is load-bearing — `release.sh` signs, notarizes, and staples — so a
/// shell bug there is a release-integrity bug.
fn check_shell() -> Result<(), String> {
    let root = workspace_root();
    let scripts = shell_scripts(&root);
    if scripts.is_empty() {
        println!("check-shell: no shell scripts found; nothing to lint.");
        return Ok(());
    }
    let shellcheck = require_system_tool(
        "shellcheck",
        "shell-script linting",
        SHELLCHECK_INSTALL_HINT,
    )?;
    let rel: Vec<String> = scripts
        .iter()
        .map(|p| p.strip_prefix(&root).unwrap_or(p).display().to_string())
        .collect();
    let args: Vec<&str> = rel.iter().map(String::as_str).collect();
    run_tool("shellcheck", &shellcheck, &args)
}

struct ReleaseWorkflowStep {
    name: Option<String>,
    start: usize,
}

fn line_offset_pairs(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0;
    text.split_inclusive('\n').map(move |line| {
        let start = offset;
        offset += line.len();
        (start, line.trim_end_matches(['\r', '\n']))
    })
}

fn normalize_yaml_key(key: &str) -> &str {
    let key = key.trim();
    match key.as_bytes() {
        [b'\'', rest @ .., b'\''] | [b'"', rest @ .., b'"'] => {
            std::str::from_utf8(rest).unwrap_or(key)
        }
        _ => key,
    }
}

fn yaml_line_key(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let without_dash = match trimmed.strip_prefix('-') {
        Some(rest) => rest.trim_start(),
        None => trimmed,
    };
    let key_end = without_dash
        .find(|ch: char| ch == ':' || ch.is_whitespace())
        .unwrap_or(without_dash.len());
    let key = &without_dash[..key_end];
    if key.is_empty() {
        return None;
    }
    let key = normalize_yaml_key(key);
    let rest = without_dash[key_end..].trim_start();
    rest.strip_prefix(':').map(|_| key)
}

fn yaml_line_value_for_key<'a>(line: &'a str, expected_key: &str) -> Option<&'a str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let without_dash = trimmed.strip_prefix('-')?.trim_start();
    let key_end = without_dash
        .find(|ch: char| ch == ':' || ch.is_whitespace())
        .unwrap_or(without_dash.len());
    let key = &without_dash[..key_end];
    let key = normalize_yaml_key(key);
    if key != expected_key {
        return None;
    }
    let rest = without_dash[key_end..].trim_start();
    rest.strip_prefix(':').map(str::trim_start)
}

fn yaml_line_value_for_any_key<'a>(line: &'a str, expected_key: &str) -> Option<&'a str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let without_dash = match trimmed.strip_prefix('-') {
        Some(rest) => rest.trim_start(),
        None => trimmed,
    };
    let key_end = without_dash
        .find(|ch: char| ch == ':' || ch.is_whitespace())
        .unwrap_or(without_dash.len());
    let key = &without_dash[..key_end];
    let key = normalize_yaml_key(key);
    if key != expected_key {
        return None;
    }
    let rest = without_dash[key_end..].trim_start();
    rest.strip_prefix(':').map(str::trim_start)
}

fn trim_yaml_scalar(value: &str) -> &str {
    value
        .split('#')
        .next()
        .unwrap_or(value)
        .trim()
        .trim_matches(['"', '\''])
}

#[derive(Debug)]
struct DependabotUpdateBlock {
    ecosystem: String,
    start_line: usize,
    groups_line: Option<usize>,
    cooldown_line: Option<usize>,
    cooldown_default_days: Option<(usize, String)>,
    open_pull_requests_limit: Option<(usize, String)>,
    target_branch_line: Option<usize>,
    multi_ecosystem_group_line: Option<usize>,
}

impl DependabotUpdateBlock {
    fn new(ecosystem: String, start_line: usize) -> Self {
        Self {
            ecosystem,
            start_line,
            groups_line: None,
            cooldown_line: None,
            cooldown_default_days: None,
            open_pull_requests_limit: None,
            target_branch_line: None,
            multi_ecosystem_group_line: None,
        }
    }
}

fn validate_dependabot_update_block(block: &DependabotUpdateBlock, errors: &mut Vec<String>) {
    match block.ecosystem.as_str() {
        "github-actions" => {
            if let Some(line) = block.groups_line {
                errors.push(format!(
                    "line {line}: github-actions updates must stay ungrouped so reviewers inspect one upstream action diff per PR"
                ));
            }
            if let Some(line) = block.multi_ecosystem_group_line {
                errors.push(format!(
                    "line {line}: github-actions updates must not join a multi-ecosystem group"
                ));
            }
            if block.cooldown_line.is_none() {
                errors.push(format!(
                    "line {}: github-actions updates must keep a 7-day cooldown for routine version bumps",
                    block.start_line
                ));
            }
            match &block.cooldown_default_days {
                Some((_, value)) if value == "7" => {}
                Some((line, value)) => errors.push(format!(
                    "line {line}: github-actions cooldown.default-days must stay 7, got `{value}`"
                )),
                None if block.cooldown_line.is_some() => errors.push(format!(
                    "line {}: github-actions cooldown must set `default-days: 7`",
                    block.start_line
                )),
                None => {}
            }
        }
        "cargo" => {
            match &block.open_pull_requests_limit {
                Some((_, value)) if value == "0" => {}
                Some((line, value)) => errors.push(format!(
                    "line {line}: cargo open-pull-requests-limit must stay 0 so routine version PRs stay manual, got `{value}`"
                )),
                None => errors.push(format!(
                    "line {}: cargo updates must set `open-pull-requests-limit: 0` so only security-update PRs are automated",
                    block.start_line
                )),
            }
            if let Some(line) = block.cooldown_line {
                errors.push(format!(
                    "line {line}: cargo security-update PRs must not be delayed by Dependabot cooldown"
                ));
            }
            if let Some(line) = block.groups_line {
                errors.push(format!(
                    "line {line}: cargo security-update PRs must stay ungrouped unless a PR explicitly changes the supply-chain policy"
                ));
            }
            if let Some(line) = block.target_branch_line {
                errors.push(format!(
                    "line {line}: cargo security-update configuration must target the default branch; remove target-branch"
                ));
            }
            if let Some(line) = block.multi_ecosystem_group_line {
                errors.push(format!(
                    "line {line}: cargo updates must not join a multi-ecosystem group"
                ));
            }
        }
        _ => {}
    }
}

fn validate_dependabot_policy(text: &str) -> Result<(), String> {
    let mut errors = Vec::new();
    let mut github_actions_entries = 0usize;
    let mut cargo_entries = 0usize;
    let mut current: Option<DependabotUpdateBlock> = None;

    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        if yaml_line_key(line) == Some("multi-ecosystem-groups") {
            errors.push(format!(
                "line {line_no}: multi-ecosystem Dependabot groups would batch unrelated supply-chain diffs"
            ));
        }

        if let Some(value) = yaml_line_value_for_any_key(line, "package-ecosystem") {
            if let Some(block) = current.take() {
                validate_dependabot_update_block(&block, &mut errors);
                match block.ecosystem.as_str() {
                    "github-actions" => github_actions_entries += 1,
                    "cargo" => cargo_entries += 1,
                    _ => {}
                }
            }
            current = Some(DependabotUpdateBlock::new(
                trim_yaml_scalar(value).to_string(),
                line_no,
            ));
            continue;
        }

        let Some(block) = current.as_mut() else {
            continue;
        };
        match yaml_line_key(line) {
            Some("groups") => block.groups_line = Some(line_no),
            Some("cooldown") => block.cooldown_line = Some(line_no),
            Some("target-branch") => block.target_branch_line = Some(line_no),
            Some("multi-ecosystem-group") => block.multi_ecosystem_group_line = Some(line_no),
            _ => {}
        }
        if let Some(value) = yaml_line_value_for_any_key(line, "default-days") {
            block.cooldown_default_days = Some((line_no, trim_yaml_scalar(value).to_string()));
        }
        if let Some(value) = yaml_line_value_for_any_key(line, "open-pull-requests-limit") {
            block.open_pull_requests_limit = Some((line_no, trim_yaml_scalar(value).to_string()));
        }
    }

    if let Some(block) = current.take() {
        validate_dependabot_update_block(&block, &mut errors);
        match block.ecosystem.as_str() {
            "github-actions" => github_actions_entries += 1,
            "cargo" => cargo_entries += 1,
            _ => {}
        }
    }

    if github_actions_entries != 1 {
        errors.push(format!(
            "expected exactly one github-actions Dependabot update entry, found {github_actions_entries}"
        ));
    }
    if cargo_entries != 1 {
        errors.push(format!(
            "expected exactly one cargo Dependabot update entry, found {cargo_entries}"
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n  "))
    }
}

fn check_dependabot_policy() -> Result<(), String> {
    let root = workspace_root();
    let rel = ".github/dependabot.yml";
    let text = std::fs::read_to_string(root.join(rel))
        .map_err(|e| format!("check-dependabot-policy: FAIL — could not read {rel}: {e}"))?;
    validate_dependabot_policy(&text).map_err(|e| {
        format!(
            "check-dependabot-policy: FAIL —\n  {e}\n\
             \n\
             xPare keeps automated dependency PRs narrow so each update gets a real\n\
             supply-chain review. Leave GitHub Actions ungrouped with a 7-day version\n\
             cooldown, keep Cargo routine version PRs disabled, and do not delay or\n\
             batch Cargo security-update PRs without an explicit posture change."
        )
    })?;
    println!(
        "check-dependabot-policy: Dependabot keeps action updates reviewable and Cargo security updates undelayed."
    );
    Ok(())
}

fn release_workflow_steps(text: &str) -> Vec<ReleaseWorkflowStep> {
    let mut steps = Vec::new();
    for (start, line) in line_offset_pairs(text) {
        if !line.starts_with("      - ") {
            continue;
        }
        let name = yaml_line_value_for_key(line, "name")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        steps.push(ReleaseWorkflowStep { name, start });
    }
    steps
}

fn require_ordered_release_workflow_steps(
    text: &str,
    steps: &[&str],
) -> Result<Vec<usize>, String> {
    let workflow_steps = release_workflow_steps(text);
    let mut positions = Vec::with_capacity(steps.len());
    for required_step in steps {
        let matches: Vec<&ReleaseWorkflowStep> = workflow_steps
            .iter()
            .filter(|step| step.name.as_deref() == Some(*required_step))
            .collect();
        match matches.as_slice() {
            [] => return Err(format!("missing required step `{required_step}`")),
            [step] => positions.push(step.start),
            _ => {
                return Err(format!(
                    "duplicate required step `{required_step}`; release credential boundary must be unique"
                ));
            }
        }
    }
    for ((left_step, left_pos), (right_step, right_pos)) in steps
        .iter()
        .zip(positions.iter())
        .zip(steps.iter().skip(1).zip(positions.iter().skip(1)))
    {
        if left_pos >= right_pos {
            return Err(format!("step `{left_step}` must run before `{right_step}`"));
        }
    }
    Ok(positions)
}

fn find_action_step_line(text: &str) -> Option<(usize, String)> {
    text.lines().enumerate().find_map(|(idx, line)| {
        if yaml_line_key(line) == Some("uses") {
            Some((idx + 1, line.trim_start().to_string()))
        } else {
            None
        }
    })
}

fn reject_action_steps_in_window(text: &str, label: &str) -> Result<(), String> {
    if let Some((line, action)) = find_action_step_line(text) {
        return Err(format!(
            "third-party or external `uses:` action appears {label} at window line {line}: `{action}`"
        ));
    }
    Ok(())
}

fn workflow_job_slice<'a>(
    text: &'a str,
    job_name: &str,
    next_job_name: Option<&str>,
) -> Result<&'a str, String> {
    let marker = format!("  {job_name}:\n");
    let start = text
        .find(&marker)
        .ok_or_else(|| format!("missing workflow job `{job_name}`"))?;
    let end = match next_job_name {
        Some(next_job_name) => {
            let next_marker = format!("  {next_job_name}:\n");
            text[start + marker.len()..]
                .find(&next_marker)
                .map(|idx| start + marker.len() + idx)
                .ok_or_else(|| format!("missing workflow job `{next_job_name}`"))?
        }
        None => text.len(),
    };
    if start >= end {
        return Err(format!(
            "workflow job `{job_name}` is empty or out of order"
        ));
    }
    Ok(&text[start..end])
}

fn non_comment_line_contains(text: &str, snippet: &str) -> bool {
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with('#'))
        .any(|line| line.contains(snippet))
}

fn non_comment_line_starts_with(text: &str, snippet: &str) -> bool {
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with('#'))
        .any(|line| line.starts_with(snippet))
}

fn require_non_comment_lines_in_order(
    text: &str,
    snippets: &[&str],
    label: &str,
) -> Result<(), String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut cursor = 0usize;
    for snippet in snippets {
        let Some(match_idx) = lines
            .iter()
            .enumerate()
            .skip(cursor)
            .find_map(|(idx, line)| {
                let trimmed = line.trim_start();
                if !trimmed.starts_with('#') && trimmed.starts_with(snippet) {
                    Some(idx)
                } else {
                    None
                }
            })
        else {
            return Err(format!(
                "{label} must contain executable line `{snippet}` in order"
            ));
        };
        cursor = match_idx + 1;
    }
    Ok(())
}

fn reject_release_mutation_primitives(text: &str, label: &str) -> Result<(), String> {
    for banned in [
        "gh release upload",
        "--clobber",
        "gh release delete-asset",
        "uploads/releases/assets",
    ] {
        if non_comment_line_contains(text, banned) {
            return Err(format!(
                "{label} must not mutate or delete existing release assets: banned `{banned}`"
            ));
        }
    }
    Ok(())
}

fn non_comment_lines_containing<'a>(text: &'a str, snippet: &str) -> Vec<&'a str> {
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with('#') && line.contains(snippet))
        .collect()
}

fn reject_release_delete_lines(text: &str, label: &str) -> Result<(), String> {
    if non_comment_line_contains(text, "gh release delete") {
        return Err(format!(
            "{label} must not delete releases outside scoped same-run draft cleanup"
        ));
    }
    Ok(())
}

fn continued_shell_command_starting_with(text: &str, command_start: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || !trimmed.starts_with(command_start) {
            continue;
        }
        let mut command = String::new();
        for line in &lines[idx..] {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }
            let line = trimmed.trim_end();
            let continued = line.ends_with('\\');
            let line = line.trim_end_matches('\\').trim_end();
            if !command.is_empty() {
                command.push(' ');
            }
            command.push_str(line);
            if !continued {
                break;
            }
        }
        return Some(command);
    }
    None
}

/// Validate the official release workflow's signing and publication boundary.
/// Apple Developer ID and notary material may exist only during native signing;
/// signed assets are verified, encrypted before any workflow artifact handoff,
/// attested, paired with an SBOM, then published by a run-only create job. The
/// workflow must not mutate or delete assets on an existing release: the
/// complete draft is created once after all required metadata is ready. This is
/// deliberately textual because the invariant is about ordered named workflow
/// steps and shell command shape, not YAML syntax (actionlint covers that).
fn validate_release_workflow_credential_boundary(text: &str) -> Result<(), String> {
    reject_release_mutation_primitives(text, "official release workflow")?;
    for removed_job in [
        "attach-sbom-official",
        "retract-incomplete-official-release",
    ] {
        if non_comment_line_contains(text, removed_job) {
            return Err(format!(
                "official release workflow must not use the removed post-create mutation job `{removed_job}`"
            ));
        }
    }

    let steps = [
        "Import Developer ID certificate",
        "Store notary credentials",
        "Sign and notarize (make dist)",
        "Capture signed asset manifest",
        "Clean temporary signing material",
        "Verify signed assets before encrypted handoff",
        "Encrypt signed release handoff",
        "Upload encrypted signed release handoff",
        "Materialize signed subject checksums",
        "Attest build provenance",
        "Generate SBOM (SPDX JSON)",
        "Upload SBOM workflow artifact",
        "Download encrypted signed release handoff",
        "Download release SBOM",
        "Verify signed assets before release creation",
        "Publish GitHub Release",
    ];
    let positions = require_ordered_release_workflow_steps(text, &steps)?;

    let import_pos = positions[0];
    let cleanup_pos = positions[4];
    let credential_window = &text[import_pos..cleanup_pos];
    reject_action_steps_in_window(credential_window, "while Apple signing material is present")?;

    let signed_asset_to_handoff_window = &text[positions[3]..positions[6]];
    reject_action_steps_in_window(
        signed_asset_to_handoff_window,
        "after signed assets are captured and before encrypted handoff",
    )?;

    let sign_job = workflow_job_slice(text, "sign-official", Some("attest-official"))?;
    for required in [
        "contents: read",
        "release_subject_checksums_b64: ${{ steps.signed_manifest.outputs.checksums_b64 }}",
        "signed_manifest_sha256: ${{ steps.signed_manifest.outputs.sha256 }}",
        "signed_asset_handoff_artifact_id: ${{ steps.upload_signed_handoff.outputs.artifact-id }}",
        "signed_asset_handoff_key_hex: ${{ steps.signed_handoff.outputs.key_hex }}",
        "signed_asset_handoff_iv_hex: ${{ steps.signed_handoff.outputs.iv_hex }}",
        "signed_asset_handoff_sha256: ${{ steps.signed_handoff.outputs.sha256 }}",
    ] {
        if !non_comment_line_contains(sign_job, required) {
            return Err(format!(
                "sign-official job must contain `{required}` so signing outputs are explicit and non-publishing"
            ));
        }
    }
    if non_comment_line_contains(sign_job, "contents: write") {
        return Err("sign-official job must not grant release-asset write permission".to_string());
    }

    let attest_job = workflow_job_slice(text, "attest-official", Some("sbom-official"))?;
    for required in [
        "contents: read",
        "id-token: write",
        "attestations: write",
        "RELEASE_SUBJECT_CHECKSUMS_B64: ${{ needs['sign-official'].outputs.release_subject_checksums_b64 }}",
        "base64 -d > dist/release/signed-release-assets.sha256",
        "subject-checksums: dist/release/signed-release-assets.sha256",
    ] {
        if !non_comment_line_contains(attest_job, required) {
            return Err(format!(
                "attest-official job must contain `{required}` so provenance runs without release-asset write permission"
            ));
        }
    }
    if non_comment_line_contains(attest_job, "contents: write") {
        return Err(
            "attest-official job must not grant release-asset write permission".to_string(),
        );
    }
    if non_comment_line_contains(attest_job, "artifact-metadata: write") {
        return Err(
            "attest-official job must not grant artifact metadata write permission unless it publishes registry storage records"
                .to_string(),
        );
    }
    if non_comment_line_contains(attest_job, "gh release download") {
        return Err(
            "attest-official job must not download draft release assets; attest the signed checksum subjects instead"
                .to_string(),
        );
    }

    let sbom_job = workflow_job_slice(text, "sbom-official", Some("publish-official"))?;
    for required in [
        "contents: read",
        "release_sbom_artifact_id: ${{ steps.upload_sbom.outputs.artifact-id }}",
        "upload-release-assets: false",
        "id: upload_sbom",
        "Upload SBOM workflow artifact",
    ] {
        if !non_comment_line_contains(sbom_job, required) {
            return Err(format!(
                "sbom-official job must contain `{required}` so SBOM generation runs without release-asset write permission"
            ));
        }
    }
    if non_comment_line_contains(sbom_job, "contents: write") {
        return Err("sbom-official job must not grant release-asset write permission".to_string());
    }

    let publish_job = workflow_job_slice(text, "publish-official", None)?;
    for required in [
        "contents: write",
        "actions: read",
        "sign-official",
        "attest-official",
        "sbom-official",
        "RELEASE_VERSION: ${{ needs['sign-official'].outputs.release_version }}",
        "RELEASE_SUBJECT_CHECKSUMS_B64: ${{ needs['sign-official'].outputs.release_subject_checksums_b64 }}",
        "SIGNED_MANIFEST_SHA256: ${{ needs['sign-official'].outputs.signed_manifest_sha256 }}",
        "SIGNED_ASSET_HANDOFF_ARTIFACT_ID: ${{ needs['sign-official'].outputs.signed_asset_handoff_artifact_id }}",
        "SIGNED_ASSET_HANDOFF_KEY_HEX: ${{ needs['sign-official'].outputs.signed_asset_handoff_key_hex }}",
        "SIGNED_ASSET_HANDOFF_IV_HEX: ${{ needs['sign-official'].outputs.signed_asset_handoff_iv_hex }}",
        "SIGNED_ASSET_HANDOFF_SHA256: ${{ needs['sign-official'].outputs.signed_asset_handoff_sha256 }}",
        "RELEASE_SBOM_ARTIFACT_ID: ${{ needs['sbom-official'].outputs.release_sbom_artifact_id }}",
        "GH_REPO: ${{ github.repository }}",
        r#"dist/release/xPare-v${RELEASE_VERSION}-sbom.spdx.json"#,
    ] {
        if !non_comment_line_contains(publish_job, required) {
            return Err(format!(
                "publish-official job must contain `{required}` so the complete draft is created with explicit inputs"
            ));
        }
    }
    reject_action_steps_in_window(
        publish_job,
        "inside the release-create job with release write permission",
    )?;
    if non_comment_line_contains(publish_job, "gh run download") {
        return Err(
            "publish-official job must download artifacts by artifact-id, not `gh run download`"
                .to_string(),
        );
    }

    let store_notary_credentials = &text[positions[1]..positions[2]];
    let store_command = continued_shell_command_starting_with(
        store_notary_credentials,
        r#"xcrun notarytool store-credentials "${NOTARY_PROFILE}""#,
    )
    .ok_or_else(|| {
        "notary credentials must be stored with the xcrun notarytool store-credentials command"
            .to_string()
    })?;
    for required in [r#"--keychain "${KEYCHAIN_PATH}""#] {
        if !store_command.contains(required) {
            return Err(format!(
                "notary credentials must be stored in the temporary keychain with `{required}` on the notarytool command"
            ));
        }
    }

    let sign_and_notarize = &text[positions[2]..positions[3]];
    if !non_comment_line_contains(sign_and_notarize, r#"NOTARY_KEYCHAIN="${KEYCHAIN_PATH}""#) {
        return Err(
            "Sign and notarize (make dist) must pass NOTARY_KEYCHAIN=\"${KEYCHAIN_PATH}\" so notarytool reads the temporary keychain"
                .to_string(),
        );
    }

    let cleanup_to_verify = &text[cleanup_pos..positions[5]];
    for required in [
        "if: ${{ always() }}",
        r#"if [ -e "${KEYCHAIN_PATH}" ]; then"#,
        r#"security delete-keychain "${KEYCHAIN_PATH}""#,
        r#"rm -f "${CERTIFICATE_PATH}" "${NOTARY_KEY_PATH}""#,
    ] {
        if !non_comment_line_contains(cleanup_to_verify, required) {
            return Err(format!(
                "cleanup step must contain `{required}` before signed assets leave the signing job"
            ));
        }
    }
    for line in cleanup_to_verify.lines() {
        if line.contains("security delete-keychain") && line.contains("||") {
            return Err(
                "cleanup step must fail closed when deleting the signing keychain fails"
                    .to_string(),
            );
        }
    }

    let capture_to_cleanup = &text[positions[3]..cleanup_pos];
    for required in [
        "id: signed_manifest",
        "SIGNED_RELEASE_MANIFEST",
        "shasum -a 256 -- *.zip *.zip.sha256 SHA256SUMS*",
        r#"shasum -a 256 "${SIGNED_RELEASE_MANIFEST}""#,
        r#"echo "sha256=${manifest_sha}" >> "${GITHUB_OUTPUT}""#,
        r#"checksums_b64="$(base64 < "${SIGNED_RELEASE_MANIFEST}" | tr -d '\n')""#,
        r#"echo "checksums_b64=${checksums_b64}" >> "${GITHUB_OUTPUT}""#,
    ] {
        if !non_comment_line_contains(capture_to_cleanup, required) {
            return Err(format!(
                "signed asset manifest capture must contain `{required}`"
            ));
        }
    }

    let verify_before_encrypted_handoff = &text[positions[5]..positions[6]];
    for required in [
        "SIGNED_MANIFEST_SHA256: ${{ steps.signed_manifest.outputs.sha256 }}",
        r#"expected_manifest_sha="${SIGNED_MANIFEST_SHA256}""#,
        "shasum -a 256 --check --strict -",
        "shasum -a 256 -- *.zip *.zip.sha256 SHA256SUMS*",
        r#"diff -u "${SIGNED_RELEASE_MANIFEST}""#,
    ] {
        if !non_comment_line_contains(verify_before_encrypted_handoff, required) {
            return Err(format!(
                "signed asset verification before encrypted handoff must contain `{required}`"
            ));
        }
    }

    let encrypt_signed_handoff = &text[positions[6]..positions[7]];
    for required in [
        "id: signed_handoff",
        r#"handoff_dir="${RUNNER_TEMP}/signed-release-handoff""#,
        r#"key_hex="$(openssl rand -hex 32)""#,
        r#"iv_hex="$(openssl rand -hex 16)""#,
        r#"( cd dist/release && tar -cf "${tarball}" -- *.zip *.zip.sha256 SHA256SUMS )"#,
        r#"openssl enc -aes-256-cbc -K "${key_hex}" -iv "${iv_hex}""#,
        r#"raw_signed_asset_remaining="$(find dist/release"#,
        r#"echo "SIGNED_ASSET_HANDOFF_DIR=${handoff_dir}""#,
        r#"} >> "${GITHUB_ENV}""#,
        r#"echo "key_hex=${key_hex}""#,
        r#"echo "iv_hex=${iv_hex}""#,
        r#"echo "sha256=${encrypted_sha}""#,
        r#"} >> "${GITHUB_OUTPUT}""#,
    ] {
        if !non_comment_line_contains(encrypt_signed_handoff, required) {
            return Err(format!(
                "signed release handoff encryption must contain `{required}`"
            ));
        }
    }
    require_non_comment_lines_in_order(
        encrypt_signed_handoff,
        &[
            r#"( cd dist/release && tar -cf "${tarball}" -- *.zip *.zip.sha256 SHA256SUMS )"#,
            r#"openssl enc -aes-256-cbc -K "${key_hex}" -iv "${iv_hex}""#,
            r#"rm -f -- dist/release/*.zip"#,
            r#"raw_signed_asset_remaining="$(find dist/release"#,
            r#"if [ -n "${raw_signed_asset_remaining}" ]; then"#,
            r#"echo "raw signed release asset remains"#,
            "exit 1",
            "fi",
            r#"encrypted_sha="$(shasum -a 256 "${encrypted}""#,
        ],
        "signed release handoff raw-asset cleanup",
    )?;
    let remove_raw_assets = continued_shell_command_starting_with(
        encrypt_signed_handoff,
        "rm -f -- dist/release/*.zip",
    )
    .ok_or_else(|| {
        "signed release handoff encryption must remove raw signed assets before any action upload"
            .to_string()
    })?;
    for required in [
        "dist/release/*.zip",
        "dist/release/*.zip.sha256",
        "dist/release/SHA256SUMS*",
    ] {
        if !remove_raw_assets.contains(required) {
            return Err(format!(
                "raw signed asset cleanup must remove `{required}` before any action upload"
            ));
        }
    }
    let find_raw_assets = continued_shell_command_starting_with(
        encrypt_signed_handoff,
        r#"raw_signed_asset_remaining="$(find dist/release"#,
    )
    .ok_or_else(|| {
        "signed release handoff encryption must verify raw signed assets are absent before any action upload"
            .to_string()
    })?;
    for required in [
        "-name '*.zip'",
        "-name '*.zip.sha256'",
        "-name 'SHA256SUMS*'",
    ] {
        if !find_raw_assets.contains(required) {
            return Err(format!(
                "raw signed asset absence check must inspect `{required}` before any action upload"
            ));
        }
    }

    let upload_signed_handoff = &text[positions[7]..positions[8]];
    for required in [
        "id: upload_signed_handoff",
        "uses: actions/upload-artifact@",
        "name: encrypted-signed-release-handoff-${{ env.RELEASE_VERSION }}",
        "path: ${{ env.SIGNED_ASSET_HANDOFF_DIR }}/signed-release-assets.tar.enc",
        "if-no-files-found: error",
        "retention-days: 1",
        "compression-level: 0",
    ] {
        if !non_comment_line_contains(upload_signed_handoff, required) {
            return Err(format!(
                "encrypted signed release handoff upload must contain `{required}`"
            ));
        }
    }
    for forbidden in [
        "dist/release/*.zip",
        "dist/release/*.zip.sha256",
        "dist/release/SHA256SUMS",
    ] {
        if non_comment_line_contains(upload_signed_handoff, forbidden) {
            return Err(format!(
                "encrypted signed release handoff upload must not expose raw signed asset path `{forbidden}`"
            ));
        }
    }

    let attest_step = &text[positions[9]..positions[10]];
    for required in ["subject-checksums: dist/release/signed-release-assets.sha256"] {
        if !non_comment_line_contains(attest_step, required) {
            return Err(format!(
                "build provenance attestation must use signed release subject checksum input `{required}`"
            ));
        }
    }

    let download_signed_assets = &text[positions[12]..positions[13]];
    for required in [
        "GH_REPO: ${{ github.repository }}",
        r#"artifact_zip="${RUNNER_TEMP}/signed-release-handoff.zip""#,
        r#"handoff_dir="${RUNNER_TEMP}/signed-release-handoff""#,
        r#""/repos/${GH_REPO}/actions/artifacts/${SIGNED_ASSET_HANDOFF_ARTIFACT_ID}/zip" > "${artifact_zip}""#,
        r#"unzip -q "${artifact_zip}" -d "${handoff_dir}""#,
        r#"encrypted="${handoff_dir}/signed-release-assets.tar.enc""#,
        r#"tarball="${handoff_dir}/signed-release-assets.tar""#,
        r#"test -s "${encrypted}""#,
        r#"printf '%s  %s\n' "${SIGNED_ASSET_HANDOFF_SHA256}" "${encrypted}" | shasum -a 256 --check --strict -"#,
        r#"openssl enc -d -aes-256-cbc -K "${SIGNED_ASSET_HANDOFF_KEY_HEX}" -iv "${SIGNED_ASSET_HANDOFF_IV_HEX}""#,
        r#"tar -C dist/release -xf "${tarball}""#,
    ] {
        if !non_comment_line_contains(download_signed_assets, required) {
            return Err(format!(
                "publish-official encrypted signed handoff download must contain `{required}`"
            ));
        }
    }

    let download_release_sbom = &text[positions[13]..positions[14]];
    for required in [
        "GH_REPO: ${{ github.repository }}",
        r#"artifact_zip="${RUNNER_TEMP}/release-sbom.zip""#,
        r#"sbom_dir="${RUNNER_TEMP}/release-sbom""#,
        r#"rm -rf "${sbom_dir}""#,
        r#""/repos/${GH_REPO}/actions/artifacts/${RELEASE_SBOM_ARTIFACT_ID}/zip" > "${artifact_zip}""#,
        r#"unzip -q "${artifact_zip}" -d "${sbom_dir}""#,
        r#"sbom="${sbom_dir}/xPare-v${RELEASE_VERSION}-sbom.spdx.json""#,
        r#"test -s "${sbom}""#,
        r#"cp "${sbom}" "dist/release/xPare-v${RELEASE_VERSION}-sbom.spdx.json""#,
    ] {
        if !non_comment_line_contains(download_release_sbom, required) {
            return Err(format!(
                "publish-official SBOM download must contain `{required}`"
            ));
        }
    }

    let verify_before_release_creation = &text[positions[14]..positions[15]];
    for required in [
        r#"printf '%s' "${RELEASE_SUBJECT_CHECKSUMS_B64}" | base64 -d > "${expected_manifest}""#,
        r#"printf '%s  %s\n' "${SIGNED_MANIFEST_SHA256}" "${expected_manifest}" | shasum -a 256 --check --strict -"#,
        "shasum -a 256 -- *.zip *.zip.sha256 SHA256SUMS*",
        r#"diff -u "${expected_manifest}" "${current_manifest}""#,
    ] {
        if !non_comment_line_contains(verify_before_release_creation, required) {
            return Err(format!(
                "signed asset verification before release creation must contain `{required}`"
            ));
        }
    }

    let publish_step = &text[positions[15]..];
    reject_release_delete_lines(&text[..positions[15]], "official release workflow")?;
    require_non_comment_lines_in_order(
        publish_step,
        &[
            "release_create_started=false",
            "cleanup_partial_draft_release() {",
            r#"draft_state="$(gh release view "v${RELEASE_VERSION}" --json isDraft --jq .isDraft"#,
            r#"if [ "${draft_state}" = "true" ]; then"#,
            r#"gh release delete "v${RELEASE_VERSION}" --yes"#,
            "exit \"${status}\"",
            "}",
            "trap cleanup_partial_draft_release ERR",
            r#"if gh release view "v${RELEASE_VERSION}" >/dev/null 2>&1; then"#,
            r#"echo "Release v${RELEASE_VERSION} already exists; refusing to replace release assets.""#,
            "exit 1",
            "fi",
            "create_flags=(--draft)",
            "release_create_started=true",
            r#"gh release create "v${RELEASE_VERSION}""#,
            r#"expected_assets="${RUNNER_TEMP}/release-assets.expected""#,
            r#"gh release view "v${RELEASE_VERSION}" --json assets --jq '.assets[].name'"#,
            r#"diff -u "${expected_assets}" "${actual_assets}""#,
            "release_create_started=false",
            "trap - ERR",
        ],
        "Publish GitHub Release step",
    )?;
    if !non_comment_line_starts_with(publish_step, r#"gh release create "v${RELEASE_VERSION}""#) {
        return Err(
            "Publish GitHub Release step must execute `gh release create` directly".to_string(),
        );
    }
    let create_command = continued_shell_command_starting_with(
        publish_step,
        r#"gh release create "v${RELEASE_VERSION}""#,
    )
    .ok_or_else(|| "Publish GitHub Release step must create the release".to_string())?;
    for required in [
        r#""${zips[0]}""#,
        r#""${zips[0]}.sha256""#,
        r#""dist/release/SHA256SUMS""#,
        r#""dist/release/xPare-v${RELEASE_VERSION}-sbom.spdx.json""#,
        "--verify-tag",
        r#""${create_flags[@]}""#,
    ] {
        if !create_command.contains(required) {
            return Err(format!(
                "Publish GitHub Release command must include `{required}`"
            ));
        }
    }
    let release_delete_lines = non_comment_lines_containing(publish_step, "gh release delete");
    match release_delete_lines.as_slice() {
        [line] if *line == r#"gh release delete "v${RELEASE_VERSION}" --yes"# => {}
        [] => {
            return Err(
                "Publish GitHub Release step must delete a same-run partial draft on failure"
                    .to_string(),
            );
        }
        _ => {
            return Err(
                "Publish GitHub Release step must contain only the scoped partial-draft `gh release delete` cleanup"
                    .to_string(),
            );
        }
    }
    let asset_view_command = continued_shell_command_starting_with(
        publish_step,
        r#"gh release view "v${RELEASE_VERSION}" --json assets --jq '.assets[].name'"#,
    )
    .ok_or_else(|| {
        "Publish GitHub Release step must inspect release asset names after creation".to_string()
    })?;
    if !asset_view_command.contains(r#"sort > "${actual_assets}""#) {
        return Err(
            "Publish GitHub Release step must sort actual release asset names for verification"
                .to_string(),
        );
    }

    Ok(())
}

/// check-workflows: lint the GitHub Actions workflows for correctness (actionlint)
/// and security (zizmor). actionlint is a system tool; zizmor is cargo-installable
/// (auto-installed, pinned). This is the same workflow-security gate CI runs via
/// `cargo xtask ci`, so an agent catches workflow problems before pushing instead
/// of from a failed CI run.
fn check_workflows() -> Result<(), String> {
    let root = workspace_root();
    if !root.join(".github/workflows").is_dir() {
        println!("check-workflows: no .github/workflows directory; nothing to lint.");
        return Ok(());
    }
    let release_workflow_path = root.join(".github/workflows/release.yml");
    let release_workflow = std::fs::read_to_string(&release_workflow_path).map_err(|e| {
        format!(
            "check-workflows: FAIL — could not read {}: {e}",
            release_workflow_path.display()
        )
    })?;
    validate_release_workflow_credential_boundary(&release_workflow).map_err(|e| {
        format!(
            "check-workflows: FAIL — {} violates the official release credential boundary: {e}",
            release_workflow_path.display()
        )
    })?;
    // Correctness first (fast, offline): expression/syntax errors, bad `needs`
    // graphs, shellcheck over inline `run:` blocks.
    let actionlint = require_system_tool(
        "actionlint",
        "GitHub Actions workflow linting (correctness)",
        ACTIONLINT_INSTALL_HINT,
    )?;
    run_tool("actionlint", &actionlint, &[])?;
    // Security second: template injection, credential persistence, unpinned actions,
    // over-broad token permissions. Run --offline so the gate's exit code never
    // depends on a GitHub token or network reachability — deterministic locally and
    // in CI.
    let zizmor = ensure_cargo_tool("zizmor", "zizmor", ZIZMOR_VERSION)?;
    run_tool("zizmor", &zizmor, &["--offline", ".github/workflows"])
}

/// check-supply-chain: cargo-deny over the whole workspace per the checked-in
/// `deny.toml` — RustSec advisories, the license allowlist, banned/duplicate
/// crates, and the crates.io-only source policy. Complements `check-core-deps`
/// (which constrains *what* the core may pull in) with *known-vulnerability* and
/// license auditing across the entire tree.
fn check_supply_chain() -> Result<(), String> {
    let deny = ensure_cargo_tool("cargo-deny", "cargo-deny", CARGO_DENY_VERSION)?;
    run_tool("cargo-deny", &deny, &["check"]).map_err(|e| {
        format!(
            "{e}\n\
             \n\
             A supply-chain gate tripped (see deny.toml): a RustSec advisory, a\n\
             non-allowed license, a banned/duplicate crate, or an unknown source. Fix it\n\
             by updating, replacing, or dropping the offending dependency. Only after a\n\
             documented risk decision, add a *scoped* `ignore`/`exceptions` entry (with a\n\
             reason) in deny.toml — do not broaden the policy to make the check pass."
        )
    })
}

/// check-unused-deps: cargo-machete over the whole workspace. Orthogonal to
/// `check-core-deps` (which constrains *what* the core may pull in) and
/// `check-supply-chain` (advisories/licenses) — this asks the anti-slop question:
/// is every *declared* dependency actually *used*? AI-authored edits routinely
/// leave a dependency behind after the code that needed it is deleted. machete
/// inspects each crate's source (`--with-metadata` resolves renamed crates so it
/// does not false-positive on them) and fails if a manifest declares a crate
/// nothing references.
fn check_unused_deps() -> Result<(), String> {
    let machete = ensure_cargo_tool("cargo-machete", "cargo-machete", CARGO_MACHETE_VERSION)?;
    run_tool("cargo-machete", &machete, &["--with-metadata"]).map_err(|e| {
        format!(
            "{e}\n\
             \n\
             cargo-machete found a dependency that is declared but never used. Remove the\n\
             unused entry from the offending Cargo.toml. If it is a genuine false positive\n\
             (used only behind a cfg or via a macro machete cannot see), add it to that\n\
             crate's `[package.metadata.cargo-machete] ignored = [...]` with a reason —\n\
             do not delete a dependency the build actually needs."
        )
    })
}

/// check-docs: build the workspace docs with `RUSTDOCFLAGS=-D warnings` so a broken
/// intra-doc link, an unresolved `[item]` reference, or invalid inline HTML in a doc
/// comment fails the gate. AI-authored docs routinely leave dangling `[Foo]` links and
/// stale references behind; this makes "the docs still build" a mechanical fact rather
/// than a hope. Deterministic and offline (rustdoc on the pinned stable toolchain) —
/// no nightly — so it stays in the required `ci` gate, unlike the heavy best-effort tools.
fn check_docs() -> Result<(), String> {
    println!(r#"check-docs: $ RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps"#);
    let status = Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps"])
        .env("RUSTDOCFLAGS", "-D warnings")
        .current_dir(workspace_root())
        .status()
        .map_err(|e| format!("check-docs: FAIL — could not launch `cargo doc`: {e}"))?;
    if status.success() {
        println!("check-docs: workspace docs build clean (no broken links, no invalid doc HTML).");
        Ok(())
    } else {
        Err(format!(
            "check-docs: FAIL — `cargo doc` reported problems (exited {status}).\n\
             \n\
             A doc comment has a broken intra-doc link (a `[Item]` that does not resolve, or a\n\
             public doc linking to a private item), or invalid inline HTML (e.g. an unescaped\n\
             angle-bracket placeholder read as a tag). Fix the reference: make the link resolve,\n\
             drop the brackets so it is plain inline code, or wrap a usage snippet in a fenced\n\
             code block. Do not silence it with a blanket #[allow(...)]."
        ))
    }
}

fn fuzz_dir() -> PathBuf {
    workspace_root().join("fuzz")
}

/// Ensure `cargo +nightly ...` is usable. If a fresh rustup-based agent has not
/// installed nightly yet, install the minimal profile on demand; normal stable
/// builds remain pinned by `rust-toolchain.toml`.
fn ensure_nightly_toolchain() -> Result<(), String> {
    let available = Command::new("cargo")
        .args(["+nightly", "--version"])
        .output()
        .map_err(|e| format!("check-fuzz: FAIL — could not launch `cargo +nightly`: {e}"))?;
    if available.status.success() {
        return Ok(());
    }

    println!(
        "check-fuzz: nightly toolchain not found — installing `nightly` with rustup \
         (minimal profile)…"
    );
    let installed = Command::new("rustup")
        .args(["toolchain", "install", "nightly", "--profile", "minimal"])
        .status()
        .map_err(|e| {
            format!(
                "check-fuzz: FAIL — could not launch `rustup` to install nightly: {e}\n\
                 Install rustup/nightly manually and re-run:\n\
                 \x20 rustup toolchain install nightly --profile minimal"
            )
        })?
        .success();
    if installed {
        Ok(())
    } else {
        Err(format!(
            "check-fuzz: FAIL — could not install the nightly Rust toolchain.\n\
             Install it manually and re-run:\n\
             \x20 rustup toolchain install nightly --profile minimal\n\
             Original `cargo +nightly --version` stderr:\n{}",
            String::from_utf8_lossy(&available.stderr)
        ))
    }
}

fn parse_fuzz_targets(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_fuzz_smoke_seconds(raw: Option<&str>) -> Result<Option<u64>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let raw = raw.trim();
    if raw.is_empty() || raw == "0" {
        return Ok(None);
    }
    let seconds: u64 = raw.parse().map_err(|_| {
        format!(
            "check-fuzz: FAIL — XP_FUZZ_SMOKE_SECONDS must be a positive integer, \
             0, or empty; got `{raw}`."
        )
    })?;
    Ok(Some(seconds))
}

fn run_cargo_fuzz(args: &[&str], path_env: Option<&std::ffi::OsString>) -> Result<(), String> {
    println!("check-fuzz: $ cargo +nightly fuzz {}", args.join(" "));
    let mut command = Command::new("cargo");
    command
        .args(["+nightly", "fuzz"])
        .args(args)
        .current_dir(fuzz_dir());
    if let Some(path) = path_env {
        command.env("PATH", path);
    }
    let status = command
        .status()
        .map_err(|e| format!("check-fuzz: FAIL — could not launch `cargo +nightly fuzz`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "check-fuzz: FAIL — `cargo +nightly fuzz {}` exited with {status}. \
             Fix the fuzz target, dependency, or toolchain issue.",
            args.join(" ")
        ))
    }
}

fn cargo_fuzz_targets(path_env: Option<&std::ffi::OsString>) -> Result<Vec<String>, String> {
    let mut command = Command::new("cargo");
    command
        .args(["+nightly", "fuzz", "list"])
        .current_dir(fuzz_dir());
    if let Some(path) = path_env {
        command.env("PATH", path);
    }
    let output = command.output().map_err(|e| {
        format!("check-fuzz: FAIL — could not launch `cargo +nightly fuzz list`: {e}")
    })?;
    if !output.status.success() {
        return Err(format!(
            "check-fuzz: FAIL — `cargo +nightly fuzz list` exited with {}:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let targets = parse_fuzz_targets(&String::from_utf8_lossy(&output.stdout));
    if targets.is_empty() {
        return Err("check-fuzz: FAIL — `cargo +nightly fuzz list` found no targets.".to_string());
    }
    Ok(targets)
}

/// check-fuzz: local/CI path for the separate cargo-fuzz workspace. This stays
/// outside the required `ci` gate because nightly/libFuzzer smoke is intentionally
/// best-effort, but it is still mechanical and drift-proof: targets are discovered
/// from `cargo fuzz list`, not hard-coded in Makefile or GitHub Actions.
fn check_fuzz() -> Result<(), String> {
    let dir = fuzz_dir();
    if !dir.join("Cargo.toml").is_file() {
        println!("check-fuzz: no fuzz/Cargo.toml; nothing to check.");
        return Ok(());
    }

    ensure_nightly_toolchain()?;
    let cargo_fuzz = ensure_cargo_tool("cargo-fuzz", "cargo-fuzz", CARGO_FUZZ_VERSION)?;
    let path_env = path_with_tool_dir(&cargo_fuzz);
    let targets = cargo_fuzz_targets(path_env.as_ref())?;
    println!("check-fuzz: targets: {}", targets.join(", "));

    run_cargo_fuzz(&["build"], path_env.as_ref())?;

    let smoke_seconds_raw = std::env::var("XP_FUZZ_SMOKE_SECONDS").ok();
    if let Some(seconds) = parse_fuzz_smoke_seconds(smoke_seconds_raw.as_deref())? {
        let max_total_time = format!("-max_total_time={seconds}");
        for target in targets {
            run_cargo_fuzz(
                &["run", target.as_str(), "--", max_total_time.as_str()],
                path_env.as_ref(),
            )?;
        }
        println!("check-fuzz: all fuzz targets smoke-ran for {seconds}s each.");
    } else {
        println!(
            "check-fuzz: built all fuzz targets. Set XP_FUZZ_SMOKE_SECONDS=N to \
             run every target briefly."
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// check-miri
// ---------------------------------------------------------------------------
//
// The core is `#![forbid(unsafe_code)]`, so memory-unsafety is impossible there by
// construction — Miri would only confirm what the compiler already guarantees. ALL
// `unsafe` lives in `core-ffi` (pointer validation, the leaked-Box buffer protocol,
// zeroize-on-free, lossy-UTF-8 decode). `core-ffi/tests/abi_roundtrip.rs` drives the
// real `extern "C"` entry points through raw pointers; running just that crate under
// Miri's UB detector turns "FFI memory behavior is exercised" into "exercised AND no
// undefined behavior was detected on the tested executions". Like `check-fuzz`, this
// is nightly-only and intentionally OUTSIDE the required `cargo xtask ci` gate (the
// stable gate stays the single required signal); CI runs it as a best-effort job.
//
// Caveat: Miri checks the executions the tests actually drive, not all inputs — it
// is dynamic UB detection, not a proof. Coverage of inputs is cargo-fuzz's job.

/// Ensure the `miri` rustup component is installed on nightly, installing it on
/// demand the same way `check-fuzz` bootstraps the nightly toolchain.
fn ensure_miri_component() -> Result<(), String> {
    let available = Command::new("cargo")
        .args(["+nightly", "miri", "--version"])
        .output();
    if let Ok(out) = available {
        if out.status.success() {
            return Ok(());
        }
    }
    println!("check-miri: `miri` component not found — installing it on nightly via rustup…");
    let installed = Command::new("rustup")
        .args(["component", "add", "miri", "--toolchain", "nightly"])
        .status()
        .map_err(|e| {
            format!(
                "check-miri: FAIL — could not launch `rustup` to add the miri component: {e}\n\
                 Install it manually and re-run:\n\
                 \x20 rustup component add miri --toolchain nightly"
            )
        })?
        .success();
    if installed {
        Ok(())
    } else {
        Err(
            "check-miri: FAIL — could not install the `miri` component. Install it manually:\n\
             \x20 rustup component add miri --toolchain nightly"
                .to_string(),
        )
    }
}

/// check-miri: run the `core-ffi` boundary tests under Miri to detect undefined
/// behavior in the only crate that uses `unsafe`. Best-effort and nightly-only, so
/// it stays out of the required `ci` gate (mirrors `check-fuzz`).
fn check_miri() -> Result<(), String> {
    ensure_nightly_toolchain()?;
    ensure_miri_component()?;

    // `cargo miri test -p xpare-ffi` builds the FFI crate (and its deps) under
    // Miri and runs its tests, including the unsafe boundary round-trips. The core is
    // pulled in as a dependency and interpreted too, but we scope the *test run* to
    // the crate that owns the unsafe so the pass stays fast. Default Miri isolation
    // is fine: nothing in these tests touches the filesystem, clock, or network.
    println!("check-miri: $ cargo +nightly miri test -p xpare-ffi");
    let status = Command::new("cargo")
        .args(["+nightly", "miri", "test", "-p", "xpare-ffi"])
        .current_dir(workspace_root())
        .status()
        .map_err(|e| {
            format!("check-miri: FAIL — could not launch `cargo +nightly miri test`: {e}")
        })?;
    if status.success() {
        println!("check-miri: core-ffi boundary tests ran clean under Miri (no UB detected).");
        Ok(())
    } else {
        Err(format!(
            "check-miri: FAIL — Miri reported a problem in the core-ffi boundary (exited {status}).\n\
             \n\
             Miri detects undefined behavior in `unsafe` code. A failure here means a pointer\n\
             validity, provenance, aliasing, or buffer-ownership bug in `core-ffi` — the only\n\
             crate allowed `unsafe`. Fix the boundary code; do not silence Miri. If it is a\n\
             known-benign Miri limitation, narrow the suppression with a documented reason."
        ))
    }
}

// ---------------------------------------------------------------------------
// check-kani
// ---------------------------------------------------------------------------
//
// Kani is a bounded model checker: it proves a property for ALL inputs within bounds
// (via CBMC), not just the inputs a test happens to drive. The proof harnesses live
// in `core/src/config.rs` behind `#[cfg(kani)]`, so they are invisible to normal
// builds and to `cargo metadata` — the `kani` crate never enters the dependency tree
// that `check-core-deps` guards. They prove the crisp resource-envelope arithmetic:
// the saturating growth product gate accepts a pipeline iff its true worst-case
// growth is within the cap (no saturation wrap can falsely accept an amplifier).
//
// Kani is heavy (it downloads a CBMC toolchain via `cargo kani setup`), so — like
// check-fuzz and check-miri — it is best-effort and OUTSIDE the required `ci` gate.
// CI runs it on a cadence / on demand (.github/workflows/proofs.yml), not per-PR.

/// Ensure `cargo kani` is installed and set up, installing the pinned
/// `kani-verifier` and running `cargo kani setup` on demand (a one-time toolchain
/// download). In CI the proofs workflow pre-installs it, so this is a no-op there.
fn ensure_kani() -> Result<(), String> {
    if let Ok(out) = Command::new("cargo").args(["kani", "--version"]).output() {
        if out.status.success() {
            return Ok(());
        }
    }
    println!(
        "check-kani: `cargo kani` not found — installing kani-verifier@{KANI_VERSION} and \
         running `cargo kani setup` (one-time; downloads the CBMC toolchain)…"
    );
    let installed = Command::new("cargo")
        .args([
            "install",
            "--locked",
            &format!("kani-verifier@{KANI_VERSION}"),
        ])
        .status()
        .map_err(|e| {
            format!("check-kani: FAIL — could not launch `cargo install kani-verifier`: {e}")
        })?
        .success();
    if !installed {
        return Err(format!(
            "check-kani: FAIL — could not install kani-verifier. Install it manually:\n\
             \x20 cargo install --locked kani-verifier@{KANI_VERSION} && cargo kani setup"
        ));
    }
    let setup = Command::new("cargo")
        .args(["kani", "setup"])
        .status()
        .map_err(|e| format!("check-kani: FAIL — could not launch `cargo kani setup`: {e}"))?
        .success();
    if setup {
        Ok(())
    } else {
        Err(
            "check-kani: FAIL — `cargo kani setup` did not complete. Re-run it manually:\n\
             \x20 cargo kani setup"
                .to_string(),
        )
    }
}

/// check-kani: run the bounded proofs over the resource-envelope arithmetic in
/// `xpare-core`. Best-effort and heavy, so it stays out of the required `ci`
/// gate (mirrors `check-fuzz` / `check-miri`).
fn check_kani() -> Result<(), String> {
    ensure_kani()?;

    // `cargo kani -p xpare-core` discovers and verifies every `#[kani::proof]`
    // harness in the core crate. Harnesses are `#[cfg(kani)]`, so this is the only
    // command that compiles them at all.
    println!("check-kani: $ cargo kani -p xpare-core");
    let status = Command::new("cargo")
        .args(["kani", "-p", "xpare-core"])
        .current_dir(workspace_root())
        .status()
        .map_err(|e| format!("check-kani: FAIL — could not launch `cargo kani`: {e}"))?;
    if status.success() {
        println!("check-kani: bounded resource-envelope proofs verified.");
        Ok(())
    } else {
        Err(format!(
            "check-kani: FAIL — a bounded proof did not verify (exited {status}).\n\
             \n\
             Kani found an input within bounds that violates a proven property of the\n\
             resource-envelope arithmetic (see the `kani_proofs` module in\n\
             core/src/config.rs). This means the saturating growth gate could mis-accept or\n\
             mis-reject a pipeline. Fix `saturating_growth_product` / `max_growth_factor` or\n\
             the harness assumptions; do not weaken the proof to make it pass."
        ))
    }
}

// ---------------------------------------------------------------------------
// check-coverage / check-mutants (best-effort, event-driven — NOT in the `ci` gate)
// ---------------------------------------------------------------------------
//
// These are the deepest anti-slop signal but, like check-fuzz/miri/kani, they are HEAVY
// and DETERMINISTIC: re-running them on unchanged code proves nothing new, so they are
// outside the required gate and are NEVER scheduled — they run on demand, locally, and
// event-driven (path-filtered) in .github/workflows/hygiene.yml. See
// docs/guardrails/code-and-test-hygiene.md.
//
//   * check-coverage: a line-coverage FLOOR (ratchet). Catches whole swaths of new code
//     that no test exercises. Coverage is necessary but not sufficient — a test can run
//     a line without asserting on it — which is why check-mutants exists.
//   * check-mutants: mutates each line; a SURVIVING mutant means a test ran the line but
//     nothing constrained its behavior (dead code or an under-asserted "slop" test). The
//     fix is to strengthen a test — and that new assertion becomes a permanent regression.

/// Line-coverage floor for `check-coverage`, as a whole-number percent. A ratchet: raise
/// it (never lower it) in a deliberate, reviewed edit as coverage improves — exactly like
/// `MAX_IGNORED_TESTS`. Set from the measured full-tree baseline with a small margin so a
/// flaky run never trips it. Measured product-code baseline at introduction was ~95.6%
/// lines (the `xtask` tooling is excluded — it is the enforcement harness, not product
/// logic, and is verified by being run in CI). Note coverage jitters slightly run-to-run
/// because proptest explores fresh inputs each run, so keep a margin above the floor.
const COVERAGE_FLOOR_PCT: u32 = 95;

/// Sources-only line-coverage floor for `check-swift`, as a percent. Same ratchet
/// discipline as [`COVERAGE_FLOOR_PCT`]: raise it (never lower it) as the shell's tests
/// improve. Matches the Rust product floor (95%) — the OS-facing layers are tested
/// headlessly: `SystemPasteboard` against an app-private `NSPasteboard(name:)`, and the
/// Carbon hot-key trampoline by invoking it with a synthesized `kEventHotKeyPressed` event.
/// (The `XPareApp` SwiftUI target is the only unmeasured Swift: it's an executable,
/// not linked into the test bundle — the analog of the Rust binary crates the workspace
/// floor doesn't gate on.) Measured Sources-only baseline at this floor was ~95.8% lines
/// (Tests/ and the derived test runner are excluded); the floor sits just under so a
/// refactor doesn't spuriously trip it.
const SWIFT_COVERAGE_FLOOR_PCT: f64 = 95.0;

/// Ensure the `llvm-tools` rustup component (the instrumentation runtime cargo-llvm-cov
/// needs) is installed, adding it on demand the same way `check-miri` bootstraps `miri`.
fn ensure_llvm_tools() -> Result<(), String> {
    if let Ok(out) = Command::new("rustup")
        .args(["component", "list", "--installed"])
        .output()
    {
        if out.status.success()
            && String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|l| l.starts_with("llvm-tools"))
        {
            return Ok(());
        }
    }
    println!("check-coverage: installing the `llvm-tools-preview` component via rustup…");
    let ok = Command::new("rustup")
        .args(["component", "add", "llvm-tools-preview"])
        .status()
        .map_err(|e| format!("check-coverage: FAIL — could not launch `rustup`: {e}"))?
        .success();
    if ok {
        Ok(())
    } else {
        Err(
            "check-coverage: FAIL — could not install `llvm-tools-preview`. Install it manually:\n\
             \x20 rustup component add llvm-tools-preview"
                .to_string(),
        )
    }
}

/// check-coverage: fail if workspace line coverage falls below [`COVERAGE_FLOOR_PCT`].
/// Best-effort and heavy (an instrumented build + full test run), so — like check-mutants
/// — it is outside the required `ci` gate.
fn check_coverage() -> Result<(), String> {
    let tool = ensure_cargo_tool("cargo-llvm-cov", "cargo-llvm-cov", CARGO_LLVM_COV_VERSION)?;
    ensure_llvm_tools()?;
    let path_env = path_with_tool_dir(&tool);
    let floor = COVERAGE_FLOOR_PCT.to_string();

    // Exclude the xtask tooling from the measurement: it is the enforcement harness, not
    // product logic, and dragging it in would make the floor meaningless (and match the
    // `.cargo/mutants.toml` exclusion). `--summary-only` keeps the output to the table + verdict.
    let ignore_xtask = "(^|/)xtask/";
    println!(
        "check-coverage: $ cargo llvm-cov --workspace --summary-only \
         --ignore-filename-regex '{ignore_xtask}' --fail-under-lines {floor}"
    );
    let mut cmd = Command::new("cargo");
    cmd.args([
        "llvm-cov",
        "--workspace",
        "--summary-only",
        "--ignore-filename-regex",
        ignore_xtask,
        "--fail-under-lines",
        &floor,
    ])
    .current_dir(workspace_root());
    if let Some(p) = &path_env {
        cmd.env("PATH", p);
    }
    let status = cmd
        .status()
        .map_err(|e| format!("check-coverage: FAIL — could not launch `cargo llvm-cov`: {e}"))?;
    if status.success() {
        println!("check-coverage: workspace line coverage is at or above the {floor}% floor.");
        Ok(())
    } else {
        Err(format!(
            "check-coverage: FAIL — workspace line coverage dropped below the {floor}% floor.\n\
             \n\
             New code landed without tests exercising it. Add tests for the uncovered lines\n\
             (a reference-interpreter clause + property beats a lone example). Only raise\n\
             COVERAGE_FLOOR_PCT in xtask/src/main.rs when coverage genuinely improves — the\n\
             floor is a ratchet that moves up, never down."
        ))
    }
}

/// check-mutants: run cargo-mutants over the product logic (see `.cargo/mutants.toml`). A
/// MISSED mutant is dead code or an under-asserted test and FAILS the check; TIMEOUT
/// outcomes pass (the mutation hung the suite — that is detection, not a gap) and
/// UNVIABLE ones are non-signal. `XP_DIFF_BASE=<ref>` scopes the run to lines changed vs
/// `<ref>` (fast PR feedback via `--in-diff`); unset = full tree. Best-effort and heavy,
/// so it stays out of the required `ci` gate.
fn check_mutants() -> Result<(), String> {
    let tool = ensure_cargo_tool("cargo-mutants", "cargo-mutants", CARGO_MUTANTS_VERSION)?;
    let path_env = path_with_tool_dir(&tool);
    let root = workspace_root();

    // An empty XP_DIFF_BASE (CI passes "" on non-PR events) means full-tree, same as unset.
    let diff_base = std::env::var("XP_DIFF_BASE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut args: Vec<String> = vec!["mutants".to_string()];
    if let Some(base) = diff_base {
        println!("check-mutants: scoping to the diff vs `{base}` (XP_DIFF_BASE)");
        let diff = Command::new("git")
            .args(["diff", &base])
            .current_dir(&root)
            .output()
            .map_err(|e| format!("check-mutants: FAIL — could not run `git diff {base}`: {e}"))?;
        if !diff.status.success() {
            return Err(format!(
                "check-mutants: FAIL — `git diff {base}` failed; is XP_DIFF_BASE a valid ref?"
            ));
        }
        let diff_path = root.join("target").join("xp-mutants-in.diff");
        if let Some(parent) = diff_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("check-mutants: FAIL — could not create target dir: {e}"))?;
        }
        std::fs::write(&diff_path, &diff.stdout)
            .map_err(|e| format!("check-mutants: FAIL — could not write the diff file: {e}"))?;
        args.push("--in-diff".to_string());
        args.push(diff_path.to_string_lossy().into_owned());
    } else {
        println!("check-mutants: full-tree run (set XP_DIFF_BASE=<ref> to scope to a diff)");
    }

    // Parallelism: CI stays SERIAL for predictable memory on shared runners; a LOCAL run
    // hammers the box across all cores (cargo-mutants gives each job its own build dir).
    // GitHub Actions sets `CI`, so key off that.
    if std::env::var_os("CI").is_none() {
        let jobs = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        if jobs > 1 {
            println!(
                "check-mutants: local run — parallelizing across {jobs} jobs (CI stays serial)"
            );
            args.push("--jobs".to_string());
            args.push(jobs.to_string());
        }
    }

    println!("check-mutants: $ cargo {}", args.join(" "));
    let mut cmd = Command::new("cargo");
    cmd.args(args.iter().map(String::as_str)).current_dir(&root);
    if let Some(p) = &path_env {
        cmd.env("PATH", p);
    }
    let status = cmd
        .status()
        .map_err(|e| format!("check-mutants: FAIL — could not launch `cargo mutants`: {e}"))?;

    // The verdict comes from mutants.out, NOT the raw exit status: cargo-mutants
    // exits nonzero for TIMEOUT outcomes too, but a mutant that hangs the suite IS
    // detected (the hang is loud; it cannot ship silently) — only a MISSED mutant
    // (suite stays green under the mutation) is an assertion gap. The exit status
    // still vetoes interrupted/errored runs that never classified all mutants.
    let out_dir = root.join("mutants.out");
    let count_lines = |name: &str| -> usize {
        std::fs::read_to_string(out_dir.join(name))
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0)
    };
    let missed_list = std::fs::read_to_string(out_dir.join("missed.txt")).unwrap_or_default();
    let generated = std::fs::read_to_string(out_dir.join("mutants.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_array().map(Vec::len))
        .unwrap_or(0);
    classify_mutants_outcome(
        status.code(),
        generated,
        &missed_list,
        count_lines("caught.txt"),
        count_lines("timeout.txt"),
        count_lines("unviable.txt"),
    )
}

/// Decide pass/fail from a completed `cargo mutants` run. Pure so the policy is
/// unit-testable: FAIL on missed mutants and on runs that did not complete
/// (interrupt/internal error, or no classified outcomes despite generated
/// mutants — e.g. a baseline test failure); PASS otherwise, treating TIMEOUT
/// outcomes as detected and UNVIABLE ones as non-signal.
fn classify_mutants_outcome(
    exit_code: Option<i32>,
    generated: usize,
    missed_list: &str,
    caught: usize,
    timeout: usize,
    unviable: usize,
) -> Result<(), String> {
    let missed: Vec<&str> = missed_list
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    if !missed.is_empty() {
        return Err(format!(
            "check-mutants: FAIL — {} MISSED mutant(s); see mutants.out/missed.txt:\n  {}\n\
             \n\
             A MISSED mutant means a line was changed and the whole test suite still passed:\n\
             either the line is dead (delete it) or a test runs it without asserting on its\n\
             behavior (strengthen the assertion — that becomes a permanent regression). Only\n\
             skip a genuinely equivalent mutant via `.cargo/mutants.toml` with a documented reason.",
            missed.len(),
            missed.join("\n  ")
        ));
    }
    // Exit code 1 is cargo-mutants' usage/internal-error class, and a killed
    // process has no code at all: in both cases the sweep did not finish, so an
    // empty missed.txt proves nothing.
    match exit_code {
        None => {
            return Err(
                "check-mutants: FAIL — cargo-mutants was killed by a signal before completing; \
                 an empty missed.txt from a partial sweep proves nothing. Re-run."
                    .to_string(),
            );
        }
        Some(1) => {
            return Err(
                "check-mutants: FAIL — cargo-mutants reported a usage/internal error; \
                 see mutants.out/ and the log above."
                    .to_string(),
            );
        }
        Some(_) => {}
    }
    if generated == 0 {
        println!(
            "check-mutants: no mutants in scope (empty or non-product diff) — nothing to test."
        );
        return Ok(());
    }
    if caught + timeout + unviable == 0 {
        return Err(
            "check-mutants: FAIL — mutants were generated but none were classified \
             (baseline test failure or an interrupted run); see mutants.out/log."
                .to_string(),
        );
    }
    print!("check-mutants: 0 missed ({caught} caught, {timeout} timeout, {unviable} unviable).");
    if timeout > 0 {
        print!(
            " Timeouts count as DETECTED: the mutation hung the suite, which cannot ship silently."
        );
    }
    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// check-swift
// ---------------------------------------------------------------------------
//
// Anti-slop parity for the Swift macOS shell. The §13 anti-slop gates are
// Rust/cargo-specific (clippy, the `[workspace.lints]`, cargo-llvm-cov, cargo-mutants,
// the `.rs`-only check-test-hygiene), so `shells/macos/` had no linter/coverage/test
// gate and its tests ran only locally. This is the cross-language analog:
//   * swift-format lint (style/format) — toolchain-native, deterministic; a HARD gate.
//   * swift test                       — runs the shell's tests in CI, not just locally.
//   * Sources-only line-coverage floor — via llvm-cov; a HARD ratchet (SWIFT_COVERAGE_FLOOR_PCT).
//   * SwiftLint (style/complexity)     — OPTIONAL, run-if-present: SwiftLint has no
//                                        SHA-pinned install path like the rest of the repo's
//                                        tooling, so we don't add it to CI; it runs for devs
//                                        who have it, and skips with a note otherwise.
//
// Best-effort and macOS-only: the required suite is the Linux `cargo xtask ci`. This check is
// invoked by the `macos-shell` CI job (continue-on-error) and skips cleanly where the Swift
// toolchain is absent. Fronted by xtask so local == CI.

/// The macOS shell package, relative to the workspace root.
const SWIFT_SHELL_DIR: &str = "shells/macos";

/// CommandLineTools-only environments (no full Xcode) don't put swift-testing's
/// `Testing.framework` / interop dylib on the default search path; these `-F`/`-rpath`
/// flags let `swift test` load them. With full Xcode the dirs are absent and we add
/// nothing (Xcode resolves them itself). Mirrors `shells/macos/build.sh`.
const CLT_FRAMEWORKS_DIR: &str = "/Library/Developer/CommandLineTools/Library/Developer/Frameworks";
const CLT_INTEROP_DIR: &str = "/Library/Developer/CommandLineTools/Library/Developer/usr/lib";

/// Extra `swift test` flags so the test bundle finds swift-testing under CLT-only hosts.
fn swift_test_runtime_flags() -> Vec<String> {
    let mut flags: Vec<String> = Vec::new();
    if std::path::Path::new(CLT_FRAMEWORKS_DIR).is_dir() {
        flags.extend(
            [
                "-Xswiftc",
                "-F",
                "-Xswiftc",
                CLT_FRAMEWORKS_DIR,
                "-Xlinker",
                "-rpath",
                "-Xlinker",
                CLT_FRAMEWORKS_DIR,
            ]
            .into_iter()
            .map(String::from),
        );
    }
    if std::path::Path::new(CLT_INTEROP_DIR).is_dir() {
        flags.extend(
            ["-Xlinker", "-rpath", "-Xlinker", CLT_INTEROP_DIR]
                .into_iter()
                .map(String::from),
        );
    }
    flags
}

// llvm-cov `export -summary-only` JSON model (only the field we read).
#[derive(serde::Deserialize)]
struct LlvmCovExport {
    data: Vec<LlvmCovData>,
}
#[derive(serde::Deserialize)]
struct LlvmCovData {
    totals: LlvmCovTotals,
}
#[derive(serde::Deserialize)]
struct LlvmCovTotals {
    lines: LlvmCovMetric,
}
#[derive(serde::Deserialize)]
struct LlvmCovMetric {
    percent: f64,
}

/// Extract the line-coverage percent from `llvm-cov export -summary-only` JSON.
fn parse_llvm_cov_lines_percent(json: &str) -> Result<f64, String> {
    let export: LlvmCovExport = serde_json::from_str(json)
        .map_err(|e| format!("could not parse llvm-cov export JSON: {e}"))?;
    let first = export
        .data
        .first()
        .ok_or_else(|| "llvm-cov export JSON had no `data` entries".to_string())?;
    Ok(first.totals.lines.percent)
}

/// Pure pass/fail verdict for a measured coverage percent against the floor. Factored out
/// so the ratchet is unit-tested without an instrumented build (matching how the Rust
/// checks keep their parsing/decision logic testable).
fn swift_coverage_verdict(percent: f64, floor: f64) -> Result<(), String> {
    // Small epsilon so a value sitting exactly on the floor isn't tripped by float repr.
    if percent + 1e-9 >= floor {
        Ok(())
    } else {
        Err(format!(
            "check-swift: FAIL — macOS shell Sources line coverage {percent:.2}% is below the \
             {floor:.1}% floor.\n\
             \n\
             New Swift code landed without tests exercising it. Add tests for the uncovered\n\
             lines in shells/macos/Tests. Only raise SWIFT_COVERAGE_FLOOR_PCT in\n\
             xtask/src/main.rs when coverage genuinely improves — the floor is a ratchet that\n\
             moves up, never down."
        ))
    }
}

/// Find the SwiftPM test bundle's executable: `<dir>/<Name>.xctest/Contents/MacOS/<Name>`.
fn find_xctest_binary(debug_dir: &std::path::Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(debug_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) == Some("xctest") {
            let stem = path.file_stem()?.to_owned();
            let bin = path.join("Contents").join("MacOS").join(&stem);
            if bin.is_file() {
                return Some(bin);
            }
        }
    }
    None
}

/// Phase 1 (HARD): toolchain-native `swift format lint`, strict.
fn check_swift_format(swift: &std::path::Path, shell: &std::path::Path) -> Result<(), String> {
    println!(
        "check-swift: $ swift format lint --strict --recursive --configuration .swift-format \
         Sources Tests"
    );
    let status = Command::new(swift)
        .args([
            "format",
            "lint",
            "--strict",
            "--recursive",
            "--configuration",
            ".swift-format",
            "Sources",
            "Tests",
        ])
        .current_dir(shell)
        .status()
        .map_err(|e| format!("check-swift: FAIL — could not launch `swift format`: {e}"))?;
    if status.success() {
        println!("check-swift: swift-format lint clean.");
        Ok(())
    } else {
        Err(
            "check-swift: FAIL — `swift format lint` found style violations.\n\
             Fix them mechanically with:\n\
             \x20 swift format --in-place --recursive --configuration shells/macos/.swift-format \
             shells/macos/Sources shells/macos/Tests\n\
             Do not loosen shells/macos/.swift-format to silence the gate."
                .to_string(),
        )
    }
}

/// Phase 2: build the FFI staticlib the Swift package links over the frozen C ABI.
fn swift_build_ffi_staticlib() -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    println!("check-swift: $ {cargo} build -p xpare-ffi --release");
    let status = Command::new(&cargo)
        .args(["build", "-p", "xpare-ffi", "--release"])
        .current_dir(workspace_root())
        .status()
        .map_err(|e| format!("check-swift: FAIL — could not launch `cargo build`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(
            "check-swift: FAIL — building the FFI staticlib (`cargo build -p xpare-ffi \
             --release`) failed; the Swift package links it over the frozen C ABI."
                .to_string(),
        )
    }
}

/// Measure Sources-only line coverage from the just-run instrumented test build. Returns
/// `Ok(None)` if the coverage toolchain (`xcrun llvm-cov`) isn't available — best-effort, so
/// a host without it still gets the test-pass signal.
fn measure_swift_coverage(
    swift: &std::path::Path,
    shell: &std::path::Path,
) -> Result<Option<f64>, String> {
    // SwiftPM tells us where the exported coverage JSON lives; its directory also holds
    // default.profdata, and the parent (debug) dir holds the .xctest bundle. Deriving paths
    // this way avoids hard-coding the target triple (arm64/x86_64-apple-macosx).
    let cov_path_out = Command::new(swift)
        .args(["test", "--show-code-coverage-path"])
        .current_dir(shell)
        .output()
        .map_err(|e| format!("check-swift: FAIL — could not query coverage path: {e}"))?;
    if !cov_path_out.status.success() {
        return Err(
            "check-swift: FAIL — `swift test --show-code-coverage-path` failed.".to_string(),
        );
    }
    let cov_json_raw = String::from_utf8_lossy(&cov_path_out.stdout)
        .trim()
        .to_string();
    let cov_json = std::path::Path::new(&cov_json_raw);
    let codecov_dir = cov_json
        .parent()
        .ok_or_else(|| "check-swift: FAIL — coverage path had no parent directory.".to_string())?;
    let debug_dir = codecov_dir
        .parent()
        .ok_or_else(|| "check-swift: FAIL — could not locate the SwiftPM debug dir.".to_string())?;
    let profdata = codecov_dir.join("default.profdata");

    let Some(test_binary) = find_xctest_binary(debug_dir) else {
        return Err(format!(
            "check-swift: FAIL — could not find the .xctest bundle binary under {}.",
            debug_dir.display()
        ));
    };

    // xcrun fronts llvm-cov from the active toolchain. Absent (no Xcode/CLT) => best-effort skip.
    let Some(xcrun) = resolve_tool("xcrun") else {
        return Ok(None);
    };

    // Sources-only: exclude the test files and the SwiftPM-derived test runner so the floor
    // measures product code, not the tests measuring themselves.
    let ignore = r"(/Tests/|\.derived/|/\.build/)";
    let bin = test_binary.to_string_lossy();
    let prof = profdata.to_string_lossy();
    println!(
        "check-swift: $ xcrun llvm-cov export <test-bin> -instr-profile <profdata> \
         -ignore-filename-regex='{ignore}' -summary-only"
    );
    let out = Command::new(&xcrun)
        .args([
            "llvm-cov",
            "export",
            bin.as_ref(),
            "-instr-profile",
            prof.as_ref(),
            "-ignore-filename-regex",
            ignore,
            "-summary-only",
        ])
        .current_dir(shell)
        .output()
        .map_err(|e| format!("check-swift: FAIL — could not launch `xcrun llvm-cov`: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "check-swift: FAIL — `xcrun llvm-cov export` failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let json = String::from_utf8_lossy(&out.stdout);
    let percent =
        parse_llvm_cov_lines_percent(&json).map_err(|e| format!("check-swift: FAIL — {e}"))?;
    Ok(Some(percent))
}

/// Phase 3 (HARD): run the shell's tests, then enforce the Sources coverage floor.
fn check_swift_tests_and_coverage(
    swift: &std::path::Path,
    shell: &std::path::Path,
) -> Result<(), String> {
    let mut args: Vec<String> = vec!["test".to_string(), "--enable-code-coverage".to_string()];
    args.extend(swift_test_runtime_flags());
    println!("check-swift: $ swift {}", args.join(" "));
    let status = Command::new(swift)
        .args(&args)
        .current_dir(shell)
        .status()
        .map_err(|e| format!("check-swift: FAIL — could not launch `swift test`: {e}"))?;
    if !status.success() {
        return Err(
            "check-swift: FAIL — `swift test` reported failing test(s) in the macOS shell."
                .to_string(),
        );
    }
    println!("check-swift: swift test passed; measuring Sources line coverage…");

    match measure_swift_coverage(swift, shell)? {
        Some(percent) => {
            println!(
                "check-swift: Sources line coverage = {percent:.2}% \
                 (floor {SWIFT_COVERAGE_FLOOR_PCT:.1}%)."
            );
            swift_coverage_verdict(percent, SWIFT_COVERAGE_FLOOR_PCT)
        }
        None => {
            println!(
                "check-swift: coverage tooling (`xcrun llvm-cov`) unavailable — skipping the \
                 coverage floor (tests still passed)."
            );
            Ok(())
        }
    }
}

/// Phase 4 (OPTIONAL): SwiftLint style/complexity, run only if `swiftlint` is on PATH.
/// Not `--strict`: warnings are advisory and only `error`-severity findings fail the gate
/// (thresholds are tuned in `.swiftlint.yml` so the current code is error-clean). SourceKit
/// is disabled for determinism — a CLT-only host (no full Xcode) crashes trying to load
/// `sourcekitdInProc`, so we skip the handful of SourceKit-dependent rules and behave
/// identically locally and in CI.
fn check_swift_lint_if_present(shell: &std::path::Path) -> Result<(), String> {
    let Some(swiftlint) = resolve_tool("swiftlint") else {
        println!(
            "check-swift: `swiftlint` not on PATH — skipping the optional style/complexity pass \
             (enable it locally with `brew install swiftlint`; CI installs a pinned, \
             checksum-verified build). swift-format + tests + coverage above are the enforced \
             gates."
        );
        return Ok(());
    };
    println!("check-swift: $ SWIFTLINT_DISABLE_SOURCEKIT=1 swiftlint lint --config .swiftlint.yml");
    let status = Command::new(&swiftlint)
        .args(["lint", "--config", ".swiftlint.yml"])
        .env("SWIFTLINT_DISABLE_SOURCEKIT", "1")
        .current_dir(shell)
        .status()
        .map_err(|e| format!("check-swift: FAIL — could not launch `swiftlint`: {e}"))?;
    if status.success() {
        println!("check-swift: SwiftLint clean (no error-severity findings).");
        Ok(())
    } else {
        Err(
            "check-swift: FAIL — SwiftLint reported error-severity style/complexity violations. \
             Fix them, or for a genuine false positive add a scoped `// swiftlint:disable` with a \
             reason (or tune a threshold in shells/macos/.swiftlint.yml)."
                .to_string(),
        )
    }
}

/// check-swift: the macOS shell anti-slop tier. See the module comment above for the
/// gate/skip contract.
fn check_swift() -> Result<(), String> {
    let root = workspace_root();
    let shell = root.join(SWIFT_SHELL_DIR);
    if !shell.join("Package.swift").is_file() {
        println!("check-swift: {SWIFT_SHELL_DIR}/Package.swift not present; nothing to check.");
        return Ok(());
    }
    // macOS-only, best-effort: the required gate is the Linux `cargo xtask ci`. If the Swift
    // toolchain isn't here (e.g. the Linux CI host), skip cleanly rather than fail.
    let Some(swift) = resolve_tool("swift") else {
        println!(
            "check-swift: `swift` not found — skipping (best-effort, macOS-only gate; the required \
             suite is `cargo xtask ci`)."
        );
        return Ok(());
    };

    check_swift_format(&swift, &shell)?;
    swift_build_ffi_staticlib()?;
    check_swift_tests_and_coverage(&swift, &shell)?;
    check_swift_lint_if_present(&shell)?;

    println!("check-swift: macOS shell anti-slop gates passed.");
    Ok(())
}

// ---------------------------------------------------------------------------
// check-agent-workflow
// ---------------------------------------------------------------------------
//
// The AI-native engineering loop is encoded in repo-native docs and thin agent
// wrappers (see docs/agent-workflow.md). Those files only stay load-bearing if
// they keep their structure and point at the canonical guardrails: this check
// fails CI if one is deleted, loses a required section, or routes a wrapper to a
// nonexistent repo path. It is a pure structural check (no external tools),
// matching the other docs-structure guards.

/// The workflow files and the section headings each must keep. Headings are matched
/// as exact lines (after trimming) so a rename or accidental deletion fails the
/// check, but reordering or adding sections is fine. Kept intentionally small and
/// stable: these are the load-bearing sections, not every heading.
const AGENT_WORKFLOW_FILES: &[(&str, &[&str])] = &[
    (
        "AGENTS.md",
        &[
            "## Knowledge base",
            "## Operating Loop",
            "## Review guidelines",
            "## Pull Requests",
        ],
    ),
    (
        "CONTRIBUTING.md",
        &["## GitHub Actions lanes", "## Closing review findings"],
    ),
    ("docs/agent-workflow.md", &["## The loop", "## North star"]),
    (
        "docs/guardrails/agentic-security-finding-triage.md",
        &["## The rule", "## PR evidence"],
    ),
    (
        "docs/templates/correctness-brief.md",
        &["## Change class", "## Evidence packet", "## Proof gaps"],
    ),
    (
        ".github/pull_request_template.md",
        &[
            "## Change class",
            "## Security finding triage",
            "## Dependency review recommendation",
            "## Commands run",
        ],
    ),
    ("docs/agent-tasks/core-transform.md", AGENT_TASK_HEADINGS),
    ("docs/agent-tasks/ffi-boundary.md", AGENT_TASK_HEADINGS),
    ("docs/agent-tasks/security-privacy.md", AGENT_TASK_HEADINGS),
    ("docs/agent-tasks/dependency-ci.md", AGENT_TASK_HEADINGS),
    (
        "docs/agent-tasks/review-finding-closure.md",
        AGENT_TASK_HEADINGS,
    ),
    (
        ".agents/skills/security-finding-triage/SKILL.md",
        &["# Security Finding Triage"],
    ),
    (
        ".claude/skills/security-finding-triage/SKILL.md",
        &["# Security Finding Triage"],
    ),
];

const SECURITY_TRIAGE_SKILL_GUARDRAIL_LINKS: &[&str] = &[
    "../../../docs/guardrails/agentic-security-finding-triage.md",
    "../../../docs/guardrails/review-finding-closure.md",
];

const CONTRIBUTOR_REVIEW_FINDING_GUARDRAIL_LINKS: &[&str] = &[
    "docs/guardrails/agentic-security-finding-triage.md",
    "docs/guardrails/review-finding-closure.md",
];

/// Markers the top-level Codex/GitHub review guidance must keep. This is a
/// lightweight structural guard, not a prose parser: if the review policy is
/// intentionally rewritten, preserve these reviewer-facing concepts or update the
/// markers in the same PR.
const AGENTS_REVIEW_GUIDELINE_MARKERS: &[&str] = &[
    "docs/guardrails/dependency-posture.md#dependabot-merge-recommendations",
    "`merge`, `hold`",
    "`close/defer`",
    "upstream-diff evidence",
    "xPare usage path",
    "capability delta",
    "maintainer/repository trust signals",
    "checks inspected",
    "P1",
    "Failed checks",
];

/// Workflow entry points and agent wrappers must remain thin pointers to the
/// repo-owned guardrails. The literals are intentionally exact: if a guardrail
/// moves, update the entry point and this check in the same PR after proving the
/// new path resolves from the source file's directory.
const AGENT_WORKFLOW_GUARDRAIL_LINKS: &[(&str, &[&str])] = &[
    (
        "CONTRIBUTING.md",
        CONTRIBUTOR_REVIEW_FINDING_GUARDRAIL_LINKS,
    ),
    (
        ".agents/skills/security-finding-triage/SKILL.md",
        SECURITY_TRIAGE_SKILL_GUARDRAIL_LINKS,
    ),
    (
        ".claude/skills/security-finding-triage/SKILL.md",
        SECURITY_TRIAGE_SKILL_GUARDRAIL_LINKS,
    ),
];

/// Sections every agent-task prompt template must carry, so each stays a complete,
/// self-contained, copy-paste-ready task rather than a stub.
const AGENT_TASK_HEADINGS: &[&str] = &[
    "## Files to read",
    "## Hard constraints",
    "## Required evidence",
    "## Proof gaps to report",
];

/// Validate one workflow file's text against its required headings. Returns the
/// headings that are missing (empty = OK). Heading match is on trimmed full lines so
/// a partial-substring or a heading demoted to prose does not count.
fn missing_workflow_headings(text: &str, required: &[&str]) -> Vec<String> {
    required
        .iter()
        .filter(|heading| !text.lines().any(|line| line.trim() == **heading))
        .map(|heading| (*heading).to_string())
        .collect()
}

fn missing_text_markers(text: &str, required: &[&str]) -> Vec<String> {
    required
        .iter()
        .filter(|marker| !text.contains(**marker))
        .map(|marker| (*marker).to_string())
        .collect()
}

fn missing_agents_review_guideline_markers(text: &str) -> Vec<String> {
    missing_text_markers(text, AGENTS_REVIEW_GUIDELINE_MARKERS)
}

fn missing_guardrail_links_for_text(
    root: &Path,
    rel: &str,
    text: &str,
    required_links: &[&str],
) -> Vec<String> {
    let source_dir = Path::new(rel).parent().unwrap_or_else(|| Path::new(""));
    required_links
        .iter()
        .filter_map(|link| {
            if !text.contains(link) {
                return Some(format!(
                    "{rel} must mention required guardrail link `{link}`"
                ));
            }

            let target = root.join(source_dir).join(link);
            if target.is_file() {
                None
            } else {
                Some(format!(
                    "{rel} guardrail link `{link}` does not resolve to a file from the source directory ({})",
                    target.display()
                ))
            }
        })
        .collect()
}

fn missing_workflow_guardrail_links(root: &Path) -> Vec<String> {
    AGENT_WORKFLOW_GUARDRAIL_LINKS
        .iter()
        .flat_map(|(rel, required_links)| {
            let path = root.join(rel);
            match std::fs::read_to_string(&path) {
                Ok(text) => missing_guardrail_links_for_text(root, rel, &text, required_links),
                Err(_) => vec![format!("{rel} is missing")],
            }
        })
        .collect()
}

/// Assert every AI-native workflow doc/skill exists and still carries its
/// required sections and canonical guardrail links. This keeps
/// `docs/agent-workflow.md`, its templates, and the security-finding wrappers
/// from silently drifting or disappearing.
fn check_agent_workflow() -> Result<(), String> {
    let root = workspace_root();
    let mut errors: Vec<String> = Vec::new();

    for (rel, required) in AGENT_WORKFLOW_FILES {
        let path = root.join(rel);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let missing = missing_workflow_headings(&text, required);
                if !missing.is_empty() {
                    errors.push(format!(
                        "{rel} is missing section(s): {}",
                        missing.join(", ")
                    ));
                }
            }
            Err(_) => errors.push(format!("{rel} is missing")),
        }
    }
    errors.extend(missing_workflow_guardrail_links(&root));
    if let Ok(text) = std::fs::read_to_string(root.join("AGENTS.md")) {
        let missing = missing_agents_review_guideline_markers(&text);
        if !missing.is_empty() {
            errors.push(format!(
                "AGENTS.md Review guidelines missing dependency-review marker(s): {}",
                missing.join(", ")
            ));
        }
    }

    if errors.is_empty() {
        println!(
            "check-agent-workflow: all {} AI-native workflow artifact(s) present with required headings and guardrail links.",
            AGENT_WORKFLOW_FILES.len()
        );
        Ok(())
    } else {
        Err(format!(
            "check-agent-workflow: FAIL —\n\x20 {}\n\
             \n\
             xPare's evidence-first workflow lives in repo-native docs so future\n\
             agents have a clear loop (see docs/agent-workflow.md). These files must stay\n\
             present and structured, and workflow entry points must resolve to\n\
             the repo-owned guardrails. Restore the missing file, section, or guardrail\n\
             link; do not delete the workflow docs to make this check pass. If a section\n\
             or guardrail path is intentionally renamed, update AGENT_WORKFLOW_FILES or\n\
             AGENT_WORKFLOW_GUARDRAIL_LINKS in xtask/src/main.rs in the same PR.",
            errors.join("\n  ")
        ))
    }
}

// ---------------------------------------------------------------------------
// ci
// ---------------------------------------------------------------------------

/// Assert one workspace's committed lockfile matches its manifests, via
/// `cargo metadata --locked` (which fails without touching the lockfile when a
/// re-resolve would be needed).
fn check_lockfile_sync(label: &str, dir: &Path) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    println!("ci: $ {cargo} metadata --locked  (lockfile sync: {label})");
    let output = Command::new(&cargo)
        .args(["metadata", "--locked", "--format-version", "1"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("ci: FAIL — could not launch `{cargo} metadata --locked`: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "ci: FAIL — the committed {label} lockfile ({}) is out of sync with its\n\
             manifests:\n{}\n\
             Every cargo step in this gate runs `--locked` so CI tests exactly the\n\
             committed dependency tree. Regenerate the lockfile (any cargo build in that\n\
             directory without `--locked`, e.g. `cargo metadata --format-version 1`),\n\
             review the lockfile diff for unexpected new crates, and commit it. Do not\n\
             drop `--locked` from the gate.",
            dir.join("Cargo.lock").display(),
            String::from_utf8_lossy(&output.stderr).trim_end()
        ))
    }
}

/// Lockfile honesty for BOTH workspaces: the root and the separate `fuzz/`
/// workspace (which carries its own `Cargo.lock` precisely so nightly/libFuzzer
/// pins never leak into the stable build — and which no root cargo command ever
/// validates).
fn check_lockfiles_in_sync() -> Result<(), String> {
    check_lockfile_sync("root workspace", &workspace_root())?;
    let fuzz = fuzz_dir();
    if fuzz.join("Cargo.toml").is_file() {
        check_lockfile_sync("fuzz/ workspace", &fuzz)?;
    }
    Ok(())
}

/// Run a cargo subcommand inheriting stdio; return Ok on success, else a message.
fn run_cargo(label: &str, args: &[&str]) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    println!("ci: $ {cargo} {}", args.join(" "));
    let status = Command::new(&cargo)
        .args(args)
        .current_dir(workspace_root())
        .status()
        .map_err(|e| format!("ci: FAIL — could not launch `{cargo} {label}`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "ci: FAIL — `cargo {}` exited with {status}. Fix the reported issues; do not \
             weaken the check.",
            args.join(" ")
        ))
    }
}

/// The full local gate, run in fail-fast order. Mirrors what CI runs so a green
/// `cargo xtask ci` locally means a green CI job.
fn run_ci() -> ExitCode {
    // Lockfile honesty first: the cargo steps below run `--locked`, so a stale
    // lockfile must surface here as one clear remediation message rather than as
    // a confusing resolver error halfway through the gate.
    if let Err(msg) = check_lockfiles_in_sync() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }

    // Tooling gates next (cheap to fix, catch the most common breakage). clippy
    // and test take `--locked` so the gate builds the committed dependency tree
    // verified above; `cargo fmt` accepts no `--locked` flag (it only drives
    // rustfmt and resolves nothing, so there is nothing to lock).
    let cargo_steps: [(&str, &[&str]); 3] = [
        ("fmt", &["fmt", "--all", "--check"]),
        (
            "clippy",
            &[
                "clippy",
                "--locked",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("test", &["test", "--locked", "--workspace"]),
    ];
    for (label, args) in cargo_steps {
        if let Err(msg) = run_cargo(label, args) {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    }

    // Structural invariant checks (call our own functions, not external plugins).
    if let Err(msg) = check_unsafe_forbid() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_core_deps() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_no_network() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_no_content_logging() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_pipeline_zeroization() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_agent_workflow() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_dependabot_policy() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_clipboard_safety() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_c_ffi_surface() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_swift_no_network_apis() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_shipped_command_exec() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_swift_package_deps() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_python_tooling_posture() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_real_clipboard_tests() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_pasteboard_write_shape() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_codeql_workflow_posture() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_test_hygiene() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_docs() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }

    // check-abi reuses the existing gen-header(check_only=true) path.
    if gen_header(true) != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }

    // check-entitlements: distinguish "absent" from "present but invalid" in the
    // log, but a missing file still FAILS — the macOS shell is a deliverable.
    let ent_path = entitlements_path();
    if !ent_path.exists() {
        eprintln!(
            "ci: FAIL — check-entitlements: entitlements file is ABSENT at {} (not skipped — \
             it is a required deliverable). The macOS shell must ship a minimal, checked-in \
             entitlements file (only `com.apple.security.app-sandbox` = true).",
            ent_path.display()
        );
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_entitlements() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_release_posture() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }

    // External linters last: they shell out to third-party tools (auto-installed
    // locally, pre-installed in CI) and some touch the network (advisory DB, online
    // workflow audits). Within the phase, offline+fast first (shell, then workflow
    // correctness), then the network-touching ones, so failures surface cheapest.
    if let Err(msg) = check_shell() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_workflows() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_unused_deps() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_supply_chain() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }

    println!("ci: all checks passed.");
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_ignore_line_distinguishes_bare_reason_and_non_ignores() {
        // Bare ignore (a silent skip) vs a reasoned ignore (spacing-insensitive).
        assert_eq!(classify_ignore_line("#[ignore]"), Some(false));
        assert_eq!(
            classify_ignore_line("#[ignore = \"slow: 256 MB\"]"),
            Some(true)
        );
        assert_eq!(classify_ignore_line("#[ignore=\"slow\"]"), Some(true));
        // Not the std attribute: a longer identifier, prose/doc mentions, code strings.
        assert_eq!(classify_ignore_line("#[ignored_helper]"), None);
        assert_eq!(
            classify_ignore_line("//!   ... `#[ignore]` is honored ..."),
            None
        );
        assert_eq!(classify_ignore_line("let s = \"#[ignore]\";"), None);
        assert_eq!(classify_ignore_line("// #[ignore] in a comment"), None);
    }

    #[test]
    fn shell_scripts_finds_release_plumbing_and_skips_build_dirs() {
        let scripts = shell_scripts(&workspace_root());
        let names: Vec<String> = scripts
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        // The load-bearing macOS scripts must be discovered so check-shell lints them.
        for expected in ["release.sh", "package-app.sh", "build.sh"] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
        // Never reach into build/VCS/worktree dirs *within* the workspace. The
        // workspace root itself can live under e.g. .claude/worktrees during agent
        // runs, so compare paths RELATIVE to the root, not the absolute prefix.
        let root = workspace_root();
        for p in &scripts {
            let rel = p
                .strip_prefix(&root)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/");
            assert!(
                !rel.contains("target/")
                    && !rel.contains(".build/")
                    && !rel.contains(".git/")
                    && !rel.contains(".claude/"),
                "should not scan build/worktree dirs: {rel}"
            );
        }
    }

    #[test]
    fn fuzz_target_list_parser_ignores_blank_lines() {
        assert_eq!(
            parse_fuzz_targets("\nstrip_html\nstrip_markdown\n\nmask_identifiers\n"),
            vec![
                "strip_html".to_string(),
                "strip_markdown".to_string(),
                "mask_identifiers".to_string()
            ]
        );
    }

    #[test]
    fn fuzz_smoke_seconds_parser_accepts_empty_zero_and_positive_values() {
        assert_eq!(parse_fuzz_smoke_seconds(None).unwrap(), None);
        assert_eq!(parse_fuzz_smoke_seconds(Some("")).unwrap(), None);
        assert_eq!(parse_fuzz_smoke_seconds(Some("0")).unwrap(), None);
        assert_eq!(parse_fuzz_smoke_seconds(Some("30")).unwrap(), Some(30));
    }

    #[test]
    fn fuzz_smoke_seconds_parser_rejects_invalid_values() {
        assert!(parse_fuzz_smoke_seconds(Some("soon")).is_err());
    }

    // --- check-core-deps / check-no-network dependency closure ---

    #[test]
    fn dep_closure_follows_normal_and_build_deps_and_skips_dev_deps() {
        // The posture checks must see everything that links into a shipped
        // artifact (normal deps) or executes during a build (build deps), while
        // dev-only test/bench trees stay out. A dep that is BOTH dev and build
        // is followed — the build edge alone makes it run at build time.
        let meta: Metadata = serde_json::from_str(
            r#"{
            "packages": [
                {"id": "root 1.0.0", "name": "root"},
                {"id": "normal 1.0.0", "name": "normal-dep"},
                {"id": "build 1.0.0", "name": "build-dep"},
                {"id": "dev 1.0.0", "name": "dev-dep"},
                {"id": "dual 1.0.0", "name": "dev-and-build-dep"},
                {"id": "transitive 1.0.0", "name": "build-transitive"}
            ],
            "resolve": {"nodes": [
                {"id": "root 1.0.0", "deps": [
                    {"pkg": "normal 1.0.0", "dep_kinds": [{"kind": null}]},
                    {"pkg": "build 1.0.0", "dep_kinds": [{"kind": "build"}]},
                    {"pkg": "dev 1.0.0", "dep_kinds": [{"kind": "dev"}]},
                    {"pkg": "dual 1.0.0", "dep_kinds": [{"kind": "dev"}, {"kind": "build"}]}
                ]},
                {"id": "normal 1.0.0", "deps": []},
                {"id": "build 1.0.0", "deps": [
                    {"pkg": "transitive 1.0.0", "dep_kinds": [{"kind": null}]}
                ]},
                {"id": "dev 1.0.0", "deps": []},
                {"id": "dual 1.0.0", "deps": []},
                {"id": "transitive 1.0.0", "deps": []}
            ]},
            "workspace_members": ["root 1.0.0"]
        }"#,
        )
        .expect("synthetic cargo-metadata JSON must parse");

        let closure = normal_and_build_dep_closure(&meta, &["root 1.0.0"]);
        let got: Vec<&str> = closure.into_iter().collect();
        assert_eq!(
            got,
            vec![
                "build-dep",
                "build-transitive",
                "dev-and-build-dep",
                "normal-dep",
                "root"
            ]
        );
    }

    #[test]
    fn deny_config_present_with_all_four_checks() {
        // check-supply-chain is only meaningful with the policy file checked in.
        let text = std::fs::read_to_string(workspace_root().join("deny.toml"))
            .expect("deny.toml must exist at the workspace root for check-supply-chain");
        for section in ["[advisories]", "[licenses]", "[bans]", "[sources]"] {
            assert!(text.contains(section), "deny.toml is missing {section}");
        }
    }

    /// A minimal, correct entitlements file: exactly app-sandbox = true.
    const GOOD_MINIMAL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.app-sandbox</key>
    <true/>
</dict>
</plist>
"#;

    #[test]
    fn good_minimal_entitlements_pass() {
        assert!(validate_entitlements(GOOD_MINIMAL).is_ok());
    }

    #[test]
    fn good_with_extra_whitespace_passes() {
        let text = "<plist><dict>\n  <key>com.apple.security.app-sandbox</key>\n\n   <true/>\n</dict></plist>";
        assert!(validate_entitlements(text).is_ok());
    }

    #[test]
    fn missing_sandbox_key_fails() {
        let text = "<plist><dict></dict></plist>";
        let err = validate_entitlements(text).unwrap_err();
        assert!(err.contains("app-sandbox"), "got: {err}");
    }

    #[test]
    fn sandbox_set_false_fails() {
        let text = "<plist><dict><key>com.apple.security.app-sandbox</key><false/></dict></plist>";
        let err = validate_entitlements(text).unwrap_err();
        assert!(err.contains("not set to <true/>"), "got: {err}");
    }

    #[test]
    fn network_client_entitlement_is_banned() {
        let text = r#"<plist><dict>
            <key>com.apple.security.app-sandbox</key><true/>
            <key>com.apple.security.network.client</key><true/>
        </dict></plist>"#;
        let err = validate_entitlements(text).unwrap_err();
        assert!(
            err.contains("com.apple.security.network.client"),
            "got: {err}"
        );
    }

    #[test]
    fn network_server_entitlement_is_banned() {
        let text = r#"<plist><dict>
            <key>com.apple.security.app-sandbox</key><true/>
            <key>com.apple.security.network.server</key><true/>
        </dict></plist>"#;
        assert!(validate_entitlements(text).is_err());
    }

    #[test]
    fn device_camera_entitlement_is_banned() {
        let text = r#"<plist><dict>
            <key>com.apple.security.app-sandbox</key><true/>
            <key>com.apple.security.device.camera</key><true/>
        </dict></plist>"#;
        assert!(validate_entitlements(text).is_err());
    }

    #[test]
    fn personal_information_entitlement_is_banned() {
        let text = r#"<plist><dict>
            <key>com.apple.security.app-sandbox</key><true/>
            <key>com.apple.security.personal-information.addressbook</key><true/>
        </dict></plist>"#;
        assert!(validate_entitlements(text).is_err());
    }

    #[test]
    fn apple_events_automation_is_banned() {
        let text = r#"<plist><dict>
            <key>com.apple.security.app-sandbox</key><true/>
            <key>com.apple.security.automation.apple-events</key><true/>
        </dict></plist>"#;
        assert!(validate_entitlements(text).is_err());
    }

    #[test]
    fn broad_file_access_is_banned() {
        // files.* other than implicit "none" — any explicit files entitlement is banned.
        let text = r#"<plist><dict>
            <key>com.apple.security.app-sandbox</key><true/>
            <key>com.apple.security.files.user-selected.read-write</key><true/>
        </dict></plist>"#;
        assert!(validate_entitlements(text).is_err());
    }

    #[test]
    fn unrecognized_extra_entitlement_is_rejected() {
        let text = r#"<plist><dict>
            <key>com.apple.security.app-sandbox</key><true/>
            <key>com.example.future.extra</key><true/>
        </dict></plist>"#;
        let err = validate_entitlements(text).unwrap_err();
        assert!(err.contains("extra entitlement key"), "got: {err}");
    }

    #[test]
    fn codesign_weakening_is_banned() {
        for key in [
            "com.apple.security.cs.disable-library-validation",
            "com.apple.security.cs.allow-unsigned-executable-memory",
            "com.apple.security.cs.allow-dyld-environment-variables",
        ] {
            let text = format!(
                "<plist><dict><key>com.apple.security.app-sandbox</key><true/><key>{key}</key><true/></dict></plist>"
            );
            assert!(
                validate_entitlements(&text).is_err(),
                "expected {key} to be banned"
            );
        }
    }

    #[test]
    fn accessibility_and_input_monitoring_are_banned() {
        for key in [
            "com.apple.security.accessibility",
            "com.apple.security.device.input-monitoring",
            "com.example.app.PostEvent",
        ] {
            let text = format!(
                "<plist><dict><key>com.apple.security.app-sandbox</key><true/><key>{key}</key><true/></dict></plist>"
            );
            assert!(
                validate_entitlements(&text).is_err(),
                "expected {key} to be banned"
            );
        }
    }

    #[test]
    fn key_extraction_handles_multiple_keys() {
        let text = "<key>a</key><true/><key>b</key><false/>";
        assert_eq!(
            entitlement_keys(text),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    // --- check-release-posture ---

    #[test]
    fn current_release_posture_passes() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let entitlements =
            std::fs::read_to_string(root.join("shells/macos/xPare.entitlements")).unwrap();
        validate_release_posture(&release, &entitlements).unwrap();
    }

    #[test]
    fn release_posture_rejects_missing_alternate_path_guard() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let weakened = release.replace(
            r#"[[ "${resolved}" == "${default_resolved}" ]] || die"#,
            "# alternate entitlement paths accidentally allowed",
        );
        let err = validate_release_posture(&weakened, GOOD_MINIMAL).unwrap_err();
        assert!(err.contains("alternate SIGN_ENTITLEMENTS"), "got: {err}");
    }

    #[test]
    fn release_posture_rejects_missing_signed_entitlement_verification() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let weakened = release.replace(
            r#"verify_signed_entitlements "${EXE}""#,
            "# executable signed entitlements not verified",
        );
        let err = validate_release_posture(&weakened, GOOD_MINIMAL).unwrap_err();
        assert!(
            err.contains("executable"),
            "expected executable verification failure, got: {err}"
        );
    }

    // --- release workflow credential boundary ---

    #[test]
    fn current_release_workflow_credential_boundary_passes() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        validate_release_workflow_credential_boundary(&release_workflow).unwrap();
    }

    #[test]
    fn release_workflow_rejects_actions_while_signing_material_exists() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      - name: Capture signed asset manifest\n",
            "      - name: Compromised post-signing action\n        uses: example/malicious@0000000000000000000000000000000000000000\n\n      - name: Capture signed asset manifest\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("Apple signing material is present"),
            "got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_short_form_actions_while_signing_material_exists() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      - name: Capture signed asset manifest\n",
            "      - uses: example/malicious@0000000000000000000000000000000000000000\n\n      - name: Capture signed asset manifest\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("Apple signing material is present"),
            "got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_spaced_uses_keys_while_signing_material_exists() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      - name: Capture signed asset manifest\n",
            "      - uses : example/malicious@0000000000000000000000000000000000000000\n\n      - name: Capture signed asset manifest\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("Apple signing material is present"),
            "got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_quoted_uses_keys_before_encrypted_handoff() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      - name: Verify signed assets before encrypted handoff\n",
            "      - 'uses' : example/malicious@0000000000000000000000000000000000000000\n\n      - name: Verify signed assets before encrypted handoff\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("after signed assets are captured and before encrypted handoff"),
            "got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_duplicate_cleanup_step_before_action() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      - name: Capture signed asset manifest\n",
            "      - name: Clean temporary signing material\n        run: echo fake-cleanup\n\n      - name: Compromised post-signing action\n        uses: example/malicious@0000000000000000000000000000000000000000\n\n      - name: Capture signed asset manifest\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(err.contains("duplicate required step"), "got: {err}");
    }

    #[test]
    fn release_workflow_ignores_commented_cleanup_name_before_action() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      - name: Capture signed asset manifest\n",
            "      # - name: Clean temporary signing material\n      - name: Compromised post-signing action\n        uses: example/malicious@0000000000000000000000000000000000000000\n\n      - name: Capture signed asset manifest\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("Apple signing material is present"),
            "got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_notary_credentials_outside_temp_keychain() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened =
            release_workflow.replace("            --keychain \"${KEYCHAIN_PATH}\"\n", "");
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(err.contains("temporary keychain"), "got: {err}");
    }

    #[test]
    fn release_workflow_rejects_notary_keychain_argument_left_only_in_echo() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "            --issuer \"${MACOS_NOTARY_ISSUER_ID}\" \\\n            --keychain \"${KEYCHAIN_PATH}\"\n",
            "            --issuer \"${MACOS_NOTARY_ISSUER_ID}\"\n          echo \"--keychain ${KEYCHAIN_PATH}\"\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(err.contains("temporary keychain"), "got: {err}");
    }

    #[test]
    fn release_workflow_rejects_fail_open_keychain_cleanup() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "            security delete-keychain \"${KEYCHAIN_PATH}\"\n",
            "            security delete-keychain \"${KEYCHAIN_PATH}\" >/dev/null 2>&1 || true\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(err.contains("fail closed"), "got: {err}");
    }

    #[test]
    fn release_workflow_rejects_any_keychain_cleanup_failure_handler() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "            security delete-keychain \"${KEYCHAIN_PATH}\"\n",
            "            security delete-keychain \"${KEYCHAIN_PATH}\" || echo failed\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(err.contains("fail closed"), "got: {err}");
    }

    #[test]
    fn release_workflow_rejects_post_signing_action_before_encrypted_handoff() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      - name: Verify signed assets before encrypted handoff\n",
            "      - name: Compromised post-signing action\n        uses: example/malicious@0000000000000000000000000000000000000000\n\n      - name: Verify signed assets before encrypted handoff\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("before encrypted handoff"),
            "expected pre-encrypted-handoff action rejection, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_action_in_release_create_job() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      - name: Publish GitHub Release\n",
            "      - name: Compromised publish action\n        uses: example/malicious@0000000000000000000000000000000000000000\n\n      - name: Publish GitHub Release\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("inside the release-create job"),
            "expected release-create action rejection, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_release_write_on_attestation_job() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      contents: read\n      id-token: write\n      attestations: write\n",
            "      contents: write\n      id-token: write\n      attestations: write\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("attest-official job must contain `contents: read`"),
            "expected attestation token-scope failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_artifact_metadata_write_on_attestation_job() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      attestations: write\n",
            "      attestations: write\n      artifact-metadata: write\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("artifact metadata write"),
            "expected attestation artifact metadata permission failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_attestation_from_draft_release_download() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          mkdir -p dist/release\n",
            "          mkdir -p dist/release\n          gh release download \"v${RELEASE_VERSION}\"\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("must not download draft release assets"),
            "expected draft-download rejection, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_attestation_without_checksum_subjects() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          subject-checksums: dist/release/signed-release-assets.sha256\n",
            "          subject-path: dist/release/*.zip\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("subject-checksums"),
            "expected checksum-subject attestation failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_release_write_on_sbom_generation_job() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "  sbom-official:\n    name: generate release SBOM\n",
            "  sbom-official:\n    name: generate release SBOM\n    permissions:\n      contents: write\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("sbom-official job must not grant release-asset write permission"),
            "expected SBOM token-scope failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_missing_repo_context_for_release_downloads() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened =
            release_workflow.replacen("          GH_REPO: ${{ github.repository }}\n", "", 1);
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("GH_REPO: ${{ github.repository }}"),
            "expected release repository context failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_raw_signed_asset_artifact_upload() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          path: ${{ env.SIGNED_ASSET_HANDOFF_DIR }}/signed-release-assets.tar.enc\n",
            "          path: |\n            dist/release/*.zip\n            dist/release/*.zip.sha256\n            dist/release/SHA256SUMS\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("raw signed asset path") || err.contains("signed-release-assets.tar.enc"),
            "expected raw signed asset artifact rejection, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_missing_raw_signed_asset_cleanup_before_upload_action() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          rm -f -- dist/release/*.zip dist/release/*.zip.sha256 dist/release/SHA256SUMS*\n",
            "",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("raw signed assets") || err.contains("raw-asset cleanup"),
            "expected missing raw signed asset cleanup failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_incomplete_raw_signed_asset_cleanup_before_upload_action() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          rm -f -- dist/release/*.zip dist/release/*.zip.sha256 dist/release/SHA256SUMS*\n",
            "          rm -f -- dist/release/*.zip\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("dist/release/*.zip.sha256") || err.contains("dist/release/SHA256SUMS*"),
            "expected incomplete raw signed asset cleanup failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_echoed_raw_signed_asset_cleanup_before_upload_action() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          rm -f -- dist/release/*.zip dist/release/*.zip.sha256 dist/release/SHA256SUMS*\n",
            "          echo \"rm -f -- dist/release/*.zip dist/release/*.zip.sha256 dist/release/SHA256SUMS*\"\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("raw signed assets") || err.contains("raw-asset cleanup"),
            "expected echoed raw signed asset cleanup failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_missing_raw_signed_asset_absence_check_before_upload_action() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          raw_signed_asset_remaining=\"$(find dist/release -maxdepth 1 -type f \\( -name '*.zip' -o -name '*.zip.sha256' -o -name 'SHA256SUMS*' \\) -print -quit)\"\n          if [ -n \"${raw_signed_asset_remaining}\" ]; then\n            echo \"raw signed release asset remains after handoff encryption: ${raw_signed_asset_remaining}\"\n            exit 1\n          fi\n",
            "",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("raw_signed_asset_remaining")
                || err.contains("raw signed assets are absent"),
            "expected missing raw signed asset absence check failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_current_run_name_download_in_publish_job() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            r#"          gh api -H "Accept: application/vnd.github+json" "/repos/${GH_REPO}/actions/artifacts/${SIGNED_ASSET_HANDOFF_ARTIFACT_ID}/zip" > "${artifact_zip}""#,
            r#"          gh run download "${GITHUB_RUN_ID}" --name "encrypted-signed-release-handoff-${RELEASE_VERSION}" --dir "${handoff_dir}""#,
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("artifact-id") || err.contains("gh run download"),
            "expected current-run name-download rejection, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_publish_without_attestation_need() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      - sign-official\n      - attest-official\n      - sbom-official\n",
            "      - sign-official\n      - sbom-official\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("attest-official"),
            "expected publish attestation dependency failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_release_upload_mutation_primitive() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          gh release create \"v${RELEASE_VERSION}\" \\\n",
            "          gh release upload \"v${RELEASE_VERSION}\" \"dist/release/xPare-v${RELEASE_VERSION}-sbom.spdx.json\" --clobber\n          gh release create \"v${RELEASE_VERSION}\" \\\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("gh release upload") || err.contains("--clobber"),
            "expected release mutation primitive rejection, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_release_delete_mutation_primitive() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          gh release create \"v${RELEASE_VERSION}\" \\\n",
            "          gh release delete \"v${RELEASE_VERSION}\" --yes\n          gh release create \"v${RELEASE_VERSION}\" \\\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("gh release delete"),
            "expected release delete primitive rejection, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_missing_partial_draft_cleanup() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let cleanup_start = release_workflow
            .find("          release_create_started=false\n")
            .unwrap();
        let guard_start = release_workflow
            .find("          if gh release view \"v${RELEASE_VERSION}\" >/dev/null 2>&1; then\n")
            .unwrap();
        let weakened = format!(
            "{}{}",
            &release_workflow[..cleanup_start],
            &release_workflow[guard_start..]
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("partial draft") || err.contains("cleanup_partial_draft_release"),
            "expected missing partial draft cleanup failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_missing_post_create_asset_verification() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let verify_start = release_workflow
            .find("          expected_assets=\"${RUNNER_TEMP}/release-assets.expected\"\n")
            .unwrap();
        let verification_end_marker =
            "          release_create_started=false\n          trap - ERR\n";
        let cleanup_end = verify_start
            + release_workflow[verify_start..]
                .find(verification_end_marker)
                .unwrap()
            + verification_end_marker.len();
        let weakened = format!(
            "{}          release_create_started=false\n          trap - ERR\n{}",
            &release_workflow[..verify_start],
            &release_workflow[cleanup_end..]
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("assets") || err.contains("diff -u"),
            "expected missing post-create asset verification failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_unscoped_release_delete_before_publish_step() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      - name: Publish GitHub Release\n",
            "      - name: Delete unrelated release\n        run: gh release delete \"v${RELEASE_VERSION}\" --yes\n\n      - name: Publish GitHub Release\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("outside scoped same-run draft cleanup"),
            "expected unscoped release delete failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_partial_draft_cleanup_that_deletes_tag() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            r#"gh release delete "v${RELEASE_VERSION}" --yes"#,
            r#"gh release delete "v${RELEASE_VERSION}" --yes --cleanup-tag"#,
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("scoped partial-draft"),
            "expected tag-deleting draft cleanup failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_existing_release_guard_left_only_in_echo() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          if gh release view \"v${RELEASE_VERSION}\" >/dev/null 2>&1; then\n",
            "          echo \"if gh release view v${RELEASE_VERSION} then\"\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("Publish GitHub Release step"),
            "expected executable release-view guard failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_unprotected_per_zip_checksum_assets() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "shasum -a 256 -- *.zip *.zip.sha256 SHA256SUMS*",
            "shasum -a 256 -- *.zip SHA256SUMS*",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(err.contains("*.zip.sha256"), "got: {err}");
    }

    #[test]
    fn release_workflow_rejects_unbound_signed_asset_manifest_baseline() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          echo \"sha256=${manifest_sha}\" >> \"${GITHUB_OUTPUT}\"\n",
            "          # signed asset manifest digest no longer exported\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("GITHUB_OUTPUT"),
            "expected manifest output binding failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_missing_signed_asset_manifest_binding_check() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          expected_manifest_sha=\"${SIGNED_MANIFEST_SHA256}\"\n",
            "          # signed asset manifest digest no longer checked\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("SIGNED_MANIFEST_SHA256"),
            "expected manifest binding verification failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_missing_signed_subject_checksum_handoff() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "          echo \"checksums_b64=${checksums_b64}\" >> \"${GITHUB_OUTPUT}\"\n",
            "          # signed subject checksums no longer exported\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(err.contains("checksums_b64"), "got: {err}");
    }

    #[test]
    fn release_workflow_rejects_missing_pre_handoff_manifest_check() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "      - name: Verify signed assets before encrypted handoff\n",
            "      - name: Publish without manifest verification\n",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("Verify signed assets before encrypted handoff"),
            "got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_manifest_check_before_sbom_download() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let verify_step_start = release_workflow
            .find("      - name: Verify signed assets before release creation\n")
            .unwrap();
        let publish_step_start = release_workflow
            .find("      - name: Publish GitHub Release\n")
            .unwrap();
        let verify_step = &release_workflow[verify_step_start..publish_step_start];
        let shortened = release_workflow.replace(verify_step, "").replace(
            "      - name: Download release SBOM\n",
            &format!("{verify_step}      - name: Download release SBOM\n"),
        );
        let err = validate_release_workflow_credential_boundary(&shortened).unwrap_err();
        assert!(
            err.contains("Download release SBOM"),
            "expected SBOM-before-final-verification ordering failure, got: {err}"
        );
    }

    #[test]
    fn release_workflow_rejects_publish_without_sbom_asset() {
        let root = workspace_root();
        let release_workflow =
            std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let weakened = release_workflow.replace(
            "            \"dist/release/xPare-v${RELEASE_VERSION}-sbom.spdx.json\" \\\n",
            "",
        );
        let err = validate_release_workflow_credential_boundary(&weakened).unwrap_err();
        assert!(
            err.contains("xPare-v${RELEASE_VERSION}-sbom.spdx.json"),
            "expected publish SBOM asset failure, got: {err}"
        );
    }

    #[test]
    fn release_posture_rejects_notary_submit_without_explicit_keychain_support() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let weakened = release.replace(
            r#"notary_keychain_args=(--keychain "${NOTARY_KEYCHAIN}")"#,
            r#"notary_keychain_args=()"#,
        );
        let err = validate_release_posture(&weakened, GOOD_MINIMAL).unwrap_err();
        assert!(err.contains("explicit keychain"), "got: {err}");
    }

    #[test]
    fn release_posture_rejects_existing_release_upload_path() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let weakened = release.replace(
            r#"            die "release v${version} already exists; refusing to replace release assets. Delete the draft release before rerunning, or create a new tag for a corrected public release.""#,
            r#"            gh release upload "v${version}" "${zip}" "${zip}.sha256" "${sums}" --clobber"#,
        );
        let err = validate_release_posture(&weakened, GOOD_MINIMAL).unwrap_err();
        assert!(
            err.contains("gh release upload") || err.contains("--clobber"),
            "expected existing-release upload failure, got: {err}"
        );
    }

    #[test]
    fn release_posture_rejects_release_upload_helper_outside_github_release_case() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let weakened = release.replace(
            "verify_zip_stapled() {\n",
            "replace_release_assets() {\n    gh release upload \"v${version}\" \"${zip}\" \"${zip}.sha256\" \"${sums}\" --clobber\n}\n\nverify_zip_stapled() {\n",
        );
        let err = validate_release_posture(&weakened, GOOD_MINIMAL).unwrap_err();
        assert!(
            err.contains("gh release upload") || err.contains("--clobber"),
            "expected global release upload primitive ban, got: {err}"
        );
    }

    #[test]
    fn release_posture_allows_clobber_only_in_comments() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let commented = release.replace(
            "        # set and the attested subject set stay in sync).\n",
            "        # set and the attested subject set stay in sync).\n        # Historical note: never use gh release upload --clobber here.\n",
        );
        validate_release_posture(&commented, GOOD_MINIMAL).unwrap();
    }

    #[test]
    fn release_posture_rejects_commented_existing_release_guard() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let weakened = release.replace(
            r#"        if gh release view "v${version}" >/dev/null 2>&1; then"#,
            r#"        # if gh release view "v${version}" >/dev/null 2>&1; then"#,
        );
        let err = validate_release_posture(&weakened, GOOD_MINIMAL).unwrap_err();
        assert!(
            err.contains("existing release"),
            "expected missing executable release-view guard failure, got: {err}"
        );
    }

    #[test]
    fn release_posture_rejects_existing_release_guard_left_only_in_echo() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let weakened = release.replace(
            r#"        if gh release view "v${version}" >/dev/null 2>&1; then"#,
            r#"        echo "if gh release view v${version} then""#,
        );
        let err = validate_release_posture(&weakened, GOOD_MINIMAL).unwrap_err();
        assert!(
            err.contains("not echo or comment"),
            "expected echoed release-view guard failure, got: {err}"
        );
    }

    #[test]
    fn release_posture_rejects_existing_release_refusal_left_only_in_echo() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let weakened = release.replace(
            r#"            die "release v${version} already exists; refusing to replace release assets. Delete the draft release before rerunning, or create a new tag for a corrected public release.""#,
            r#"            echo "release v${version} already exists; refusing to replace release assets""#,
        );
        let err = validate_release_posture(&weakened, GOOD_MINIMAL).unwrap_err();
        assert!(
            err.contains("github-release existing-release guard"),
            "expected executable release refusal failure, got: {err}"
        );
    }

    #[test]
    fn release_posture_rejects_existing_release_guard_after_create() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let weakened = release
            .replacen(
                r#"        if gh release view "v${version}" >/dev/null 2>&1; then"#,
                "        if false; then",
                1,
            )
            .replace(
                r#"            gh release create "v${version}" "${zip}" "${zip}.sha256" "${sums}" "${sbom}" \"#,
                r#"            gh release create "v${version}" "${zip}" "${zip}.sha256" "${sums}" "${sbom}" \
            gh release view "v${version}" >/dev/null 2>&1"#,
            );
        let err = validate_release_posture(&weakened, GOOD_MINIMAL).unwrap_err();
        assert!(
            err.contains("before release creation"),
            "expected release-view order failure, got: {err}"
        );
    }

    #[test]
    fn release_posture_rejects_missing_sbom_in_create() {
        let root = workspace_root();
        let release = std::fs::read_to_string(root.join("shells/macos/release.sh")).unwrap();
        let weakened = release.replace(
            r#"            gh release create "v${version}" "${zip}" "${zip}.sha256" "${sums}" "${sbom}" \"#,
            r#"            gh release create "v${version}" "${zip}" "${zip}.sha256" "${sums}" \"#,
        );
        let err = validate_release_posture(&weakened, GOOD_MINIMAL).unwrap_err();
        assert!(err.contains("SBOM"), "got: {err}");
    }

    // --- check-c-ffi-surface ---

    #[test]
    fn current_c_ffi_surface_passes() {
        check_c_ffi_surface().unwrap();
    }

    #[test]
    fn c_comment_stripping_leaves_dummy_source_empty() {
        let text = "/* comment */\n/* multi\nline */\n";
        assert!(noncomment_lines(text).is_empty());
    }

    #[test]
    fn c_comment_stripping_exposes_handwritten_logic() {
        let text = "/* comment */\nint accidental_symbol(void) { return 1; }\n";
        assert_eq!(
            noncomment_lines(text),
            vec!["int accidental_symbol(void) { return 1; }".to_string()]
        );
    }

    // --- check-pipeline-zeroization ---

    #[test]
    fn current_pipeline_zeroization_passes() {
        let text = std::fs::read_to_string(pipeline_path()).unwrap();
        validate_pipeline_zeroization(&text).unwrap();
    }

    #[test]
    fn pipeline_zeroization_rejects_plain_vec_scratch() {
        let text = std::fs::read_to_string(pipeline_path()).unwrap();
        let weakened = text.replace(
            "let mut collapsed = Zeroizing::new(Vec::new());",
            "let mut collapsed = Vec::new();",
        );
        let err = validate_pipeline_zeroization(&weakened).unwrap_err();
        assert!(err.contains("plain `Vec::new()`"), "got: {err}");
    }

    #[test]
    fn pipeline_zeroization_rejects_clear_without_wipe() {
        let text = std::fs::read_to_string(pipeline_path()).unwrap();
        let weakened = text.replace("scratch.zeroize();", "scratch.clear();");
        let err = validate_pipeline_zeroization(&weakened).unwrap_err();
        assert!(err.contains("scratch.zeroize()"), "got: {err}");
    }

    #[test]
    fn pipeline_zeroization_rejects_missing_growth_guard() {
        let text = std::fs::read_to_string(pipeline_path()).unwrap();
        let weakened = text.replace(
            "if needed > scratch.capacity() {",
            "if false { // missing capacity-growth guard",
        );
        let err = validate_pipeline_zeroization(&weakened).unwrap_err();
        assert!(err.contains("check capacity"), "got: {err}");
    }

    #[test]
    fn current_op_accumulator_wipe_markers_all_present() {
        let root = workspace_root();
        for (rel, marker, reason) in OP_ACCUMULATOR_WIPE_MARKERS {
            let text = std::fs::read_to_string(root.join(rel)).unwrap();
            assert!(
                text.contains(marker),
                "{rel} lost load-bearing marker {marker:?} ({reason})"
            );
        }
    }

    #[test]
    fn op_accumulator_markers_reject_unrouted_appends() {
        // Doctor the case-mapping source to bypass the wipe helper: the tripwire
        // must notice the import (the routing's load-bearing construct) is gone.
        let root = workspace_root();
        let text = std::fs::read_to_string(root.join("core/src/ops/case.rs")).unwrap();
        let weakened = text.replace("use crate::ops::wipe::push_char_wiping;", "");
        let missing = missing_accumulator_wipe_markers("core/src/ops/case.rs", &weakened);
        assert_eq!(missing.len(), 1, "got: {missing:?}");
        assert!(missing[0].contains("ops::wipe"), "got: {missing:?}");
    }

    #[test]
    fn op_accumulator_markers_reject_unwiped_growth() {
        // Doctor the wipe helper to free the retired block without zeroizing it.
        let root = workspace_root();
        let text = std::fs::read_to_string(root.join("core/src/ops/wipe.rs")).unwrap();
        let weakened = text.replace("drop(Zeroizing::new(retired));", "drop(retired);");
        let missing = missing_accumulator_wipe_markers("core/src/ops/wipe.rs", &weakened);
        assert_eq!(missing.len(), 1, "got: {missing:?}");
        assert!(
            missing[0].contains("zeroize the retired"),
            "got: {missing:?}"
        );
    }

    // --- check-agent-workflow ---

    #[test]
    fn current_agent_workflow_passes() {
        check_agent_workflow().unwrap();
    }

    #[test]
    fn agent_workflow_detects_missing_heading() {
        let text = "# Agent task\n\n## Files to read\n\n## Hard constraints\n";
        let missing = missing_workflow_headings(text, AGENT_TASK_HEADINGS);
        assert_eq!(
            missing,
            vec![
                "## Required evidence".to_string(),
                "## Proof gaps to report".to_string()
            ]
        );
    }

    #[test]
    fn agent_workflow_heading_match_is_whole_line_not_substring() {
        // A heading demoted to prose (no leading `##`) must not satisfy the check.
        let text = "Files to read are listed below.\n";
        assert_eq!(
            missing_workflow_headings(text, &["## Files to read"]),
            vec!["## Files to read".to_string()]
        );
        // Exact heading line (with surrounding whitespace) is accepted.
        assert!(
            missing_workflow_headings("  ## Files to read  \n", &["## Files to read"]).is_empty()
        );
    }

    #[test]
    fn agent_workflow_detects_missing_dependency_review_guidance() {
        let text = "## Review guidelines\n\nReview dependency PRs carefully.\n";
        let missing = missing_agents_review_guideline_markers(text);

        assert!(
            missing.contains(
                &"docs/guardrails/dependency-posture.md#dependabot-merge-recommendations"
                    .to_string()
            ),
            "got: {missing:?}"
        );
        assert!(
            missing.contains(&"`close/defer`".to_string()),
            "got: {missing:?}"
        );
        assert!(missing.contains(&"P1".to_string()), "got: {missing:?}");
        assert!(
            missing.contains(&"Failed checks".to_string()),
            "got: {missing:?}"
        );
    }

    #[test]
    fn agent_workflow_detects_missing_skill_guardrail_link() {
        let root = workspace_root();
        let text = "# Security Finding Triage\n\nRead `docs/guardrails/agentic-security-finding-triage.md`.\n";
        let missing = missing_guardrail_links_for_text(
            &root,
            ".agents/skills/security-finding-triage/SKILL.md",
            text,
            &["../../../docs/guardrails/agentic-security-finding-triage.md"],
        );
        assert_eq!(missing.len(), 1, "got: {missing:?}");
        assert!(
            missing[0].contains("must mention required guardrail link"),
            "got: {missing:?}"
        );
    }

    #[test]
    fn agent_workflow_detects_unresolved_skill_guardrail_link() {
        let root = workspace_root();
        let text =
            "# Security Finding Triage\n\nRead `../../../docs/guardrails/not-a-guardrail.md`.\n";
        let missing = missing_guardrail_links_for_text(
            &root,
            ".agents/skills/security-finding-triage/SKILL.md",
            text,
            &["../../../docs/guardrails/not-a-guardrail.md"],
        );
        assert_eq!(missing.len(), 1, "got: {missing:?}");
        assert!(missing[0].contains("does not resolve"), "got: {missing:?}");
    }

    #[test]
    fn agent_workflow_detects_contributor_closure_without_triage() {
        let root = workspace_root();
        let text = "## Closing review findings\n\nFollow [`docs/guardrails/review-finding-closure.md`](docs/guardrails/review-finding-closure.md).\n";
        let missing = missing_guardrail_links_for_text(
            &root,
            "CONTRIBUTING.md",
            text,
            CONTRIBUTOR_REVIEW_FINDING_GUARDRAIL_LINKS,
        );
        assert_eq!(missing.len(), 1, "got: {missing:?}");
        assert!(
            missing[0].contains("agentic-security-finding-triage"),
            "got: {missing:?}"
        );
    }

    // --- check-dependabot-policy ---

    #[test]
    fn current_dependabot_policy_passes() {
        check_dependabot_policy().unwrap();
    }

    #[test]
    fn dependabot_policy_rejects_grouped_actions() {
        let text = r#"
version: 2
updates:
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
    cooldown:
      default-days: 7
    groups:
      actions:
        patterns:
          - "*"
  - package-ecosystem: cargo
    directory: /
    schedule:
      interval: weekly
    open-pull-requests-limit: 0
"#;
        let err = validate_dependabot_policy(text).unwrap_err();
        assert!(
            err.contains("github-actions updates must stay ungrouped"),
            "got: {err}"
        );
    }

    #[test]
    fn dependabot_policy_rejects_actions_default_days_without_cooldown() {
        let text = r#"
version: 2
updates:
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
    default-days: 7
  - package-ecosystem: cargo
    directory: /
    schedule:
      interval: weekly
    open-pull-requests-limit: 0
"#;
        let err = validate_dependabot_policy(text).unwrap_err();
        assert!(
            err.contains("github-actions updates must keep a 7-day cooldown"),
            "got: {err}"
        );
    }

    #[test]
    fn dependabot_policy_rejects_cargo_cooldown() {
        let text = r#"
version: 2
updates:
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
    cooldown:
      default-days: 7
  - package-ecosystem: cargo
    directory: /
    schedule:
      interval: weekly
    open-pull-requests-limit: 0
    cooldown:
      default-days: 7
"#;
        let err = validate_dependabot_policy(text).unwrap_err();
        assert!(
            err.contains("cargo security-update PRs must not be delayed"),
            "got: {err}"
        );
    }

    #[test]
    fn dependabot_policy_rejects_cargo_version_prs() {
        let text = r#"
version: 2
updates:
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
    cooldown:
      default-days: 7
  - package-ecosystem: cargo
    directory: /
    schedule:
      interval: weekly
"#;
        let err = validate_dependabot_policy(text).unwrap_err();
        assert!(err.contains("open-pull-requests-limit: 0"), "got: {err}");
    }

    #[test]
    fn dependabot_policy_rejects_multi_ecosystem_grouping() {
        let text = r#"
version: 2
multi-ecosystem-groups:
  infrastructure:
    schedule:
      interval: weekly
updates:
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
    cooldown:
      default-days: 7
  - package-ecosystem: cargo
    directory: /
    schedule:
      interval: weekly
    open-pull-requests-limit: 0
"#;
        let err = validate_dependabot_policy(text).unwrap_err();
        assert!(
            err.contains("multi-ecosystem Dependabot groups"),
            "got: {err}"
        );
    }

    // --- check-no-content-logging ---

    #[test]
    fn content_logging_flags_logging_of_clipboard_content() {
        assert!(flags_content_logging(
            r#"os_log("stripped clipboard: %@", payload)"#
        ));
        assert!(flags_content_logging(
            r#"println!("pasteboard = {}", transformed)"#
        ));
    }

    #[test]
    fn content_logging_flags_persisting_clipboard_content() {
        assert!(flags_content_logging(
            "UserDefaults.standard.set(clipboardText, forKey: key)"
        ));
    }

    #[test]
    fn content_logging_allows_legitimate_lines() {
        // The CLI's intentional write of transformed output to stdout is not logging
        // (no log/persist token matches `write_all`, and `output` is not a trigger word).
        assert!(!flags_content_logging(
            "stdout().write_all(output.as_bytes())?;"
        ));
        // Logging a fixed operational state with no content word is fine.
        assert!(!flags_content_logging(
            r#"eprintln!("error: {}", err.code)"#
        ));
        // Persisting user *settings* (no content word) is fine.
        assert!(!flags_content_logging(
            r#"UserDefaults.standard.set(operations, forKey: "ops")"#
        ));
        // A content word with no sink call is fine.
        assert!(!flags_content_logging(
            "let transformed = strip(&clipboard);"
        ));
    }

    #[test]
    fn content_persistence_marker_exempts_only_the_allowlisted_file() {
        let sink_line = "try Data(text.utf8).write(to: url) \
                         // xpare:allow-content-persistence: transformed clipboard result";
        // In the sanctioned store file, the marker exempts the line.
        assert_eq!(content_line_violation(sink_line, true), None);
        // Anywhere else, carrying the marker is itself a violation — the exemption
        // cannot be copied around to silence findings.
        assert!(content_line_violation(sink_line, false).is_some());
        // Even a harmless line is flagged if it smuggles the marker into a
        // non-allowlisted file.
        assert!(content_line_violation("// xpare:allow-content-persistence", false).is_some());
        // Without the marker, the ordinary scan applies regardless of file.
        assert!(content_line_violation(
            "UserDefaults.standard.set(clipboardText, forKey: key)",
            true
        )
        .is_some());
        assert_eq!(
            content_line_violation("let transformed = strip(&clipboard);", false),
            None
        );
    }

    #[test]
    fn content_persistence_allowlist_names_only_the_paste_file_store() {
        // The exemption stays exactly this narrow; widening it is a posture change
        // that must be made deliberately (and update SECURITY.md + the guardrail).
        assert_eq!(
            CONTENT_PERSISTENCE_ALLOWED_FILES,
            &["shells/macos/Sources/XPareKit/PasteFileStore.swift"]
        );
        // ...and the file it names must actually exist (rename protection).
        assert!(
            workspace_root()
                .join(CONTENT_PERSISTENCE_ALLOWED_FILES[0])
                .is_file(),
            "allowlisted paste-as-file store is missing — update the allowlist with the move"
        );
    }

    // --- source posture checks ---

    #[test]
    fn swift_package_dependency_guard_rejects_external_package() {
        let text = r#"
let package = Package(
    name: "x",
    dependencies: [
        .package(url: "https://example.com/pkg.git", from: "1.0.0"),
    ],
    targets: [
        .target(name: "App", dependencies: [.product(name: "Pkg", package: "pkg")])
    ])
"#;
        let err = validate_swift_package_deps(text).unwrap_err();
        assert!(err.contains(".package("), "got: {err}");
        assert!(err.contains(".product("), "got: {err}");
    }

    #[test]
    fn swift_package_dependency_guard_allows_local_target_dependencies() {
        let text = r#"
let package = Package(
    name: "x",
    targets: [
        .target(name: "XPareCore", dependencies: ["CXPare"]),
        .target(name: "XPareKit", dependencies: ["XPareCore"]),
        .testTarget(name: "XPareKitTests", dependencies: ["XPareKit"]),
    ])
"#;
        validate_swift_package_deps(text).unwrap();
    }

    #[test]
    fn python_helper_posture_accepts_icon_generator_imports() {
        let text = r#"
from __future__ import annotations

import argparse
import math
import struct
import zlib
from pathlib import Path
"#;
        validate_python_helper_posture(text).unwrap();
    }

    #[test]
    fn python_helper_posture_rejects_network_and_process_imports() {
        let text = r#"
import subprocess
from urllib.request import urlopen

subprocess.run(["say", "oops"])
"#;
        let err = validate_python_helper_posture(text).unwrap_err();
        assert!(err.contains("subprocess"), "got: {err}");
        assert!(err.contains("urllib"), "got: {err}");
    }

    #[test]
    fn pasteboard_write_shape_accepts_plain_string_rewrite_only() {
        let text = r#"
public final class SystemPasteboard {
    public func writePlain(_ text: String) -> Int? {
        pasteboard.clearContents()
        guard pasteboard.setString(text, forType: .string) else { return nil }
        return pasteboard.changeCount
    }

    public func writeFileURL(_ url: URL) -> Int? {
        pasteboard.clearContents()
        guard pasteboard.writeObjects([url as NSURL]) else { return nil }
        return pasteboard.changeCount
    }
}
"#;
        validate_pasteboard_write_shape(&[(
            "shells/macos/Sources/XPareKit/Pasteboard.swift".to_string(),
            text.to_string(),
        )])
        .unwrap();
    }

    #[test]
    fn pasteboard_write_shape_rejects_extra_string_write() {
        let text = r#"
public final class SystemPasteboard {
    public func writePlain(_ text: String) -> Int? {
        pasteboard.clearContents()
        _ = pasteboard.setString(text, forType: .string)
        guard pasteboard.setString("extra", forType: .string) else { return nil }
        return pasteboard.changeCount
    }
}
"#;
        let err = validate_pasteboard_write_shape(&[(
            "shells/macos/Sources/XPareKit/Pasteboard.swift".to_string(),
            text.to_string(),
        )])
        .unwrap_err();
        assert!(err.contains("exactly one plain string"), "got: {err}");
    }

    #[test]
    fn codeql_workflow_posture_rejects_unpinned_actions() {
        let text = r#"
# Additive signal only.
# Keep this workflow out of branch protection.
permissions:
  contents: read
jobs:
  rust:
    permissions:
      security-events: write
    steps:
      - uses: actions/checkout@v6
      - uses: github/codeql-action/init@v4
        with:
          languages: rust
          build-mode: none
          queries: security-extended
          dependency-caching: true
      - uses: github/codeql-action/analyze@v4
"#;
        let err = validate_codeql_workflow_posture(text).unwrap_err();
        assert!(err.contains("pinned"), "got: {err}");
    }

    #[test]
    fn codeql_workflow_posture_accepts_pinned_security_extended() {
        let pin = CODEQL_ACTION_PIN;
        let version = CODEQL_ACTION_VERSION_COMMENT;
        let text = format!(
            r#"# CodeQL security analysis.
# Additive signal only: the required local/CI gate remains `cargo xtask ci`.
# Keep this workflow out of branch protection until the first alert baseline has
# been triaged and false positives are understood.
name: CodeQL
permissions:
  contents: read
jobs:
  rust:
    permissions:
      contents: read
      security-events: write
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: github/codeql-action/init@{pin} # {version}
        with:
          languages: rust
          build-mode: none
          queries: security-extended,./.github/codeql/queries/rust
          dependency-caching: true
      - uses: github/codeql-action/analyze@{pin} # {version}
        with:
          category: "/language:rust"
  python:
    permissions:
      contents: read
      security-events: write
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: github/codeql-action/init@{pin} # {version}
        with:
          languages: python
          queries: security-extended,./.github/codeql/queries/python
      - uses: github/codeql-action/analyze@{pin} # {version}
        with:
          category: "/language:python"
  actions:
    permissions:
      contents: read
      security-events: write
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: github/codeql-action/init@{pin} # {version}
        with:
          languages: actions
          queries: security-extended
      - uses: github/codeql-action/analyze@{pin} # {version}
        with:
          category: "/language:actions"
"#
        );
        validate_codeql_workflow_posture(&text).unwrap();
    }

    #[test]
    fn codeql_workflow_posture_rejects_annotated_tag_object_pin() {
        let pin = CODEQL_ACTION_ANNOTATED_TAG_OBJECT_PINS[0];
        let version = CODEQL_ACTION_VERSION_COMMENT;
        let text = format!(
            r#"# CodeQL security analysis.
# Additive signal only: the required local/CI gate remains `cargo xtask ci`.
# Keep this workflow out of branch protection until the first alert baseline has
# been triaged and false positives are understood.
name: CodeQL
permissions:
  contents: read
jobs:
  rust:
    permissions:
      contents: read
      security-events: write
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: github/codeql-action/init@{pin} # {version}
        with:
          languages: rust
          build-mode: none
          queries: security-extended
          dependency-caching: true
      - uses: github/codeql-action/analyze@{pin} # {version}
        with:
          category: "/language:rust"
  python-actions:
    permissions:
      contents: read
      security-events: write
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: github/codeql-action/init@{pin} # {version}
        with:
          languages: python, actions
          queries: security-extended
      - uses: github/codeql-action/analyze@{pin} # {version}
        with:
          category: "/language:python-actions"
"#
        );
        let err = validate_codeql_workflow_posture(&text).unwrap_err();
        assert!(err.contains("annotated tag object SHA"), "got: {err}");
    }

    // --- check-clipboard-safety ---

    #[test]
    fn make_rule_parsing() {
        assert_eq!(
            parse_make_rule("check: guardrails smoke-general"),
            Some(("check", "guardrails smoke-general"))
        );
        assert_eq!(parse_make_rule("\t@cargo test"), None); // recipe
        assert_eq!(parse_make_rule("VERSION ?="), None); // assignment
        assert_eq!(parse_make_rule(".DEFAULT_GOAL := help"), None); // assignment
        assert_eq!(
            parse_make_rule("preview: ## help text"),
            Some(("preview", ""))
        );
    }

    // ---- check-swift -------------------------------------------------------

    #[test]
    fn llvm_cov_lines_percent_is_read_from_export_json() {
        // Shape of `llvm-cov export -summary-only`: data[0].totals.lines.percent.
        let json = r#"{"data":[{"totals":{"lines":{"count":780,"covered":608,"percent":77.95}}}],"type":"llvm.coverage.json.export","version":"2.0.1"}"#;
        let pct = parse_llvm_cov_lines_percent(json).unwrap();
        assert!((pct - 77.95).abs() < 1e-9);
    }

    #[test]
    fn llvm_cov_export_with_no_data_is_an_error() {
        assert!(parse_llvm_cov_lines_percent(r#"{"data":[]}"#).is_err());
        assert!(parse_llvm_cov_lines_percent("not json").is_err());
    }

    #[test]
    fn swift_coverage_verdict_ratchets_on_the_floor() {
        // Above and exactly-on the floor pass; below fails.
        assert!(swift_coverage_verdict(80.0, 75.0).is_ok());
        assert!(swift_coverage_verdict(75.0, 75.0).is_ok());
        assert!(swift_coverage_verdict(74.9, 75.0).is_err());
    }

    #[test]
    fn swift_coverage_floor_is_at_or_below_the_measured_baseline() {
        // The floor must sit under the ~95.8% Sources baseline measured at introduction,
        // or every run trips it. Guards against an accidental bump above reality.
        const { assert!(SWIFT_COVERAGE_FLOOR_PCT <= 95.8) }
    }

    // --- check-mutants verdict policy ---

    #[test]
    fn mutants_clean_run_passes() {
        assert!(classify_mutants_outcome(Some(0), 89, "", 89, 0, 0).is_ok());
    }

    #[test]
    fn mutants_timeouts_pass_as_detected() {
        // cargo-mutants exits nonzero when timeouts exist; a hang is detection,
        // not an assertion gap, so the check must still pass.
        assert!(classify_mutants_outcome(Some(3), 89, "", 85, 2, 2).is_ok());
    }

    #[test]
    fn mutants_missed_fails_and_names_them() {
        let err = classify_mutants_outcome(
            Some(2),
            89,
            "core/src/x.rs:1:1: replace a with b\n",
            88,
            0,
            0,
        )
        .unwrap_err();
        assert!(err.contains("1 MISSED"), "{err}");
        assert!(err.contains("core/src/x.rs:1:1"), "{err}");
    }

    #[test]
    fn mutants_signal_kill_fails_despite_empty_missed_list() {
        // The original incident: the runner was reclaimed mid-sweep, missed.txt
        // was empty only because the run never finished.
        assert!(classify_mutants_outcome(None, 913, "", 21, 0, 4).is_err());
    }

    #[test]
    fn mutants_internal_error_fails() {
        assert!(classify_mutants_outcome(Some(1), 0, "", 0, 0, 0).is_err());
    }

    #[test]
    fn mutants_empty_scope_passes() {
        // Docs-only / workflow-only diffs generate zero mutants; that is a pass,
        // not a baseline failure.
        assert!(classify_mutants_outcome(Some(0), 0, "", 0, 0, 0).is_ok());
    }

    #[test]
    fn mutants_generated_but_unclassified_fails() {
        // Baseline test failure: mutants were enumerated but the sweep never ran.
        assert!(classify_mutants_outcome(Some(4), 89, "", 0, 0, 0).is_err());
    }
}
