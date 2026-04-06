package io.imads

import scala.scalanative.unsafe.*
import scala.scalanative.unsigned.*

/** C FFI bindings for Scala Native. */
@extern
@link("imads_ffi")
object ImadsCFFI:
  def imads_config_from_preset(name: CString): Ptr[Byte] = extern
  def imads_config_free(cfg: Ptr[Byte]): Unit = extern
  def imads_engine_new(): Ptr[Byte] = extern
  def imads_engine_free(engine: Ptr[Byte]): Unit = extern

  // Simplified: run with toy evaluator only.
  // Full vtable support requires additional struct bindings.
  def imads_engine_run(
      engine: Ptr[Byte], cfg: Ptr[Byte],
      env: Ptr[Byte], workers: CUnsignedInt,
  ): CStruct7[CDouble, CBool, Ptr[CLong], CSize,
              CStruct8[CUnsignedLongLong, CUnsignedLongLong, CUnsignedLongLong,
                       CUnsignedLongLong, CUnsignedLongLong, CUnsignedLongLong,
                       CUnsignedLongLong, CUnsignedLongLong],
              Byte, Byte] = extern // simplified

/** Scala Native platform backend via C FFI. */
object ImadsPlatform extends ImadsPlatformOps:
  type ConfigHandle = Ptr[Byte]
  type EngineHandle = Ptr[Byte]

  def presetNames: Seq[String] =
    Seq("legacy_baseline", "balanced", "conservative", "throughput")

  def configFromPreset(name: String): Ptr[Byte] = Zone { implicit z =>
    val p = ImadsCFFI.imads_config_from_preset(toCString(name))
    if p == null then throw new IllegalArgumentException(s"Unknown preset: $name")
    p
  }

  def configFree(handle: Ptr[Byte]): Unit = ImadsCFFI.imads_config_free(handle)
  def engineNew(): Ptr[Byte] = ImadsCFFI.imads_engine_new()
  def engineFree(handle: Ptr[Byte]): Unit = ImadsCFFI.imads_engine_free(handle)

  def engineRun(engine: Ptr[Byte], cfg: Ptr[Byte], env: Env, workers: Int): Output =
    Zone { implicit z =>
      // Allocate ImadsEnv struct (4 x uint64_t)
      val envBuf = alloc[CUnsignedLongLong](4)
      envBuf(0) = env.runId.toULong
      envBuf(1) = env.configHash.toULong
      envBuf(2) = env.dataSnapshotId.toULong
      envBuf(3) = env.rngMasterSeed.toULong

      val out = ImadsCFFI.imads_engine_run(engine, cfg, envBuf.asInstanceOf[Ptr[Byte]], workers.toUInt)
      // Extract from C struct — simplified for demonstration.
      // Production code would use proper struct field accessors.
      Output(
        fBest = None, // TODO: extract from C struct
        xBest = Array.empty,
        truthEvals = 0L,
        partialSteps = 0L,
        cheapRejects = 0L,
        invalidEvalRejects = 0L,
      )
    }

  def engineRun(engine: Ptr[Byte], cfg: Ptr[Byte], env: Env,
                evaluator: Evaluator, numConstraints: Int, workers: Int): Output =
    // Scala Native custom evaluator requires C function pointer vtable setup.
    // For now, fall back to toy evaluator with a warning.
    System.err.println("Warning: custom evaluator not yet supported on Scala Native; using toy evaluator")
    engineRun(engine, cfg, env, workers)
