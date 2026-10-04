//! Builds GRQ identified observation archives in a private temporary
//! directory, byte for byte in the layout GRQ's `ObservationArchiveWriter`
//! publishes, so the reader is tested against the real format.

#![allow(dead_code, missing_docs, clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use neat_ai_predict::archive::{prefix_for_symbol, sha256_hex};
use serde_json::json;

pub const EXTENSION: u32 = 116;
pub const FINGERPRINT: &str = "b448fe6b43db398e";

/// A unique directory removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "neat-ai-predict-{label}-{}-{n}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One archived row.
#[derive(Clone)]
pub struct Row {
    pub symbol: String,
    pub date: String,
    pub values: Vec<f32>,
}

pub fn row(symbol: &str, date: &str, values: &[f32]) -> Row {
    Row {
        symbol: symbol.to_owned(),
        date: date.to_owned(),
        values: values.to_vec(),
    }
}

/// Writes GRQ archives under `<root>/<EXTENSION>/<FINGERPRINT>`.
pub struct ArchiveBuilder {
    pub fp_root: PathBuf,
    pub input_count: usize,
    chunk: usize,
    previous: Option<String>,
}

impl ArchiveBuilder {
    pub fn new(root: &Path, input_count: usize) -> Self {
        let fp_root = root.join(EXTENSION.to_string()).join(FINGERPRINT);
        fs::create_dir_all(fp_root.join("datasets")).unwrap();
        Self {
            fp_root,
            input_count,
            chunk: 0,
            previous: None,
        }
    }

    /// Publishes one snapshot holding `rows`, one shard per partition, and
    /// points `latest.json` at it. Returns the dataset id.
    pub fn publish(&mut self, dataset_id: &str, rows: &[Row]) -> String {
        let mut by_partition: std::collections::BTreeMap<(String, String, String), Vec<&Row>> =
            Default::default();
        for r in rows {
            assert_eq!(r.values.len(), self.input_count);
            let key = (
                r.date[0..4].to_owned(),
                r.date[5..7].to_owned(),
                prefix_for_symbol(&r.symbol),
            );
            by_partition.entry(key).or_default().push(r);
        }
        let mut shards = Vec::new();
        for ((year, month, prefix), rows) in by_partition {
            let chunk = format!("{:06}", self.chunk);
            self.chunk += 1;
            let dir = self.fp_root.join(&year).join(&month).join(&prefix);
            fs::create_dir_all(&dir).unwrap();
            let mut payload = Vec::new();
            for r in &rows {
                for v in &r.values {
                    payload.extend_from_slice(&v.to_le_bytes());
                }
            }
            let sha = sha256_hex(&payload);
            fs::write(dir.join(format!("{chunk}.bin")), &payload).unwrap();
            let index = json!({
                "schema": "grq.observations.shard/1",
                "byteOrder": "little-endian",
                "observationExtension": EXTENSION,
                "inputCount": self.input_count,
                "featureFingerprint": FINGERPRINT,
                "semanticIdentity": {},
                "prefix": prefix,
                "year": year.parse::<u16>().unwrap(),
                "month": month.parse::<u8>().unwrap(),
                "chunkId": chunk,
                "shardFile": format!("{chunk}.bin"),
                "shardBytes": payload.len(),
                "shardSha256": sha,
                "rowCount": rows.len(),
                "dates": [],
                "generatorRevision": "test",
                "createdUTC": "2026-10-04T00:00:00Z",
                "rows": rows.iter().enumerate().map(|(i, r)| json!({
                    "symbol": r.symbol, "date": r.date, "exchange": "NASDAQ", "row": i
                })).collect::<Vec<_>>(),
            });
            fs::write(
                dir.join(format!("{chunk}.index.json")),
                serde_json::to_string_pretty(&index).unwrap(),
            )
            .unwrap();
            shards.push(json!({
                "path": format!("{year}/{month}/{prefix}/{chunk}.bin"),
                "sha256": sha,
                "bytes": payload.len(),
                "rowCount": rows.len(),
                "prefix": prefix,
                "year": year.parse::<u16>().unwrap(),
                "month": month.parse::<u8>().unwrap(),
                "chunkId": chunk,
            }));
        }
        let manifest = json!({
            "schema": "grq.observations.dataset/1",
            "datasetId": dataset_id,
            "createdUTC": "2026-10-04T00:00:00Z",
            "byteOrder": "little-endian",
            "observationExtension": EXTENSION,
            "inputCount": self.input_count,
            "featureFingerprint": FINGERPRINT,
            "semanticIdentity": {},
            "generatorRevision": "test",
            "shards": shards,
            "coveredDates": [],
            "replacements": [],
            "exclusions": [],
            "missingWork": [],
            "previousDatasetId": self.previous,
            "totals": {"shardCount": 0, "rowCount": 0, "shardBytes": 0},
        });
        fs::write(
            self.fp_root
                .join("datasets")
                .join(format!("{dataset_id}.json")),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            self.fp_root.join("latest.json"),
            json!({"datasetId": dataset_id, "updatedUTC": "2026-10-04T00:00:00Z"}).to_string(),
        )
        .unwrap();
        self.previous = Some(dataset_id.to_owned());
        dataset_id.to_owned()
    }
}

/// in0, in1, in2 → h (TANH) → out; in2 also feeds out directly.
pub const CREATURE: &str = r#"{
  "input": 3, "output": 1, "forwardOnly": true,
  "neurons": [
    {"type": "hidden", "uuid": "h", "bias": 0.1, "squash": "TANH"},
    {"type": "output", "uuid": "out", "bias": -0.2, "squash": "IDENTITY"}
  ],
  "synapses": [
    {"fromUUID": "input-0", "toUUID": "h", "weight": 0.5},
    {"fromUUID": "input-1", "toUUID": "h", "weight": -1.25},
    {"fromUUID": "h", "toUUID": "out", "weight": 2.0},
    {"fromUUID": "input-2", "toUUID": "out", "weight": 0.75}
  ]
}"#;

/// The fixture creature, computed by hand.
pub fn creature_output(values: &[f32]) -> f32 {
    let h = (0.5f32 * values[0] - 1.25 * values[1] + 0.1).tanh();
    2.0 * h + 0.75 * values[2] - 0.2
}

pub fn write_creature(dir: &Path, json: &str) -> PathBuf {
    let path = dir.join("creature.json");
    fs::write(&path, json).unwrap();
    path
}
