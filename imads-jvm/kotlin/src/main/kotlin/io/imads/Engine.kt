package io.imads

import java.lang.foreign.*
import java.lang.foreign.MemoryLayout.PathElement
import java.lang.foreign.ValueLayout.*
import java.lang.invoke.MethodHandle
import java.lang.invoke.MethodHandles
import java.lang.invoke.MethodType

/** Environment descriptor for deterministic hashing. */
data class ImadsEnv(
    val runId: Long = 1,
    val configHash: Long = 0,
    val dataSnapshotId: Long = 0,
    val rngMasterSeed: Long = 0,
)

/** Engine output. */
data class ImadsOutput(
    val fBest: Double?,
    val xBest: LongArray,
    val truthEvals: Long,
    val partialSteps: Long,
    val cheapRejects: Long,
    val invalidEvalRejects: Long,
) {
    override fun toString(): String =
        "ImadsOutput(fBest=$fBest, truthEvals=$truthEvals, partialSteps=$partialSteps)"

    override fun equals(other: Any?): Boolean =
        other is ImadsOutput && fBest == other.fBest && xBest.contentEquals(other.xBest)

    override fun hashCode(): Int = 31 * (fBest?.hashCode() ?: 0) + xBest.contentHashCode()
}

/** Custom evaluator interface. All methods must be deterministic. */
interface Evaluator {
    /** Monte Carlo sample. Return [objective, c0, c1, ...]. */
    fun mcSample(x: DoubleArray, tau: Long, smc: Int, k: Int): DoubleArray

    /** Optional cheap constraint gate. Return true to accept. */
    fun cheapConstraints(x: DoubleArray): Boolean = true

    /** Optional search space dimension hint. null = infer from config. */
    fun searchDim(): Int? = null
}

/**
 * IMADS engine wrapper using JDK 22+ Foreign Function & Memory API.
 *
 * Loads the native `imads_jvm` shared library and calls C functions directly
 * without JNI overhead.
 */
class Engine private constructor(private val handle: MemorySegment) : AutoCloseable {
    companion object {
        private val linker = Linker.nativeLinker()
        private val arena = Arena.ofAuto()

        private val lib: SymbolLookup by lazy {
            SymbolLookup.libraryLookup(System.mapLibraryName("imads_jvm"), arena)
        }

        private fun findOrThrow(name: String): MemorySegment =
            lib.find(name).orElseThrow { UnsatisfiedLinkError("Missing symbol: $name") }

        // ---- Struct layouts ----

        private val envLayout: StructLayout = MemoryLayout.structLayout(
            JAVA_LONG.withName("run_id"),
            JAVA_LONG.withName("config_hash"),
            JAVA_LONG.withName("data_snapshot_id"),
            JAVA_LONG.withName("rng_master_seed"),
        )

        private val statsLayout: StructLayout = MemoryLayout.structLayout(
            JAVA_LONG.withName("truth_evals"),
            JAVA_LONG.withName("partial_steps"),
            JAVA_LONG.withName("cheap_rejects"),
            JAVA_LONG.withName("invalid_eval_rejects"),
            JAVA_LONG.withName("_pad0"),
            JAVA_LONG.withName("_pad1"),
            JAVA_LONG.withName("_pad2"),
            JAVA_LONG.withName("_pad3"),
        )

        private val outputLayout: StructLayout = MemoryLayout.structLayout(
            JAVA_DOUBLE.withName("f_best"),
            JAVA_INT.withName("f_best_valid"),
            MemoryLayout.paddingLayout(4),
            ADDRESS.withName("x_best_ptr"),
            JAVA_LONG.withName("x_best_len"),
            statsLayout.withName("stats"),
        )

        private val vtableLayout: StructLayout = MemoryLayout.structLayout(
            ADDRESS.withName("cheap_constraints"),
            ADDRESS.withName("mc_sample"),
            JAVA_LONG.withName("num_constraints"),
            JAVA_LONG.withName("search_dim"),
            ADDRESS.withName("user_data"),
        )

        private val cheapConstraintsFD = FunctionDescriptor.of(
            JAVA_INT, ADDRESS, JAVA_LONG, ADDRESS,
        )
        private val mcSampleFD = FunctionDescriptor.ofVoid(
            ADDRESS, JAVA_LONG, JAVA_LONG, JAVA_INT, JAVA_INT,
            ADDRESS, ADDRESS, JAVA_LONG, ADDRESS,
        )

        private val statsOffset: Long =
            outputLayout.byteOffset(PathElement.groupElement("stats"))

        // ---- Downcall handles ----

        private val engineNew: MethodHandle by lazy {
            linker.downcallHandle(findOrThrow("imads_engine_new"), FunctionDescriptor.of(ADDRESS))
        }

        private val engineFree: MethodHandle by lazy {
            linker.downcallHandle(findOrThrow("imads_engine_free"), FunctionDescriptor.ofVoid(ADDRESS))
        }

        private val configFromPreset: MethodHandle by lazy {
            linker.downcallHandle(findOrThrow("imads_config_from_preset"), FunctionDescriptor.of(ADDRESS, ADDRESS))
        }

        private val configFree: MethodHandle by lazy {
            linker.downcallHandle(findOrThrow("imads_config_free"), FunctionDescriptor.ofVoid(ADDRESS))
        }

        private val hEngineRun: MethodHandle by lazy {
            linker.downcallHandle(
                findOrThrow("imads_engine_run"),
                FunctionDescriptor.of(outputLayout, ADDRESS, ADDRESS, envLayout, JAVA_INT),
            )
        }

        private val hEngineRunWithEvaluator: MethodHandle by lazy {
            linker.downcallHandle(
                findOrThrow("imads_engine_run_with_evaluator"),
                FunctionDescriptor.of(outputLayout, ADDRESS, ADDRESS, envLayout, JAVA_INT, vtableLayout),
            )
        }

        fun create(): Engine {
            val ptr = engineNew.invoke() as MemorySegment
            return Engine(ptr)
        }

        fun presetNames(): List<String> = listOf(
            "legacy_baseline", "balanced", "conservative", "throughput"
        )

        private fun extractOutput(seg: MemorySegment): ImadsOutput {
            val fBest = seg.get(JAVA_DOUBLE, 0)
            val fBestValid = seg.get(JAVA_INT, 8)
            val xBestPtr = seg.get(ADDRESS, 16)
            val xBestLen = seg.get(JAVA_LONG, 16 + ADDRESS.byteSize()).toInt()

            val xBest = if (xBestLen > 0 && xBestPtr != MemorySegment.NULL) {
                val sized = xBestPtr.reinterpret((xBestLen * JAVA_LONG.byteSize()))
                LongArray(xBestLen) { i -> sized.getAtIndex(JAVA_LONG, i.toLong()) }
            } else {
                LongArray(0)
            }

            return ImadsOutput(
                fBest = if (fBestValid != 0) fBest else null,
                xBest = xBest,
                truthEvals = seg.get(JAVA_LONG, statsOffset),
                partialSteps = seg.get(JAVA_LONG, statsOffset + 8),
                cheapRejects = seg.get(JAVA_LONG, statsOffset + 16),
                invalidEvalRejects = seg.get(JAVA_LONG, statsOffset + 24),
            )
        }
    }

    /** Run with the built-in toy evaluator. */
    fun run(preset: String, env: ImadsEnv = ImadsEnv(), workers: Int = 1): ImadsOutput {
        val cfg = configFromPreset.invoke(Arena.ofConfined().use { it.allocateFrom(preset) }) as MemorySegment
        require(cfg != MemorySegment.NULL) { "Unknown preset: $preset" }
        try {
            Arena.ofConfined().use { callArena ->
                val envSeg = callArena.allocate(envLayout)
                envSeg.set(JAVA_LONG, 0, env.runId)
                envSeg.set(JAVA_LONG, 8, env.configHash)
                envSeg.set(JAVA_LONG, 16, env.dataSnapshotId)
                envSeg.set(JAVA_LONG, 24, env.rngMasterSeed)

                val outSeg = hEngineRun.invoke(handle, cfg, envSeg, workers) as MemorySegment
                return extractOutput(outSeg)
            }
        } finally {
            configFree.invoke(cfg)
        }
    }

    /** Run with a custom evaluator. */
    fun run(
        preset: String,
        env: ImadsEnv = ImadsEnv(),
        evaluator: Evaluator,
        numConstraints: Int,
        workers: Int = 1,
    ): ImadsOutput {
        val cfg = configFromPreset.invoke(Arena.ofConfined().use { it.allocateFrom(preset) }) as MemorySegment
        require(cfg != MemorySegment.NULL) { "Unknown preset: $preset" }
        try {
            val callArena = Arena.ofShared()
            try {
                val envSeg = callArena.allocate(envLayout)
                envSeg.set(JAVA_LONG, 0, env.runId)
                envSeg.set(JAVA_LONG, 8, env.configHash)
                envSeg.set(JAVA_LONG, 16, env.dataSnapshotId)
                envSeg.set(JAVA_LONG, 24, env.rngMasterSeed)

                val mcCallback = McSampleCallback(evaluator)
                val mcHandle = MethodHandles.lookup().findVirtual(
                    McSampleCallback::class.java, "invoke",
                    MethodType.methodType(
                        Void.TYPE,
                        MemorySegment::class.java, Long::class.javaPrimitiveType,
                        Long::class.javaPrimitiveType, Int::class.javaPrimitiveType,
                        Int::class.javaPrimitiveType,
                        MemorySegment::class.java, MemorySegment::class.java,
                        Long::class.javaPrimitiveType, MemorySegment::class.java,
                    ),
                ).bindTo(mcCallback)
                val mcStub = linker.upcallStub(mcHandle, mcSampleFD, callArena)

                val cheapCallback = CheapConstraintsCallback(evaluator)
                val cheapHandle = MethodHandles.lookup().findVirtual(
                    CheapConstraintsCallback::class.java, "invoke",
                    MethodType.methodType(
                        Int::class.javaPrimitiveType,
                        MemorySegment::class.java, Long::class.javaPrimitiveType,
                        MemorySegment::class.java,
                    ),
                ).bindTo(cheapCallback)
                val cheapStub = linker.upcallStub(cheapHandle, cheapConstraintsFD, callArena)

                val vtableSeg = callArena.allocate(vtableLayout)
                vtableSeg.set(ADDRESS, 0, cheapStub)
                vtableSeg.set(ADDRESS, 8, mcStub)
                vtableSeg.set(JAVA_LONG, 16, numConstraints.toLong())
                vtableSeg.set(JAVA_LONG, 24, (evaluator.searchDim() ?: 0).toLong())
                vtableSeg.set(ADDRESS, 32, MemorySegment.NULL)

                val outSeg = hEngineRunWithEvaluator.invoke(
                    handle, cfg, envSeg, workers, vtableSeg,
                ) as MemorySegment
                return extractOutput(outSeg)
            } finally {
                callArena.close()
            }
        } finally {
            configFree.invoke(cfg)
        }
    }

    override fun close() {
        engineFree.invoke(handle)
    }
}

private class McSampleCallback(private val eval: Evaluator) {
    fun invoke(
        x: MemorySegment, dim: Long, tau: Long, smc: Int, k: Int,
        fOut: MemorySegment, cOut: MemorySegment, m: Long, @Suppress("UNUSED_PARAMETER") userData: MemorySegment,
    ) {
        val xArr = DoubleArray(dim.toInt()) { i ->
            x.reinterpret(dim * JAVA_DOUBLE.byteSize()).getAtIndex(JAVA_DOUBLE, i.toLong())
        }
        val result = eval.mcSample(xArr, tau, smc, k)
        fOut.reinterpret(JAVA_DOUBLE.byteSize()).set(JAVA_DOUBLE, 0, result[0])
        val mInt = m.toInt()
        val cSized = cOut.reinterpret(mInt.toLong() * JAVA_DOUBLE.byteSize())
        for (j in 0 until mInt) {
            cSized.setAtIndex(JAVA_DOUBLE, j.toLong(), if (j + 1 < result.size) result[j + 1] else 0.0)
        }
    }
}

private class CheapConstraintsCallback(private val eval: Evaluator) {
    fun invoke(x: MemorySegment, dim: Long, @Suppress("UNUSED_PARAMETER") userData: MemorySegment): Int {
        val xArr = DoubleArray(dim.toInt()) { i ->
            x.reinterpret(dim * JAVA_DOUBLE.byteSize()).getAtIndex(JAVA_DOUBLE, i.toLong())
        }
        return if (eval.cheapConstraints(xArr)) 1 else 0
    }
}

/** Sealed interface for compile-time objective count safety. */
sealed interface ObjectiveCount {
    data object N1 : ObjectiveCount
    data object N2 : ObjectiveCount
    data object N3 : ObjectiveCount
}
