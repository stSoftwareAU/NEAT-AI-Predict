//! Creature loading, the input-width contract and activation (issue #4).
//!
//! Activation goes through `neat-core`, the engine the fleet's `rust_scorer`
//! scores with (GRQ #4280: one scoring engine per fleet). There is no
//! fallback engine.
//!
//! **Which `neat-core` path.** Every row goes through
//! `CompiledNetwork::activate_into`, the scalar single-record kernel, with the
//! rows spread across the rayon pool. `neat-core` also has a batched SIMD
//! kernel (`score_records_parallel_flat`), but it evaluates squashes such as
//! `TANH` with its own vector approximation and so differs from the scalar
//! kernel by a few ULP. Measured with GRQ's production cluster creature
//! (2 511 inputs, 8 236 neurons) on 2 000 rows against
//! `@stsoftware/neat-ai` 7.0.48 `creature.activate` — the call GRQ's daily
//! scoring makes — the scalar kernel agreed bit for bit on 1 879 outputs (max
//! |diff| 1.9e-6) and the batched one on 1 654 (max 2.4e-6). The remainder is
//! native `libm` against JavaScript `Math` in WASM, about 3e-7 relative. The
//! scalar kernel is also the one `neat-core`'s own `activate` uses, so the bulk
//! path here is bit-identical to `neat-core`'s reference activation.
//! `parity/run.sh` re-measures this against neat-ai.
//!
//! Contract rules — each refusal is typed, none is defaulted:
//!
//! - The creature's top-level `input` / `output` are authoritative and must be
//!   at least 1 (NEAT-AI-core #550); `neat-core` refuses anything else on
//!   parse.
//! - A creature **narrower** than the archive is run as if extended with
//!   unconnected inputs — GRQ's "extend, never contract" rule. Each record is
//!   handed to `neat-core` at the archive's full width and only the first
//!   `input` values reach an input neuron, which is exactly what an
//!   unconnected extra input contributes: nothing.
//! - A creature **wider** than the archive is refused: feeding it would mean
//!   inventing observations the archive does not hold.
//! - Only a `forwardOnly` creature is accepted. A recurrent creature carries
//!   state from one activation to the next, so its predictions would depend on
//!   the order rows happen to be scored in.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use neat_core::creature::{CreatureExport, compile_creature, parse_creature_json};
use neat_core::network::CompiledNetwork;

use rayon::prelude::*;

use crate::archive::sha256_hex;

/// Rows one rayon task activates before work-stealing can rebalance.
pub const ROWS_PER_TASK: usize = 16;

/// Name of the activation path every prediction is produced by.
pub const ENGINE: &str = "neat-core/CompiledNetwork::activate_into";

/// Resolved `neat-core` version, read from `Cargo.lock` at build time.
pub const ENGINE_VERSION: &str = env!("NEAT_CORE_VERSION");

/// Resolved `neat-core` source (git URL, tag and commit), from `Cargo.lock`.
pub const ENGINE_SOURCE: &str = env!("NEAT_CORE_SOURCE");

/// Why a creature cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The creature file could not be read.
    Io {
        /// The file.
        path: PathBuf,
        /// The operating-system error.
        detail: String,
    },
    /// `neat-core` refused the creature (bad JSON, width, topology).
    Creature {
        /// The file.
        path: PathBuf,
        /// `neat-core`'s own error text.
        detail: String,
    },
    /// The creature is recurrent, so its predictions would depend on row order.
    NotForwardOnly,
    /// The creature consumes more inputs than the archive holds per row.
    WiderThanArchive {
        /// The creature's `input`.
        creature: usize,
        /// The archive's `inputCount`.
        archive: usize,
    },
    /// A batch is not a whole number of archive-width rows.
    BatchShape {
        /// Values supplied.
        values: usize,
        /// Values per row.
        stride: usize,
    },
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, detail } => {
                write!(f, "cannot read creature {}: {detail}", path.display())
            }
            Self::Creature { path, detail } => {
                write!(f, "creature {} refused: {detail}", path.display())
            }
            Self::NotForwardOnly => write!(
                f,
                "the creature is not forwardOnly: a recurrent creature's predictions would depend on row order"
            ),
            Self::WiderThanArchive { creature, archive } => write!(
                f,
                "the creature consumes {creature} inputs but the archive holds {archive} per row; observations extend, never contract — score it against an archive assembled under its own semantics"
            ),
            Self::BatchShape { values, stride } => write!(
                f,
                "a batch of {values} values is not a whole number of {stride}-value rows"
            ),
        }
    }
}

impl std::error::Error for EngineError {}

/// A creature file, parsed and validated, with the hash that identifies it.
#[derive(Debug, Clone)]
pub struct CreatureFile {
    /// Where it was read from.
    pub path: PathBuf,
    /// SHA-256 of the file bytes — the creature's identity in every manifest.
    pub sha256: String,
    /// The parsed export.
    pub export: CreatureExport,
}

impl CreatureFile {
    /// The identity recorded as `creatureUUID`: `sha256:<file hash>`.
    #[must_use]
    pub fn identity(&self) -> String {
        format!("sha256:{}", self.sha256)
    }
}

/// Reads and parses a `CreatureExport` JSON file.
///
/// # Errors
///
/// [`EngineError::Io`] when the file cannot be read, [`EngineError::Creature`]
/// when `neat-core` refuses its contents.
pub fn load_creature(path: &Path) -> Result<CreatureFile, EngineError> {
    let bytes = fs::read(path).map_err(|err| EngineError::Io {
        path: path.to_path_buf(),
        detail: err.to_string(),
    })?;
    let text = String::from_utf8(bytes).map_err(|err| EngineError::Creature {
        path: path.to_path_buf(),
        detail: format!("not UTF-8: {err}"),
    })?;
    let export = parse_creature_json(&text).map_err(|err| EngineError::Creature {
        path: path.to_path_buf(),
        detail: err.to_string(),
    })?;
    Ok(CreatureFile {
        path: path.to_path_buf(),
        sha256: sha256_hex(text.as_bytes()),
        export,
    })
}

/// A compiled creature bound to an archive's row width.
#[derive(Clone)]
pub struct Predictor {
    network: CompiledNetwork,
    input_count: usize,
    output_count: usize,
    stride: usize,
}

impl fmt::Debug for Predictor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Predictor")
            .field("input_count", &self.input_count)
            .field("output_count", &self.output_count)
            .field("stride", &self.stride)
            .finish_non_exhaustive()
    }
}

impl Predictor {
    /// Compiles `creature` for rows of `archive_input_count` values.
    ///
    /// # Errors
    ///
    /// [`EngineError::NotForwardOnly`], [`EngineError::WiderThanArchive`], or
    /// [`EngineError::Creature`] when `neat-core` cannot compile it.
    pub fn new(creature: &CreatureFile, archive_input_count: usize) -> Result<Self, EngineError> {
        let export = &creature.export;
        if !export.forward_only {
            return Err(EngineError::NotForwardOnly);
        }
        if export.input > archive_input_count {
            return Err(EngineError::WiderThanArchive {
                creature: export.input,
                archive: archive_input_count,
            });
        }
        let network = compile_creature(export).map_err(|err| EngineError::Creature {
            path: creature.path.clone(),
            detail: err.to_string(),
        })?;
        Ok(Self {
            network,
            input_count: export.input,
            output_count: export.output,
            stride: archive_input_count,
        })
    }

    /// The creature's own input count.
    #[must_use]
    pub const fn input_count(&self) -> usize {
        self.input_count
    }

    /// Outputs per row.
    #[must_use]
    pub const fn output_count(&self) -> usize {
        self.output_count
    }

    /// Values per archive row.
    #[must_use]
    pub const fn stride(&self) -> usize {
        self.stride
    }

    /// Activates every row of `flat` (row-major, [`Self::stride`] values per
    /// row) across the rayon pool; returns [`Self::output_count`] values per
    /// row, in row order.
    ///
    /// Each rayon task owns a clone of the compiled network (its activation
    /// buffers are per-call scratch) and works through [`ROWS_PER_TASK`] rows,
    /// so output order and values do not depend on the thread count.
    ///
    /// # Errors
    ///
    /// [`EngineError::BatchShape`] when `flat` is not whole rows.
    pub fn predict(&self, flat: &[f32]) -> Result<Vec<f32>, EngineError> {
        if !flat.len().is_multiple_of(self.stride) {
            return Err(EngineError::BatchShape {
                values: flat.len(),
                stride: self.stride,
            });
        }
        if flat.is_empty() {
            return Ok(Vec::new());
        }
        let mut outputs = vec![0.0f32; flat.len() / self.stride * self.output_count];
        outputs
            .par_chunks_mut(self.output_count * ROWS_PER_TASK)
            .zip(flat.par_chunks(self.stride * ROWS_PER_TASK))
            .for_each_init(
                || self.network.clone(),
                |network, (out, rows)| {
                    for (row_out, row) in out
                        .chunks_exact_mut(self.output_count)
                        .zip(rows.chunks_exact(self.stride))
                    {
                        network.activate_into(row, row_out);
                    }
                },
            );
        Ok(outputs)
    }

    /// Activates one row the single-record way (`CompiledNetwork::activate`).
    ///
    /// The bulk path must agree with this bit for bit; it exists so that
    /// parity is checked against `neat-core`'s reference activation rather
    /// than against itself.
    #[must_use]
    pub fn activate_one(&self, row: &[f32]) -> Vec<f32> {
        let mut network = self.network.clone();
        network.activate(row, self.output_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// in0, in1, in2 → h (TANH, bias 0.1) → out (IDENTITY, bias -0.2);
    /// in2 also feeds the output directly.
    pub(crate) const FIXTURE: &str = r#"{
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

    fn creature(json: &str) -> CreatureFile {
        CreatureFile {
            path: PathBuf::from("fixture.json"),
            sha256: sha256_hex(json.as_bytes()),
            export: parse_creature_json(json).unwrap(),
        }
    }

    fn expected(row: &[f32]) -> f32 {
        let h = (0.5f32 * row[0] - 1.25 * row[1] + 0.1).tanh();
        2.0 * h + 0.75 * row[2] - 0.2
    }

    #[test]
    fn bulk_matches_the_hand_computed_network() {
        let predictor = Predictor::new(&creature(FIXTURE), 3).unwrap();
        let rows: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, -1.0, 0.5], [-0.3, 0.9, -1.0]];
        let flat: Vec<f32> = rows.iter().flatten().copied().collect();
        let out = predictor.predict(&flat).unwrap();
        for (row, got) in rows.iter().zip(&out) {
            assert!(
                (got - expected(row)).abs() < 1e-6,
                "{got} vs {}",
                expected(row)
            );
        }
    }

    #[test]
    fn bulk_is_bit_identical_to_single_record_activation() {
        let predictor = Predictor::new(&creature(FIXTURE), 3).unwrap();
        // More rows than one rayon task, with a short final task.
        let flat: Vec<f32> = (0..3 * 203)
            .map(|i| ((i * 37 % 101) as f32 / 50.0) - 1.0)
            .collect();
        let bulk = predictor.predict(&flat).unwrap();
        for (i, row) in flat.as_chunks::<3>().0.iter().enumerate() {
            let single = predictor.activate_one(row);
            assert_eq!(bulk[i].to_bits(), single[0].to_bits(), "row {i}");
        }
    }

    #[test]
    fn a_narrower_creature_ignores_the_extra_archive_values() {
        let predictor = Predictor::new(&creature(FIXTURE), 5).unwrap();
        let row = [1.0f32, -1.0, 0.5];
        let flat = [1.0f32, -1.0, 0.5, 99.0, -99.0];
        let out = predictor.predict(&flat).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].to_bits(),
            Predictor::new(&creature(FIXTURE), 3)
                .unwrap()
                .predict(&row)
                .unwrap()[0]
                .to_bits()
        );
    }

    #[test]
    fn a_wider_creature_is_refused() {
        assert_eq!(
            Predictor::new(&creature(FIXTURE), 2).unwrap_err(),
            EngineError::WiderThanArchive {
                creature: 3,
                archive: 2
            }
        );
    }

    #[test]
    fn a_recurrent_creature_is_refused() {
        let json = FIXTURE.replace("\"forwardOnly\": true", "\"forwardOnly\": false");
        assert_eq!(
            Predictor::new(&creature(&json), 3).unwrap_err(),
            EngineError::NotForwardOnly
        );
    }

    #[test]
    fn widthless_creatures_are_refused_on_parse() {
        for json in [
            FIXTURE.replace("\"input\": 3", "\"input\": 0"),
            FIXTURE.replace("\"output\": 1", "\"output\": 0"),
            FIXTURE.replace("\"input\": 3, ", ""),
        ] {
            assert!(parse_creature_json(&json).is_err(), "{json}");
        }
    }

    #[test]
    fn a_ragged_batch_is_refused() {
        let predictor = Predictor::new(&creature(FIXTURE), 3).unwrap();
        assert_eq!(
            predictor.predict(&[0.0; 4]).unwrap_err(),
            EngineError::BatchShape {
                values: 4,
                stride: 3
            }
        );
        assert!(predictor.predict(&[]).unwrap().is_empty());
    }

    #[test]
    fn the_engine_version_is_resolved_from_the_lockfile() {
        assert!(!ENGINE_VERSION.is_empty());
        assert!(ENGINE_SOURCE.contains("NEAT-AI-core"));
    }
}
