//! Headless harness over `safetystrip-core`.
//!
//! This is the core's own validation/fuzz driver and a manual testing aid — it has
//! no clipboard or OS integration. It reads bytes from stdin, decodes them
//! losslessly to text (mirroring the FFI's behavior on adversarial input), applies
//! a JSON config, and writes the result to stdout.
//!
//! Usage:
//!   safetystrip capabilities
//!   safetystrip transform [--config <file> | --config-json <json>]   < input > output
//!
//! With no config, `transform` applies the identity pipeline. Errors go to stderr;
//! transformed text only ever reaches stdout. Exit codes:
//!   0 success · 2 usage error · 3 config error.
//!
//! **Owner: CLI stream (C).** Functional baseline; extend with tests and polish.

use std::io::{Read, Write};
use std::process::ExitCode;

use safetystrip_core::{capabilities, parse_config, transform, Config};

const USAGE: &str = "\
safetystrip — strip and transform text via safetystrip-core

USAGE:
    safetystrip capabilities
    safetystrip transform [--config <file> | --config-json <json>]
    safetystrip --help

Reads stdin, writes stdout. With no config, applies the identity pipeline.
The two config flags are mutually exclusive and may each appear at most once.";

/// Exit code for a usage error (bad command, bad/conflicting flags).
const EXIT_USAGE: u8 = 2;
/// Exit code for a config error (unreadable file, bad JSON, version mismatch).
const EXIT_CONFIG: u8 = 3;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Diagnostics never go to stdout: stdout carries only transformed text,
            // so a caller can pipe it safely even on failure.
            eprintln!("error: {}", err.message);
            ExitCode::from(err.code)
        }
    }
}

/// A failure carrying its message and the process exit code to surface it with.
struct CliError {
    message: String,
    code: u8,
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: EXIT_USAGE,
        }
    }
    fn config(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: EXIT_CONFIG,
        }
    }
}

fn run(args: &[String]) -> Result<(), CliError> {
    match args.first().map(String::as_str) {
        Some("capabilities") => run_capabilities(&args[1..]),
        Some("transform") => run_transform(&args[1..]),
        Some("--help" | "-h") | None => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(CliError::usage(format!(
            "unknown command '{other}'\n\n{USAGE}"
        ))),
    }
}

/// `capabilities` takes no arguments; print the core's self-description to stdout.
fn run_capabilities(args: &[String]) -> Result<(), CliError> {
    if let Some(extra) = args.first() {
        return Err(CliError::usage(format!(
            "'capabilities' takes no arguments, got '{extra}'"
        )));
    }
    print!("{}", capabilities());
    Ok(())
}

fn run_transform(args: &[String]) -> Result<(), CliError> {
    let config = match parse_transform_config(args)? {
        // `transform --help` documents the subcommand and exits successfully
        // without reading stdin or transforming anything.
        TransformArgs::Help => {
            println!("{USAGE}");
            return Ok(());
        }
        TransformArgs::Run(config) => config,
    };

    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .map_err(|e| CliError::usage(format!("failed to read stdin: {e}")))?;
    // Mirror the FFI: never fail on adversarial bytes. Lossy decoding turns invalid
    // UTF-8 into U+FFFD and preserves embedded NULs, so this can never panic.
    let text = String::from_utf8_lossy(&input);

    let output = transform(&text, &config);
    std::io::stdout()
        .write_all(output.as_bytes())
        .map_err(|e| CliError::usage(format!("failed to write stdout: {e}")))?;
    Ok(())
}

/// The outcome of parsing `transform`'s arguments: either show help, or run with a
/// resolved [`Config`]. Keeping help as data (rather than exiting mid-parse) lets the
/// dispatch layer own all I/O and process control.
enum TransformArgs {
    /// `--help` / `-h` was present: print usage and exit successfully.
    Help,
    /// Run the transform with this config (identity if no flag was given).
    Run(Config),
}

/// Which config source `transform` was asked to use.
enum ConfigSource<'a> {
    /// `--help` / `-h`: show usage instead of transforming.
    Help,
    /// No flag given: identity pipeline.
    Identity,
    /// `--config <path>`: read and parse the file at `path`.
    File(&'a str),
    /// `--config-json <json>`: parse `json` inline.
    Inline(&'a str),
}

/// Parse `transform`'s arguments into a [`TransformArgs`].
///
/// Accepts at most one of `--config <file>` / `--config-json <json>`; the two are
/// mutually exclusive and neither may repeat. `--help`/`-h` short-circuits to help.
/// Any other token, a missing flag value, a duplicate, or a conflict is a usage
/// error. Reading or parsing a config is a config error.
fn parse_transform_config(args: &[String]) -> Result<TransformArgs, CliError> {
    let config = match parse_config_source(args)? {
        ConfigSource::Help => return Ok(TransformArgs::Help),
        ConfigSource::Identity => Config::empty(),
        ConfigSource::File(path) => {
            let json = std::fs::read_to_string(path)
                .map_err(|e| CliError::config(format!("cannot read {path}: {e}")))?;
            parse_config(&json).map_err(|e| CliError::config(e.to_string()))?
        }
        ConfigSource::Inline(json) => {
            parse_config(json).map_err(|e| CliError::config(e.to_string()))?
        }
    };
    Ok(TransformArgs::Run(config))
}

/// Scan `transform`'s arguments and resolve the single config source, rejecting
/// duplicate, conflicting, unknown, or value-less flags. Side-effect-free.
fn parse_config_source(args: &[String]) -> Result<ConfigSource<'_>, CliError> {
    let mut source: Option<ConfigSource<'_>> = None;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        // The two config flags are the only tokens that take a value. `is_json`
        // records which kind matched so we can build the source after pulling its value.
        let is_json = match arg.as_str() {
            "--config" => false,
            "--config-json" => true,
            "--help" | "-h" => return Ok(ConfigSource::Help),
            other if other.starts_with('-') => {
                return Err(CliError::usage(format!(
                    "unknown flag '{other}'\n\n{USAGE}"
                )));
            }
            other => {
                return Err(CliError::usage(format!(
                    "unexpected argument '{other}'\n\n{USAGE}"
                )));
            }
        };

        // Every config flag requires exactly one following value.
        let value = iter
            .next()
            .map(String::as_str)
            .ok_or_else(|| CliError::usage(format!("{arg} requires an argument")))?;
        // Guard against the value itself being a config flag, e.g. `--config --config-json`.
        if value == "--config" || value == "--config-json" {
            return Err(CliError::usage(format!(
                "{arg} requires an argument, found flag '{value}'"
            )));
        }

        if source.is_some() {
            return Err(CliError::usage(
                "--config and --config-json are mutually exclusive and may each appear only once"
                    .to_string(),
            ));
        }
        source = Some(if is_json {
            ConfigSource::Inline(value)
        } else {
            ConfigSource::File(value)
        });
    }

    Ok(source.unwrap_or(ConfigSource::Identity))
}
