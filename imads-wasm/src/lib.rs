//! WASI Component Model bindings for IMADS via wit-bindgen.

wit_bindgen::generate!({
    world: "imads",
    path: "wit/imads.wit",
});

use imads_core::core::engine::Engine;
use imads_core::core::DefaultBundle;
use imads_core::presets::Preset;
use imads_core::types::Env as CoreEnv;

use exports::honey_be::imads::engine::Guest;
use honey_be::imads::types::{Env, Output, Preset as WitPreset, Stats};

struct ImadsEngine;

impl Guest for ImadsEngine {
    fn run(preset: WitPreset, env: Env, workers: u32) -> Output {
        let core_preset = match preset {
            WitPreset::LegacyBaseline => Preset::LegacyBaseline,
            WitPreset::Balanced => Preset::Balanced,
            WitPreset::Conservative => Preset::Conservative,
            WitPreset::Throughput => Preset::Throughput,
        };

        let core_env = CoreEnv {
            run_id: env.run_id as u128,
            config_hash: env.config_hash as u128,
            data_snapshot_id: env.data_snapshot_id as u128,
            rng_master_seed: env.rng_master_seed as u128,
        };

        let cfg = core_preset.config();
        let mut engine = Engine::<DefaultBundle>::default();
        let out = engine.run(&cfg, &core_env, workers as usize);

        Output {
            f_best: out.f_best,
            x_best: out.x_best.map(|xm| xm.0),
            stats: Stats {
                truth_evals: out.stats.truth_evals,
                partial_steps: out.stats.partial_steps,
                cheap_rejects: out.stats.cheap_rejects,
                invalid_eval_rejects: out.stats.invalid_eval_rejects,
            },
        }
    }
}

export!(ImadsEngine);
