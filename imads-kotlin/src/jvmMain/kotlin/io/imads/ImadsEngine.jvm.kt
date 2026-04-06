package io.imads

import io.imads.ImadsNative as JNI

// ---- ImadsConfig (JVM via JNI) ----

actual class ImadsConfig private constructor(internal val ptr: Long) : AutoCloseable {
    actual companion object {
        actual fun fromPreset(name: String): ImadsConfig {
            val p = JNI.configFromPreset(name)
            require(p != 0L) { "Unknown preset: $name" }
            return ImadsConfig(p)
        }

        actual fun presetNames(): List<String> = JNI.presetNames().toList()
    }

    actual override fun close() {
        if (ptr != 0L) JNI.configFree(ptr)
    }
}

// ---- ImadsEngine (JVM via JNI) ----

actual class ImadsEngine actual constructor() : AutoCloseable {
    private var ptr: Long = JNI.engineNew()

    actual fun run(cfg: ImadsConfig, env: ImadsEnv, workers: Int): ImadsOutput {
        val packed = JNI.engineRun(
            ensurePtr(), cfg.ptr,
            env.runId, env.configHash, env.dataSnapshotId, env.rngMasterSeed,
            workers,
        )
        return unpackOutput(packed)
    }

    actual fun run(
        cfg: ImadsConfig,
        env: ImadsEnv,
        evaluator: ImadsEvaluator,
        numConstraints: Int,
        workers: Int,
    ): ImadsOutput {
        val bridge = object : ImadsJvmEvaluator {
            override fun mcSample(x: DoubleArray, tau: Long, smc: Int, k: Int): DoubleArray =
                evaluator.mcSample(x, tau, smc, k)

            override fun cheapConstraints(x: DoubleArray): Boolean =
                evaluator.cheapConstraints(x)
        }
        val packed = JNI.engineRunWithEvaluator(
            ensurePtr(), cfg.ptr,
            env.runId, env.configHash, env.dataSnapshotId, env.rngMasterSeed,
            workers, bridge, numConstraints,
        )
        return unpackOutput(packed)
    }

    actual override fun close() {
        if (ptr != 0L) {
            JNI.engineFree(ptr)
            ptr = 0
        }
    }

    private fun ensurePtr(): Long {
        check(ptr != 0L) { "Engine already closed" }
        return ptr
    }
}

private fun unpackOutput(packed: LongArray): ImadsOutput {
    val fBits = packed[0]
    val f = Double.fromBits(fBits)
    val xLen = packed[1].toInt()
    val xBest = LongArray(xLen)
    System.arraycopy(packed, 6, xBest, 0, xLen)
    return ImadsOutput(
        fBest = if (f.isNaN()) null else f,
        xBest = xBest,
        truthEvals = packed[2],
        partialSteps = packed[3],
        cheapRejects = packed[4],
        invalidEvalRejects = packed[5],
    )
}
