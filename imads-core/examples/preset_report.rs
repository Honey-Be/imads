use std::fmt::Debug;
use std::sync::Arc;
use std::time::Instant;

use imads_core::core::{DefaultBundle, Engine, ToyEvaluator};
use imads_core::presets::Preset;
use imads_core::types::{Env, XMesh};

fn report_env() -> Env {
    Env {
        run_id: 101,
        config_hash: 202,
        data_snapshot_id: 303,
        rng_master_seed: 404,
    }
}

fn format_optional<T: Debug + Clone, F: FnOnce(&T) -> String>(input: Option<T>, fmt: F) -> String {
    if let Some(elem) = input {
        fmt(&elem)
    } else {
        "None".to_string()
    }
}

fn format_xmesh(x: &XMesh) -> String {
    let coords =
        x.0.iter()
            .map(i64::to_string)
            .collect::<Vec<String>>()
            .join(", ");
    format!("({coords})")
}

fn main() {
    let workers: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let env = report_env();
    println!(
        "preset,workers,elapsed_ms,truth_evals,partial_steps,cheap_rejects,invalid_eval_rejects,f_best,x_best"
    );
    for preset in Preset::ALL {
        let cfg = preset.config();
        let evaluator = Arc::new(ToyEvaluator {
            m: cfg.num_constraints,
            dim: cfg.search_dim.unwrap_or(4),
        });
        let mut engine = Engine::<DefaultBundle>::default();
        let t0 = Instant::now();
        let out = engine.run_with_evaluator(&cfg, &env, workers, evaluator);
        let dt = t0.elapsed().as_secs_f64() * 1e3;
        println!(
            "{},{},{:.3},{},{},{},{},{},{:?}",
            preset.name(),
            workers,
            dt,
            out.stats.truth_evals,
            out.stats.partial_steps,
            out.stats.cheap_rejects,
            out.stats.invalid_eval_rejects,
            format_optional(out.f_best, f64::to_string),
            format_optional(out.x_best, format_xmesh),
        );
    }
}
