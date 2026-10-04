//! Library half of `neat_ai_predict`: bulk inference of one NEAT-AI creature
//! over a GRQ identified observation archive.
//!
//! - [`archive`] resolves a published snapshot and reads its rows, verifying
//!   every index, size and SHA-256 (issue #3).
//! - [`engine`] loads the creature, enforces the input-width contract and
//!   activates rows through `neat-core` (issue #4).
//! - [`output`] writes GRQ-format prediction partitions and the provenance
//!   manifest, atomically (issue #5).
//! - [`run`] drives one run across the rayon pool (issue #6).
//!
//! This module holds the request the command line is parsed into and the
//! validation every run performs before it touches the archive.
//! `src/main.rs` stays a thin shell that parses arguments, calls [`run::run`]
//! and maps errors to exit codes.

#![deny(missing_docs)]
#![deny(unsafe_code)]

pub mod archive;
pub mod engine;
pub mod output;
pub mod run;

use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// Longest dataset identifier accepted, in bytes.
///
/// GRQ's archive writer names a snapshot `<UTC stamp>-<pid>-<nonce>`, well
/// under this; the bound keeps a mistyped path from being treated as an id.
pub const MAX_DATASET_ID_BYTES: usize = 128;

/// A UTC market date, `YYYY-MM-DD`, as the archive keys every row.
///
/// Ordering is chronological, so a request's `from`/`to` pair can be checked
/// with `<=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MarketDate {
    year: u16,
    month: u8,
    day: u8,
}

impl MarketDate {
    /// Builds a date, refusing an impossible calendar day.
    ///
    /// # Errors
    ///
    /// Returns [`DateError::OutOfRange`] when the month or day does not exist
    /// in that year (leap years included).
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, DateError> {
        if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return Err(DateError::OutOfRange { year, month, day });
        }
        Ok(Self { year, month, day })
    }

    /// Calendar year.
    #[must_use]
    pub const fn year(self) -> u16 {
        self.year
    }

    /// Calendar month, `1..=12`.
    #[must_use]
    pub const fn month(self) -> u8 {
        self.month
    }

    /// Day of the month, `1..=31`.
    #[must_use]
    pub const fn day(self) -> u8 {
        self.day
    }
}

const fn is_leap_year(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Why a date was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateError {
    /// The text was not exactly `YYYY-MM-DD` with ASCII digits.
    Format(String),
    /// The fields parsed but name a day that does not exist.
    OutOfRange {
        /// Year as written.
        year: u16,
        /// Month as written.
        month: u8,
        /// Day as written.
        day: u8,
    },
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(text) => write!(f, "'{text}' is not a YYYY-MM-DD market date"),
            Self::OutOfRange { year, month, day } => {
                write!(f, "{year:04}-{month:02}-{day:02} is not a calendar day")
            }
        }
    }
}

impl Error for DateError {}

impl FromStr for MarketDate {
    type Err = DateError;

    /// Parses exactly `YYYY-MM-DD`; no whitespace, no other separators.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = text.as_bytes();
        let shape_ok = bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes
                .iter()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
        if !shape_ok {
            return Err(DateError::Format(text.to_owned()));
        }
        let field = |range: std::ops::Range<usize>| -> Result<u16, DateError> {
            text.get(range)
                .and_then(|s| s.parse::<u16>().ok())
                .ok_or_else(|| DateError::Format(text.to_owned()))
        };
        let year = field(0..4)?;
        let month = u8::try_from(field(5..7)?).map_err(|_| DateError::Format(text.to_owned()))?;
        let day = u8::try_from(field(8..10)?).map_err(|_| DateError::Format(text.to_owned()))?;
        Self::new(year, month, day)
    }
}

impl fmt::Display for MarketDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Whether `id` can name an archive dataset snapshot.
///
/// A dataset id becomes the file name `datasets/<id>.json`, so it must be a
/// single path component: one or more ASCII letters, digits, `.`, `_` or `-`,
/// never `.` or `..` — GRQ's `validateDatasetId` rule exactly — and at most
/// [`MAX_DATASET_ID_BYTES`] long.
#[must_use]
pub fn is_valid_dataset_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_DATASET_ID_BYTES
        && id != "."
        && id != ".."
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// One prediction run, as parsed from the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredictRequest {
    /// `CreatureExport` JSON to activate.
    pub creature: PathBuf,
    /// Archive fingerprint root: `<root>/<extension>/<fingerprint>`.
    pub archive: PathBuf,
    /// Snapshot to read; `None` reads the one `<archive>/latest.json` names.
    pub dataset: Option<String>,
    /// First market date to score, inclusive.
    pub from: Option<MarketDate>,
    /// Last market date to score, inclusive.
    pub to: Option<MarketDate>,
    /// Directory the prediction partitions and manifest are written to.
    pub output: PathBuf,
}

/// Why a [`PredictRequest`] was refused before any work started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    /// A path argument was empty.
    EmptyPath(&'static str),
    /// `--from` is after `--to`.
    DateOrder {
        /// The `--from` date.
        from: MarketDate,
        /// The `--to` date.
        to: MarketDate,
    },
    /// `--dataset` is not a single safe path component.
    DatasetId(String),
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath(flag) => write!(f, "--{flag} must name a path"),
            Self::DateOrder { from, to } => write!(f, "--from {from} is after --to {to}"),
            Self::DatasetId(id) => write!(
                f,
                "--dataset '{id}' is not a dataset id (letters, digits, '.', '_', '-'; at most {MAX_DATASET_ID_BYTES} bytes)"
            ),
        }
    }
}

impl Error for RequestError {}

impl PredictRequest {
    /// Checks the request's own consistency; it touches no file.
    ///
    /// # Errors
    ///
    /// Returns the first [`RequestError`] found, in field order.
    pub fn validate(&self) -> Result<(), RequestError> {
        for (flag, path) in [
            ("creature", &self.creature),
            ("archive", &self.archive),
            ("output", &self.output),
        ] {
            if path.as_os_str().is_empty() {
                return Err(RequestError::EmptyPath(flag));
            }
        }
        if let (Some(from), Some(to)) = (self.from, self.to)
            && from > to
        {
            return Err(RequestError::DateOrder { from, to });
        }
        if let Some(id) = &self.dataset
            && !is_valid_dataset_id(id)
        {
            return Err(RequestError::DatasetId(id.clone()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> MarketDate {
        text.parse().expect("test date")
    }

    fn request() -> PredictRequest {
        PredictRequest {
            creature: PathBuf::from("creature.json"),
            archive: PathBuf::from("observations/116/fp"),
            dataset: None,
            from: None,
            to: None,
            output: PathBuf::from("predictions"),
        }
    }

    #[test]
    fn parses_and_prints_a_market_date() {
        let d = date("2021-05-31");
        assert_eq!((d.year(), d.month(), d.day()), (2021, 5, 31));
        assert_eq!(d.to_string(), "2021-05-31");
    }

    #[test]
    fn rejects_malformed_dates() {
        for text in [
            "2021-5-31",
            "2021/05/31",
            " 2021-05-31",
            "20210531",
            "2021-05-31T00",
        ] {
            assert_eq!(
                text.parse::<MarketDate>(),
                Err(DateError::Format(text.to_owned()))
            );
        }
    }

    #[test]
    fn rejects_impossible_calendar_days() {
        assert_eq!(
            "2021-02-29".parse::<MarketDate>(),
            Err(DateError::OutOfRange {
                year: 2021,
                month: 2,
                day: 29
            })
        );
        assert!("2020-02-29".parse::<MarketDate>().is_ok());
        assert!("2100-02-29".parse::<MarketDate>().is_err());
        assert!("2000-02-29".parse::<MarketDate>().is_ok());
        assert!("2021-04-31".parse::<MarketDate>().is_err());
        assert!("2021-13-01".parse::<MarketDate>().is_err());
        assert!("2021-01-00".parse::<MarketDate>().is_err());
    }

    #[test]
    fn dates_order_chronologically() {
        assert!(date("2007-12-31") < date("2008-01-01"));
        assert!(date("2021-05-31") < date("2021-06-01"));
    }

    #[test]
    fn dataset_ids_are_single_safe_path_components() {
        assert!(is_valid_dataset_id("20261004T093634Z-31163-6lgt1w"));
        assert!(is_valid_dataset_id("a.b_c-d"));
        assert!(is_valid_dataset_id("-x"), "GRQ's rule allows a leading '-'");
        for bad in ["", ".", "..", "a/b", "a b", "a\\b", "ü"] {
            assert!(!is_valid_dataset_id(bad), "{bad:?} must be refused");
        }
        assert!(!is_valid_dataset_id(&"a".repeat(MAX_DATASET_ID_BYTES + 1)));
        assert!(is_valid_dataset_id(&"a".repeat(MAX_DATASET_ID_BYTES)));
    }

    #[test]
    fn a_plain_request_validates() {
        assert_eq!(request().validate(), Ok(()));
    }

    #[test]
    fn from_after_to_is_refused() {
        let mut r = request();
        r.from = Some(date("2021-06-01"));
        r.to = Some(date("2021-05-31"));
        assert_eq!(
            r.validate(),
            Err(RequestError::DateOrder {
                from: date("2021-06-01"),
                to: date("2021-05-31")
            })
        );
        r.to = Some(date("2021-06-01"));
        assert_eq!(r.validate(), Ok(()), "an equal pair is one day and valid");
    }

    #[test]
    fn empty_paths_and_bad_dataset_ids_are_refused() {
        let mut r = request();
        r.archive = PathBuf::new();
        assert_eq!(r.validate(), Err(RequestError::EmptyPath("archive")));
        let mut r = request();
        r.dataset = Some("../escape".to_owned());
        assert_eq!(
            r.validate(),
            Err(RequestError::DatasetId("../escape".to_owned()))
        );
    }

    #[test]
    fn errors_name_the_flag_and_the_value() {
        assert_eq!(
            RequestError::DateOrder {
                from: date("2021-06-01"),
                to: date("2021-05-31")
            }
            .to_string(),
            "--from 2021-06-01 is after --to 2021-05-31"
        );
        assert_eq!(
            RequestError::EmptyPath("output").to_string(),
            "--output must name a path"
        );
        assert_eq!(
            DateError::Format("x".to_owned()).to_string(),
            "'x' is not a YYYY-MM-DD market date"
        );
    }
}
