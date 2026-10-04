//! Activation throughput, in rows per second (issue #6).
//!
//! Run on demand on an otherwise idle machine — never in `./quality.sh` or CI:
//!
//! ```text
//! cargo bench --bench predict                                  # synthetic creature
//! cargo bench --bench predict -- --creature <creature.json>    # a real one
//! cargo bench --bench predict -- --rows 50000
//! ```
//!
//! The synthetic creature has the shape of GRQ's production cluster creature
//! (2 511 inputs, 8 236 non-input neurons, ~56 000 synapses), wired
//! feed-forward with deterministic weights. Rows are deterministic
//! pseudo-random values in `[-1, 1]`. Prints rows/s on one thread and on the
//! whole pool, so the parallel speed-up is visible.

// Test-only crate: clippy.toml relaxes unwrap/expect inside #[test] functions
// only, and a failed fixture step here should panic with its message.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::Instant;

use neat_ai_predict::engine::{CreatureFile, Predictor, load_creature};

const INPUTS: usize = 2511;
const HIDDEN: usize = 8235;
const FAN_IN: usize = 7;

fn synthetic_creature() -> CreatureFile {
    let mut seed = 0x5eed_u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut neurons = Vec::with_capacity(HIDDEN + 1);
    let mut synapses = Vec::new();
    let squashes = ["TANH", "IDENTITY", "ReLU", "LOGISTIC"];
    for h in 0..HIDDEN {
        let squash = squashes[(next() % 4) as usize];
        neurons.push(format!(
            r#"{{"type":"hidden","uuid":"h{h}","bias":{:.6},"squash":"{squash}"}}"#,
            (next() % 2000) as f64 / 1000.0 - 1.0
        ));
        for _ in 0..FAN_IN {
            // Feed-forward: a hidden neuron reads inputs or earlier neurons only.
            let from = (next() as usize) % (INPUTS + h);
            let from_uuid = if from < INPUTS {
                format!("input-{from}")
            } else {
                format!("h{}", from - INPUTS)
            };
            synapses.push((from_uuid, format!("h{h}")));
        }
    }
    neurons.push(r#"{"type":"output","uuid":"out","bias":0.0,"squash":"TANH"}"#.to_owned());
    for h in (0..HIDDEN).step_by(97) {
        synapses.push((format!("h{h}"), "out".to_owned()));
    }
    synapses.sort();
    synapses.dedup();
    let synapse_json: Vec<String> = synapses
        .iter()
        .map(|(from, to)| {
            format!(
                r#"{{"fromUUID":"{from}","toUUID":"{to}","weight":{:.6}}}"#,
                (next() % 2000) as f64 / 1000.0 - 1.0
            )
        })
        .collect();
    let json = format!(
        r#"{{"input":{INPUTS},"output":1,"forwardOnly":true,"neurons":[{}],"synapses":[{}]}}"#,
        neurons.join(","),
        synapse_json.join(",")
    );
    let path =
        std::env::temp_dir().join(format!("neat-ai-predict-bench-{}.json", std::process::id()));
    std::fs::write(&path, json).expect("write synthetic creature");
    let creature = load_creature(&path).expect("synthetic creature parses");
    let _ = std::fs::remove_file(&path);
    creature
}

fn rows(count: usize, width: usize) -> Vec<f32> {
    let mut state = 4780_u32;
    (0..count * width)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}

fn measure(label: &str, predictor: &Predictor, flat: &[f32], threads: usize) {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("thread pool");
    let count = flat.len() / predictor.stride();
    // One warm-up pass, then the timed pass.
    pool.install(|| predictor.predict(flat)).expect("warm-up");
    let started = Instant::now();
    let outputs = pool.install(|| predictor.predict(flat)).expect("predict");
    let secs = started.elapsed().as_secs_f64();
    assert_eq!(outputs.len(), count * predictor.output_count());
    eprintln!(
        "{label:<12} {threads:>3} thread(s): {count} rows in {secs:.3}s = {:>10.0} rows/s",
        count as f64 / secs
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
    };
    let count: usize = value("--rows").map_or(20_000, |v| v.parse().expect("--rows"));
    let (label, creature) = match value("--creature") {
        Some(path) => (
            "creature",
            load_creature(&PathBuf::from(path)).expect("--creature parses"),
        ),
        None => ("synthetic", synthetic_creature()),
    };
    let width = creature.export.input;
    eprintln!(
        "{label}: {} inputs, {} neurons, {} synapses; {count} rows",
        creature.export.input,
        creature.export.neurons.len(),
        creature.export.synapses.len()
    );
    let predictor = Predictor::new(&creature, width).expect("compile");
    let flat = rows(count, width);
    measure(label, &predictor, &flat, 1);
    measure(label, &predictor, &flat, rayon::current_num_threads());
}
