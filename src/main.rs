//! Command-line entry point for `neat_ai_predict`.
//!
//! `neat_ai_predict predict --creature <json> --archive <fingerprint-root>
//! [--dataset <id>] [--from <date>] [--to <date>] --output <dir>` validates
//! the request and will run the creature over the archive. Every refusal is
//! reported on stderr with a distinct exit code — never silently.
//!
//! GRQ's ensure helpers log `neat_ai_predict --version`, which clap provides
//! from `Cargo.toml`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use neat_ai_predict::{MarketDate, PredictRequest};

/// `EX_USAGE`: the request refused its own arguments (BSD `sysexits.h`).
const EXIT_USAGE: u8 = 64;
/// `EX_NOINPUT`: an input path does not exist.
const EXIT_NOINPUT: u8 = 66;
/// `EX_UNAVAILABLE`: the prediction engine is not available in this build.
const EXIT_UNAVAILABLE: u8 = 69;

/// Issue tracking the engine that turns a validated request into predictions.
const ENGINE_TRACKER: &str = "https://github.com/stSoftwareAU/NEAT-AI-Predict/issues/1";

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
    /// Dataset snapshot id; defaults to datasets/latest.json.
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
    if let Err(err) = request.validate() {
        eprintln!("error: {err}");
        return ExitCode::from(EXIT_USAGE);
    }
    if !request.creature.is_file() {
        eprintln!(
            "error: --creature {} is not a file",
            request.creature.display()
        );
        return ExitCode::from(EXIT_NOINPUT);
    }
    if !request.archive.is_dir() {
        eprintln!(
            "error: --archive {} is not a directory",
            request.archive.display()
        );
        return ExitCode::from(EXIT_NOINPUT);
    }
    eprintln!(
        "error: the prediction engine is not implemented in this build — see {ENGINE_TRACKER}"
    );
    ExitCode::from(EXIT_UNAVAILABLE)
}
