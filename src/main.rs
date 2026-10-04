//! Command-line entry point for the template crate.
//!
//! Usage: `template-rust <name>` prints a greeting on stdout. Any error is
//! reported on stderr and the process exits non-zero — never fail silently.

use std::io::{self, Write};
use std::process::ExitCode;

/// Exit code for a usage error (wrong number of arguments), following the BSD
/// `sysexits.h` convention `EX_USAGE`.
const EXIT_USAGE: u8 = 64;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(raw), None) = (args.next(), args.next()) else {
        eprintln!("usage: template-rust <name>");
        return ExitCode::from(EXIT_USAGE);
    };

    let Some(name) = raw.to_str() else {
        eprintln!("error: name is not valid UTF-8");
        return ExitCode::FAILURE;
    };

    match template_rust::greet(name) {
        Ok(greeting) => {
            if let Err(err) = writeln!(io::stdout().lock(), "{greeting}") {
                eprintln!("error: could not write to stdout: {err}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
