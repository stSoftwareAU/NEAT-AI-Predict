//! Command-line entry point for `neat_ai_predict`.
//!
//! `neat_ai_predict predict --creature <json> --archive <fingerprint-root>
//! [--dataset <id>] [--from <date>] [--to <date>] --output <dir>` runs the
//! creature over every archived row in range and writes the predictions.
//! Progress and every refusal go to stderr; each refusal has its own exit
//! code (BSD `sysexits.h`) — never silent.
//!
//! GRQ's ensure helpers log `neat_ai_predict --version`, which clap provides
//! from `Cargo.toml`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use neat_ai_predict::archive::ArchiveErrorKind;
use neat_ai_predict::engine::EngineError;
use neat_ai_predict::output::OutputError;
use neat_ai_predict::run::{RunError, run};
use neat_ai_predict::{MarketDate, PredictRequest};

/// The run finished and every row in range was written, except rows the
/// creature produced a non-finite output for — listed in the manifest. The
/// same "the batch ran, some rows did not" code `rust_scorer` uses.
const EXIT_PARTIAL: u8 = 3;
/// `EX_USAGE`: the request refused its own arguments.
const EXIT_USAGE: u8 = 64;
/// `EX_DATAERR`: the archive or the creature is malformed or incompatible.
const EXIT_DATAERR: u8 = 65;
/// `EX_NOINPUT`: an input path does not exist or cannot be read.
const EXIT_NOINPUT: u8 = 66;
/// `EX_CANTCREAT`: `--output` holds a different run.
const EXIT_CANTCREAT: u8 = 73;
/// `EX_IOERR`: output could not be written.
const EXIT_IOERR: u8 = 74;

#[derive(Parser)]
#[command(name = "neat_ai_predict", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run one creature over an identified observation archive.
    Predict(PredictArgs),
}

#[derive(Args)]
struct PredictArgs {
    /// CreatureExport JSON to activate.
    #[arg(long, value_name = "FILE")]
    creature: PathBuf,
    /// Archive fingerprint root: <root>/<extension>/<fingerprint>.
    #[arg(long, value_name = "DIR")]
    archive: PathBuf,
    /// Dataset snapshot id; defaults to the one <archive>/latest.json names.
    #[arg(long, value_name = "ID")]
    dataset: Option<String>,
    /// First market date to score, inclusive (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    from: Option<MarketDate>,
    /// Last market date to score, inclusive (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    to: Option<MarketDate>,
    /// Directory the prediction partitions and manifest are written to.
    #[arg(long, value_name = "DIR")]
    output: PathBuf,
}

impl From<PredictArgs> for PredictRequest {
    fn from(args: PredictArgs) -> Self {
        Self {
            creature: args.creature,
            archive: args.archive,
            dataset: args.dataset,
            from: args.from,
            to: args.to,
            output: args.output,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Predict(args) => predict(args.into()),
    }
}

fn predict(request: PredictRequest) -> ExitCode {
    let progress = |line: &str| eprintln!("{line}");
    match run(&request, &progress) {
        Ok(summary) if summary.manifest.refused.is_empty() => ExitCode::SUCCESS,
        Ok(summary) => {
            eprintln!(
                "error: {} row(s) produced a non-finite output and were not written; see {}/manifest.json",
                summary.manifest.refused.len(),
                summary.output.display()
            );
            ExitCode::from(EXIT_PARTIAL)
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(exit_code(&err))
        }
    }
}

fn exit_code(err: &RunError) -> u8 {
    match err {
        RunError::Request(_) => EXIT_USAGE,
        RunError::Archive(err) if err.kind == ArchiveErrorKind::Io => EXIT_NOINPUT,
        RunError::Archive(_) => EXIT_DATAERR,
        RunError::Engine(EngineError::Io { .. }) => EXIT_NOINPUT,
        RunError::Engine(_) => EXIT_DATAERR,
        RunError::Output(OutputError::Occupied { .. }) => EXIT_CANTCREAT,
        RunError::Output(OutputError::Io { .. }) => EXIT_IOERR,
    }
}
