//! Exercises the crate's public API exactly as a downstream consumer would.
//!
//! These tests are in-process and parallel-safe: no environment variables, no
//! working-directory changes, no child processes and no fixed paths or ports.

use std::path::PathBuf;

use neat_ai_predict::{MarketDate, PredictRequest, RequestError, is_valid_dataset_id};

#[test]
fn a_request_built_from_parsed_dates_validates() {
    let request = PredictRequest {
        creature: PathBuf::from("best.json"),
        archive: PathBuf::from("observations/116/b448fe6b43db398e"),
        dataset: Some("20261004T093634Z-31163-6lgt1w".to_owned()),
        from: "2007-01-01".parse::<MarketDate>().ok(),
        to: "2026-10-01".parse::<MarketDate>().ok(),
        output: PathBuf::from("predictions"),
    };
    assert_eq!(request.validate(), Ok(()));
}

#[test]
fn errors_are_typed_and_implement_std_error() {
    let request = PredictRequest {
        creature: PathBuf::from("best.json"),
        archive: PathBuf::from("observations"),
        dataset: Some("../../etc".to_owned()),
        from: None,
        to: None,
        output: PathBuf::from("out"),
    };
    let err: Box<dyn std::error::Error> = Box::new(request.validate().unwrap_err());
    assert!(
        err.to_string()
            .starts_with("--dataset '../../etc' is not a dataset id")
    );
    assert!(!is_valid_dataset_id("../../etc"));
    let _: RequestError = RequestError::EmptyPath("creature");
}

#[test]
fn market_dates_round_trip_through_display() {
    for text in ["2007-01-01", "2020-02-29", "2026-10-01"] {
        let date: MarketDate = text.parse().unwrap();
        assert_eq!(date.to_string(), text);
    }
}
