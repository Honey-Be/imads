package io.imads

/**
 * Cross-platform IMADS engine configuration.
 *
 * Use [ImadsConfig.fromPreset] to create.
 * Must be [close]d after use to release native resources.
 */
expect class ImadsConfig : AutoCloseable {
    companion object {
        /** Create from preset name: "legacy_baseline", "balanced", "conservative", "throughput". */
        fun fromPreset(name: String): ImadsConfig

        /** List all available preset names. */
        fun presetNames(): List<String>
    }

    override fun close()
}

/**
 * Cross-platform IMADS optimization engine.
 *
 * Must be [close]d after use to release native resources.
 *
 * ```kotlin
 * ImadsConfig.fromPreset("balanced").use { cfg ->
 *     ImadsEngine().use { engine ->
 *         val output = engine.run(cfg, ImadsEnv(runId = 1), workers = 4)
 *         println(output.fBest)
 *     }
 * }
 * ```
 */
expect class ImadsEngine() : AutoCloseable {
    /** Run with built-in toy evaluator. */
    fun run(cfg: ImadsConfig, env: ImadsEnv, workers: Int = 1): ImadsOutput

    /** Run with custom evaluator. */
    fun run(
        cfg: ImadsConfig,
        env: ImadsEnv,
        evaluator: ImadsEvaluator,
        numConstraints: Int,
        workers: Int = 1,
    ): ImadsOutput

    override fun close()
}

/**
 * DSL: auto-close config and engine, run with toy evaluator.
 */
inline fun imadsRun(
    preset: String = "balanced",
    env: ImadsEnv = ImadsEnv(),
    workers: Int = 1,
    block: (ImadsOutput) -> Unit,
) {
    ImadsConfig.fromPreset(preset).use { cfg ->
        ImadsEngine().use { engine ->
            block(engine.run(cfg, env, workers))
        }
    }
}

/**
 * DSL: auto-close config and engine, run with custom evaluator.
 */
inline fun imadsRun(
    preset: String = "balanced",
    env: ImadsEnv = ImadsEnv(),
    evaluator: ImadsEvaluator,
    numConstraints: Int,
    workers: Int = 1,
    block: (ImadsOutput) -> Unit,
) {
    ImadsConfig.fromPreset(preset).use { cfg ->
        ImadsEngine().use { engine ->
            block(engine.run(cfg, env, evaluator, numConstraints, workers))
        }
    }
}
