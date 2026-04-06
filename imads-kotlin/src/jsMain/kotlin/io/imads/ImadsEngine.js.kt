package io.imads

// External declarations for the imads-wasm npm package.
// With --target bundler, wasm-bindgen exports are plain ESM — no init() needed.
// Bundlers (Webpack 5+, Vite, Rollup+plugin) resolve "imads-wasm" via package.json
// exports field and handle .wasm loading automatically.

@JsModule("imads-wasm")
external object ImadsWasm {
    @JsName("EngineConfig")
    class WasmEngineConfig {
        companion object {
            fun fromPreset(name: String): WasmEngineConfig
            fun presetNames(): Array<dynamic>
        }
        fun free()
    }

    @JsName("Env")
    class WasmEnv(runId: Double, configHash: Double, dataSnapshotId: Double, rngMasterSeed: Double)

    @JsName("Engine")
    class WasmEngine() {
        fun run(cfg: WasmEngineConfig, env: WasmEnv): dynamic
        fun runWithEvaluator(
            cfg: WasmEngineConfig,
            env: WasmEnv,
            mcSampleFn: dynamic,
            numConstraints: Int,
            cheapFn: dynamic = definedExternally,
        ): dynamic

        fun free()
    }
}

// ---- ImadsConfig (JS via WASM) ----

actual class ImadsConfig private constructor(internal val wasm: ImadsWasm.WasmEngineConfig) : AutoCloseable {
    actual companion object {
        actual fun fromPreset(name: String): ImadsConfig =
            ImadsConfig(ImadsWasm.WasmEngineConfig.fromPreset(name))

        actual fun presetNames(): List<String> {
            val arr = ImadsWasm.WasmEngineConfig.presetNames()
            return (0 until arr.size).map { arr[it].toString() }
        }
    }

    actual override fun close() {
        wasm.free()
    }
}

// ---- ImadsEngine (JS via WASM) ----

actual class ImadsEngine actual constructor() : AutoCloseable {
    private val wasm = ImadsWasm.WasmEngine()

    actual fun run(cfg: ImadsConfig, env: ImadsEnv, workers: Int): ImadsOutput {
        val wasmEnv = ImadsWasm.WasmEnv(
            env.runId.toDouble(), env.configHash.toDouble(),
            env.dataSnapshotId.toDouble(), env.rngMasterSeed.toDouble(),
        )
        val out = wasm.run(cfg.wasm, wasmEnv)
        return extractOutput(out)
    }

    actual fun run(
        cfg: ImadsConfig,
        env: ImadsEnv,
        evaluator: ImadsEvaluator,
        numConstraints: Int,
        workers: Int,
    ): ImadsOutput {
        val wasmEnv = ImadsWasm.WasmEnv(
            env.runId.toDouble(), env.configHash.toDouble(),
            env.dataSnapshotId.toDouble(), env.rngMasterSeed.toDouble(),
        )
        val mcFn: (dynamic, Double, Int) -> dynamic = { x, tau, k ->
            val xArr = DoubleArray(x.length as Int) { i -> x[i] as Double }
            val result = evaluator.mcSample(xArr, tau.toLong(), 0, k)
            result.toTypedArray()
        }
        val cheapFn: (dynamic) -> Boolean = { x ->
            val xArr = DoubleArray(x.length as Int) { i -> x[i] as Double }
            evaluator.cheapConstraints(xArr)
        }
        val out = wasm.runWithEvaluator(cfg.wasm, wasmEnv, mcFn, numConstraints, cheapFn)
        return extractOutput(out)
    }

    actual override fun close() {
        wasm.free()
    }
}

private fun extractOutput(out: dynamic): ImadsOutput {
    val fBestRaw = out.fBest
    val fBest: Double? = if (fBestRaw == null || fBestRaw == js("null")) null else fBestRaw as Double
    val xBestArr = out.xBest as? LongArray ?: longArrayOf()
    return ImadsOutput(
        fBest = fBest,
        xBest = xBestArr,
        truthEvals = (out.truthEvals as Number).toLong(),
        partialSteps = (out.partialSteps as Number).toLong(),
        cheapRejects = (out.cheapRejects as Number).toLong(),
        invalidEvalRejects = (out.invalidEvalRejects as Number).toLong(),
    )
}
