//! C/C++ FFI bindings for IMADS.
//!
//! Provides an opaque-handle API for creating and running the IMADS engine from C/C++.
//! Custom evaluators are supported via a function-pointer vtable.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::Arc;

use imads_core::core::engine::{Engine, EngineConfig};
use imads_core::core::evaluator::Evaluator;
use imads_core::core::{DefaultBundle, ToyEvaluator};
use imads_core::presets::Preset;
use imads_core::types::{Env, Phi, XReal};

// ---------------------------------------------------------------------------
// Opaque handle
// ---------------------------------------------------------------------------

/// Opaque engine handle.
pub struct ImadsEngine {
    inner: Engine<DefaultBundle>,
}

// ---------------------------------------------------------------------------
// C-compatible types
// ---------------------------------------------------------------------------

/// Environment descriptor.
#[repr(C)]
pub struct ImadsEnv {
    pub run_id: u64,
    pub config_hash: u64,
    pub data_snapshot_id: u64,
    pub rng_master_seed: u64,
}

/// Engine statistics.
#[repr(C)]
pub struct ImadsStats {
    pub truth_evals: u64,
    pub truth_decision_cache_hits: u64,
    pub truth_eval_cache_hits: u64,
    pub partial_steps: u64,
    pub partial_decision_cache_hits: u64,
    pub partial_eval_cache_hits: u64,
    pub cheap_rejects: u64,
    pub invalid_eval_rejects: u64,
}

/// Engine output returned from a run.
///
/// - If `f_best_valid` is false, no feasible solution was found.
/// - `x_best_ptr` points to `x_best_len` elements owned by this struct. The caller must
///   call `imads_output_free` to release the memory.
#[repr(C)]
pub struct ImadsOutput {
    pub f_best: f64,
    pub f_best_valid: bool,
    pub x_best_ptr: *mut i64,
    pub x_best_len: usize,
    pub stats: ImadsStats,
}

/// Function-pointer vtable for custom evaluators.
///
/// All function pointers must be non-null and safe to call from any thread.
/// `user_data` is passed through unchanged to every callback.
#[repr(C)]
pub struct ImadsEvaluatorVTable {
    /// Stage A cheap constraint gate. Return non-zero (true) to accept.
    pub cheap_constraints:
        Option<unsafe extern "C" fn(x: *const f64, dim: usize, user_data: *mut u8) -> i32>,

    /// Monte Carlo sample. Write objective to `*f_out` and constraints to `c_out[0..m]`.
    pub mc_sample: unsafe extern "C" fn(
        x: *const f64,
        dim: usize,
        tau: u64,
        smc: u32,
        k: u32,
        f_out: *mut f64,
        c_out: *mut f64,
        m: usize,
        user_data: *mut u8,
    ),

    /// Number of constraints.
    pub num_constraints: usize,

    /// Opaque user data pointer.
    pub user_data: *mut u8,
}

// Safety: The user guarantees thread safety of the vtable callbacks.
unsafe impl Send for ImadsEvaluatorVTable {}
unsafe impl Sync for ImadsEvaluatorVTable {}

// ---------------------------------------------------------------------------
// FFI evaluator wrapper
// ---------------------------------------------------------------------------

impl std::fmt::Debug for ImadsEvaluatorVTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImadsEvaluatorVTable")
            .field("num_constraints", &self.num_constraints)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct FfiEvaluator {
    vtable: ImadsEvaluatorVTable,
}

impl Evaluator for FfiEvaluator {
    fn cheap_constraints(&self, x: &XReal, _env: &Env) -> bool {
        if let Some(f) = self.vtable.cheap_constraints {
            let vals: Vec<f64> = x.as_f64_slice();
            let result = unsafe { f(vals.as_ptr(), vals.len(), self.vtable.user_data) };
            result != 0
        } else {
            true
        }
    }

    fn mc_sample(&self, x: &XReal, phi: Phi, _env: &Env, k: u32) -> (f64, Vec<f64>) {
        let vals: Vec<f64> = x.as_f64_slice();
        let m = self.vtable.num_constraints;
        let mut f_out: f64 = 0.0;
        let mut c_out = vec![0.0f64; m];
        unsafe {
            (self.vtable.mc_sample)(
                vals.as_ptr(),
                vals.len(),
                phi.tau.0,
                phi.smc.0,
                k,
                &mut f_out,
                c_out.as_mut_ptr(),
                m,
                self.vtable.user_data,
            );
        }
        (f_out, c_out)
    }

    fn num_constraints(&self) -> usize {
        self.vtable.num_constraints
    }
}

// ---------------------------------------------------------------------------
// Helper conversions
// ---------------------------------------------------------------------------

fn ffi_env_to_env(e: &ImadsEnv) -> Env {
    Env {
        run_id: e.run_id as u128,
        config_hash: e.config_hash as u128,
        data_snapshot_id: e.data_snapshot_id as u128,
        rng_master_seed: e.rng_master_seed as u128,
    }
}

fn engine_output_to_ffi(out: imads_core::core::engine::EngineOutput) -> ImadsOutput {
    let stats = ImadsStats {
        truth_evals: out.stats.truth_evals,
        truth_decision_cache_hits: out.stats.truth_decision_cache_hits,
        truth_eval_cache_hits: out.stats.truth_eval_cache_hits,
        partial_steps: out.stats.partial_steps,
        partial_decision_cache_hits: out.stats.partial_decision_cache_hits,
        partial_eval_cache_hits: out.stats.partial_eval_cache_hits,
        cheap_rejects: out.stats.cheap_rejects,
        invalid_eval_rejects: out.stats.invalid_eval_rejects,
    };

    let (f_best, f_best_valid) = match out.f_best {
        Some(f) => (f, true),
        None => (f64::NAN, false),
    };

    let (x_best_ptr, x_best_len) = match out.x_best {
        Some(xm) => {
            let mut v = xm.0.into_boxed_slice();
            let ptr = v.as_mut_ptr();
            let len = v.len();
            std::mem::forget(v);
            (ptr, len)
        }
        None => (std::ptr::null_mut(), 0),
    };

    ImadsOutput {
        f_best,
        f_best_valid,
        x_best_ptr,
        x_best_len,
        stats,
    }
}

// ---------------------------------------------------------------------------
// Public C API
// ---------------------------------------------------------------------------

/// Create an `EngineConfig` from a preset name.
///
/// Valid names: `"legacy_baseline"`, `"balanced"`, `"conservative"`, `"throughput"`.
/// Returns null on invalid name.
///
/// The caller must free the returned pointer with `imads_config_free`.
///
/// # Safety
/// `name` must be a valid, null-terminated C string pointer (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imads_config_from_preset(name: *const c_char) -> *mut EngineConfig {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    let c_str = unsafe { CStr::from_ptr(name) };
    let s = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let preset = match s {
        "legacy_baseline" => Preset::LegacyBaseline,
        "balanced" => Preset::Balanced,
        "conservative" => Preset::Conservative,
        "throughput" => Preset::Throughput,
        _ => return std::ptr::null_mut(),
    };
    Box::into_raw(Box::new(preset.config()))
}

/// Free an `EngineConfig` created by `imads_config_from_preset`.
///
/// # Safety
/// `cfg` must be null or a pointer returned by `imads_config_from_preset`, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imads_config_free(cfg: *mut EngineConfig) {
    if !cfg.is_null() {
        drop(unsafe { Box::from_raw(cfg) });
    }
}

/// Create a new engine instance.
///
/// The caller must free the returned pointer with `imads_engine_free`.
#[unsafe(no_mangle)]
pub extern "C" fn imads_engine_new() -> *mut ImadsEngine {
    Box::into_raw(Box::new(ImadsEngine {
        inner: Engine::<DefaultBundle>::default(),
    }))
}

/// Free an engine instance.
///
/// # Safety
/// `engine` must be null or a pointer returned by `imads_engine_new`, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imads_engine_free(engine: *mut ImadsEngine) {
    if !engine.is_null() {
        drop(unsafe { Box::from_raw(engine) });
    }
}

/// Run the engine with the built-in toy evaluator.
///
/// The caller must call `imads_output_free` on the returned output to release `x_best_ptr`.
///
/// # Safety
/// All pointer arguments must be valid, non-null, and not concurrently accessed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imads_engine_run(
    engine: *mut ImadsEngine,
    cfg: *const EngineConfig,
    env: *const ImadsEnv,
    workers: u32,
) -> ImadsOutput {
    let engine = unsafe { &mut *engine };
    let cfg = unsafe { &*cfg };
    let env = ffi_env_to_env(unsafe { &*env });
    let evaluator: Arc<dyn Evaluator> = Arc::new(ToyEvaluator {
        m: cfg.num_constraints,
    });
    let out = engine
        .inner
        .run_with_evaluator(cfg, &env, workers as usize, evaluator);
    engine_output_to_ffi(out)
}

/// Run the engine with a custom evaluator provided via function-pointer vtable.
///
/// The caller must call `imads_output_free` on the returned output to release `x_best_ptr`.
///
/// # Safety
/// All pointer arguments must be valid, non-null, and not concurrently accessed.
/// The vtable function pointers must be safe to call from any thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imads_engine_run_with_evaluator(
    engine: *mut ImadsEngine,
    cfg: *const EngineConfig,
    env: *const ImadsEnv,
    workers: u32,
    vtable: ImadsEvaluatorVTable,
) -> ImadsOutput {
    let engine = unsafe { &mut *engine };
    let cfg = unsafe { &*cfg };
    let env = ffi_env_to_env(unsafe { &*env });
    let evaluator: Arc<dyn Evaluator> = Arc::new(FfiEvaluator { vtable });
    let out = engine
        .inner
        .run_with_evaluator(cfg, &env, workers as usize, evaluator);
    engine_output_to_ffi(out)
}

/// Free the `x_best_ptr` allocation in an `ImadsOutput`.
///
/// This must be called exactly once per output that has `x_best_ptr != null`.
///
/// # Safety
/// `output` must be null or point to a valid `ImadsOutput` whose `x_best_ptr` has not
/// been previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imads_output_free(output: *mut ImadsOutput) {
    if output.is_null() {
        return;
    }
    let out = unsafe { &mut *output };
    if !out.x_best_ptr.is_null() && out.x_best_len > 0 {
        let _ = unsafe {
            Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                out.x_best_ptr,
                out.x_best_len,
            ))
        };
        out.x_best_ptr = std::ptr::null_mut();
        out.x_best_len = 0;
    }
}
