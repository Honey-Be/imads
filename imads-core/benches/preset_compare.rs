use std::sync::Arc;
use std::time::{Duration, Instant};

use imads_core::core::{DefaultBundle, Engine, ToyEvaluator};
use imads_core::presets::Preset;
use imads_core::types::Env;

fn bench_env() -> Env {
    Env {
        run_id: 11,
        config_hash: 22,
        data_snapshot_id: 33,
        rng_master_seed: 44,
    }
}

fn run_once(preset: Preset, workers: usize) -> (Duration, imads_core::core::engine::EngineOutput) {
    let cfg = preset.config();
    let env = bench_env();
    let evaluator = Arc::new(ToyEvaluator {
        m: cfg.num_constraints,
        dim: cfg.search_dim.unwrap_or(4)
    });
    let start = Instant::now();
    let mut engine = Engine::<DefaultBundle>::default();
    let out = engine.run_with_evaluator(&cfg, &env, workers, evaluator);
    (start.elapsed(), out)
}

fn median_ms(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs[xs.len() / 2]
}

fn main() {
    let reps = 5usize;
    println!(
        "preset,workers,rep,elapsed_ms,truth_evals,partial_steps,cheap_rejects,invalid_eval_rejects,cache_hits,f_best"
    );

    // Explicit before/after compare: old baseline vs current recommended default.
    for preset in [Preset::LegacyBaseline, Preset::Balanced] {
        for rep in 0..reps {
            let (elapsed, out) = run_once(preset, 1);
            let hits = out.stats.truth_eval_cache_hits + out.stats.truth_decision_cache_hits;
            println!(
                "{},{},{},{:.3},{},{},{},{},{},{}",
                preset.name(),
                1,
                rep,
                elapsed.as_secs_f64() * 1e3,
                out.stats.truth_evals,
                out.stats.partial_steps,
                out.stats.cheap_rejects,
                out.stats.invalid_eval_rejects,
                hits,
                out.f_best.unwrap_or(f64::NAN),
            );
        }
    }

    eprintln!("\n# preset matrix summary (median of {} cold runs)", reps);
    eprintln!(
        "preset,workers,median_ms,truth_evals,partial_steps,cheap_rejects,invalid_eval_rejects,f_best"
    );
    for preset in Preset::ALL {
        let mut elapsed = Vec::new();
        let mut truth = Vec::new();
        let mut partial = Vec::new();
        let mut cheap = Vec::new();
        let mut invalid = Vec::new();
        let mut best = Vec::new();
        for _ in 0..reps {
            let (dt, out) = run_once(preset, 1);
            elapsed.push(dt.as_secs_f64() * 1e3);
            truth.push(out.stats.truth_evals as f64);
            partial.push(out.stats.partial_steps as f64);
            cheap.push(out.stats.cheap_rejects as f64);
            invalid.push(out.stats.invalid_eval_rejects as f64);
            best.push(out.f_best.unwrap_or(f64::NAN));
        }
        let mean = |xs: &[f64]| xs.iter().sum::<f64>() / xs.len() as f64;
        eprintln!(
            "{},{},{:.3},{:.1},{:.1},{:.1},{:.1},{:.6}",
            preset.name(),
            1,
            median_ms(elapsed),
            mean(&truth),
            mean(&partial),
            mean(&cheap),
            mean(&invalid),
            mean(&best),
        );
    }
}
