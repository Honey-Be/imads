package io.imads

import io.imads.{ImadsNative => JNI}

/** JVM platform backend via JNI. */
object ImadsPlatform extends ImadsPlatformOps:
  type ConfigHandle = Long
  type EngineHandle = Long

  def presetNames: Seq[String] = JNI.presetNames().toSeq

  def configFromPreset(name: String): Long =
    val p = JNI.configFromPreset(name)
    require(p != 0L, s"Unknown preset: $name")
    p

  def configFree(handle: Long): Unit = JNI.configFree(handle)
  def engineNew(): Long = JNI.engineNew()
  def engineFree(handle: Long): Unit = JNI.engineFree(handle)

  def engineRun(engine: Long, cfg: Long, env: Env, workers: Int): Output =
    val packed = JNI.engineRun(
      engine, cfg,
      env.runId, env.configHash, env.dataSnapshotId, env.rngMasterSeed,
      workers,
    )
    unpackOutput(packed)

  def engineRun(engine: Long, cfg: Long, env: Env,
                evaluator: Evaluator, numConstraints: Int, workers: Int): Output =
    val bridge = new ImadsJvmEvaluator:
      def mcSample(x: Array[Double], tau: Long, smc: Int, k: Int): Array[Double] =
        evaluator.mcSample(x, tau, smc, k)
      override def cheapConstraints(x: Array[Double]): Boolean =
        evaluator.cheapConstraints(x)
      override def searchDim(): Integer =
        evaluator.searchDim match
          case Some(d) => Integer.valueOf(d)
          case None    => null
    val packed = JNI.engineRunWithEvaluator(
      engine, cfg,
      env.runId, env.configHash, env.dataSnapshotId, env.rngMasterSeed,
      workers, bridge, numConstraints,
    )
    unpackOutput(packed)

  private def unpackOutput(packed: Array[Long]): Output =
    val f = java.lang.Double.longBitsToDouble(packed(0))
    val xLen = packed(1).toInt
    val xBest = new Array[Long](xLen)
    System.arraycopy(packed, 6, xBest, 0, xLen)
    Output(
      fBest = if f.isNaN then None else Some(f),
      xBest = xBest,
      truthEvals = packed(2),
      partialSteps = packed(3),
      cheapRejects = packed(4),
      invalidEvalRejects = packed(5),
    )
