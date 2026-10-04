//! Records the resolved `neat-core` version and source revision from
//! `Cargo.lock`, so every prediction manifest names the exact engine that
//! produced it (issue #5) without a hand-maintained constant that could drift.

use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    println!("cargo::rerun-if-changed=Cargo.lock");
    let lock = match fs::read_to_string("Cargo.lock") {
        Ok(text) => text,
        Err(err) => {
            eprintln!("build.rs: cannot read Cargo.lock: {err}");
            return ExitCode::FAILURE;
        }
    };
    let Some((version, source)) = neat_core_entry(&lock) else {
        eprintln!("build.rs: Cargo.lock has no neat-core package entry");
        return ExitCode::FAILURE;
    };
    println!("cargo::rustc-env=NEAT_CORE_VERSION={version}");
    println!("cargo::rustc-env=NEAT_CORE_SOURCE={source}");
    ExitCode::SUCCESS
}

/// `(version, source)` of the `neat-core` `[[package]]` table.
fn neat_core_entry(lock: &str) -> Option<(String, String)> {
    for package in lock.split("[[package]]") {
        let field = |key: &str| {
            package.lines().find_map(|line| {
                line.strip_prefix(key)
                    .and_then(|rest| rest.trim().strip_prefix('='))
                    .map(|value| value.trim().trim_matches('"').to_owned())
            })
        };
        if field("name ").as_deref() == Some("neat-core") {
            return Some((field("version ")?, field("source ").unwrap_or_default()));
        }
    }
    None
}
