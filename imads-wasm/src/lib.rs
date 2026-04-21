//! WASI Component Model bindings for IMADS via wit-bindgen.

wit_bindgen::generate!({
    world: "imads",
    path: "wit/imads.wit",
});

use std::sync::Arc;

use imads_core::core::engine::Engine;
use imads_core::core::evaluator::{Evaluator, EvaluatorErased};
use imads_core::core::DefaultBundle;
use imads_core::presets::Preset;
use imads_core::types::{Env as CoreEnv, Phi, XReal};

use exports::honey_be::imads::engine::Guest;
use honey_be::imads::evaluator_host::Evaluator as WitEvaluator;
use honey_be::imads::types::{
    Env, MultiOutput, Output, Preset as WitPreset, Stats,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn wit_preset_to_core(preset: WitPreset) -> Preset {
    match preset {
        WitPreset::LegacyBaseline => Preset::LegacyBaseline,
        WitPreset::Balanced => Preset::Balanced,
        WitPreset::Conservative => Preset::Conservative,
        WitPreset::Throughput => Preset::Throughput,
    }
}

fn wit_env_to_core(env: &Env) -> CoreEnv {
    CoreEnv {
        run_id: env.run_id as u128,
        config_hash: env.config_hash as u128,
        data_snapshot_id: env.data_snapshot_id as u128,
        rng_master_seed: env.rng_master_seed as u128,
    }
}

fn core_output_to_wit(out: imads_core::core::engine::EngineOutput) -> Output {
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

fn core_output_to_multi_wit(out: imads_core::core::engine::EngineOutput) -> MultiOutput {
    MultiOutput {
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

// ---------------------------------------------------------------------------
// Evaluator adapter: WIT resource → imads-core Evaluator trait
// ---------------------------------------------------------------------------

struct WitEvaluatorAdapter {
    /// SAFETY: The WitEvaluator borrow is valid for the duration of the engine
    /// run call. WASM components are single-threaded, so no concurrent access.
    inner: &'static WitEvaluator,
    cached_num_constraints: usize,
    cached_search_dim: Option<usize>,
}

impl std::fmt::Debug for WitEvaluatorAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WitEvaluatorAdapter")
            .field("num_constraints", &self.cached_num_constraints)
            .finish_non_exhaustive()
    }
}

// SAFETY: WASM is single-threaded; these are only needed to satisfy trait bounds.
unsafe impl Send for WitEvaluatorAdapter {}
unsafe impl Sync for WitEvaluatorAdapter {}

impl Evaluator for WitEvaluatorAdapter {
    type Objectives = f64;

    fn cheap_constraints(&self, x: &XReal, _env: &CoreEnv) -> bool {
        let vals: Vec<f64> = x.as_f64_slice();
        self.inner.cheap_constraints(&vals)
    }

    fn mc_sample(&self, x: &XReal, phi: Phi, _env: &CoreEnv, k: u32) -> (f64, Vec<f64>) {
        let vals: Vec<f64> = x.as_f64_slice();
        let result = self.inner.mc_sample(&vals, phi.tau.0, phi.smc.0, k);
        let f = result[0];
        let c = result[1..].to_vec();
        (f, c)
    }

    fn num_objectives(&self) -> usize {
        1
    }

    fn num_constraints(&self) -> usize {
        self.cached_num_constraints
    }

    fn search_dim(&self) -> Option<usize> {
        self.cached_search_dim
    }
}

/// Multi-objective variant of the evaluator adapter.
struct WitMultiEvaluatorAdapter {
    inner: &'static WitEvaluator,
    cached_num_constraints: usize,
    cached_search_dim: Option<usize>,
}

impl std::fmt::Debug for WitMultiEvaluatorAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WitMultiEvaluatorAdapter")
            .field("num_constraints", &self.cached_num_constraints)
            .finish_non_exhaustive()
    }
}

unsafe impl Send for WitMultiEvaluatorAdapter {}
unsafe impl Sync for WitMultiEvaluatorAdapter {}

impl Evaluator for WitMultiEvaluatorAdapter {
    type Objectives = Vec<f64>;

    fn cheap_constraints(&self, x: &XReal, _env: &CoreEnv) -> bool {
        let vals: Vec<f64> = x.as_f64_slice();
        self.inner.cheap_constraints(&vals)
    }

    fn mc_sample(&self, x: &XReal, phi: Phi, _env: &CoreEnv, k: u32) -> (Vec<f64>, Vec<f64>) {
        let vals: Vec<f64> = x.as_f64_slice();
        let result = self.inner.mc_sample(&vals, phi.tau.0, phi.smc.0, k);
        // For multi-objective: host returns [obj0, obj1, ..., c0, c1, ...]
        // The split point is determined by num_constraints from the end.
        let m = self.cached_num_constraints;
        let n_obj = result.len().saturating_sub(m);
        let f = result[..n_obj].to_vec();
        let c = result[n_obj..].to_vec();
        (f, c)
    }

    fn num_objectives(&self) -> usize {
        // Inferred from first mc_sample call via the engine.
        // Return 0 to let the engine determine from the evaluator output.
        0
    }

    fn num_constraints(&self) -> usize {
        self.cached_num_constraints
    }

    fn search_dim(&self) -> Option<usize> {
        self.cached_search_dim
    }
}

// ---------------------------------------------------------------------------
// Guest implementation
// ---------------------------------------------------------------------------

struct ImadsEngine;

impl Guest for ImadsEngine {
    fn run(preset: WitPreset, env: Env, workers: u32) -> Output {
        let cfg = wit_preset_to_core(preset).config();
        let core_env = wit_env_to_core(&env);
        let mut engine = Engine::<DefaultBundle>::default();
        let out = engine.run(&cfg, &core_env, workers as usize);
        core_output_to_wit(out)
    }

    fn run_with_evaluator(
        preset: WitPreset,
        env: Env,
        workers: u32,
        evaluator: &WitEvaluator,
    ) -> Output {
        let cfg = wit_preset_to_core(preset).config();
        let core_env = wit_env_to_core(&env);

        // SAFETY: `evaluator` is borrowed for the duration of this function call,
        // and the engine run completes before we return. WASM is single-threaded.
        let evaluator_static: &'static WitEvaluator = unsafe { std::mem::transmute(evaluator) };

        let adapter = WitEvaluatorAdapter {
            inner: evaluator_static,
            cached_num_constraints: evaluator.num_constraints() as usize,
            cached_search_dim: evaluator.search_dim().map(|d| d as usize),
        };
        let evaluator_arc: Arc<dyn EvaluatorErased> = Arc::new(adapter);

        let mut engine = Engine::<DefaultBundle>::default();
        let out = engine.run_with_evaluator(&cfg, &core_env, workers as usize, evaluator_arc);
        core_output_to_wit(out)
    }

    fn run_multi(
        preset: WitPreset,
        env: Env,
        workers: u32,
        evaluator: &WitEvaluator,
    ) -> MultiOutput {
        let cfg = wit_preset_to_core(preset).config();
        let core_env = wit_env_to_core(&env);

        let evaluator_static: &'static WitEvaluator = unsafe { std::mem::transmute(evaluator) };

        let adapter = WitMultiEvaluatorAdapter {
            inner: evaluator_static,
            cached_num_constraints: evaluator.num_constraints() as usize,
            cached_search_dim: evaluator.search_dim().map(|d| d as usize),
        };
        let evaluator_arc: Arc<dyn EvaluatorErased> = Arc::new(adapter);

        let mut engine = Engine::<DefaultBundle>::default();
        let out = engine.run_with_evaluator(&cfg, &core_env, workers as usize, evaluator_arc);
        core_output_to_multi_wit(out)
    }
}

export!(ImadsEngine);
