//! JNI bindings for IMADS — shared native library for Java, Kotlin, Scala 3, and Clojure.
//!
//! The Java class `io.imads.ImadsNative` declares native methods that map to these functions.
//! The engine is stored as an opaque pointer (long) on the Java side.

use std::sync::Arc;

use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::{jint, jlong, jlongArray};
use jni::JNIEnv;

use imads_core::core::engine::{Engine, EngineConfig};
use imads_core::core::evaluator::Evaluator;
use imads_core::core::{DefaultBundle, ToyEvaluator};
use imads_core::presets::Preset;
use imads_core::types::{Env, Phi, XReal};

// ---------------------------------------------------------------------------
// Java evaluator wrapper
// ---------------------------------------------------------------------------

struct JavaEvaluator {
    jvm: jni::JavaVM,
    evaluator_ref: jni::objects::GlobalRef,
    m: usize,
}

impl std::fmt::Debug for JavaEvaluator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JavaEvaluator")
            .field("num_constraints", &self.m)
            .finish()
    }
}

unsafe impl Send for JavaEvaluator {}
unsafe impl Sync for JavaEvaluator {}

impl Evaluator for JavaEvaluator {
    fn cheap_constraints(&self, x: &XReal, _env: &Env) -> bool {
        let mut env = match self.jvm.attach_current_thread() {
            Ok(env) => env,
            Err(_) => return true,
        };
        let vals = x.as_f64_slice();
        let arr = match env.new_double_array(vals.len() as i32) {
            Ok(a) => a,
            Err(_) => return true,
        };
        if env.set_double_array_region(&arr, 0, &vals).is_err() {
            return true;
        }
        match env.call_method(
            &self.evaluator_ref,
            "cheapConstraints",
            "([D)Z",
            &[JValue::Object(&arr.into())],
        ) {
            Ok(v) => v.z().unwrap_or(true),
            Err(_) => true,
        }
    }

    fn mc_sample(&self, x: &XReal, phi: Phi, _env: &Env, k: u32) -> (f64, Vec<f64>) {
        let mut env = self
            .jvm
            .attach_current_thread()
            .expect("JVM attach failed");
        let vals = x.as_f64_slice();
        let x_arr = env
            .new_double_array(vals.len() as i32)
            .expect("new_double_array failed");
        env.set_double_array_region(&x_arr, 0, &vals)
            .expect("set_double_array_region failed");

        let result = env
            .call_method(
                &self.evaluator_ref,
                "mcSample",
                "([DJII)[D",
                &[
                    JValue::Object(&x_arr.into()),
                    JValue::Long(phi.tau.0 as jlong),
                    JValue::Int(phi.smc.0 as jint),
                    JValue::Int(k as jint),
                ],
            )
            .expect("mcSample call failed");

        let result_arr = result.l().expect("mcSample must return double[]");
        let result_arr = jni::objects::JDoubleArray::from(result_arr);
        let len = env
            .get_array_length(&result_arr)
            .expect("get_array_length failed") as usize;
        let mut buf = vec![0.0f64; len];
        env.get_double_array_region(&result_arr, 0, &mut buf)
            .expect("get_double_array_region failed");

        // buf[0] = f, buf[1..] = constraints
        let f_val = buf.first().copied().unwrap_or(0.0);
        let c = buf[1..].to_vec();
        (f_val, c)
    }

    fn num_constraints(&self) -> usize {
        self.m
    }
}

// ---------------------------------------------------------------------------
// Helper: box to pointer / pointer to ref
// ---------------------------------------------------------------------------

fn box_to_ptr<T>(b: Box<T>) -> jlong {
    Box::into_raw(b) as jlong
}

/// # Safety
/// Caller must ensure ptr is valid and was created by `box_to_ptr`.
unsafe fn ptr_to_ref<'a, T>(ptr: jlong) -> &'a mut T {
    unsafe { &mut *(ptr as *mut T) }
}

/// # Safety
/// Caller must ensure ptr is valid and was created by `box_to_ptr`.
/// Takes ownership back, dropping after.
unsafe fn ptr_drop<T>(ptr: jlong) {
    if ptr != 0 {
        drop(unsafe { Box::from_raw(ptr as *mut T) });
    }
}

fn preset_from_str(name: &str) -> Option<Preset> {
    match name {
        "legacy_baseline" => Some(Preset::LegacyBaseline),
        "balanced" => Some(Preset::Balanced),
        "conservative" => Some(Preset::Conservative),
        "throughput" => Some(Preset::Throughput),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// JNI exports: io.imads.ImadsNative
// ---------------------------------------------------------------------------

/// `static native long configFromPreset(String name);`
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_imads_ImadsNative_configFromPreset(
    mut env: JNIEnv,
    _class: JClass,
    name: JString,
) -> jlong {
    let name_str: String = match env.get_string(&name) {
        Ok(s) => s.into(),
        Err(_) => return 0,
    };
    match preset_from_str(&name_str) {
        Some(p) => box_to_ptr(Box::new(p.config())),
        None => 0,
    }
}

/// `static native String[] presetNames();`
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_imads_ImadsNative_presetNames(
    mut env: JNIEnv,
    _class: JClass,
) -> jni::sys::jobjectArray {
    let names: Vec<&str> = Preset::ALL.iter().map(|p| p.name()).collect();
    let string_class = env
        .find_class("java/lang/String")
        .expect("String class not found");
    let arr = env
        .new_object_array(names.len() as i32, &string_class, JObject::null())
        .expect("new_object_array failed");
    for (i, name) in names.iter().enumerate() {
        let js = env.new_string(name).expect("new_string failed");
        env.set_object_array_element(&arr, i as i32, &js)
            .expect("set_object_array_element failed");
    }
    arr.into_raw()
}

/// `static native void configFree(long ptr);`
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_imads_ImadsNative_configFree(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    unsafe { ptr_drop::<EngineConfig>(ptr) };
}

/// `static native long engineNew();`
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_imads_ImadsNative_engineNew(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    box_to_ptr(Box::new(Engine::<DefaultBundle>::default()))
}

/// `static native void engineFree(long ptr);`
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_imads_ImadsNative_engineFree(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    unsafe { ptr_drop::<Engine<DefaultBundle>>(ptr) };
}

/// `static native long[] engineRun(long enginePtr, long cfgPtr, long runId, long configHash, long dataSnapshotId, long rngMasterSeed, int workers);`
///
/// Returns `[f_best_bits, x_best_len, truth_evals, partial_steps, cheap_rejects, invalid_eval_rejects, x0, x1, ...]`
/// where `f_best_bits` is `Double.doubleToRawLongBits(f_best)` (0x7FF8000000000000L for NaN = no solution).
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_imads_ImadsNative_engineRun(
    mut env: JNIEnv,
    _class: JClass,
    engine_ptr: jlong,
    cfg_ptr: jlong,
    run_id: jlong,
    config_hash: jlong,
    data_snapshot_id: jlong,
    rng_master_seed: jlong,
    workers: jint,
) -> jlongArray {
    let engine = unsafe { ptr_to_ref::<Engine<DefaultBundle>>(engine_ptr) };
    let cfg = unsafe { ptr_to_ref::<EngineConfig>(cfg_ptr) };
    let env_val = Env {
        run_id: run_id as u128,
        config_hash: config_hash as u128,
        data_snapshot_id: data_snapshot_id as u128,
        rng_master_seed: rng_master_seed as u128,
    };
    let evaluator: Arc<dyn Evaluator> = Arc::new(ToyEvaluator {
        m: cfg.num_constraints,
    });
    let out = engine.run_with_evaluator(cfg, &env_val, workers.max(1) as usize, evaluator);

    pack_output(&mut env, out)
}

/// `static native long[] engineRunWithEvaluator(long enginePtr, long cfgPtr, long runId, long configHash, long dataSnapshotId, long rngMasterSeed, int workers, Object evaluator, int numConstraints);`
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_imads_ImadsNative_engineRunWithEvaluator(
    mut env: JNIEnv,
    _class: JClass,
    engine_ptr: jlong,
    cfg_ptr: jlong,
    run_id: jlong,
    config_hash: jlong,
    data_snapshot_id: jlong,
    rng_master_seed: jlong,
    workers: jint,
    evaluator_obj: JObject,
    num_constraints: jint,
) -> jlongArray {
    let engine = unsafe { ptr_to_ref::<Engine<DefaultBundle>>(engine_ptr) };
    let cfg = unsafe { ptr_to_ref::<EngineConfig>(cfg_ptr) };
    let env_val = Env {
        run_id: run_id as u128,
        config_hash: config_hash as u128,
        data_snapshot_id: data_snapshot_id as u128,
        rng_master_seed: rng_master_seed as u128,
    };

    let jvm = env.get_java_vm().expect("get_java_vm failed");
    let global_ref = env
        .new_global_ref(evaluator_obj)
        .expect("new_global_ref failed");

    let evaluator: Arc<dyn Evaluator> = Arc::new(JavaEvaluator {
        jvm,
        evaluator_ref: global_ref,
        m: num_constraints.max(0) as usize,
    });
    let out = engine.run_with_evaluator(cfg, &env_val, workers.max(1) as usize, evaluator);

    pack_output(&mut env, out)
}

fn pack_output(env: &mut JNIEnv, out: imads_core::core::engine::EngineOutput) -> jlongArray {
    let f_bits: i64 = match out.f_best {
        Some(f) => f.to_bits() as i64,
        None => f64::NAN.to_bits() as i64,
    };
    let x_best = out.x_best.map(|xm| xm.0).unwrap_or_default();
    let x_len = x_best.len() as i64;

    // Header: [f_bits, x_len, truth_evals, partial_steps, cheap_rejects, invalid_eval_rejects]
    let header_len = 6;
    let total = header_len + x_best.len();
    let arr = env
        .new_long_array(total as i32)
        .expect("new_long_array failed");

    let header = [
        f_bits,
        x_len,
        out.stats.truth_evals as i64,
        out.stats.partial_steps as i64,
        out.stats.cheap_rejects as i64,
        out.stats.invalid_eval_rejects as i64,
    ];
    env.set_long_array_region(&arr, 0, &header)
        .expect("set header failed");

    if !x_best.is_empty() {
        env.set_long_array_region(&arr, header_len as i32, &x_best)
            .expect("set x_best failed");
    }

    arr.into_raw()
}
