//! End-to-end runs through the public API (issues #4, #5, #6): fixture
//! archive in, prediction partitions and manifest out.

// Test-only crate: clippy.toml relaxes unwrap/expect inside #[test] functions
// only, and a failed fixture step here should panic with its message.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;

use common::{ArchiveBuilder, CREATURE, TempDir, creature_output, row, write_creature};
use neat_ai_predict::PredictRequest;
use neat_ai_predict::output::{OutputError, PartitionIndex, RunManifest};
use neat_ai_predict::run::{RunError, run};

fn request(tmp: &TempDir, b: &ArchiveBuilder, creature: &str) -> PredictRequest {
    PredictRequest {
        creature: write_creature(&tmp.0, creature),
        archive: b.fp_root.clone(),
        dataset: None,
        from: None,
        to: None,
        output: tmp.0.join("predictions"),
    }
}

fn quiet(_: &str) {}

fn read_outputs(dir: &std::path::Path, partition: &str) -> (PartitionIndex, Vec<f64>) {
    let index: PartitionIndex = serde_json::from_str(
        &fs::read_to_string(dir.join(format!("{partition}.index.json"))).unwrap(),
    )
    .unwrap();
    let bytes = fs::read(dir.join(format!("{partition}.bin"))).unwrap();
    let values = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| f64::from_le_bytes(*b))
        .collect();
    (index, values)
}

#[test]
fn every_row_is_predicted_and_written_with_provenance() {
    let tmp = TempDir::new("run");
    let mut b = ArchiveBuilder::new(&tmp.0, 3);
    let rows = [
        row("AAPL", "2021-05-03", &[0.0, 0.0, 0.0]),
        row("AMZN", "2021-05-03", &[1.0, -1.0, 0.5]),
        row("BHP", "2021-06-01", &[-0.3, 0.9, -1.0]),
    ];
    b.publish("ds1", &rows);
    let summary = run(&request(&tmp, &b, CREATURE), &quiet).unwrap();
    assert!(!summary.already_complete);

    let out = tmp.0.join("predictions");
    let manifest: RunManifest =
        serde_json::from_str(&fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest, summary.manifest);
    assert_eq!(manifest.row_count, 3);
    assert_eq!(manifest.identity.dataset_id, "ds1");
    assert_eq!(manifest.identity.observation_extension, common::EXTENSION);
    assert!(manifest.identity.creature_uuid.starts_with("sha256:"));
    assert!(manifest.refused.is_empty());
    let paths: Vec<&str> = manifest
        .partitions
        .iter()
        .map(|p| p.path.as_str())
        .collect();
    assert_eq!(paths, ["2021/05/A", "2021/06/B"]);

    let (index, values) = read_outputs(&out.join("2021/05"), "A");
    assert_eq!(index.schema, "grq.predictions.partition/1");
    assert_eq!(index.key, manifest.key);
    assert_eq!(
        index
            .rows
            .iter()
            .map(|r| r.symbol.as_str())
            .collect::<Vec<_>>(),
        ["AAPL", "AMZN"]
    );
    for (got, r) in values.iter().zip(&rows[..2]) {
        assert!((got - f64::from(creature_output(&r.values))).abs() < 1e-6);
    }
    // No staging directory is left behind.
    let leftovers: Vec<_> = fs::read_dir(&tmp.0)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".partial-"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn a_repeated_run_is_a_no_op_and_a_different_run_is_refused() {
    let tmp = TempDir::new("repeat");
    let mut b = ArchiveBuilder::new(&tmp.0, 3);
    b.publish("ds1", &[row("AAPL", "2021-05-03", &[0.1, 0.2, 0.3])]);
    let req = request(&tmp, &b, CREATURE);
    let first = run(&req, &quiet).unwrap();
    let again = run(&req, &quiet).unwrap();
    assert!(again.already_complete);
    assert_eq!(again.manifest, first.manifest);

    // A new snapshot is a different identity: the old output is never overwritten.
    b.publish("ds2", &[row("AAPL", "2021-05-04", &[0.1, 0.2, 0.3])]);
    let err = run(&req, &quiet).unwrap_err();
    assert!(
        matches!(err, RunError::Output(OutputError::Occupied { .. })),
        "{err}"
    );
}

#[test]
fn a_narrower_creature_runs_against_a_wider_archive() {
    let tmp = TempDir::new("narrow");
    let mut b = ArchiveBuilder::new(&tmp.0, 5);
    b.publish(
        "ds1",
        &[row("AAPL", "2021-05-03", &[1.0, -1.0, 0.5, 42.0, -42.0])],
    );
    let summary = run(&request(&tmp, &b, CREATURE), &quiet).unwrap();
    assert_eq!(summary.manifest.identity.input_count, 5);
    assert_eq!(summary.manifest.creature_input, 3);
    let (_, values) = read_outputs(&tmp.0.join("predictions/2021/05"), "A");
    assert!((values[0] - f64::from(creature_output(&[1.0, -1.0, 0.5]))).abs() < 1e-6);
}

#[test]
fn a_wider_creature_is_refused_and_leaves_no_output() {
    let tmp = TempDir::new("wide");
    let mut b = ArchiveBuilder::new(&tmp.0, 2);
    b.publish("ds1", &[row("AAPL", "2021-05-03", &[1.0, 2.0])]);
    let err = run(&request(&tmp, &b, CREATURE), &quiet).unwrap_err();
    assert!(matches!(err, RunError::Engine(_)), "{err}");
    assert!(!tmp.0.join("predictions").exists());
}

#[test]
fn a_non_finite_observation_fails_the_run_as_archive_corruption() {
    // GRQ never archives one; neat-core would clamp it into a finite,
    // plausible-looking prediction, so it must never reach the engine.
    let tmp = TempDir::new("nonfinite");
    let mut b = ArchiveBuilder::new(&tmp.0, 3);
    b.publish(
        "ds1",
        &[
            row("AAPL", "2021-05-03", &[0.0, 0.0, f32::INFINITY]),
            row("AMZN", "2021-05-03", &[0.0, 0.0, 0.5]),
        ],
    );
    let err = run(&request(&tmp, &b, CREATURE), &quiet).unwrap_err();
    let RunError::Archive(err) = err else {
        panic!("expected an archive fault, got {err}");
    };
    assert!(err.detail.contains("AAPL@2021-05-03 input[2]"), "{err}");
    assert!(!tmp.0.join("predictions").exists());
}

#[test]
fn a_corrupt_shard_fails_the_run_and_leaves_no_output() {
    let tmp = TempDir::new("corrupt");
    let mut b = ArchiveBuilder::new(&tmp.0, 3);
    b.publish("ds1", &[row("AAPL", "2021-05-03", &[0.1, 0.2, 0.3])]);
    fs::write(b.fp_root.join("2021/05/A/000000.bin"), [0u8; 12]).unwrap();
    let err = run(&request(&tmp, &b, CREATURE), &quiet).unwrap_err();
    assert!(matches!(err, RunError::Archive(_)), "{err}");
    assert!(!tmp.0.join("predictions").exists());
    let leftovers = fs::read_dir(&tmp.0)
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().contains(".partial-"));
    assert!(!leftovers, "a failed run cleans up its staging directory");
}
