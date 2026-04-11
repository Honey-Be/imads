package io.imads

import java.lang.foreign.*
import java.lang.foreign.ValueLayout.*
import java.lang.invoke.MethodHandle

// ---- FFM (Foreign Function & Memory) bindings to imads_jvm shared library ----

private object FFM {
    private val linker: Linker = Linker.nativeLinker()
    private val lookup: SymbolLookup = SymbolLookup.libraryLookup(
        System.mapLibraryName("imads_jvm"),
        Arena.global(),
    )

    private fun findOrThrow(name: String): MemorySegment =
        lookup.find(name).orElseThrow { UnsatisfiedLinkError("Missing symbol: $name") }

    // ImadsEnv struct: 4 x uint64_t
    val envLayout: StructLayout = MemoryLayout.structLayout(
        JAVA_LONG.withName("run_id"),
        JAVA_LONG.withName("config_hash"),
        JAVA_LONG.withName("data_snapshot_id"),
        JAVA_LONG.withName("rng_master_seed"),
    )

    // Stats sub-struct (8 x uint64_t, we use the first 4)
    val statsLayout: StructLayout = MemoryLayout.structLayout(
        JAVA_LONG.withName("truth_evals"),
        JAVA_LONG.withName("partial_steps"),
        JAVA_LONG.withName("cheap_rejects"),
        JAVA_LONG.withName("invalid_eval_rejects"),
        JAVA_LONG.withName("_pad0"),
        JAVA_LONG.withName("_pad1"),
        JAVA_LONG.withName("_pad2"),
        JAVA_LONG.withName("_pad3"),
    )

    // ImadsOutput struct
    val outputLayout: StructLayout = MemoryLayout.structLayout(
        JAVA_DOUBLE.withName("f_best"),
        JAVA_INT.withName("f_best_valid"),
        MemoryLayout.paddingLayout(4),
        ADDRESS.withName("x_best_ptr"),
        JAVA_LONG.withName("x_best_len"),
        statsLayout.withName("stats"),
    )

    val configFromPreset: MethodHandle = linker.downcallHandle(
        findOrThrow("imads_config_from_preset"),
        FunctionDescriptor.of(ADDRESS, ADDRESS),
    )

    val configFree: MethodHandle = linker.downcallHandle(
        findOrThrow("imads_config_free"),
        FunctionDescriptor.ofVoid(ADDRESS),
    )

    val engineNew: MethodHandle = linker.downcallHandle(
        findOrThrow("imads_engine_new"),
        FunctionDescriptor.of(ADDRESS),
    )

    val engineFree: MethodHandle = linker.downcallHandle(
        findOrThrow("imads_engine_free"),
        FunctionDescriptor.ofVoid(ADDRESS),
    )

    val engineRun: MethodHandle = linker.downcallHandle(
        findOrThrow("imads_engine_run"),
        FunctionDescriptor.of(outputLayout, ADDRESS, ADDRESS, envLayout, JAVA_INT),
    )

    val statsOffset: Long = outputLayout.byteOffset(MemoryLayout.PathElement.groupElement("stats"))
}

// ---- ImadsConfig (JVM via FFM) ----

actual class ImadsConfig private constructor(internal val handle: MemorySegment) : AutoCloseable {
    actual companion object {
        actual fun fromPreset(name: String): ImadsConfig {
            Arena.ofConfined().use { arena ->
                val cName = arena.allocateFrom(name)
                val p = FFM.configFromPreset.invoke(cName) as MemorySegment
                require(p != MemorySegment.NULL) { "Unknown preset: $name" }
                return ImadsConfig(p)
            }
        }

        actual fun presetNames(): List<String> =
            listOf("legacy_baseline", "balanced", "conservative", "throughput")
    }

    actual override fun close() {
        if (handle != MemorySegment.NULL) {
            FFM.configFree.invoke(handle)
        }
    }
}

// ---- ImadsEngine (JVM via FFM) ----

actual class ImadsEngine actual constructor() : AutoCloseable {
    private var handle: MemorySegment = FFM.engineNew.invoke() as MemorySegment

    actual fun run(cfg: ImadsConfig, env: ImadsEnv, workers: Int): ImadsOutput {
        Arena.ofConfined().use { arena ->
            val envSeg = arena.allocate(FFM.envLayout)
            envSeg.set(JAVA_LONG, 0, env.runId)
            envSeg.set(JAVA_LONG, 8, env.configHash)
            envSeg.set(JAVA_LONG, 16, env.dataSnapshotId)
            envSeg.set(JAVA_LONG, 24, env.rngMasterSeed)

            val outSeg = FFM.engineRun.invoke(ensureHandle(), cfg.handle, envSeg, workers) as MemorySegment
            return extractOutput(outSeg)
        }
    }

    actual fun run(
        cfg: ImadsConfig,
        env: ImadsEnv,
        evaluator: ImadsEvaluator,
        numConstraints: Int,
        workers: Int,
    ): ImadsOutput {
        // TODO: Custom evaluator via FFM requires upcall stubs for callback function pointers.
        // For now, fall back to toy evaluator with a warning.
        System.err.println("Warning: custom evaluator not yet supported via FFM; using toy evaluator")
        return run(cfg, env, workers)
    }

    actual override fun close() {
        if (handle != MemorySegment.NULL) {
            FFM.engineFree.invoke(handle)
            handle = MemorySegment.NULL
        }
    }

    private fun ensureHandle(): MemorySegment {
        check(handle != MemorySegment.NULL) { "Engine already closed" }
        return handle
    }
}

private fun extractOutput(seg: MemorySegment): ImadsOutput {
    val fBest = seg.get(JAVA_DOUBLE, 0)
    val fBestValid = seg.get(JAVA_INT, 8)
    // After f_best(8) + f_best_valid(4) + padding(4) = offset 16
    val xBestPtr = seg.get(ADDRESS, 16)
    val xBestLen = seg.get(JAVA_LONG, 16 + ADDRESS.byteSize()).toInt()

    val xBest = if (xBestLen > 0 && xBestPtr != MemorySegment.NULL) {
        val sized = xBestPtr.reinterpret((xBestLen * JAVA_LONG.byteSize()))
        LongArray(xBestLen) { i -> sized.getAtIndex(JAVA_LONG, i.toLong()) }
    } else {
        LongArray(0)
    }

    val statsOffset = FFM.statsOffset
    val truthEvals = seg.get(JAVA_LONG, statsOffset)
    val partialSteps = seg.get(JAVA_LONG, statsOffset + 8)
    val cheapRejects = seg.get(JAVA_LONG, statsOffset + 16)
    val invalidEvalRejects = seg.get(JAVA_LONG, statsOffset + 24)

    return ImadsOutput(
        fBest = if (fBestValid != 0) fBest else null,
        xBest = xBest,
        truthEvals = truthEvals,
        partialSteps = partialSteps,
        cheapRejects = cheapRejects,
        invalidEvalRejects = invalidEvalRejects,
    )
}
