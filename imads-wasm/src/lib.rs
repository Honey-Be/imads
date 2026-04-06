//! WASM/TypeScript bindings for IMADS via wasm-bindgen.

use std::sync::Arc;

use wasm_bindgen::prelude::*;

use imads_core::core::engine::{Engine, EngineConfig as CoreEngineConfig};
use imads_core::core::evaluator::Evaluator;
use imads_core::core::DefaultBundle;
use imads_core::presets::Preset;
use imads_core::types::{Env as CoreEnv, Phi, XReal};

// ---------------------------------------------------------------------------
// EngineConfig
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct EngineConfig {
    inner: CoreEngineConfig,
}

#[wasm_bindgen]
impl EngineConfig {
    /// Create an EngineConfig from a preset name.
    ///
    /// Valid names: "legacy_baseline", "balanced", "conservative", "throughput".
    #[wasm_bindgen(js_name = "fromPreset")]
    pub fn from_preset(name: &str) -> Result<EngineConfig, JsError> {
        let preset = match name {
            "legacy_baseline" => Preset::LegacyBaseline,
            "balanced" => Preset::Balanced,
            "conservative" => Preset::Conservative,
            "throughput" => Preset::Throughput,
            _ => return Err(JsError::new(&format!("Unknown preset: {name}"))),
        };
        Ok(Self {
            inner: preset.config(),
        })
    }

    /// List all available preset names.
    #[wasm_bindgen(js_name = "presetNames")]
    pub fn preset_names() -> Vec<JsValue> {
        Preset::ALL
            .iter()
            .map(|p| JsValue::from_str(p.name()))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Env
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct Env {
    inner: CoreEnv,
}

#[wasm_bindgen]
impl Env {
    #[wasm_bindgen(constructor)]
    pub fn new(
        run_id: u64,
        config_hash: u64,
        data_snapshot_id: u64,
        rng_master_seed: u64,
    ) -> Self {
        Self {
            inner: CoreEnv {
                run_id: run_id as u128,
                config_hash: config_hash as u128,
                data_snapshot_id: data_snapshot_id as u128,
                rng_master_seed: rng_master_seed as u128,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// EngineOutput
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct EngineOutput {
    f_best: Option<f64>,
    x_best: Option<Vec<i64>>,
    truth_evals: u64,
    partial_steps: u64,
    cheap_rejects: u64,
    invalid_eval_rejects: u64,
}

#[wasm_bindgen]
impl EngineOutput {
    #[wasm_bindgen(getter, js_name = "fBest")]
    pub fn f_best(&self) -> JsValue {
        match self.f_best {
            Some(f) => JsValue::from_f64(f),
            None => JsValue::NULL,
        }
    }

    #[wasm_bindgen(getter, js_name = "xBest")]
    pub fn x_best(&self) -> Vec<i64> {
        self.x_best.clone().unwrap_or_default()
    }

    #[wasm_bindgen(getter, js_name = "truthEvals")]
    pub fn truth_evals(&self) -> u64 {
        self.truth_evals
    }

    #[wasm_bindgen(getter, js_name = "partialSteps")]
    pub fn partial_steps(&self) -> u64 {
        self.partial_steps
    }

    #[wasm_bindgen(getter, js_name = "cheapRejects")]
    pub fn cheap_rejects(&self) -> u64 {
        self.cheap_rejects
    }

    #[wasm_bindgen(getter, js_name = "invalidEvalRejects")]
    pub fn invalid_eval_rejects(&self) -> u64 {
        self.invalid_eval_rejects
    }
}

// ---------------------------------------------------------------------------
// JS evaluator wrapper
// ---------------------------------------------------------------------------

struct JsEvaluator {
    mc_sample_fn: js_sys::Function,
    cheap_fn: Option<js_sys::Function>,
    m: usize,
}

impl std::fmt::Debug for JsEvaluator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsEvaluator")
            .field("num_constraints", &self.m)
            .finish()
    }
}

// WASM is single-threaded (for wasm32-unknown-unknown); these bounds are trivially satisfied.
unsafe impl Send for JsEvaluator {}
unsafe impl Sync for JsEvaluator {}

impl Evaluator for JsEvaluator {
    fn cheap_constraints(&self, x: &XReal, _env: &imads_core::types::Env) -> bool {
        if let Some(f) = &self.cheap_fn {
            let vals = x.as_f64_slice();
            let arr = js_sys::Float64Array::from(vals.as_slice());
            match f.call1(&JsValue::NULL, &arr) {
                Ok(v) => v.as_bool().unwrap_or(true),
                Err(_) => true,
            }
        } else {
            true
        }
    }

    fn mc_sample(
        &self,
        x: &XReal,
        phi: Phi,
        _env: &imads_core::types::Env,
        k: u32,
    ) -> (f64, Vec<f64>) {
        let vals = x.as_f64_slice();
        let arr = js_sys::Float64Array::from(vals.as_slice());
        let result = self
            .mc_sample_fn
            .call3(
                &JsValue::NULL,
                &arr,
                &JsValue::from(phi.tau.0 as f64),
                &JsValue::from(k),
            )
            .expect("mc_sample call failed");

        // Expect result to be [f, c0, c1, ..., c_{m-1}].
        let array = js_sys::Array::from(&result);
        let f_val = array.get(0).as_f64().unwrap_or(0.0);
        let mut c = Vec::with_capacity(self.m);
        for j in 0..self.m {
            c.push(array.get((j + 1) as u32).as_f64().unwrap_or(0.0));
        }
        (f_val, c)
    }

    fn num_constraints(&self) -> usize {
        self.m
    }
}

// ---------------------------------------------------------------------------
// ImadsEngine
// ---------------------------------------------------------------------------

#[wasm_bindgen(js_name = "Engine")]
pub struct ImadsEngine {
    inner: Engine<DefaultBundle>,
}

impl Default for ImadsEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen(js_class = "Engine")]
impl ImadsEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: Engine::<DefaultBundle>::default(),
        }
    }

    /// Run with the built-in toy evaluator.
    pub fn run(&mut self, cfg: &EngineConfig, env: &Env) -> EngineOutput {
        let out = self.inner.run(&cfg.inner, &env.inner, 1);
        EngineOutput {
            f_best: out.f_best,
            x_best: out.x_best.map(|xm| xm.0),
            truth_evals: out.stats.truth_evals,
            partial_steps: out.stats.partial_steps,
            cheap_rejects: out.stats.cheap_rejects,
            invalid_eval_rejects: out.stats.invalid_eval_rejects,
        }
    }

    /// Run with a custom JS evaluator.
    ///
    /// `mc_sample_fn`: `(x: Float64Array, tau: number, k: number) => [f, c0, c1, ...]`
    /// `cheap_fn`: optional `(x: Float64Array) => boolean`
    #[wasm_bindgen(js_name = "runWithEvaluator")]
    pub fn run_with_evaluator(
        &mut self,
        cfg: &EngineConfig,
        env: &Env,
        mc_sample_fn: js_sys::Function,
        num_constraints: usize,
        cheap_fn: Option<js_sys::Function>,
    ) -> EngineOutput {
        let eval: Arc<dyn Evaluator> = Arc::new(JsEvaluator {
            mc_sample_fn,
            cheap_fn,
            m: num_constraints,
        });
        let out = self
            .inner
            .run_with_evaluator(&cfg.inner, &env.inner, 1, eval);
        EngineOutput {
            f_best: out.f_best,
            x_best: out.x_best.map(|xm| xm.0),
            truth_evals: out.stats.truth_evals,
            partial_steps: out.stats.partial_steps,
            cheap_rejects: out.stats.cheap_rejects,
            invalid_eval_rejects: out.stats.invalid_eval_rejects,
        }
    }
}
