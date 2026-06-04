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
//! With no config, `transform` applies the identity pipeline. Exit codes:
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

Reads stdin, writes stdout. With no config, applies the identity pipeline.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {}", err.message);
            ExitCode::from(err.code)
        }
    }
}

struct CliError {
    message: String,
    code: u8,
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: 2,
        }
    }
    fn config(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: 3,
        }
    }
}

fn run(args: &[String]) -> Result<(), CliError> {
    match args.first().map(String::as_str) {
        Some("capabilities") => {
            print!("{}", capabilities());
            Ok(())
        }
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

fn run_transform(args: &[String]) -> Result<(), CliError> {
    let config = parse_transform_config(args)?;

    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .map_err(|e| CliError::usage(format!("failed to read stdin: {e}")))?;
    // Mirror the FFI: never fail on adversarial bytes.
    let text = String::from_utf8_lossy(&input);

    let output = transform(&text, &config);
    std::io::stdout()
        .write_all(output.as_bytes())
        .map_err(|e| CliError::usage(format!("failed to write stdout: {e}")))?;
    Ok(())
}

fn parse_transform_config(args: &[String]) -> Result<Config, CliError> {
    match args {
        [] => Ok(Config::empty()),
        [flag, value, rest @ ..] if flag == "--config" => {
            reject_extra(rest)?;
            let json = std::fs::read_to_string(value)
                .map_err(|e| CliError::config(format!("cannot read {value}: {e}")))?;
            parse_config(&json).map_err(|e| CliError::config(e.to_string()))
        }
        [flag, value, rest @ ..] if flag == "--config-json" => {
            reject_extra(rest)?;
            parse_config(value).map_err(|e| CliError::config(e.to_string()))
        }
        [flag] if flag == "--config" || flag == "--config-json" => {
            Err(CliError::usage(format!("{flag} requires an argument")))
        }
        [other, ..] => Err(CliError::usage(format!("unexpected argument '{other}'"))),
    }
}

fn reject_extra(rest: &[String]) -> Result<(), CliError> {
    match rest.first() {
        Some(extra) => Err(CliError::usage(format!("unexpected argument '{extra}'"))),
        None => Ok(()),
    }
}
