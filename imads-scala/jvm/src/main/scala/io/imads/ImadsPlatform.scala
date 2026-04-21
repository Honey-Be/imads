package io.imads

import java.lang.foreign.*
import java.lang.foreign.ValueLayout.*
import java.lang.invoke.{MethodHandle, MethodHandles, MethodType}

/** FFM (Foreign Function & Memory) bindings to imads_jvm shared library. */
private object FFM:
  private val linker: Linker = Linker.nativeLinker()
  private val lookup: SymbolLookup = SymbolLookup.libraryLookup(
    System.mapLibraryName("imads_jvm"),
    Arena.global(),
  )

  private def findOrThrow(name: String): MemorySegment =
    lookup.find(name).orElseThrow(() => new UnsatisfiedLinkError(s"Missing symbol: $name"))

  // ImadsEnv struct: 4 x uint64_t
  val envLayout: StructLayout = MemoryLayout.structLayout(
    JAVA_LONG.withName("run_id"),
    JAVA_LONG.withName("config_hash"),
    JAVA_LONG.withName("data_snapshot_id"),
    JAVA_LONG.withName("rng_master_seed"),
  )

  // Stats sub-struct (8 x uint64_t, first 4 are meaningful)
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

  // ImadsEvaluatorVTable struct
  val vtableLayout: StructLayout = MemoryLayout.structLayout(
    ADDRESS.withName("cheap_constraints"),
    ADDRESS.withName("mc_sample"),
    JAVA_LONG.withName("num_constraints"),
    JAVA_LONG.withName("search_dim"),
    ADDRESS.withName("user_data"),
  )

  val cheapConstraintsFD: FunctionDescriptor = FunctionDescriptor.of(
    JAVA_INT, ADDRESS, JAVA_LONG, ADDRESS,
  )
  val mcSampleFD: FunctionDescriptor = FunctionDescriptor.ofVoid(
    ADDRESS, JAVA_LONG, JAVA_LONG, JAVA_INT, JAVA_INT,
    ADDRESS, ADDRESS, JAVA_LONG, ADDRESS,
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

  val engineRunWithEvaluator: MethodHandle = linker.downcallHandle(
    findOrThrow("imads_engine_run_with_evaluator"),
    FunctionDescriptor.of(outputLayout, ADDRESS, ADDRESS, envLayout, JAVA_INT, vtableLayout),
  )

  val statsOffset: Long = outputLayout.byteOffset(MemoryLayout.PathElement.groupElement("stats"))
end FFM

// ---- Upcall callback holders ----

private class McSampleCallback(eval: Evaluator):
  def invoke(
      x: MemorySegment, dim: Long, tau: Long, smc: Int, k: Int,
      fOut: MemorySegment, cOut: MemorySegment, m: Long, userData: MemorySegment,
  ): Unit =
    val xArr = Array.tabulate(dim.toInt)(i =>
      x.reinterpret(dim * JAVA_DOUBLE.byteSize()).getAtIndex(JAVA_DOUBLE, i.toLong))
    val result = eval.mcSample(xArr, tau, smc, k)
    fOut.reinterpret(JAVA_DOUBLE.byteSize()).set(JAVA_DOUBLE, 0, result(0))
    val mInt = m.toInt
    val cSized = cOut.reinterpret(m * JAVA_DOUBLE.byteSize())
    for j <- 0 until mInt do
      cSized.setAtIndex(JAVA_DOUBLE, j.toLong, if j + 1 < result.length then result(j + 1) else 0.0)

private class CheapConstraintsCallback(eval: Evaluator):
  def invoke(x: MemorySegment, dim: Long, userData: MemorySegment): Int =
    val xArr = Array.tabulate(dim.toInt)(i =>
      x.reinterpret(dim * JAVA_DOUBLE.byteSize()).getAtIndex(JAVA_DOUBLE, i.toLong))
    if eval.cheapConstraints(xArr) then 1 else 0

/** JVM platform backend via FFM (Foreign Function & Memory). */
object ImadsPlatform extends ImadsPlatformOps:
  type ConfigHandle = MemorySegment
  type EngineHandle = MemorySegment

  def presetNames: Seq[String] =
    Seq("legacy_baseline", "balanced", "conservative", "throughput")

  def configFromPreset(name: String): MemorySegment =
    val arena = Arena.ofConfined()
    try
      val cName = arena.allocateFrom(name)
      val p = FFM.configFromPreset.invoke(cName).asInstanceOf[MemorySegment]
      require(p != MemorySegment.NULL, s"Unknown preset: $name")
      p
    finally arena.close()

  def configFree(handle: MemorySegment): Unit =
    if handle != MemorySegment.NULL then
      FFM.configFree.invoke(handle)

  def engineNew(): MemorySegment =
    FFM.engineNew.invoke().asInstanceOf[MemorySegment]

  def engineFree(handle: MemorySegment): Unit =
    if handle != MemorySegment.NULL then
      FFM.engineFree.invoke(handle)

  def engineRun(engine: MemorySegment, cfg: MemorySegment, env: Env, workers: Int): Output =
    val arena = Arena.ofConfined()
    try
      val envSeg = arena.allocate(FFM.envLayout)
      envSeg.set(JAVA_LONG, 0, env.runId)
      envSeg.set(JAVA_LONG, 8, env.configHash)
      envSeg.set(JAVA_LONG, 16, env.dataSnapshotId)
      envSeg.set(JAVA_LONG, 24, env.rngMasterSeed)

      val outSeg = FFM.engineRun.invoke(engine, cfg, envSeg, workers).asInstanceOf[MemorySegment]
      extractOutput(outSeg)
    finally arena.close()

  def engineRun(engine: MemorySegment, cfg: MemorySegment, env: Env,
                evaluator: Evaluator, numConstraints: Int, workers: Int): Output =
    val arena = Arena.ofShared()
    try
      val envSeg = arena.allocate(FFM.envLayout)
      envSeg.set(JAVA_LONG, 0, env.runId)
      envSeg.set(JAVA_LONG, 8, env.configHash)
      envSeg.set(JAVA_LONG, 16, env.dataSnapshotId)
      envSeg.set(JAVA_LONG, 24, env.rngMasterSeed)

      // mc_sample upcall stub
      val mcCallback = McSampleCallback(evaluator)
      val mcHandle = MethodHandles.lookup().findVirtual(
        classOf[McSampleCallback], "invoke",
        MethodType.methodType(
          Void.TYPE,
          classOf[MemorySegment], java.lang.Long.TYPE,
          java.lang.Long.TYPE, Integer.TYPE,
          Integer.TYPE,
          classOf[MemorySegment], classOf[MemorySegment],
          java.lang.Long.TYPE, classOf[MemorySegment],
        ),
      ).bindTo(mcCallback)
      val mcStub = FFM.linker.upcallStub(mcHandle, FFM.mcSampleFD, arena)

      // cheap_constraints upcall stub
      val cheapCallback = CheapConstraintsCallback(evaluator)
      val cheapHandle = MethodHandles.lookup().findVirtual(
        classOf[CheapConstraintsCallback], "invoke",
        MethodType.methodType(
          Integer.TYPE,
          classOf[MemorySegment], java.lang.Long.TYPE,
          classOf[MemorySegment],
        ),
      ).bindTo(cheapCallback)
      val cheapStub = FFM.linker.upcallStub(cheapHandle, FFM.cheapConstraintsFD, arena)

      // Build vtable struct
      val vtableSeg = arena.allocate(FFM.vtableLayout)
      vtableSeg.set(ADDRESS, 0, cheapStub)
      vtableSeg.set(ADDRESS, 8, mcStub)
      vtableSeg.set(JAVA_LONG, 16, numConstraints.toLong)
      vtableSeg.set(JAVA_LONG, 24, evaluator.searchDim.getOrElse(0).toLong)
      vtableSeg.set(ADDRESS, 32, MemorySegment.NULL)

      val outSeg = FFM.engineRunWithEvaluator.invoke(
        engine, cfg, envSeg, workers.asInstanceOf[AnyRef], vtableSeg,
      ).asInstanceOf[MemorySegment]
      extractOutput(outSeg)
    finally arena.close()

  private def extractOutput(seg: MemorySegment): Output =
    val fBest = seg.get(JAVA_DOUBLE, 0)
    val fBestValid = seg.get(JAVA_INT, 8)
    val xBestPtr = seg.get(ADDRESS, 16)
    val xBestLen = seg.get(JAVA_LONG, 16 + ADDRESS.byteSize()).toInt

    val xBest =
      if xBestLen > 0 && xBestPtr != MemorySegment.NULL then
        val sized = xBestPtr.reinterpret(xBestLen * JAVA_LONG.byteSize())
        Array.tabulate(xBestLen)(i => sized.getAtIndex(JAVA_LONG, i.toLong))
      else
        Array.empty[Long]

    val statsOff = FFM.statsOffset
    Output(
      fBest = if fBestValid != 0 then Some(fBest) else None,
      xBest = xBest,
      truthEvals = seg.get(JAVA_LONG, statsOff),
      partialSteps = seg.get(JAVA_LONG, statsOff + 8),
      cheapRejects = seg.get(JAVA_LONG, statsOff + 16),
      invalidEvalRejects = seg.get(JAVA_LONG, statsOff + 24),
    )
