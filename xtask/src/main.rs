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
         \x20 ci                   fmt + clippy + test + every structural check"
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
    let mut hits: Vec<String> = Vec::new();
    for key in entitlement_keys(text) {
        if is_banned_entitlement(&key) {
            hits.push(key);
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
            if matches!(name.as_ref(), "target" | ".build" | ".git" | ".swiftpm") {
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
    if let Err(msg) = check_clipboard_safety() {
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

    println!("ci: all checks passed.");
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
