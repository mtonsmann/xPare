//! Integration tests for the `xpare` binary.
//!
//! These run the actual built binary (via `CARGO_BIN_EXE_xpare`) as a
//! subprocess, feed it stdin, and assert on stdout, stderr, and the exit code.
//! They exercise the harness as a black box — the contract a fuzz/validation
//! driver and manual users depend on: stdout carries only transformed text,
//! diagnostics go to stderr, and the exit codes are 0 / 2 / 3.

use std::io::Write;
use std::process::{Command, Stdio};

/// Path to the binary under test, provided by Cargo for integration tests.
const BIN: &str = env!("CARGO_BIN_EXE_xpare");

/// Captured result of one binary invocation.
struct Output {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: String,
}

impl Output {
    fn stdout_str(&self) -> &str {
        std::str::from_utf8(&self.stdout).expect("stdout should be valid UTF-8 in these tests")
    }
}

/// Run the binary with `args` and the given raw `stdin` bytes; capture everything.
fn run(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn xpare binary");

    // Write and then drop stdin so the pipe closes (EOF) before we wait — otherwise
    // a child that reads to end-of-input would block forever. A child that exits
    // early without reading (e.g. `--help` or a usage error) closes the read end,
    // so a `BrokenPipe` here is expected and ignored; any other write error is a bug.
    {
        let mut child_stdin = child.stdin.take().expect("child stdin");
        match child_stdin.write_all(stdin) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
            Err(e) => panic!("failed to write child stdin: {e}"),
        }
    }

    let out = child.wait_with_output().expect("failed to wait on child");

    Output {
        code: out.status.code(),
        stdout: out.stdout,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Convenience for the common UTF-8 stdin case.
fn run_str(args: &[&str], stdin: &str) -> Output {
    run(args, stdin.as_bytes())
}

const STRIP_HTML: &str = r#"{"version":2,"operations":[{"op":"strip_html"}]}"#;

#[test]
fn capabilities_prints_valid_json_to_stdout() {
    let out = run(&["capabilities"], b"");
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        out.stderr.is_empty(),
        "stderr should be empty: {}",
        out.stderr
    );

    // The CLI crate has no JSON dependency (it parses config inside core), so we
    // validate structurally rather than with a parser: it must be a single brace-
    // delimited object with balanced braces and the expected top-level keys.
    let json = out.stdout_str();
    assert!(
        is_well_formed_json_object(json),
        "capabilities must be a balanced JSON object: {json}"
    );
    assert!(
        json.contains(r#""name":"xpare-core""#),
        "missing name: {json}"
    );
    assert!(
        json.contains(r#""config_version":2"#),
        "missing config_version: {json}"
    );
    assert!(
        json.contains(r#""operations":["#),
        "missing operations array: {json}"
    );
    assert!(
        json.contains(r#"{"op":"strip_html"}"#),
        "missing strip_html op: {json}"
    );
}

/// Minimal, dependency-free sanity check that `s` is a single well-formed JSON
/// object: it starts with `{`, ends with `}`, and braces/brackets are balanced
/// outside of string literals (with `\"` escapes handled). Not a full validator —
/// just enough to catch truncated or structurally broken output at the boundary.
fn is_well_formed_json_object(s: &str) -> bool {
    let s = s.trim();
    if !s.starts_with('{') || !s.ends_with('}') {
        return false;
    }
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for ch in s.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0 && !in_string
}

#[test]
fn capabilities_rejects_extra_arguments() {
    let out = run(&["capabilities", "extra"], b"");
    assert_eq!(out.code, Some(2));
    assert!(out.stdout.is_empty(), "no stdout on usage error");
    assert!(!out.stderr.is_empty());
}

#[test]
fn strip_html_via_config_json_over_stdin() {
    let out = run_str(
        &["transform", "--config-json", STRIP_HTML],
        "<p>Hello <b>world</b></p>",
    );
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(out.stdout_str(), "Hello world");
    assert!(out.stderr.is_empty());
}

#[test]
fn config_from_temp_file() {
    // Write a config file to the OS temp dir, with a PID-unique name so concurrent
    // test binaries never clash. Clean it up regardless of assertion outcome.
    let path = std::env::temp_dir().join(format!("xpare-cli-test-{}.json", std::process::id()));
    std::fs::write(&path, STRIP_HTML).expect("failed to write temp config");

    let out = run_str(
        &[
            "transform",
            "--config",
            path.to_str().expect("utf-8 temp path"),
        ],
        "<i>hi</i> there",
    );
    let _ = std::fs::remove_file(&path);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(out.stdout_str(), "hi there");
}

#[test]
fn empty_stdin_yields_empty_stdout() {
    let out = run(&["transform", "--config-json", STRIP_HTML], b"");
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        out.stdout.is_empty(),
        "empty input should yield empty output"
    );
}

#[test]
fn identity_transform_when_no_config_given() {
    // With no config flag the pipeline is the identity: bytes pass through unchanged,
    // including HTML, which is *not* stripped because no operation was requested.
    let input = "raw <b>text</b>  with   spaces\nand a line";
    let out = run_str(&["transform"], input);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(out.stdout_str(), input);
}

#[test]
fn multi_op_pipeline_runs_in_order() {
    // strip_html -> collapse_whitespace -> trim_trailing_whitespace.
    // The two block elements become two paragraphs separated by a blank line;
    // internal runs of spaces collapse to one; trailing spaces on each line go.
    let config = r#"{"version":2,"operations":[
        {"op":"strip_html"},
        {"op":"collapse_whitespace"},
        {"op":"trim_trailing_whitespace"}
    ]}"#;
    let input = "<p>Hello    <b>world</b>   </p>\n<div>foo     bar   </div>";
    let out = run_str(&["transform", "--config-json", config], input);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(out.stdout_str(), "Hello world\n\nfoo bar");
}

#[test]
fn invalid_json_is_a_config_error() {
    let out = run_str(&["transform", "--config-json", "this is not json"], "x");
    assert_eq!(out.code, Some(3));
    assert!(out.stdout.is_empty(), "no stdout on config error");
    assert!(
        out.stderr.contains("invalid config json"),
        "stderr should explain the JSON failure: {}",
        out.stderr
    );
}

#[test]
fn unsupported_version_is_a_config_error() {
    let out = run_str(&["transform", "--config-json", r#"{"version":99}"#], "x");
    assert_eq!(out.code, Some(3));
    assert!(
        out.stderr.contains("unsupported config version"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn missing_config_file_is_a_config_error() {
    let missing = std::env::temp_dir().join("xpare-cli-does-not-exist-xyz.json");
    let out = run_str(
        &[
            "transform",
            "--config",
            missing.to_str().expect("utf-8 path"),
        ],
        "x",
    );
    assert_eq!(out.code, Some(3));
    assert!(!out.stderr.is_empty());
}

#[test]
fn unknown_command_is_a_usage_error() {
    let out = run(&["frobnicate"], b"");
    assert_eq!(out.code, Some(2));
    assert!(out.stdout.is_empty(), "no stdout on usage error");
    assert!(
        out.stderr.contains("unknown command"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn unknown_flag_is_a_usage_error() {
    let out = run_str(&["transform", "--nonexistent"], "x");
    assert_eq!(out.code, Some(2));
    assert!(out.stdout.is_empty());
    assert!(
        out.stderr.contains("unknown flag"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn missing_flag_value_is_a_usage_error() {
    let out = run_str(&["transform", "--config"], "x");
    assert_eq!(out.code, Some(2));
    assert!(
        out.stderr.contains("requires an argument"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn flag_shaped_config_value_is_a_usage_error() {
    // `--config` immediately followed by another flag must NOT swallow that flag as the
    // value: parse_config_source rejects a flag-shaped value as a usage error (exit 2),
    // not as a config error (exit 3, which is what happens if the guard is weakened to &&).
    let out = run_str(&["transform", "--config", "--config-json"], "x");
    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(!out.stderr.is_empty());
}

#[test]
fn duplicate_config_flag_is_a_usage_error() {
    let out = run_str(
        &["transform", "--config-json", "{}", "--config-json", "{}"],
        "x",
    );
    assert_eq!(out.code, Some(2));
    assert!(out.stdout.is_empty());
    assert!(
        out.stderr.contains("mutually exclusive"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn conflicting_config_flags_are_a_usage_error() {
    let out = run_str(
        &["transform", "--config", "some.json", "--config-json", "{}"],
        "x",
    );
    assert_eq!(out.code, Some(2));
    assert!(
        out.stderr.contains("mutually exclusive"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn unexpected_positional_argument_is_a_usage_error() {
    let out = run_str(&["transform", "stray"], "x");
    assert_eq!(out.code, Some(2));
    assert!(
        out.stderr.contains("unexpected argument"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn no_args_prints_usage_to_stdout() {
    let out = run(&[], b"");
    assert_eq!(out.code, Some(0));
    assert!(
        out.stdout_str().contains("USAGE:"),
        "no-args should print usage to stdout"
    );
}

#[test]
fn help_flag_prints_usage_to_stdout() {
    for flag in ["--help", "-h"] {
        let out = run(&[flag], b"");
        assert_eq!(out.code, Some(0), "flag {flag}");
        assert!(
            out.stdout_str().contains("USAGE:"),
            "{flag} should print usage to stdout"
        );
    }
}

#[test]
fn transform_help_prints_usage_and_skips_stdin() {
    // `--help` under `transform` short-circuits to usage without transforming, and
    // wins even alongside an otherwise-conflicting flag.
    for args in [
        &["transform", "--help"][..],
        &["transform", "-h"][..],
        &["transform", "--config-json", "{}", "--help"][..],
    ] {
        let out = run_str(args, "ignored stdin");
        assert_eq!(out.code, Some(0), "args {args:?}");
        assert!(
            out.stdout_str().contains("USAGE:"),
            "args {args:?} should print usage to stdout, got {:?}",
            out.stdout_str()
        );
    }
}

#[test]
fn invalid_utf8_stdin_does_not_panic() {
    // 0xff 0xfe are not valid UTF-8; lossy decoding maps each to U+FFFD. The binary
    // must transform without panicking and exit cleanly.
    let out = run(&["transform", "--config-json", STRIP_HTML], &[0xff, 0xfe]);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    // Two replacement characters, with no HTML present to strip.
    assert_eq!(out.stdout_str(), "\u{FFFD}\u{FFFD}");
}

#[test]
fn embedded_nul_in_stdin_does_not_panic() {
    // An interior NUL is a valid Rust `char` and must pass through losslessly.
    let out = run(
        &["transform", "--config-json", STRIP_HTML],
        b"a\x00<b>c</b>",
    );
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(out.stdout, b"a\x00c");
}

#[test]
fn invalid_utf8_through_change_case_does_not_panic() {
    // A different op path over invalid bytes, to exercise more of the pipeline.
    let config = r#"{"version":2,"operations":[{"op":"change_case","case":"upper"}]}"#;
    let out = run(&["transform", "--config-json", config], b"h\xffi");
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    // 'h','i' uppercase around a replacement char for the lone 0xff byte.
    assert_eq!(out.stdout_str(), "H\u{FFFD}I");
}

#[test]
fn cli_defaults_to_as_given_but_canonical_reorders() {
    // Pipeline listed in the "wrong" order: defang before clean_urls.
    let config = r#"{"version":2,"operations":[{"op":"defang"},{"op":"clean_urls"}]}"#;
    let input = "https://e.com/?utm_source=x";

    // Default: as-given, so defang runs first and clean_urls can't match the now-
    // mangled URL — the tracker survives.
    let out = run_str(&["transform", "--config-json", config], input);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(out.stdout_str(), "hxxps[://]e[.]com/?utm_source=x");

    // --canonical reorders to clean_urls then defang: the tracker is stripped first.
    let out = run_str(
        &["transform", "--config-json", config, "--canonical"],
        input,
    );
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(out.stdout_str(), "hxxps[://]e[.]com/");
}
