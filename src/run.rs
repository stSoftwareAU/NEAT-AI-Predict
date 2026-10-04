//! One prediction run, end to end (issue #6).
//!
//! Partitions are spread across the rayon pool, and inside a partition its rows
//! are too ([`Predictor::predict`]), so a small partition never leaves cores
//! idle and a large one is still split. Each partition is read, verified,
//! activated and written independently; results are merged back in partition
//! order, so the manifest does not depend on scheduling.
//!
//! Peak memory is roughly one partition's inputs and outputs per pool thread —
//! `threads × rows × inputCount × 4` bytes — never the whole archive. A GRQ
//! partition (one symbol letter, one month) holds a few hundred rows of about
//! 10 kB each, so tens of megabytes on a 10-core host.

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rayon::prelude::*;

use crate::archive::{self, ArchiveError, DateRange, RowIdentity};
use crate::engine::{self, EngineError, Predictor};
use crate::output::{
    ExistingOutput, OutputError, PartitionRecord, PredictionIdentity, RESEARCH_MODE,
    RUN_MANIFEST_SCHEMA, RefusedRow, RunManifest, StagedRun, inspect_output, utc_now,
};
use crate::{PredictRequest, RequestError};

/// Why a run stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The request refused itself.
    Request(RequestError),
    /// The archive is unreadable or inconsistent.
    Archive(ArchiveError),
    /// The creature cannot be used.
    Engine(EngineError),
    /// The output cannot be written.
    Output(OutputError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(err) => err.fmt(f),
            Self::Archive(err) => err.fmt(f),
            Self::Engine(err) => err.fmt(f),
            Self::Output(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for RunError {}

impl From<ArchiveError> for RunError {
    fn from(err: ArchiveError) -> Self {
        Self::Archive(err)
    }
}

impl From<EngineError> for RunError {
    fn from(err: EngineError) -> Self {
        Self::Engine(err)
    }
}

impl From<OutputError> for RunError {
    fn from(err: OutputError) -> Self {
        Self::Output(err)
    }
}

/// What a finished run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    /// The output directory.
    pub output: PathBuf,
    /// The run's manifest.
    pub manifest: RunManifest,
    /// True when `<output>` already held this exact run and nothing was done.
    pub already_complete: bool,
}

/// Runs `request`, reporting progress lines through `progress` (called from
/// pool threads, hence `Sync`).
///
/// # Errors
///
/// The first [`RunError`]; a failed run leaves no partial `<output>`.
pub fn run(
    request: &PredictRequest,
    progress: &(dyn Fn(&str) + Sync),
) -> Result<RunSummary, RunError> {
    request.validate().map_err(RunError::Request)?;
    let started_utc = utc_now();
    let clock = Instant::now();

    let creature = engine::load_creature(&request.creature)?;
    let range = DateRange {
        from: request.from,
        to: request.to,
    };
    let archive = archive::open(&request.archive, request.dataset.as_deref(), range)?;
    let predictor = Predictor::new(&creature, archive.semantics.input_count)?;

    let identity = PredictionIdentity {
        creature_uuid: creature.identity(),
        input_count: archive.semantics.input_count,
        output_count: predictor.output_count(),
        observation_extension: archive.semantics.observation_extension,
        feature_fingerprint: archive.semantics.feature_fingerprint.clone(),
        dataset_id: archive.dataset_id.clone(),
        engine: engine::ENGINE.to_owned(),
        engine_version: engine::ENGINE_VERSION.to_owned(),
    };
    let key = identity.key();
    let from = request.from.map(|d| d.to_string());
    let to = request.to.map(|d| d.to_string());

    if let ExistingOutput::Finished(existing) = inspect_output(&request.output)? {
        if existing.key == key && existing.from == from && existing.to == to {
            progress(&format!(
                "[neat_ai_predict] {} already holds run {key}; nothing to do",
                request.output.display()
            ));
            return Ok(RunSummary {
                output: request.output.clone(),
                manifest: *existing,
                already_complete: true,
            });
        }
        return Err(RunError::Output(OutputError::Occupied {
            path: request.output.clone(),
            detail: format!(
                "it holds run {} for {}..{}",
                existing.key,
                existing.from.as_deref().unwrap_or("*"),
                existing.to.as_deref().unwrap_or("*")
            ),
        }));
    }

    progress(&format!(
        "[neat_ai_predict] {} rows in {} partitions from dataset {} (extension {}, {} inputs); creature {} ({} inputs, {} outputs); engine {} {}",
        archive.row_count(),
        archive.partitions.len(),
        archive.dataset_id,
        archive.semantics.observation_extension,
        archive.semantics.input_count,
        creature.identity(),
        predictor.input_count(),
        predictor.output_count(),
        engine::ENGINE,
        engine::ENGINE_VERSION
    ));

    let staged = StagedRun::begin(&request.output, identity.clone())?;
    let written = write_partitions(&staged, &archive, &predictor, progress);
    let (partitions, refused) = match written {
        Ok(done) => done,
        Err(err) => {
            staged.abandon();
            return Err(err);
        }
    };

    let row_count = partitions.iter().map(|p| p.row_count).sum();
    let manifest = RunManifest {
        schema: RUN_MANIFEST_SCHEMA.to_owned(),
        key,
        identity,
        research_mode: RESEARCH_MODE.to_owned(),
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        engine_source: engine::ENGINE_SOURCE.to_owned(),
        creature_path: request.creature.display().to_string(),
        creature_input: predictor.input_count(),
        archive_root: request.archive.display().to_string(),
        dataset_chain: archive.dataset_chain.clone(),
        from,
        to,
        partitions,
        row_count,
        superseded_rows: archive.superseded_rows,
        refused,
        started_utc,
        finished_utc: utc_now(),
    };
    let output = staged.finish(&manifest)?;
    let secs = clock.elapsed().as_secs_f64();
    progress(&format!(
        "[neat_ai_predict] wrote {row_count} rows to {} in {secs:.1}s ({:.0} rows/s); {} refused",
        output.display(),
        if secs > 0.0 {
            row_count as f64 / secs
        } else {
            0.0
        },
        manifest.refused.len()
    ));
    Ok(RunSummary {
        output,
        manifest,
        already_complete: false,
    })
}

/// One partition's record (absent when every row was refused) and its refusals.
type PartitionWritten = (Option<PartitionRecord>, Vec<RefusedRow>);

fn write_partitions(
    staged: &StagedRun,
    archive: &archive::Archive,
    predictor: &Predictor,
    progress: &(dyn Fn(&str) + Sync),
) -> Result<(Vec<PartitionRecord>, Vec<RefusedRow>), RunError> {
    let total = archive.partitions.len();
    let done = AtomicUsize::new(0);
    let results: Vec<Result<PartitionWritten, RunError>> = archive
        .partitions
        .par_iter()
        .map(|partition| {
            let inputs = archive::read_partition(archive, partition)?;
            let outputs = predictor.predict(&inputs)?;
            let rows: Vec<&RowIdentity> = partition.rows.iter().map(|r| &r.identity).collect();
            let written = staged.write_partition(
                partition.year,
                partition.month,
                &partition.prefix,
                &rows,
                &outputs,
            )?;
            for row in &written.1 {
                progress(&format!(
                    "[neat_ai_predict] refused {}@{}: {}",
                    row.symbol, row.date, row.reason
                ));
            }
            let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
            if finished.is_multiple_of(100) || finished == total {
                progress(&format!(
                    "[neat_ai_predict] {finished}/{total} partitions written"
                ));
            }
            Ok(written)
        })
        .collect();

    let mut partitions = Vec::with_capacity(total);
    let mut refused = Vec::new();
    for result in results {
        let (record, bad) = result?;
        partitions.extend(record);
        refused.extend(bad);
    }
    Ok((partitions, refused))
}
