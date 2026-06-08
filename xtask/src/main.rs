//! Mechanical invariant enforcement for SafetyStrip.
//!
//! The single, portable enforcer of the §5 invariants — no external cargo plugins
//! required, so the same checks run locally and in CI.
//!
//! Subcommands:
//!   gen-header          (re)write the frozen C header from the FFI source
//!   check-abi           fail if the checked-in C header has drifted
//!   check-unsafe-forbid assert `#![forbid(unsafe_code)]` is present in core
//!   check-core-deps     strict allowlist on `safetystrip-core`'s dependency tree
//!   check-no-network    workspace-wide banlist of network/OS-capable crates
//!   check-entitlements  assert the macOS entitlements file is minimal
//!   check-no-content-logging  assert no clipboard content is logged/persisted
//!   check-clipboard-safety    assert default targets avoid the real clipboard
//!   check-pipeline-zeroization assert fused core scratch storage is wiped before release
//!   check-agent-workflow      assert the AI-native workflow docs exist with required headings
//!   check-c-ffi-surface       assert C/SwiftPM interop stays header-only and tiny
//!   check-test-hygiene        assert every ignored test has a reason and the count is ratcheted
//!   check-release-posture     assert official signing cannot broaden entitlements
//!   check-supply-chain  cargo-deny: advisories + licenses + bans + sources
//!   check-unused-deps   cargo-machete: fail on a declared-but-unused dependency
//!   check-docs          build docs with -D warnings (broken intra-doc links, bad HTML)
//!   check-workflows     lint GitHub Actions workflows (actionlint + zizmor)
//!   check-shell         shellcheck the shell scripts
//!   check-fuzz          build cargo-fuzz targets; optionally smoke-run all targets
//!   check-miri          run the core-ffi boundary tests under Miri (UB detection)
//!   check-kani          run the bounded Kani proofs over the resource-envelope arithmetic
//!   ci                  run fmt --check, clippy -D warnings, test, and all the above
//!
//! Every check exits nonzero on violation with a remediation-oriented message so a
//! future agent learns how to fix it rather than how to silence it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
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
        Some("check-c-ffi-surface") => report(check_c_ffi_surface()),
        Some("check-test-hygiene") => report(check_test_hygiene()),
        Some("check-release-posture") => report(check_release_posture()),
        Some("check-supply-chain") => report(check_supply_chain()),
        Some("check-unused-deps") => report(check_unused_deps()),
        Some("check-docs") => report(check_docs()),
        Some("check-workflows") => report(check_workflows()),
        Some("check-shell") => report(check_shell()),
        Some("check-fuzz") => report(check_fuzz()),
        Some("check-miri") => report(check_miri()),
        Some("check-kani") => report(check_kani()),
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
         \x20 check-agent-workflow       assert the AI-native workflow docs exist with required headings\n\
         \x20 check-c-ffi-surface        assert C/SwiftPM interop stays header-only and tiny\n\
         \x20 check-test-hygiene         assert every #[ignore] has a reason and the count is ratcheted\n\
         \x20 check-release-posture      assert official signing cannot broaden entitlements\n\
         \x20 check-supply-chain   cargo-deny: advisories + licenses + bans + sources\n\
         \x20 check-unused-deps    cargo-machete: fail on a declared-but-unused dependency\n\
         \x20 check-docs           build docs with -D warnings (broken intra-doc links, bad HTML)\n\
         \x20 check-workflows      lint GitHub Actions workflows (actionlint + zizmor)\n\
         \x20 check-shell          shellcheck the shell scripts\n\
         \x20 check-fuzz           build fuzz targets; set SS_FUZZ_SMOKE_SECONDS=N to run them\n\
         \x20 check-miri           run core-ffi boundary tests under Miri (UB detection in the unsafe shim)\n\
         \x20 check-kani           run the bounded Kani proofs over the resource-envelope arithmetic\n\
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

/// Run `cargo metadata --format-version 1` and parse it. We invoke the same
/// `cargo` that launched xtask (via `$CARGO`, falling back to `cargo`) so the
/// pinned toolchain is honored.
fn cargo_metadata() -> Result<Metadata, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(&cargo)
        .args(["metadata", "--format-version", "1"])
        .current_dir(workspace_root())
        .output()
        .map_err(|e| format!("failed to run `{cargo} metadata`: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "`{cargo} metadata` exited with {}:\n{}",
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

/// Walk the transitive *normal* dependency closure from `start` ids. Dev- and
/// build-dependencies are skipped (their `dep_kinds` carry no `null`/normal
/// entry), so e.g. a crate's `proptest`/`cbindgen` does not pollute the result —
/// but every real runtime/proc-macro dependency does.
fn normal_dep_closure<'a>(meta: &'a Metadata, start: &[&'a str]) -> BTreeSet<&'a str> {
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
                let is_normal = dep.dep_kinds.iter().any(|k| k.kind.is_none());
                if is_normal && !seen_ids.contains(dep.pkg.as_str()) {
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

/// Explicit allowlist for `safetystrip-core`'s full transitive *normal*
/// dependency tree.
///
/// This is the mechanical form of "the core has no OS, filesystem, or network
/// dependencies". The set is intentionally tiny and consists only of pure-data /
/// text / proc-macro crates:
///
/// * `serde`, `serde_core`, `serde_derive`, `serde_json` — config (de)serialization.
/// * `pulldown-cmark` — CommonMark → events for the Markdown stripper.
/// * proc-macro toolchain pulled in by `serde_derive`: `proc-macro2`, `quote`,
///   `syn`, `unicode-ident`, `unicode-xid`.
/// * pure formatting / data helpers: `itoa`, `ryu`, `zmij` (float formatting),
///   `memchr`, `bitflags`, `unicase`.
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
    "safetystrip-core",
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
    "unicode-xid",
    // pure formatting / data helpers
    "itoa",
    "ryu",
    "zmij",
    "memchr",
    "bitflags",
    "unicase",
    // best-effort heap zeroization of clipboard-derived pipeline intermediates
    // (alloc feature only; no transitive crates, no OS/IO/network surface).
    "zeroize",
];

/// Assert that every crate in the core's transitive normal-dependency tree is on
/// [`CORE_DEP_ALLOWLIST`]. This is how a future OS/IO/network dependency sneaking
/// into the core gets caught at CI time.
fn check_core_deps() -> Result<(), String> {
    let meta = cargo_metadata().map_err(|e| format!("check-core-deps: FAIL — {e}"))?;

    let core_ids = meta.package_ids_named("safetystrip-core");
    if core_ids.is_empty() {
        return Err(
            "check-core-deps: FAIL — `safetystrip-core` not found in `cargo metadata`. \
             Did the core crate get renamed or removed?"
                .to_string(),
        );
    }

    let allow: HashSet<&str> = CORE_DEP_ALLOWLIST.iter().copied().collect();
    let closure = normal_dep_closure(&meta, &core_ids);

    let mut offenders: Vec<&str> = closure
        .iter()
        .copied()
        .filter(|name| !allow.contains(name))
        .collect();
    offenders.sort_unstable();

    if offenders.is_empty() {
        println!(
            "check-core-deps: core's {} transitive normal deps are all on the allowlist.",
            closure.len()
        );
        Ok(())
    } else {
        Err(format!(
            "check-core-deps: FAIL — `safetystrip-core` depends (transitively) on crate(s) \
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
/// build. If any of these appears anywhere in the workspace dependency tree, that
/// is a posture change that must be caught, explained, and justified (or, far more
/// likely, reverted).
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

/// Walk the WHOLE workspace dependency tree and fail if any banned network/OS
/// crate is present anywhere.
fn check_no_network() -> Result<(), String> {
    let meta = cargo_metadata().map_err(|e| format!("check-no-network: FAIL — {e}"))?;

    // Start from every workspace member so the closure spans core, core-ffi, cli,
    // and xtask (and thus catches a network dep introduced into any of them).
    let members: Vec<&str> = meta.workspace_members.iter().map(String::as_str).collect();
    let closure = normal_dep_closure(&meta, &members);

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
             SafetyStrip's privacy posture is no-network-anywhere: a plain-text clipboard\n\
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
    workspace_root().join("shells/macos/SafetyStrip.entitlements")
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
        r#"DEFAULT_SIGN_ENTITLEMENTS="${SCRIPT_DIR}/SafetyStrip.entitlements""#,
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

    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "release entitlement posture guard(s) missing:\n  {}\n\
             \n\
             Official Developer ID releases must sign with the checked \
             app-sandbox-only entitlement file, reject alternate SIGN_ENTITLEMENTS \
             paths, and verify the signed entitlement payload remains minimal. \
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
             official App Sandbox minimality:\n{e}",
            release_path.display()
        )
    })?;

    println!(
        "check-release-posture: official signing path rejects alternate entitlements and verifies minimal signed payloads."
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// check-c-ffi-surface
// ---------------------------------------------------------------------------

const ALLOWED_C_FFI_SURFACE: &[&str] = &[
    "core-ffi/include/safetystrip.h",
    "shells/macos/Sources/CSafetyStrip/dummy.c",
    "shells/macos/Sources/CSafetyStrip/include/module.modulemap",
    "shells/macos/Sources/CSafetyStrip/include/shim.h",
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
    let rel = "core-ffi/include/safetystrip.h";
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
        "#ifndef SAFETYSTRIP_FFI_H",
        "uint32_t ss_abi_version(void);",
        "const char *ss_capabilities_json(void);",
        "enum SsStatus ss_transform(",
        "void ss_buffer_free(uint8_t *ptr, size_t len);",
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
        "shells/macos/Sources/CSafetyStrip/dummy.c",
        &[],
        &mut errors,
    );
    check_file_lines(
        &root,
        "shells/macos/Sources/CSafetyStrip/include/shim.h",
        &[r#"#include "../../../../../core-ffi/include/safetystrip.h""#],
        &mut errors,
    );
    check_file_lines(
        &root,
        "shells/macos/Sources/CSafetyStrip/include/module.modulemap",
        &[
            "module CSafetyStrip {",
            r#"header "shim.h""#,
            "export *",
            "}",
        ],
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
             SafetyStrip keeps handwritten C logic out of the project. The only allowed \
             C-adjacent surface is the cbindgen-generated ABI header, a SwiftPM shim \
             that includes that header, the module map, and an empty dummy translation \
             unit required by SwiftPM. Do not add C/C++/Objective-C implementation code \
             without an explicit compatibility/security review.",
            errors.join("\n  ")
        ))
    }
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
            // Only real attributes: a trimmed line that *starts* with `#[ignore`. This
            // skips doc-comment / prose mentions of `#[ignore]` (e.g. in module docs).
            if !trimmed.starts_with("#[ignore") {
                continue;
            }
            ignored += 1;
            // A reason takes the form `#[ignore = "..."]` (spacing-insensitive). Anything
            // else — bare `#[ignore]` — is a silent skip.
            let rest = trimmed["#[ignore".len()..].trim_start();
            if !rest.starts_with('=') {
                let shown = path.strip_prefix(&root).unwrap_or(path.as_path());
                bare.push(format!("{}:{}: {}", shown.display(), i + 1, trimmed));
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
        for (i, line) in text.lines().enumerate() {
            if flags_content_logging(line) {
                let shown = path.strip_prefix(&root).unwrap_or(path.as_path());
                hits.push(format!("{}:{}: {}", shown.display(), i + 1, line.trim()));
            }
        }
    }

    if hits.is_empty() {
        println!(
            "check-no-content-logging: scanned {} shipped source file(s); no clipboard-content \
             logging or persistence.",
            files.len()
        );
        Ok(())
    } else {
        Err(format!(
            "check-no-content-logging: FAIL — line(s) appear to log or persist clipboard-derived \
             content:\n\x20 {}\n\
             \n\
             SafetyStrip must never write clipboard content to a log sink, to disk, or to user\n\
             defaults. Log fixed operational states only; persist user *settings* (operation\n\
             choices, shortcuts), never clipboard input/output/derived text. If this is a false\n\
             positive, rename the local so the line no longer reads as logging real content.",
            hits.join("\n  ")
        ))
    }
}

// ---------------------------------------------------------------------------
// check-pipeline-zeroization
// ---------------------------------------------------------------------------
//
// Security finding class: fused pipeline scratch buffers can hold
// clipboard-derived bytes just like full pipeline intermediates. They must be
// wiped before their storage is released or reallocated and on drop; otherwise an
// optimization silently weakens the documented in-memory hygiene posture.

fn pipeline_path() -> PathBuf {
    workspace_root().join("core/src/pipeline.rs")
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

    println!(
        "check-pipeline-zeroization: fused pipeline scratch storage is zeroized before release and on drop."
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
            "check-fuzz: FAIL — SS_FUZZ_SMOKE_SECONDS must be a positive integer, \
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

    let smoke_seconds_raw = std::env::var("SS_FUZZ_SMOKE_SECONDS").ok();
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
            "check-fuzz: built all fuzz targets. Set SS_FUZZ_SMOKE_SECONDS=N to \
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

    // `cargo miri test -p safetystrip-ffi` builds the FFI crate (and its deps) under
    // Miri and runs its tests, including the unsafe boundary round-trips. The core is
    // pulled in as a dependency and interpreted too, but we scope the *test run* to
    // the crate that owns the unsafe so the pass stays fast. Default Miri isolation
    // is fine: nothing in these tests touches the filesystem, clock, or network.
    println!("check-miri: $ cargo +nightly miri test -p safetystrip-ffi");
    let status = Command::new("cargo")
        .args(["+nightly", "miri", "test", "-p", "safetystrip-ffi"])
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
/// `safetystrip-core`. Best-effort and heavy, so it stays out of the required `ci`
/// gate (mirrors `check-fuzz` / `check-miri`).
fn check_kani() -> Result<(), String> {
    ensure_kani()?;

    // `cargo kani -p safetystrip-core` discovers and verifies every `#[kani::proof]`
    // harness in the core crate. Harnesses are `#[cfg(kani)]`, so this is the only
    // command that compiles them at all.
    println!("check-kani: $ cargo kani -p safetystrip-core");
    let status = Command::new("cargo")
        .args(["kani", "-p", "safetystrip-core"])
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
// check-agent-workflow
// ---------------------------------------------------------------------------
//
// The AI-native engineering loop is encoded in repo-native docs (see
// docs/agent-workflow.md). Those files only stay load-bearing if they keep their
// structure: this check fails CI if one is deleted or loses a required section, so
// the workflow cannot silently rot into a stale README. It is a pure structural
// check (no external tools), matching the other docs-structure guards.

/// The workflow files and the section headings each must keep. Headings are matched
/// as exact lines (after trimming) so a rename or accidental deletion fails the
/// check, but reordering or adding sections is fine. Kept intentionally small and
/// stable: these are the load-bearing sections, not every heading.
const AGENT_WORKFLOW_FILES: &[(&str, &[&str])] = &[
    ("docs/agent-workflow.md", &["## The loop", "## North star"]),
    (
        "docs/templates/correctness-brief.md",
        &["## Change class", "## Evidence packet", "## Proof gaps"],
    ),
    (
        ".github/pull_request_template.md",
        &["## Change class", "## Commands run"],
    ),
    ("docs/agent-tasks/core-transform.md", AGENT_TASK_HEADINGS),
    ("docs/agent-tasks/ffi-boundary.md", AGENT_TASK_HEADINGS),
    ("docs/agent-tasks/security-privacy.md", AGENT_TASK_HEADINGS),
    ("docs/agent-tasks/dependency-ci.md", AGENT_TASK_HEADINGS),
    (
        "docs/agent-tasks/review-finding-closure.md",
        AGENT_TASK_HEADINGS,
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

/// Assert every AI-native workflow doc exists and still carries its required
/// sections. This keeps `docs/agent-workflow.md` and its templates from silently
/// drifting or disappearing.
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

    if errors.is_empty() {
        println!(
            "check-agent-workflow: all {} AI-native workflow doc(s) present with required headings.",
            AGENT_WORKFLOW_FILES.len()
        );
        Ok(())
    } else {
        Err(format!(
            "check-agent-workflow: FAIL —\n\x20 {}\n\
             \n\
             SafetyStrip's evidence-first workflow lives in repo-native docs so future\n\
             agents have a clear loop (see docs/agent-workflow.md). These files must stay\n\
             present and structured. Restore the missing file or section; do not delete the\n\
             workflow docs to make this check pass. If a section is intentionally renamed,\n\
             update AGENT_WORKFLOW_FILES in xtask/src/main.rs in the same PR.",
            errors.join("\n  ")
        ))
    }
}

// ---------------------------------------------------------------------------
// ci
// ---------------------------------------------------------------------------

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
    // Tooling gates first (cheap to fix, catch the most common breakage).
    let cargo_steps: [(&str, &[&str]); 3] = [
        ("fmt", &["fmt", "--all", "--check"]),
        (
            "clippy",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("test", &["test", "--workspace"]),
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
    if let Err(msg) = check_clipboard_safety() {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(msg) = check_c_ffi_surface() {
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
            std::fs::read_to_string(root.join("shells/macos/SafetyStrip.entitlements")).unwrap();
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
}
