package io.imads

import scala.scalajs.js
import scala.scalajs.js.annotation.*

/** Scala.js facade for imads-wasm exports.
 *
 * With --target bundler, wasm-bindgen produces ESM named exports.
 * Bundlers (Webpack 5+, Vite) resolve "imads-wasm" via package.json
 * exports field and handle .wasm loading automatically.
 */
@js.native
@JSImport("imads-wasm", JSImport.Namespace)
object ImadsWasm extends js.Object:
  @js.native
  @JSName("EngineConfig")
  class WasmConfig extends js.Object:
    def free(): Unit = js.native
  object WasmConfig:
    @JSName("fromPreset")
    def fromPreset(name: String): WasmConfig = js.native
    @JSName("presetNames")
    def presetNames(): js.Array[String] = js.native

  @js.native
  @JSName("Env")
  class WasmEnv(runId: Double, configHash: Double, dataSnapshotId: Double, rngMasterSeed: Double) extends js.Object

  @js.native
  @JSName("Engine")
  class WasmEngine() extends js.Object:
    def run(cfg: WasmConfig, env: WasmEnv): js.Dynamic = js.native
    def runWithEvaluator(
        cfg: WasmConfig, env: WasmEnv,
        mcSampleFn: js.Function3[js.typedarray.Float64Array, Double, Int, js.Array[Double]],
        numConstraints: Int,
        cheapFn: js.UndefOr[js.Function1[js.typedarray.Float64Array, Boolean]] = js.undefined,
    ): js.Dynamic = js.native
    def free(): Unit = js.native

/** Scala.js platform backend via WASM. */
object ImadsPlatform extends ImadsPlatformOps:
  type ConfigHandle = ImadsWasm.WasmConfig
  type EngineHandle = ImadsWasm.WasmEngine

  def presetNames: Seq[String] =
    ImadsWasm.WasmConfig.presetNames().toSeq

  def configFromPreset(name: String): ImadsWasm.WasmConfig =
    ImadsWasm.WasmConfig.fromPreset(name)

  def configFree(handle: ImadsWasm.WasmConfig): Unit = handle.free()
  def engineNew(): ImadsWasm.WasmEngine = new ImadsWasm.WasmEngine()
  def engineFree(handle: ImadsWasm.WasmEngine): Unit = handle.free()

  def engineRun(engine: ImadsWasm.WasmEngine, cfg: ImadsWasm.WasmConfig, env: Env, workers: Int): Output =
    val wasmEnv = new ImadsWasm.WasmEnv(
      env.runId.toDouble, env.configHash.toDouble,
      env.dataSnapshotId.toDouble, env.rngMasterSeed.toDouble,
    )
    extractOutput(engine.run(cfg, wasmEnv))

  def engineRun(engine: ImadsWasm.WasmEngine, cfg: ImadsWasm.WasmConfig, env: Env,
                evaluator: Evaluator, numConstraints: Int, workers: Int): Output =
    val wasmEnv = new ImadsWasm.WasmEnv(
      env.runId.toDouble, env.configHash.toDouble,
      env.dataSnapshotId.toDouble, env.rngMasterSeed.toDouble,
    )
    val mcFn: js.Function3[js.typedarray.Float64Array, Double, Int, js.Array[Double]] =
      (x: js.typedarray.Float64Array, tau: Double, k: Int) =>
        val xArr = (0 until x.length).map(x(_)).toArray
        val result = evaluator.mcSample(xArr, tau.toLong, 0, k)
        js.Array(result*)
    val cheapFn: js.UndefOr[js.Function1[js.typedarray.Float64Array, Boolean]] =
      js.defined { (x: js.typedarray.Float64Array) =>
        val xArr = (0 until x.length).map(x(_)).toArray
        evaluator.cheapConstraints(xArr)
      }
    extractOutput(engine.runWithEvaluator(cfg, wasmEnv, mcFn, numConstraints, cheapFn))

  private def extractOutput(out: js.Dynamic): Output =
    val fBest = out.fBest.asInstanceOf[Any]
    Output(
      fBest = fBest match
        case null          => None
        case d: Double     => Some(d)
        case _             => None
      ,
      xBest = out.xBest.asInstanceOf[js.Array[Any]].map(_.asInstanceOf[Number].longValue()).toArray,
      truthEvals = out.truthEvals.asInstanceOf[Number].longValue(),
      partialSteps = out.partialSteps.asInstanceOf[Number].longValue(),
      cheapRejects = out.cheapRejects.asInstanceOf[Number].longValue(),
      invalidEvalRejects = out.invalidEvalRejects.asInstanceOf[Number].longValue(),
    )
