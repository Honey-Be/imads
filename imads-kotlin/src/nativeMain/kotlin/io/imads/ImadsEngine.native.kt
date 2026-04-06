package io.imads

import kotlinx.cinterop.*
import imads_ffi.*

// ---- ImadsConfig (Native via C FFI) ----

actual class ImadsConfig private constructor(internal val ptr: COpaquePointer) : AutoCloseable {
    actual companion object {
        actual fun fromPreset(name: String): ImadsConfig = memScoped {
            val p = imads_config_from_preset(name.cstr.ptr)
                ?: throw IllegalArgumentException("Unknown preset: $name")
            ImadsConfig(p)
        }

        actual fun presetNames(): List<String> =
            listOf("legacy_baseline", "balanced", "conservative", "throughput")
    }

    actual override fun close() {
        imads_config_free(ptr)
    }
}

// ---- ImadsEngine (Native via C FFI) ----

actual class ImadsEngine actual constructor() : AutoCloseable {
    private var ptr: COpaquePointer? = imads_engine_new()

    actual fun run(cfg: ImadsConfig, env: ImadsEnv, workers: Int): ImadsOutput = memScoped {
        val cEnv = alloc<ImadsEnv>().apply {
            run_id = env.runId.toULong()
            config_hash = env.configHash.toULong()
            data_snapshot_id = env.dataSnapshotId.toULong()
            rng_master_seed = env.rngMasterSeed.toULong()
        }
        val out = imads_engine_run(ensurePtr(), cfg.ptr, cEnv.ptr, workers.toUInt())
        extractOutput(out)
    }

    actual fun run(
        cfg: ImadsConfig,
        env: ImadsEnv,
        evaluator: ImadsEvaluator,
        numConstraints: Int,
        workers: Int,
    ): ImadsOutput = memScoped {
        val cEnv = alloc<ImadsEnv>().apply {
            run_id = env.runId.toULong()
            config_hash = env.configHash.toULong()
            data_snapshot_id = env.dataSnapshotId.toULong()
            rng_master_seed = env.rngMasterSeed.toULong()
        }
        // Store evaluator reference for callbacks
        val stableRef = StableRef.create(evaluator)
        val vtable = alloc<ImadsEvaluatorVTable>().apply {
            mc_sample = staticCFunction { x, dim, tau, smc, k, fOut, cOut, m, userData ->
                val eval = userData!!.asStableRef<ImadsEvaluator>().get()
                val xArr = DoubleArray(dim.toInt()) { i -> x!![i] }
                val result = eval.mcSample(xArr, tau.toLong(), smc.toInt(), k.toInt())
                fOut!!.pointed.value = result[0]
                for (j in 0 until m.toInt()) {
                    cOut!![j] = if (j + 1 < result.size) result[j + 1] else 0.0
                }
            }
            cheap_constraints = staticCFunction { x, dim, userData ->
                val eval = userData!!.asStableRef<ImadsEvaluator>().get()
                val xArr = DoubleArray(dim.toInt()) { i -> x!![i] }
                if (eval.cheapConstraints(xArr)) 1 else 0
            }
            this.num_constraints = numConstraints.toULong()
            user_data = stableRef.asCPointer().reinterpret()
        }
        val out = imads_engine_run_with_evaluator(ensurePtr(), cfg.ptr, cEnv.ptr, workers.toUInt(), vtable.readValue())
        stableRef.dispose()
        extractOutput(out)
    }

    actual override fun close() {
        ptr?.let { imads_engine_free(it) }
        ptr = null
    }

    private fun ensurePtr(): COpaquePointer {
        return ptr ?: throw IllegalStateException("Engine already closed")
    }
}

private fun extractOutput(out: imads_ffi.ImadsOutput): ImadsOutput {
    val f = out.f_best
    val xLen = out.x_best_len.toInt()
    val xBest = LongArray(xLen) { i -> out.x_best_ptr!![i] }
    // Free the x_best allocation
    memScoped {
        val outVar = alloc<imads_ffi.ImadsOutput>()
        // Copy to mutable for freeing
        outVar.f_best = out.f_best
        outVar.f_best_valid = out.f_best_valid
        outVar.x_best_ptr = out.x_best_ptr
        outVar.x_best_len = out.x_best_len
        imads_output_free(outVar.ptr)
    }
    return ImadsOutput(
        fBest = if (out.f_best_valid) f else null,
        xBest = xBest,
        truthEvals = out.stats.truth_evals.toLong(),
        partialSteps = out.stats.partial_steps.toLong(),
        cheapRejects = out.stats.cheap_rejects.toLong(),
        invalidEvalRejects = out.stats.invalid_eval_rejects.toLong(),
    )
}
