//! Prediction partitions and the run's provenance manifest (issue #5).
//!
//! The layout is GRQ's prediction cache (`src/inference/PredictionStore.ts`),
//! so GRQ's TypeScript and GRQ-AutoTraderBackTesting can read it without this
//! crate:
//!
//! ```text
//! <output>/<yyyy>/<mm>/<prefix>.bin          outputCount little-endian f64 per row
//! <output>/<yyyy>/<mm>/<prefix>.index.json   schema grq.predictions.partition/1
//! <output>/manifest.json                     schema neat-ai-predict.run/1, written last
//! ```
//!
//! Outputs are widened from `f32` to `f64` exactly (every `f32` is an `f64`),
//! matching the store's "reuse must be identical" contract.
//!
//! A run is **atomic**: everything is written into a sibling staging directory
//! and renamed onto `<output>` only once the manifest is complete, so a killed
//! run never leaves a partition that looks finished. An output directory that
//! already holds the same run — same identity key and date range — is a no-op;
//! one that holds anything else is refused, never overwritten.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::archive::{RowIdentity, sha256_hex};

/// Schema of a prediction partition index — GRQ's `PREDICTION_INDEX_SCHEMA`.
pub const PREDICTION_INDEX_SCHEMA: &str = "grq.predictions.partition/1";
/// Schema of the run manifest.
pub const RUN_MANIFEST_SCHEMA: &str = "neat-ai-predict.run/1";
/// GRQ's research mode for a fixed creature replayed over past observations.
pub const RESEARCH_MODE: &str = "retrospective_fixed_model";
/// Bytes per stored output (`f64`).
pub const BYTES_PER_OUTPUT: usize = 8;

/// Everything a set of predictions is keyed by — GRQ's `PredictionIdentity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictionIdentity {
    /// `sha256:<creature file hash>`.
    #[serde(rename = "creatureUUID")]
    pub creature_uuid: String,
    /// Inputs the executed creature consumed: the archive width (a narrower
    /// creature runs extended with unconnected inputs).
    pub input_count: usize,
    /// Outputs per row.
    pub output_count: usize,
    /// Observation extension of the archived inputs.
    pub observation_extension: u32,
    /// Feature fingerprint of the archived inputs.
    pub feature_fingerprint: String,
    /// The snapshot the inputs came from.
    pub dataset_id: String,
    /// Activation engine.
    pub engine: String,
    /// Exact engine version.
    pub engine_version: String,
}

impl PredictionIdentity {
    /// The 32-hex-digit digest GRQ's `predictionIdentity` computes over the
    /// same fields, in the same order.
    #[must_use]
    pub fn key(&self) -> String {
        let canonical = [
            RESEARCH_MODE.to_owned(),
            format!("creature={}", self.creature_uuid),
            format!("inputs={}", self.input_count),
            format!("outputs={}", self.output_count),
            format!("extension={}", self.observation_extension),
            format!("features={}", self.feature_fingerprint),
            format!("width={}", self.input_count),
            format!("dataset={}", self.dataset_id),
            format!("engine={}", self.engine),
            format!("engineVersion={}", self.engine_version),
        ]
        .join("|");
        let mut key = sha256_hex(canonical.as_bytes());
        key.truncate(32);
        key
    }
}

/// One row of a partition index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRow {
    /// Canonical symbol.
    pub symbol: String,
    /// UTC market date, `YYYY-MM-DD`.
    pub date: String,
    /// Exchange-qualified identity, when the archive recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange: Option<String>,
    /// Row ordinal in the payload.
    pub row: usize,
}

/// A partition index, field for field GRQ's `PredictionPartitionIndex`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartitionIndex {
    /// [`PREDICTION_INDEX_SCHEMA`].
    pub schema: String,
    /// [`PredictionIdentity::key`].
    pub key: String,
    /// The identity's fields, flattened as GRQ writes them.
    #[serde(flatten)]
    pub identity: PredictionIdentity,
    /// [`RESEARCH_MODE`].
    pub research_mode: String,
    /// UTC year.
    pub year: u16,
    /// UTC month.
    pub month: u8,
    /// Symbol prefix.
    pub prefix: String,
    /// Payload file name, beside the index.
    pub payload_file: String,
    /// Payload size.
    pub payload_bytes: u64,
    /// Payload SHA-256.
    pub payload_sha256: String,
    /// Rows in the payload.
    pub row_count: usize,
    /// Row identities, in payload order.
    pub rows: Vec<IndexRow>,
    /// When the partition was written.
    #[serde(rename = "createdUTC")]
    pub created_utc: String,
}

/// A row the creature produced a non-finite output for. It is never written
/// as a number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusedRow {
    /// Canonical symbol.
    pub symbol: String,
    /// UTC market date.
    pub date: String,
    /// Why it was refused.
    pub reason: String,
}

/// A written partition, as the manifest lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartitionRecord {
    /// `yyyy/mm/prefix`.
    pub path: String,
    /// Rows written.
    pub row_count: usize,
    /// Payload SHA-256.
    pub payload_sha256: String,
}

/// The run manifest: proof of which model scored which rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunManifest {
    /// [`RUN_MANIFEST_SCHEMA`].
    pub schema: String,
    /// [`PredictionIdentity::key`].
    pub key: String,
    /// The identity the predictions are keyed by.
    pub identity: PredictionIdentity,
    /// [`RESEARCH_MODE`].
    pub research_mode: String,
    /// `neat_ai_predict` version.
    pub tool_version: String,
    /// `neat-core` git source, tag and commit.
    pub engine_source: String,
    /// The creature file as given.
    pub creature_path: String,
    /// The creature's own declared input count, before extension.
    pub creature_input: usize,
    /// The fingerprint root read.
    pub archive_root: String,
    /// Snapshots in the chain, head first.
    pub dataset_chain: Vec<String>,
    /// First date requested, inclusive (`null`: open).
    pub from: Option<String>,
    /// Last date requested, inclusive (`null`: open).
    pub to: Option<String>,
    /// Partitions written, in order.
    pub partitions: Vec<PartitionRecord>,
    /// Rows written across all partitions.
    pub row_count: usize,
    /// Rows the archive restated, resolved to their latest copy.
    pub superseded_rows: usize,
    /// Rows refused for a non-finite output.
    pub refused: Vec<RefusedRow>,
    /// When the run started.
    #[serde(rename = "startedUTC")]
    pub started_utc: String,
    /// When the run finished.
    #[serde(rename = "finishedUTC")]
    pub finished_utc: String,
}

/// Why output could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputError {
    /// `<output>` already holds something other than this run.
    Occupied {
        /// The output directory.
        path: PathBuf,
        /// What is there.
        detail: String,
    },
    /// A file-system operation failed.
    Io {
        /// The path involved.
        path: PathBuf,
        /// The operating-system error.
        detail: String,
    },
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Occupied { path, detail } => write!(
                f,
                "--output {} is not empty and is not this run ({detail}); choose a new directory",
                path.display()
            ),
            Self::Io { path, detail } => write!(f, "cannot write {}: {detail}", path.display()),
        }
    }
}

impl std::error::Error for OutputError {}

fn io_error(path: &Path, err: &std::io::Error) -> OutputError {
    OutputError::Io {
        path: path.to_path_buf(),
        detail: err.to_string(),
    }
}

/// What `<output>` already holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExistingOutput {
    /// Nothing: the directory is absent or empty.
    Empty,
    /// A finished run with this manifest.
    Finished(Box<RunManifest>),
}

/// Inspects `<output>` before a run.
///
/// # Errors
///
/// [`OutputError::Occupied`] when the directory holds anything but a finished
/// run's manifest-bearing tree.
pub fn inspect_output(output: &Path) -> Result<ExistingOutput, OutputError> {
    let mut entries = match fs::read_dir(output) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(ExistingOutput::Empty),
        Err(err) => return Err(io_error(output, &err)),
    };
    if entries.next().is_none() {
        return Ok(ExistingOutput::Empty);
    }
    let manifest_path = output.join("manifest.json");
    let text = fs::read_to_string(&manifest_path).map_err(|_| OutputError::Occupied {
        path: output.to_path_buf(),
        detail: "it has no manifest.json".to_owned(),
    })?;
    let manifest: RunManifest =
        serde_json::from_str(&text).map_err(|err| OutputError::Occupied {
            path: output.to_path_buf(),
            detail: format!("its manifest.json is not a {RUN_MANIFEST_SCHEMA} manifest: {err}"),
        })?;
    if manifest.schema != RUN_MANIFEST_SCHEMA {
        return Err(OutputError::Occupied {
            path: output.to_path_buf(),
            detail: format!("its manifest declares schema {:?}", manifest.schema),
        });
    }
    Ok(ExistingOutput::Finished(Box::new(manifest)))
}

/// A run being written into a staging directory beside `<output>`.
#[derive(Debug)]
pub struct StagedRun {
    output: PathBuf,
    staging: PathBuf,
    identity: PredictionIdentity,
    key: String,
}

impl StagedRun {
    /// Creates a fresh staging directory for `output`.
    ///
    /// # Errors
    ///
    /// [`OutputError::Io`] when it cannot be created.
    pub fn begin(output: &Path, identity: PredictionIdentity) -> Result<Self, OutputError> {
        let name = output
            .file_name()
            .map_or_else(|| "output".into(), |n| n.to_string_lossy().into_owned());
        let parent = output.parent().unwrap_or_else(|| Path::new("."));
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        fs::create_dir_all(parent).map_err(|err| io_error(parent, &err))?;
        let staging = parent.join(format!(".{name}.partial-{}", std::process::id()));
        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(|err| io_error(&staging, &err))?;
        }
        fs::create_dir(&staging).map_err(|err| io_error(&staging, &err))?;
        let key = identity.key();
        Ok(Self {
            output: output.to_path_buf(),
            staging,
            identity,
            key,
        })
    }

    /// The staging directory.
    #[must_use]
    pub fn staging_dir(&self) -> &Path {
        &self.staging
    }

    /// Writes one partition. Rows whose outputs are not all finite are left
    /// out of the payload and returned as [`RefusedRow`]s.
    ///
    /// `outputs` holds `output_count` values per row of `rows`, in order.
    ///
    /// # Errors
    ///
    /// [`OutputError::Io`] when a file cannot be written.
    pub fn write_partition(
        &self,
        year: u16,
        month: u8,
        prefix: &str,
        rows: &[&RowIdentity],
        outputs: &[f32],
    ) -> Result<(Option<PartitionRecord>, Vec<RefusedRow>), OutputError> {
        let width = self.identity.output_count;
        let mut payload = Vec::with_capacity(rows.len() * width * BYTES_PER_OUTPUT);
        let mut index_rows = Vec::with_capacity(rows.len());
        let mut refused = Vec::new();
        for (identity, values) in rows.iter().zip(outputs.chunks_exact(width)) {
            if let Some(bad) = values.iter().find(|v| !v.is_finite()) {
                refused.push(RefusedRow {
                    symbol: identity.symbol.clone(),
                    date: identity.date.to_string(),
                    reason: format!("non-finite output {bad}"),
                });
                continue;
            }
            for value in values {
                payload.extend_from_slice(&f64::from(*value).to_le_bytes());
            }
            index_rows.push(IndexRow {
                symbol: identity.symbol.clone(),
                date: identity.date.to_string(),
                exchange: identity.exchange.clone(),
                row: index_rows.len(),
            });
        }
        if index_rows.is_empty() {
            return Ok((None, refused));
        }

        let dir = self
            .staging
            .join(format!("{year:04}"))
            .join(format!("{month:02}"));
        fs::create_dir_all(&dir).map_err(|err| io_error(&dir, &err))?;
        let payload_file = format!("{prefix}.bin");
        let bin_path = dir.join(&payload_file);
        fs::write(&bin_path, &payload).map_err(|err| io_error(&bin_path, &err))?;

        let payload_sha256 = sha256_hex(&payload);
        let index = PartitionIndex {
            schema: PREDICTION_INDEX_SCHEMA.to_owned(),
            key: self.key.clone(),
            identity: self.identity.clone(),
            research_mode: RESEARCH_MODE.to_owned(),
            year,
            month,
            prefix: prefix.to_owned(),
            payload_file,
            payload_bytes: u64::try_from(payload.len()).unwrap_or(u64::MAX),
            payload_sha256: payload_sha256.clone(),
            row_count: index_rows.len(),
            rows: index_rows,
            created_utc: utc_now(),
        };
        let index_path = dir.join(format!("{prefix}.index.json"));
        write_json(&index_path, &index)?;
        Ok((
            Some(PartitionRecord {
                path: format!("{year:04}/{month:02}/{prefix}"),
                row_count: index.row_count,
                payload_sha256,
            }),
            refused,
        ))
    }

    /// Writes the manifest last and moves the finished run onto `<output>`.
    ///
    /// # Errors
    ///
    /// [`OutputError::Io`] when the manifest or the rename fails; the staging
    /// directory is left for inspection.
    pub fn finish(self, manifest: &RunManifest) -> Result<PathBuf, OutputError> {
        write_json(&self.staging.join("manifest.json"), manifest)?;
        // An empty `<output>` (created by the caller ahead of time) is replaced.
        if self.output.exists() {
            fs::remove_dir(&self.output).map_err(|err| io_error(&self.output, &err))?;
        }
        fs::rename(&self.staging, &self.output).map_err(|err| io_error(&self.output, &err))?;
        Ok(self.output)
    }

    /// Removes the staging directory after a failed run.
    pub fn abandon(self) {
        let _ = fs::remove_dir_all(&self.staging);
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), OutputError> {
    let mut text = serde_json::to_string_pretty(value).map_err(|err| OutputError::Io {
        path: path.to_path_buf(),
        detail: err.to_string(),
    })?;
    text.push('\n');
    fs::write(path, text).map_err(|err| io_error(path, &err))
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
#[must_use]
pub fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format_utc(secs)
}

/// Formats seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
#[must_use]
pub fn format_utc(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
    let rem = secs % 86_400;
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> PredictionIdentity {
        PredictionIdentity {
            creature_uuid: "sha256:abc".to_owned(),
            input_count: 2511,
            output_count: 1,
            observation_extension: 116,
            feature_fingerprint: "b448fe6b43db398e".to_owned(),
            dataset_id: "20261004T093634Z-31163-6lgt1w".to_owned(),
            engine: "neat-core/score_records_parallel_flat".to_owned(),
            engine_version: "0.22.16".to_owned(),
        }
    }

    #[test]
    fn the_key_is_grqs_digest_of_the_identity() {
        // GRQ: sha256HexOfText(canonical).slice(0, 32) over the same fields.
        let canonical = "retrospective_fixed_model|creature=sha256:abc|inputs=2511|outputs=1|extension=116|features=b448fe6b43db398e|width=2511|dataset=20261004T093634Z-31163-6lgt1w|engine=neat-core/score_records_parallel_flat|engineVersion=0.22.16";
        assert_eq!(identity().key(), sha256_hex(canonical.as_bytes())[..32]);
        let mut other = identity();
        other.dataset_id = "x".to_owned();
        assert_ne!(other.key(), identity().key());
    }

    #[test]
    fn the_index_serialises_with_grqs_field_names() {
        let index = PartitionIndex {
            schema: PREDICTION_INDEX_SCHEMA.to_owned(),
            key: identity().key(),
            identity: identity(),
            research_mode: RESEARCH_MODE.to_owned(),
            year: 2021,
            month: 5,
            prefix: "A".to_owned(),
            payload_file: "A.bin".to_owned(),
            payload_bytes: 8,
            payload_sha256: "x".to_owned(),
            row_count: 1,
            rows: vec![IndexRow {
                symbol: "AAPL".to_owned(),
                date: "2021-05-03".to_owned(),
                exchange: None,
                row: 0,
            }],
            created_utc: "2026-10-04T00:00:00Z".to_owned(),
        };
        let json: serde_json::Value = serde_json::to_value(&index).unwrap();
        for field in [
            "schema",
            "key",
            "creatureUUID",
            "inputCount",
            "outputCount",
            "observationExtension",
            "featureFingerprint",
            "datasetId",
            "engine",
            "engineVersion",
            "researchMode",
            "year",
            "month",
            "prefix",
            "payloadFile",
            "payloadBytes",
            "payloadSha256",
            "rowCount",
            "rows",
            "createdUTC",
        ] {
            assert!(json.get(field).is_some(), "missing {field}");
        }
        assert!(
            json["rows"][0].get("exchange").is_none(),
            "an absent exchange is omitted"
        );
    }

    #[test]
    fn non_finite_outputs_are_refused_by_identity_and_never_written() {
        use crate::MarketDate;
        let root = std::env::temp_dir().join(format!(
            "neat-ai-predict-output-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let staged = StagedRun::begin(&root.join("out"), identity()).unwrap();
        let date: MarketDate = "2021-05-03".parse().unwrap();
        let ids: Vec<RowIdentity> = ["AAPL", "AMZN", "ANZ"]
            .iter()
            .map(|s| RowIdentity {
                symbol: (*s).to_owned(),
                date,
                exchange: None,
                alias: None,
            })
            .collect();
        let rows: Vec<&RowIdentity> = ids.iter().collect();
        let (record, refused) = staged
            .write_partition(2021, 5, "A", &rows, &[f32::NAN, 0.25, f32::NEG_INFINITY])
            .unwrap();
        let record = record.unwrap();
        assert_eq!(record.row_count, 1);
        let refused: Vec<&str> = refused.iter().map(|r| r.symbol.as_str()).collect();
        assert_eq!(refused, ["AAPL", "ANZ"]);
        let payload = fs::read(staged.staging_dir().join("2021/05/A.bin")).unwrap();
        assert_eq!(payload, 0.25f64.to_le_bytes());
        staged.abandon();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn utc_formatting_handles_leap_years_and_epoch() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_utc(1_791_072_000 + 3_661), "2026-10-04T01:01:01Z");
    }
}
