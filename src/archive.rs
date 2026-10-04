//! Reader for the GRQ identified observation archive (issue #3).
//!
//! The format is owned by GRQ (`docs/Identified_Observation_Archive.md`; the
//! TypeScript reference is `src/observations/ObservationArchiveReader.ts`).
//! This reader resolves one **published snapshot** and the rows it holds:
//!
//! ```text
//! <fp>/latest.json                         {"datasetId": …} — the newest snapshot
//! <fp>/datasets/<id>.json                  immutable manifest, chained to its predecessor
//! <fp>/<yyyy>/<mm>/<prefix>/<chunk>.bin    inputCount little-endian f32 per row, no target
//! <fp>/<yyyy>/<mm>/<prefix>/<chunk>.index.json
//! ```
//!
//! `<fp>` is a fingerprint root, `<root>/<observationExtension>/<featureFingerprint>`.
//! Each manifest lists only the shards its own publication committed, so the
//! snapshot is the whole chain. Rows resolve oldest publication first, shards
//! in manifest order, so a restated row (a later chunk for the same
//! `symbol@date`) supersedes the earlier one — the rule GRQ's reader applies.
//!
//! Nothing here trusts the archive: every path segment is checked before it is
//! joined onto the root, every index is checked against its manifest entry and
//! the snapshot's semantics, and every payload's size and SHA-256 are verified
//! before any row of it is used. A fault names the file and fails the run.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{MarketDate, is_valid_dataset_id};

/// Schema of a shard index file. Readers reject anything else.
pub const SHARD_INDEX_SCHEMA: &str = "grq.observations.shard/1";
/// Schema of a dataset manifest. Readers reject anything else.
pub const DATASET_MANIFEST_SCHEMA: &str = "grq.observations.dataset/1";
/// Declared payload byte order.
pub const ARCHIVE_BYTE_ORDER: &str = "little-endian";
/// Bytes per stored observation value (`f32`).
pub const BYTES_PER_VALUE: usize = 4;

/// What kind of fault an [`ArchiveError`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveErrorKind {
    /// A file could not be read.
    Io,
    /// A file is not the JSON its schema requires.
    Json,
    /// A file declares a schema or byte order this reader does not support.
    Schema,
    /// A value is inconsistent with the rest of the archive (sizes, row
    /// ordinals, identities, a looping chain, an unsafe path).
    Corrupt,
    /// A payload's SHA-256 differs from the one its index declares.
    Hash,
    /// A file was written under different feature semantics.
    Semantics,
}

impl fmt::Display for ArchiveErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Io => "io",
            Self::Json => "json",
            Self::Schema => "schema",
            Self::Corrupt => "corrupt",
            Self::Hash => "hash",
            Self::Semantics => "semantics",
        })
    }
}

/// A refused archive, naming the file at fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveError {
    /// The kind of fault.
    pub kind: ArchiveErrorKind,
    /// The file (or directory) at fault.
    pub path: PathBuf,
    /// What is wrong with it.
    pub detail: String,
}

impl ArchiveError {
    fn new(kind: ArchiveErrorKind, path: &Path, detail: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "archive {} fault at {}: {}",
            self.kind,
            self.path.display(),
            self.detail
        )
    }
}

impl std::error::Error for ArchiveError {}

/// Feature semantics every row of a snapshot was assembled under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveSemantics {
    /// Observation extension (e.g. 116).
    pub observation_extension: u32,
    /// `f32` values per row.
    pub input_count: usize,
    /// Fingerprint of the feature semantics; names the fingerprint root.
    pub feature_fingerprint: String,
}

/// Identity of one archived row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowIdentity {
    /// Exact canonical symbol as GRQ's Market supplies it.
    pub symbol: String,
    /// UTC market date.
    pub date: MarketDate,
    /// Exchange-qualified identity, when recorded.
    pub exchange: Option<String>,
    /// Dated alias, when recorded.
    pub alias: Option<String>,
}

/// Where a resolved row's payload lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowRef {
    /// The row's identity.
    pub identity: RowIdentity,
    /// Index into [`Archive::shards`].
    pub shard: usize,
    /// Row ordinal within that shard.
    pub row: usize,
}

/// One shard the snapshot references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardRef {
    /// Payload path.
    pub bin_path: PathBuf,
    /// SHA-256 (lower-case hex) the manifest and the index both declare.
    pub sha256: String,
    /// Payload size in bytes.
    pub bytes: u64,
    /// Rows in the payload.
    pub row_count: usize,
}

/// One `yyyy/mm/prefix` partition of resolved rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    /// UTC year.
    pub year: u16,
    /// UTC month, `1..=12`.
    pub month: u8,
    /// Symbol-prefix directory name.
    pub prefix: String,
    /// Resolved rows, ascending by `(symbol, date)`.
    pub rows: Vec<RowRef>,
}

impl Partition {
    /// `yyyy/mm/prefix`, the partition's path relative to a root.
    #[must_use]
    pub fn id(&self) -> String {
        format!("{:04}/{:02}/{}", self.year, self.month, self.prefix)
    }
}

/// An inclusive market-date window; `None` leaves that side open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DateRange {
    /// First date kept, inclusive.
    pub from: Option<MarketDate>,
    /// Last date kept, inclusive.
    pub to: Option<MarketDate>,
}

impl DateRange {
    /// Whether `date` falls inside the window.
    #[must_use]
    pub fn contains(&self, date: MarketDate) -> bool {
        self.from.is_none_or(|from| date >= from) && self.to.is_none_or(|to| date <= to)
    }
}

/// A resolved snapshot: its semantics, its shards and its rows by partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    /// The fingerprint root that was opened.
    pub fingerprint_root: PathBuf,
    /// The head snapshot resolved.
    pub dataset_id: String,
    /// Snapshots in the chain, head first.
    pub dataset_chain: Vec<String>,
    /// Semantics every shard shares.
    pub semantics: ArchiveSemantics,
    /// Every shard the chain references, oldest publication first.
    pub shards: Vec<ShardRef>,
    /// Rows by partition, ascending by `(year, month, prefix)`; only rows
    /// inside the requested [`DateRange`], and only non-empty partitions.
    pub partitions: Vec<Partition>,
    /// Rows a later chunk restated; each counted once per superseded copy.
    pub superseded_rows: usize,
}

impl Archive {
    /// Rows resolved inside the requested range.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.partitions.iter().map(|p| p.rows.len()).sum()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PointerFile {
    dataset_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestFile {
    schema: String,
    dataset_id: String,
    byte_order: String,
    observation_extension: u32,
    input_count: usize,
    feature_fingerprint: String,
    shards: Vec<ManifestShard>,
    #[serde(default)]
    previous_dataset_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestShard {
    path: String,
    sha256: String,
    bytes: u64,
    row_count: usize,
    prefix: String,
    year: u16,
    month: u8,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexFile {
    schema: String,
    byte_order: String,
    observation_extension: u32,
    input_count: usize,
    feature_fingerprint: String,
    prefix: String,
    year: u16,
    month: u8,
    shard_file: String,
    shard_bytes: u64,
    shard_sha256: String,
    row_count: usize,
    rows: Vec<IndexRow>,
}

#[derive(Deserialize)]
struct IndexRow {
    symbol: String,
    date: String,
    #[serde(default)]
    exchange: Option<String>,
    #[serde(default)]
    alias: Option<String>,
    row: usize,
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ArchiveError> {
    let text = fs::read_to_string(path)
        .map_err(|err| ArchiveError::new(ArchiveErrorKind::Io, path, err.to_string()))?;
    serde_json::from_str(&text)
        .map_err(|err| ArchiveError::new(ArchiveErrorKind::Json, path, err.to_string()))
}

/// The symbol-prefix directory GRQ files a canonical symbol under: its first
/// character upper-cased when that is an ASCII letter or digit, `_` otherwise.
#[must_use]
pub fn prefix_for_symbol(symbol: &str) -> String {
    match symbol.chars().next().map(|c| c.to_ascii_uppercase()) {
        Some(c) if c.is_ascii_alphanumeric() => c.to_string(),
        _ => "_".to_owned(),
    }
}

/// Whether `path` is relative and stays inside the root it is joined onto:
/// no empty, `.` or `..` segment, no backslash. GRQ's `validateShardPath`.
fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// Lower-case hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    hex
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Opens the snapshot `dataset` (or the one `latest.json` names) under the
/// fingerprint root `fp_root`, keeping only rows inside `range`.
///
/// Reads every manifest and shard index of the chain; no payload is read
/// until [`read_partition`].
///
/// # Errors
///
/// Returns an [`ArchiveError`] naming the first file at fault.
pub fn open(
    fp_root: &Path,
    dataset: Option<&str>,
    range: DateRange,
) -> Result<Archive, ArchiveError> {
    let head = match dataset {
        Some(id) => id.to_owned(),
        None => read_json::<PointerFile>(&fp_root.join("latest.json"))?.dataset_id,
    };
    let chain = read_chain(fp_root, &head)?;
    let semantics = chain_semantics(fp_root, &chain)?;
    check_root_names_semantics(fp_root, &semantics)?;

    let mut shards = Vec::new();
    let mut resolved: HashMap<(String, MarketDate), RowRef> = HashMap::new();
    let mut superseded_rows = 0usize;
    let mut seen_paths = HashSet::new();
    // Oldest publication first, shards in manifest order: a later copy of a
    // row supersedes an earlier one.
    for (manifest_path, manifest) in chain.iter().rev() {
        for entry in &manifest.shards {
            if !seen_paths.insert(entry.path.clone()) {
                return Err(ArchiveError::new(
                    ArchiveErrorKind::Corrupt,
                    manifest_path,
                    format!("shard {} is referenced twice in the chain", entry.path),
                ));
            }
            let shard_index = shards.len();
            let (shard, rows) = read_shard(fp_root, manifest_path, entry, &semantics)?;
            shards.push(shard);
            for (identity, row) in rows {
                let key = (identity.symbol.clone(), identity.date);
                let located = RowRef {
                    identity,
                    shard: shard_index,
                    row,
                };
                if resolved.insert(key, located).is_some() {
                    superseded_rows += 1;
                }
            }
        }
    }

    let mut by_partition: HashMap<(u16, u8, String), Vec<RowRef>> = HashMap::new();
    for located in resolved.into_values() {
        if !range.contains(located.identity.date) {
            continue;
        }
        let key = (
            located.identity.date.year(),
            located.identity.date.month(),
            prefix_for_symbol(&located.identity.symbol),
        );
        by_partition.entry(key).or_default().push(located);
    }
    let mut partitions: Vec<Partition> = by_partition
        .into_iter()
        .map(|((year, month, prefix), mut rows)| {
            rows.sort_by(|a, b| {
                (&a.identity.symbol, a.identity.date).cmp(&(&b.identity.symbol, b.identity.date))
            });
            Partition {
                year,
                month,
                prefix,
                rows,
            }
        })
        .collect();
    partitions.sort_by(|a, b| (a.year, a.month, &a.prefix).cmp(&(b.year, b.month, &b.prefix)));

    Ok(Archive {
        fingerprint_root: fp_root.to_path_buf(),
        dataset_id: head,
        dataset_chain: chain.iter().map(|(_, m)| m.dataset_id.clone()).collect(),
        semantics,
        shards,
        partitions,
        superseded_rows,
    })
}

fn read_chain(fp_root: &Path, head: &str) -> Result<Vec<(PathBuf, ManifestFile)>, ArchiveError> {
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut current = Some(head.to_owned());
    let mut referrer = fp_root.join("latest.json");
    while let Some(id) = current {
        if !is_valid_dataset_id(&id) {
            return Err(ArchiveError::new(
                ArchiveErrorKind::Corrupt,
                &referrer,
                format!("dataset id {id:?} is not a single safe path component"),
            ));
        }
        if !seen.insert(id.clone()) {
            return Err(ArchiveError::new(
                ArchiveErrorKind::Corrupt,
                &referrer,
                format!("dataset chain loops at {id}"),
            ));
        }
        let path = fp_root.join("datasets").join(format!("{id}.json"));
        let manifest: ManifestFile = read_json(&path)?;
        if manifest.schema != DATASET_MANIFEST_SCHEMA {
            return Err(ArchiveError::new(
                ArchiveErrorKind::Schema,
                &path,
                format!(
                    "declares schema {:?}, this reader supports {DATASET_MANIFEST_SCHEMA}",
                    manifest.schema
                ),
            ));
        }
        if manifest.byte_order != ARCHIVE_BYTE_ORDER {
            return Err(ArchiveError::new(
                ArchiveErrorKind::Schema,
                &path,
                format!("declares byte order {:?}", manifest.byte_order),
            ));
        }
        if manifest.dataset_id != id {
            return Err(ArchiveError::new(
                ArchiveErrorKind::Corrupt,
                &path,
                format!("names itself {:?}, expected {id:?}", manifest.dataset_id),
            ));
        }
        current = manifest.previous_dataset_id.clone();
        referrer = path.clone();
        chain.push((path, manifest));
    }
    Ok(chain)
}

fn chain_semantics(
    fp_root: &Path,
    chain: &[(PathBuf, ManifestFile)],
) -> Result<ArchiveSemantics, ArchiveError> {
    let Some((_, head)) = chain.first() else {
        return Err(ArchiveError::new(
            ArchiveErrorKind::Corrupt,
            fp_root,
            "the dataset chain is empty",
        ));
    };
    let semantics = ArchiveSemantics {
        observation_extension: head.observation_extension,
        input_count: head.input_count,
        feature_fingerprint: head.feature_fingerprint.clone(),
    };
    if semantics.input_count < 1 {
        return Err(ArchiveError::new(
            ArchiveErrorKind::Corrupt,
            fp_root,
            "the snapshot declares inputCount 0",
        ));
    }
    for (path, manifest) in chain {
        if manifest.observation_extension != semantics.observation_extension
            || manifest.input_count != semantics.input_count
            || manifest.feature_fingerprint != semantics.feature_fingerprint
        {
            return Err(ArchiveError::new(
                ArchiveErrorKind::Semantics,
                path,
                format!(
                    "written under extension {} / {} inputs / fingerprint {}, but the snapshot is {} / {} / {}",
                    manifest.observation_extension,
                    manifest.input_count,
                    manifest.feature_fingerprint,
                    semantics.observation_extension,
                    semantics.input_count,
                    semantics.feature_fingerprint
                ),
            ));
        }
    }
    Ok(semantics)
}

/// The fingerprint root must be `<…>/<observationExtension>/<featureFingerprint>`,
/// so a run pointed at the wrong archive is refused rather than mixed.
fn check_root_names_semantics(
    fp_root: &Path,
    semantics: &ArchiveSemantics,
) -> Result<(), ArchiveError> {
    let fingerprint = fp_root.file_name().and_then(|n| n.to_str());
    let extension = fp_root
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str());
    let expected_extension = semantics.observation_extension.to_string();
    if fingerprint != Some(semantics.feature_fingerprint.as_str())
        || extension != Some(expected_extension.as_str())
    {
        return Err(ArchiveError::new(
            ArchiveErrorKind::Semantics,
            fp_root,
            format!(
                "--archive must be the fingerprint root <root>/{}/{} that the snapshot names",
                semantics.observation_extension, semantics.feature_fingerprint
            ),
        ));
    }
    Ok(())
}

fn read_shard(
    fp_root: &Path,
    manifest_path: &Path,
    entry: &ManifestShard,
    semantics: &ArchiveSemantics,
) -> Result<(ShardRef, Vec<(RowIdentity, usize)>), ArchiveError> {
    let corrupt =
        |path: &Path, detail: String| ArchiveError::new(ArchiveErrorKind::Corrupt, path, detail);

    if !is_safe_relative_path(&entry.path) {
        return Err(corrupt(
            manifest_path,
            format!(
                "shard path {:?} is not relative inside the fingerprint root",
                entry.path
            ),
        ));
    }
    let Some(stem) = entry.path.strip_suffix(".bin") else {
        return Err(corrupt(
            manifest_path,
            format!("shard path {:?} does not name a .bin payload", entry.path),
        ));
    };
    let bin_path = fp_root.join(&entry.path);
    let index_path = fp_root.join(format!("{stem}.index.json"));
    let index: IndexFile = read_json(&index_path)?;

    if index.schema != SHARD_INDEX_SCHEMA {
        return Err(ArchiveError::new(
            ArchiveErrorKind::Schema,
            &index_path,
            format!(
                "declares schema {:?}, this reader supports {SHARD_INDEX_SCHEMA}",
                index.schema
            ),
        ));
    }
    if index.byte_order != ARCHIVE_BYTE_ORDER {
        return Err(ArchiveError::new(
            ArchiveErrorKind::Schema,
            &index_path,
            format!("declares byte order {:?}", index.byte_order),
        ));
    }
    if index.observation_extension != semantics.observation_extension
        || index.input_count != semantics.input_count
        || index.feature_fingerprint != semantics.feature_fingerprint
    {
        return Err(ArchiveError::new(
            ArchiveErrorKind::Semantics,
            &index_path,
            format!(
                "written under extension {} / {} inputs / fingerprint {}",
                index.observation_extension, index.input_count, index.feature_fingerprint
            ),
        ));
    }
    let file_name = bin_path.file_name().and_then(|n| n.to_str());
    if file_name != Some(index.shard_file.as_str()) {
        return Err(corrupt(
            &index_path,
            format!(
                "names shard file {:?}, but the manifest references {}",
                index.shard_file, entry.path
            ),
        ));
    }
    if index.rows.len() != index.row_count || index.row_count != entry.row_count {
        return Err(corrupt(
            &index_path,
            format!(
                "declares {} rows and lists {}; the manifest says {}",
                index.row_count,
                index.rows.len(),
                entry.row_count
            ),
        ));
    }
    let expected_bytes = u64::try_from(index.row_count * semantics.input_count * BYTES_PER_VALUE)
        .map_err(|_| corrupt(&index_path, "payload size overflows".to_owned()))?;
    if index.shard_bytes != expected_bytes || entry.bytes != expected_bytes {
        return Err(corrupt(
            &index_path,
            format!(
                "declares {} payload bytes (manifest {}); {} rows of {} inputs is {expected_bytes}",
                index.shard_bytes, entry.bytes, index.row_count, semantics.input_count
            ),
        ));
    }
    if index.shard_sha256 != entry.sha256 {
        return Err(ArchiveError::new(
            ArchiveErrorKind::Hash,
            &index_path,
            format!(
                "declares SHA-256 {}, the manifest {}",
                index.shard_sha256, entry.sha256
            ),
        ));
    }
    if index.year != entry.year || index.month != entry.month || index.prefix != entry.prefix {
        return Err(corrupt(
            &index_path,
            format!(
                "is partition {:04}/{:02}/{}, the manifest says {:04}/{:02}/{}",
                index.year, index.month, index.prefix, entry.year, entry.month, entry.prefix
            ),
        ));
    }

    let mut rows = Vec::with_capacity(index.rows.len());
    let mut ordinals = HashSet::with_capacity(index.rows.len());
    for listed in index.rows {
        if listed.symbol.is_empty() || listed.symbol.trim() != listed.symbol {
            return Err(corrupt(
                &index_path,
                format!("row symbol {:?} is not a canonical symbol", listed.symbol),
            ));
        }
        let date: MarketDate = listed.date.parse().map_err(|err| {
            corrupt(
                &index_path,
                format!("row {}@{}: {err}", listed.symbol, listed.date),
            )
        })?;
        if listed.row >= index.row_count || !ordinals.insert(listed.row) {
            return Err(corrupt(
                &index_path,
                format!(
                    "{}@{date} names row {} of a {}-row shard more than once or out of range",
                    listed.symbol, listed.row, index.row_count
                ),
            ));
        }
        if date.year() != index.year
            || date.month() != index.month
            || prefix_for_symbol(&listed.symbol) != index.prefix
        {
            return Err(corrupt(
                &index_path,
                format!(
                    "{}@{date} does not belong in partition {:04}/{:02}/{}",
                    listed.symbol, index.year, index.month, index.prefix
                ),
            ));
        }
        rows.push((
            RowIdentity {
                symbol: listed.symbol,
                date,
                exchange: listed.exchange,
                alias: listed.alias,
            },
            listed.row,
        ));
    }

    Ok((
        ShardRef {
            bin_path,
            sha256: entry.sha256.clone(),
            bytes: expected_bytes,
            row_count: index.row_count,
        },
        rows,
    ))
}

/// Reads a partition's rows into one flat buffer, `input_count` values per
/// row, in [`Partition::rows`] order.
///
/// Every shard the partition touches is read whole, and its size and SHA-256
/// are verified before any of its rows is copied.
///
/// # Errors
///
/// Returns an [`ArchiveError`] when a payload is missing, has the wrong size,
/// does not hash to the declared SHA-256, or holds a non-finite value.
pub fn read_partition(archive: &Archive, partition: &Partition) -> Result<Vec<f32>, ArchiveError> {
    let width = archive.semantics.input_count;
    let mut payloads: HashMap<usize, Vec<u8>> = HashMap::new();
    for located in &partition.rows {
        if payloads.contains_key(&located.shard) {
            continue;
        }
        let Some(shard) = archive.shards.get(located.shard) else {
            return Err(ArchiveError::new(
                ArchiveErrorKind::Corrupt,
                &archive.fingerprint_root,
                format!("row refers to unknown shard {}", located.shard),
            ));
        };
        payloads.insert(located.shard, read_verified_payload(shard)?);
    }

    let row_bytes = width * BYTES_PER_VALUE;
    let mut flat = Vec::with_capacity(partition.rows.len() * width);
    for located in &partition.rows {
        let Some(payload) = payloads.get(&located.shard) else {
            continue;
        };
        let start = located.row * row_bytes;
        let Some(bytes) = payload.get(start..start + row_bytes) else {
            return Err(ArchiveError::new(
                ArchiveErrorKind::Corrupt,
                &archive.fingerprint_root,
                format!("row {} lies outside its shard", located.row),
            ));
        };
        let start = flat.len();
        flat.extend(
            bytes
                .as_chunks::<BYTES_PER_VALUE>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b)),
        );
        // GRQ's writer refuses a non-finite observation (`add/non-finite`), so
        // one here is corruption — and neat-core would clamp it into a
        // plausible-looking prediction rather than fail.
        if let Some(column) = flat[start..].iter().position(|v| !v.is_finite()) {
            let shard = archive
                .shards
                .get(located.shard)
                .map_or_else(|| archive.fingerprint_root.clone(), |s| s.bin_path.clone());
            return Err(ArchiveError::new(
                ArchiveErrorKind::Corrupt,
                &shard,
                format!(
                    "{}@{} input[{column}] is {}; observations are always finite",
                    located.identity.symbol,
                    located.identity.date,
                    flat[start + column]
                ),
            ));
        }
    }
    Ok(flat)
}

fn read_verified_payload(shard: &ShardRef) -> Result<Vec<u8>, ArchiveError> {
    let payload = fs::read(&shard.bin_path)
        .map_err(|err| ArchiveError::new(ArchiveErrorKind::Io, &shard.bin_path, err.to_string()))?;
    if u64::try_from(payload.len()).ok() != Some(shard.bytes) {
        return Err(ArchiveError::new(
            ArchiveErrorKind::Corrupt,
            &shard.bin_path,
            format!(
                "is {} bytes; its index declares {}",
                payload.len(),
                shard.bytes
            ),
        ));
    }
    let actual = sha256_hex(&payload);
    if actual != shard.sha256 {
        return Err(ArchiveError::new(
            ArchiveErrorKind::Hash,
            &shard.bin_path,
            format!("hashes to {actual}; its index declares {}", shard.sha256),
        ));
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_match_grq() {
        assert_eq!(prefix_for_symbol("AAPL"), "A");
        assert_eq!(prefix_for_symbol("bhp"), "B");
        assert_eq!(prefix_for_symbol("3IN"), "3");
        assert_eq!(prefix_for_symbol("^GSPC"), "_");
        assert_eq!(prefix_for_symbol("Ü"), "_");
    }

    #[test]
    fn shard_paths_must_stay_inside_the_root() {
        assert!(is_safe_relative_path("2021/05/A/000000.bin"));
        for bad in [
            "",
            "/2021/05/A/x.bin",
            "2021//A/x.bin",
            "../x.bin",
            "2021/./x.bin",
            "a\\b.bin",
            "x/",
        ] {
            assert!(!is_safe_relative_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn sha256_is_lower_case_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn date_ranges_are_inclusive_and_open_ended() {
        let d = |s: &str| s.parse::<MarketDate>().unwrap();
        let range = DateRange {
            from: Some(d("2021-05-01")),
            to: Some(d("2021-05-31")),
        };
        assert!(range.contains(d("2021-05-01")));
        assert!(range.contains(d("2021-05-31")));
        assert!(!range.contains(d("2021-06-01")));
        assert!(DateRange::default().contains(d("1999-01-01")));
    }
}
