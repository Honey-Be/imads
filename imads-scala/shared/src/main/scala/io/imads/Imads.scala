package io.imads

/**
 * Cross-platform IMADS engine API.
 *
 * The companion object provides the user-facing API. Platform-specific backends
 * are injected via [[ImadsPlatform]].
 *
 * {{{
 * import io.imads.*
 *
 * Imads.run("balanced", workers = 4) { output =>
 *   println(s"f_best = $${output.fBest}")
 * }
 * }}}
 */
object Imads:
  /** Available preset names. */
  def presetNames: Seq[String] = ImadsPlatform.presetNames

  /**
   * Run the engine with resource management.
   *
   * @param preset    preset name (default "balanced")
   * @param env       environment descriptor
   * @param workers   parallel worker count (JS targets ignore this)
   * @param evaluator optional (evaluator, numConstraints)
   * @param block     callback receiving the output
   */
  def run[A](
      preset: String = "balanced",
      env: Env = Env(),
      workers: Int = 1,
      evaluator: Option[(Evaluator, Int)] = None,
  )(block: Output => A): A =
    val cfg = ImadsPlatform.configFromPreset(preset)
    try
      val engine = ImadsPlatform.engineNew()
      try
        val output = evaluator match
          case Some((eval, m)) => ImadsPlatform.engineRun(engine, cfg, env, eval, m, workers)
          case None            => ImadsPlatform.engineRun(engine, cfg, env, workers)
        block(output)
      finally ImadsPlatform.engineFree(engine)
    finally ImadsPlatform.configFree(cfg)

end Imads

/**
 * Platform-specific backend. Implemented differently on JVM, JS, and Native.
 */
trait ImadsPlatformOps:
  type ConfigHandle
  type EngineHandle

  def presetNames: Seq[String]
  def configFromPreset(name: String): ConfigHandle
  def configFree(handle: ConfigHandle): Unit
  def engineNew(): EngineHandle
  def engineFree(handle: EngineHandle): Unit
  def engineRun(engine: EngineHandle, cfg: ConfigHandle, env: Env, workers: Int): Output
  def engineRun(engine: EngineHandle, cfg: ConfigHandle, env: Env,
                evaluator: Evaluator, numConstraints: Int, workers: Int): Output
