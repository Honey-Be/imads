package io.imads

import scala.scalanative.unsafe.*
import scala.scalanative.unsigned.*
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

// Type alias for the ImadsOutput C struct returned by imads_engine_run.
private type CImadsOutput = CStruct7[CDouble, CBool, Ptr[CLong], CSize,
    CStruct8[CUnsignedLongLong, CUnsignedLongLong, CUnsignedLongLong,
             CUnsignedLongLong, CUnsignedLongLong, CUnsignedLongLong,
             CUnsignedLongLong, CUnsignedLongLong],
    Byte, Byte]

/** C FFI bindings for Scala Native. */
@extern
@link("imads_ffi")
object ImadsCFFI:
  def imads_config_from_preset(name: CString): Ptr[Byte] = extern
  def imads_config_free(cfg: Ptr[Byte]): Unit = extern
  def imads_engine_new(): Ptr[Byte] = extern
  def imads_engine_free(engine: Ptr[Byte]): Unit = extern

  def imads_engine_run(
      engine: Ptr[Byte], cfg: Ptr[Byte],
      env: Ptr[Byte], workers: CUnsignedInt,
  ): CImadsOutput = extern

  def imads_engine_run_with_evaluator_ptr(
      engine: Ptr[Byte], cfg: Ptr[Byte],
      env: Ptr[Byte], workers: CUnsignedInt,
      vtable: Ptr[Byte],
  ): CImadsOutput = extern

/** Thread-safe evaluator registry for C callback interop. */
private object EvaluatorRegistry:
  private val map = new ConcurrentHashMap[Long, Evaluator]()
  private val nextId = new AtomicLong(1)

  def register(eval: Evaluator): Long =
    val id = nextId.getAndIncrement()
    map.put(id, eval)
    id

  def get(id: Long): Evaluator = map.get(id)

  def unregister(id: Long): Unit = map.remove(id)

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
      val envBuf = alloc[CUnsignedLongLong](4)
      envBuf(0) = env.runId.toULong
      envBuf(1) = env.configHash.toULong
      envBuf(2) = env.dataSnapshotId.toULong
      envBuf(3) = env.rngMasterSeed.toULong

      val out = ImadsCFFI.imads_engine_run(engine, cfg, envBuf.asInstanceOf[Ptr[Byte]], workers.toUInt)
      extractOutput(out)
    }

  def engineRun(engine: Ptr[Byte], cfg: Ptr[Byte], env: Env,
                evaluator: Evaluator, numConstraints: Int, workers: Int): Output =
    Zone { implicit z =>
      val envBuf = alloc[CUnsignedLongLong](4)
      envBuf(0) = env.runId.toULong
      envBuf(1) = env.configHash.toULong
      envBuf(2) = env.dataSnapshotId.toULong
      envBuf(3) = env.rngMasterSeed.toULong

      val evalId = EvaluatorRegistry.register(evaluator)
      try
        // Allocate vtable: 5 fields x 8 bytes = 40 bytes
        // Layout: cheap_constraints(ptr), mc_sample(ptr), num_constraints(usize),
        //         search_dim(usize), user_data(ptr)
        val vtable = alloc[Byte](40)
        val vtableAsLong = vtable.asInstanceOf[Ptr[CLong]]

        // Store evaluator ID in user_data field — callbacks read it back via the registry
        val userDataPtr = alloc[CLong](1)
        !userDataPtr = evalId

        // cheap_constraints fn ptr at offset 0
        val cheapFnPtr = CFuncPtr.toPtr(CFuncPtr3.fromScalaFunction(cheapConstraintsCB))
        !(vtable.asInstanceOf[Ptr[Ptr[Byte]]]) = cheapFnPtr.asInstanceOf[Ptr[Byte]]
        // mc_sample fn ptr at offset 8
        !(vtable + 8).asInstanceOf[Ptr[Ptr[Byte]]] = CFuncPtr.toPtr(
            CFuncPtr9.fromScalaFunction(mcSampleCB)).asInstanceOf[Ptr[Byte]]
        // num_constraints at offset 16
        !(vtable + 16).asInstanceOf[Ptr[CSize]] = numConstraints.toULong
        // search_dim at offset 24
        !(vtable + 24).asInstanceOf[Ptr[CSize]] = evaluator.searchDim.getOrElse(0).toULong
        // user_data at offset 32
        !(vtable + 32).asInstanceOf[Ptr[Ptr[Byte]]] = userDataPtr.asInstanceOf[Ptr[Byte]]

        val out = ImadsCFFI.imads_engine_run_with_evaluator_ptr(
            engine, cfg, envBuf.asInstanceOf[Ptr[Byte]], workers.toUInt, vtable)
        extractOutput(out)
      finally EvaluatorRegistry.unregister(evalId)
    }

  // C callback: cheap_constraints(x, dim, user_data) -> i32
  private val cheapConstraintsCB: (Ptr[CDouble], CSize, Ptr[Byte]) => CInt =
    (x: Ptr[CDouble], dim: CSize, userData: Ptr[Byte]) =>
      val evalId = !(userData.asInstanceOf[Ptr[CLong]])
      val eval = EvaluatorRegistry.get(evalId)
      val xArr = Array.tabulate(dim.toInt)(i => x(i))
      if eval.cheapConstraints(xArr) then 1 else 0

  // C callback: mc_sample(x, dim, tau, smc, k, f_out, c_out, m, user_data) -> void
  private val mcSampleCB: (Ptr[CDouble], CSize, CUnsignedLongLong, CUnsignedInt, CUnsignedInt,
                           Ptr[CDouble], Ptr[CDouble], CSize, Ptr[Byte]) => Unit =
    (x: Ptr[CDouble], dim: CSize, tau: CUnsignedLongLong, smc: CUnsignedInt, k: CUnsignedInt,
     fOut: Ptr[CDouble], cOut: Ptr[CDouble], m: CSize, userData: Ptr[Byte]) =>
      val evalId = !(userData.asInstanceOf[Ptr[CLong]])
      val eval = EvaluatorRegistry.get(evalId)
      val xArr = Array.tabulate(dim.toInt)(i => x(i))
      val result = eval.mcSample(xArr, tau.toLong, smc.toInt, k.toInt)
      !fOut = result(0)
      val mInt = m.toInt
      for j <- 0 until mInt do
        cOut(j) = if j + 1 < result.length then result(j + 1) else 0.0

  private def extractOutput(out: CImadsOutput): Output =
    val fBest = out._1
    val fBestValid = out._2
    val xBestPtr = out._3
    val xBestLen = out._4.toInt
    val stats = out._5

    val xBest =
      if xBestLen > 0 && xBestPtr != null then
        Array.tabulate(xBestLen)(i => xBestPtr(i))
      else
        Array.empty[Long]

    Output(
      fBest = if fBestValid then Some(fBest) else None,
      xBest = xBest,
      truthEvals = stats._1.toLong,
      partialSteps = stats._4.toLong,
      cheapRejects = stats._7.toLong,
      invalidEvalRejects = stats._8.toLong,
    )
