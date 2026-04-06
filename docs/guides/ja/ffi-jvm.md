# Cross-Platform FFI ガイド (Kotlin, Scala 3, Clojure)

各言語は、すべてのサポート対象ターゲットで同一に動作する**単一の統一 API** を提供します。

| Language | JVM (JNI) | JS (WASM) | Native (C FFI) |
|----------|:---------:|:---------:|:--------------:|
| **Kotlin** | `jvmMain` | `jsMain` | `nativeMain` |
| **Scala 3** | `jvm/` | `js/` (Scala.js) | `native/` (Scala Native) |
| **Clojure** | `clj/` | `cljs/` (ClojureScript) | — |

## アーキテクチャ

```
           ┌─────────────────────────────────────────┐
           │     Common API (platform-independent)     │
           │  Types, Evaluator interface, run() DSL    │
           └───────┬───────────┬───────────┬──────────┘
                   │           │           │
            ┌──────┴──┐  ┌────┴────┐  ┌───┴───────┐
            │ JVM/JNI │  │ JS/WASM │  │ Native/C  │
            │ ImadsNative│ imads-wasm│  │ imads-ffi │
            └──────┬──┘  └────┬────┘  └───┬───────┘
                   └──────────┴────────────┘
                         imads-core (Rust)
```

## ネイティブライブラリのビルド

```bash
# JNI shared library (JVM targets)
cargo build -p imads-jni --release

# C static/shared library (Kotlin/Native, Scala Native)
cargo build -p imads-ffi --release

# WASM module (JS targets)
cd imads-wasm && wasm-pack build --target web --release
```

---

## Kotlin Multiplatform

プロジェクト: `imads-kotlin/` (Gradle KMP)

### API (すべてのターゲットで同一)

```kotlin
import io.imads.*

// DSL style
imadsRun(preset = "balanced", env = ImadsEnv(runId = 1), workers = 4) { output ->
    println("f_best = ${output.fBest}")
}

// Manual resource management
ImadsConfig.fromPreset("balanced").use { cfg ->
    ImadsEngine().use { engine ->
        val output = engine.run(cfg, ImadsEnv(runId = 1), workers = 4)
        println(output)
    }
}

// Custom evaluator
val evaluator = object : ImadsEvaluator {
    override fun mcSample(x: DoubleArray, tau: Long, smc: Int, k: Int): DoubleArray {
        val f = x.sumOf { it * it }
        return doubleArrayOf(f, x.sum() - 1.0, x.sum() - 2.0)
    }
    override fun searchDim(): Int? = null  // None = config または incumbent から推論
}
imadsRun(evaluator = evaluator, numConstraints = 2, workers = 4) { println(it) }
```

### ソースレイアウト

```
imads-kotlin/src/
├── commonMain/kotlin/io/imads/    # expect declarations + shared types
│   ├── ImadsTypes.kt              # ImadsEnv, ImadsOutput, ImadsEvaluator
│   └── ImadsEngine.kt             # expect ImadsConfig, expect ImadsEngine, imadsRun()
├── jvmMain/kotlin/io/imads/       # actual via JNI (ImadsNative)
│   └── ImadsEngine.jvm.kt
├── jsMain/kotlin/io/imads/        # actual via imads-wasm
│   └── ImadsEngine.js.kt
└── nativeMain/kotlin/io/imads/    # actual via C FFI (cinterop)
    └── ImadsEngine.native.kt
```

---

## Scala 3

プロジェクト: `imads-scala/` (sbt, クロスコンパイル)

### API (すべてのターゲットで同一)

```scala
import io.imads.*

// Simple run
Imads.run("balanced", workers = 4) { output =>
  println(s"f_best = ${output.fBest}")
}

// Custom evaluator
val eval = new Evaluator:
  def mcSample(x: Array[Double], tau: Long, smc: Int, k: Int): Array[Double] =
    val f = x.map(xi => xi * xi).sum
    Array(f, x.sum - 1, x.sum - 2)
  def searchDim: Option[Int] = None  // None = config または incumbent から推論

Imads.run("balanced", evaluator = Some((eval, 2)), workers = 4) { output =>
  println(s"f_best = ${output.fBest}")
}

// Presets
Imads.presetNames // Seq("legacy_baseline", "balanced", "conservative", "throughput")
```

### ソースレイアウト

```
imads-scala/
├── shared/src/main/scala/io/imads/    # Pure types + API trait
│   ├── Types.scala                     # Env, Output case classes
│   ├── Evaluator.scala                 # Evaluator trait
│   └── Imads.scala                     # Imads object + ImadsPlatformOps trait
├── jvm/src/main/scala/io/imads/       # ImadsPlatform via JNI
│   └── ImadsPlatform.scala
├── js/src/main/scala/io/imads/        # ImadsPlatform via Scala.js → WASM
│   └── ImadsPlatform.scala
└── native/src/main/scala/io/imads/    # ImadsPlatform via Scala Native → C FFI
    └── ImadsPlatform.scala
```

---

## Clojure / ClojureScript

プロジェクト: `imads-clj/`

### API (CLJ と CLJS で同一)

```clojure
(require '[imads.core :as imads])

;; Basic run
(imads/run {:preset "balanced"
            :workers 4
            :env {:run-id 1 :config-hash 2
                  :data-snapshot-id 3 :rng-master-seed 4}})
;; => {:f-best 0.0, :x-best [0 0 0], :truth-evals 42, ...}

;; Presets
(imads/preset-names)
;; => ["legacy_baseline" "balanced" "conservative" "throughput"]

;; Custom evaluator
(imads/run {:preset "balanced"
            :workers 4
            :env {:run-id 1}
            :evaluator {:mc-sample (fn [x tau smc k]
                                     (let [f (reduce + (map #(* % %) x))]
                                       #?(:clj  (double-array [f 0.0 0.0])
                                          :cljs (clj->js [f 0.0 0.0]))))
                        :num-constraints 2
                        :search-dim nil}})
```

### ソースレイアウト

```
imads-clj/src/
├── cljc/imads/core.cljc       # Shared API (reader conditionals for platform dispatch)
├── clj/imads/platform.clj     # JVM backend via JNI (ImadsNative)
└── cljs/imads/platform.cljs   # JS backend via WASM (imads-wasm)
```

`.cljc` ファイルは `imads.platform` を使用しており、Clojure/ClojureScript コンパイラがソースパスに基づいて正しいバックエンドに解決します。

---

## スレッドセーフティ

Engine ハンドルはすべてのプラットフォームにおいてスレッドセーフ**ではありません**。同一の Engine インスタンスに対して `run` を同時に呼び出さないでください。内部の `AdaptiveExecutor` は独自のワーカースレッドを管理します (JVM/Native ターゲットのみ。JS はシングルスレッドです)。
