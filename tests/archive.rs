//! The archive reader against GRQ-format fixture archives (issue #3).
//!
//! In-process; each test builds its own archive in a private temporary
//! directory, so the tests are parallel-safe.

// Test-only crate: clippy.toml relaxes unwrap/expect inside #[test] functions
// only, and a failed fixture step here should panic with its message.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;

use common::{ArchiveBuilder, TempDir, row};
use neat_ai_predict::MarketDate;
use neat_ai_predict::archive::{ArchiveErrorKind, DateRange, open, read_partition};

fn d(s: &str) -> MarketDate {
    s.parse().unwrap()
}

#[test]
fn resolves_rows_by_partition_in_symbol_date_order() {
    let tmp = TempDir::new("resolve");
    let mut b = ArchiveBuilder::new(&tmp.0, 3);
    b.publish(
        "ds1",
        &[
            row("AMZN", "2021-05-03", &[1.0, 2.0, 3.0]),
            row("AAPL", "2021-05-03", &[0.0, -0.5, 1.0]),
            row("BHP", "2021-06-01", &[0.25, 0.5, 0.75]),
            row("^GSPC", "2021-05-03", &[9.0, 9.0, 9.0]),
        ],
    );
    let archive = open(&b.fp_root, None, DateRange::default()).unwrap();
    assert_eq!(archive.dataset_id, "ds1");
    assert_eq!(archive.semantics.input_count, 3);
    assert_eq!(archive.row_count(), 4);
    let ids: Vec<String> = archive.partitions.iter().map(|p| p.id()).collect();
    assert_eq!(ids, ["2021/05/A", "2021/05/_", "2021/06/B"]);

    let a = &archive.partitions[0];
    let symbols: Vec<&str> = a.rows.iter().map(|r| r.identity.symbol.as_str()).collect();
    assert_eq!(symbols, ["AAPL", "AMZN"]);
    assert_eq!(a.rows[0].identity.exchange.as_deref(), Some("NASDAQ"));
    // Exact f32 round trip, including zero and negatives, in row order.
    assert_eq!(
        read_partition(&archive, a).unwrap(),
        [0.0, -0.5, 1.0, 1.0, 2.0, 3.0]
    );
}

#[test]
fn a_later_publication_restating_a_row_supersedes_it() {
    let tmp = TempDir::new("supersede");
    let mut b = ArchiveBuilder::new(&tmp.0, 1);
    b.publish(
        "ds1",
        &[
            row("AAPL", "2021-05-03", &[1.0]),
            row("AMZN", "2021-05-03", &[2.0]),
        ],
    );
    b.publish(
        "ds2",
        &[
            row("AAPL", "2021-05-03", &[7.0]),
            row("AAPL", "2021-05-04", &[3.0]),
        ],
    );

    let latest = open(&b.fp_root, None, DateRange::default()).unwrap();
    assert_eq!(latest.dataset_chain, ["ds2", "ds1"]);
    assert_eq!(latest.superseded_rows, 1);
    assert_eq!(latest.row_count(), 3);
    assert_eq!(
        read_partition(&latest, &latest.partitions[0]).unwrap(),
        [7.0, 3.0, 2.0]
    );

    // Pinning the older snapshot reads it as it was.
    let pinned = open(&b.fp_root, Some("ds1"), DateRange::default()).unwrap();
    assert_eq!(pinned.row_count(), 2);
    assert_eq!(
        read_partition(&pinned, &pinned.partitions[0]).unwrap(),
        [1.0, 2.0]
    );
}

#[test]
fn the_date_range_is_inclusive() {
    let tmp = TempDir::new("range");
    let mut b = ArchiveBuilder::new(&tmp.0, 1);
    b.publish(
        "ds1",
        &[
            row("A", "2021-04-30", &[1.0]),
            row("A", "2021-05-01", &[2.0]),
            row("A", "2021-05-31", &[3.0]),
            row("A", "2021-06-01", &[4.0]),
        ],
    );
    let range = DateRange {
        from: Some(d("2021-05-01")),
        to: Some(d("2021-05-31")),
    };
    let archive = open(&b.fp_root, None, range).unwrap();
    assert_eq!(archive.row_count(), 2);
    assert_eq!(archive.partitions.len(), 1);
}

#[test]
fn a_tampered_payload_fails_its_hash() {
    let tmp = TempDir::new("hash");
    let mut b = ArchiveBuilder::new(&tmp.0, 1);
    b.publish("ds1", &[row("AAPL", "2021-05-03", &[1.0])]);
    let bin = b.fp_root.join("2021/05/A/000000.bin");
    fs::write(&bin, 2.0f32.to_le_bytes()).unwrap();
    let archive = open(&b.fp_root, None, DateRange::default()).unwrap();
    let err = read_partition(&archive, &archive.partitions[0]).unwrap_err();
    assert_eq!(err.kind, ArchiveErrorKind::Hash);
    assert_eq!(err.path, bin);
}

#[test]
fn a_truncated_payload_is_refused() {
    let tmp = TempDir::new("truncated");
    let mut b = ArchiveBuilder::new(&tmp.0, 2);
    b.publish("ds1", &[row("AAPL", "2021-05-03", &[1.0, 2.0])]);
    fs::write(b.fp_root.join("2021/05/A/000000.bin"), 1.0f32.to_le_bytes()).unwrap();
    let archive = open(&b.fp_root, None, DateRange::default()).unwrap();
    let err = read_partition(&archive, &archive.partitions[0]).unwrap_err();
    assert_eq!(err.kind, ArchiveErrorKind::Corrupt);
}

fn edit_json(path: &std::path::Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let mut v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    edit(&mut v);
    fs::write(path, v.to_string()).unwrap();
}

#[test]
fn index_faults_are_refused_at_open() {
    type Case = (&'static str, fn(&mut serde_json::Value), ArchiveErrorKind);
    let cases: [Case; 7] = [
        (
            "schema",
            |v| v["schema"] = "grq.observations.shard/2".into(),
            ArchiveErrorKind::Schema,
        ),
        (
            "width",
            |v| v["inputCount"] = 2.into(),
            ArchiveErrorKind::Semantics,
        ),
        (
            "fingerprint",
            |v| v["featureFingerprint"] = "other".into(),
            ArchiveErrorKind::Semantics,
        ),
        (
            "row-count",
            |v| v["rowCount"] = 2.into(),
            ArchiveErrorKind::Corrupt,
        ),
        (
            "shard-file",
            |v| v["shardFile"] = "../x.bin".into(),
            ArchiveErrorKind::Corrupt,
        ),
        (
            "partition",
            |v| v["rows"][0]["date"] = "2021-06-03".into(),
            ArchiveErrorKind::Corrupt,
        ),
        (
            "ordinal",
            |v| v["rows"][0]["row"] = 5.into(),
            ArchiveErrorKind::Corrupt,
        ),
    ];
    for (label, edit, kind) in cases {
        let tmp = TempDir::new(label);
        let mut b = ArchiveBuilder::new(&tmp.0, 1);
        b.publish("ds1", &[row("AAPL", "2021-05-03", &[1.0])]);
        edit_json(&b.fp_root.join("2021/05/A/000000.index.json"), edit);
        let err = open(&b.fp_root, None, DateRange::default()).unwrap_err();
        assert_eq!(err.kind, kind, "{label}: {err}");
    }
}

#[test]
fn manifest_faults_are_refused_at_open() {
    let tmp = TempDir::new("traversal");
    let mut b = ArchiveBuilder::new(&tmp.0, 1);
    b.publish("ds1", &[row("AAPL", "2021-05-03", &[1.0])]);
    edit_json(&b.fp_root.join("datasets/ds1.json"), |v| {
        v["shards"][0]["path"] = "../../escape.bin".into();
    });
    assert_eq!(
        open(&b.fp_root, None, DateRange::default())
            .unwrap_err()
            .kind,
        ArchiveErrorKind::Corrupt
    );

    let tmp = TempDir::new("loop");
    let mut b = ArchiveBuilder::new(&tmp.0, 1);
    b.publish("ds1", &[row("AAPL", "2021-05-03", &[1.0])]);
    b.publish("ds2", &[row("AAPL", "2021-05-04", &[1.0])]);
    edit_json(&b.fp_root.join("datasets/ds1.json"), |v| {
        v["previousDatasetId"] = "ds2".into()
    });
    let err = open(&b.fp_root, None, DateRange::default()).unwrap_err();
    assert!(err.detail.contains("loops"), "{err}");

    let tmp = TempDir::new("pointer");
    let mut b = ArchiveBuilder::new(&tmp.0, 1);
    b.publish("ds1", &[row("AAPL", "2021-05-03", &[1.0])]);
    fs::write(
        b.fp_root.join("latest.json"),
        r#"{"datasetId": "../../etc"}"#,
    )
    .unwrap();
    assert_eq!(
        open(&b.fp_root, None, DateRange::default())
            .unwrap_err()
            .kind,
        ArchiveErrorKind::Corrupt
    );
}

#[test]
fn the_root_must_be_the_fingerprint_root_the_snapshot_names() {
    let tmp = TempDir::new("root");
    let mut b = ArchiveBuilder::new(&tmp.0, 1);
    b.publish("ds1", &[row("AAPL", "2021-05-03", &[1.0])]);
    let moved = tmp.0.join("117").join(common::FINGERPRINT);
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::rename(&b.fp_root, &moved).unwrap();
    assert_eq!(
        open(&moved, None, DateRange::default()).unwrap_err().kind,
        ArchiveErrorKind::Semantics
    );
}

#[test]
fn a_missing_archive_is_an_io_fault() {
    let tmp = TempDir::new("missing");
    let err = open(&tmp.0.join("116/none"), None, DateRange::default()).unwrap_err();
    assert_eq!(err.kind, ArchiveErrorKind::Io);
}
