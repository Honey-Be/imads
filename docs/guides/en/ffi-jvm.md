# Cross-Platform FFI Guide (Kotlin, Scala 3, Clojure)

Each language provides a **single unified API** that works identically across all supported targets.

| Language | JVM (FFM) | JS (WASM) | Native (C FFI) |
|----------|:---------:|:---------:|:--------------:|
| **Kotlin** | `jvmMain` | `jsMain` | `nativeMain` |
| **Scala 3** | `jvm/` | `js/` (Scala.js) | `native/` (Scala Native) |
| **Clojure** | `clj/` | `cljs/` (ClojureScript) | — |

> **Note:** JVM targets have migrated from JNI (`imads-jni`, now removed) to **FFM**
> (Foreign Function & Memory API, JDK 22+). The `imads-jvm` crate provides the FFM
> bridge. Existing user-facing APIs are unchanged; only the internal binding mechanism
> has been replaced.

## Architecture

```
           ┌─────────────────────────────────────────┐
           │     Common API (platform-independent)     │
           │  Types, Evaluator interface, run() DSL    │
           └───────┬───────────┬───────────┬──────────┘
                   │           │           │
            ┌──────┴──┐  ┌────┴────┐  ┌───┴───────┐
            │ JVM/FFM │  │ JS/WASM │  │ Native/C  │
            │imads-jvm│  │imads-wasm│  │ imads-ffi │
            └──────┬──┘  └────┬────┘  └───┬───────┘
                   └──────────┴────────────┘
                         imads-core (Rust)
```

## Building Native Libraries

```bash
# FFM shared library (JVM targets — requires JDK 22+)
cargo build -p imads-ffi --release

# Compile FFM Java bridge
javac -d imads-jvm/target imads-jvm/src/main/java/io/imads/*.java

# C static/shared library (Kotlin/Native, Scala Native)
cargo build -p imads-ffi --release

# WASM module (JS targets)
cd imads-wasm && cargo component build --release
```

## JVM FFM Requirements

The FFM API requires **JDK 22 or later**. At runtime, the JVM must be able to locate
the `libimads_ffi` shared library:

```bash
# Pass the library path at launch
java -Djava.library.path=target/release -cp imads-jvm/target your.MainClass

# Or set LD_LIBRARY_PATH / DYLD_LIBRARY_PATH
export LD_LIBRARY_PATH=$PWD/target/release:$LD_LIBRARY_PATH
```

FFM replaces JNI entirely. The `imads-jni` crate has been removed.

---

## Kotlin Multiplatform

Project: `imads-kotlin/` (Gradle KMP)

### API (identical on all targets)

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
    override fun searchDim(): Int? = null  // None = infer from config or incumbent
}
imadsRun(evaluator = evaluator, numConstraints = 2, workers = 4) { println(it) }
```

### Source Layout

```
imads-kotlin/src/
├── commonMain/kotlin/io/imads/    # expect declarations + shared types
│   ├── ImadsTypes.kt              # ImadsEnv, ImadsOutput, ImadsEvaluator
│   └── ImadsEngine.kt             # expect ImadsConfig, expect ImadsEngine, imadsRun()
├── jvmMain/kotlin/io/imads/       # actual via FFM (imads-jvm, JDK 22+)
│   └── ImadsEngine.jvm.kt
├── jsMain/kotlin/io/imads/        # actual via imads-wasm
│   └── ImadsEngine.js.kt
└── nativeMain/kotlin/io/imads/    # actual via C FFI (cinterop)
    └── ImadsEngine.native.kt
```

---

## Scala 3

Project: `imads-scala/` (sbt, cross-compiled)

### API (identical on all targets)

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
  def searchDim: Option[Int] = None  // None = infer from config or incumbent

Imads.run("balanced", evaluator = Some((eval, 2)), workers = 4) { output =>
  println(s"f_best = ${output.fBest}")
}

// Presets
Imads.presetNames // Seq("legacy_baseline", "balanced", "conservative", "throughput")
```

### Source Layout

```
imads-scala/
├── shared/src/main/scala/io/imads/    # Pure types + API trait
│   ├── Types.scala                     # Env, Output case classes
│   ├── Evaluator.scala                 # Evaluator trait
│   └── Imads.scala                     # Imads object + ImadsPlatformOps trait
├── jvm/src/main/scala/io/imads/       # ImadsPlatform via FFM (JDK 22+)
│   └── ImadsPlatform.scala
├── js/src/main/scala/io/imads/        # ImadsPlatform via Scala.js → WASM
│   └── ImadsPlatform.scala
└── native/src/main/scala/io/imads/    # ImadsPlatform via Scala Native → C FFI
    └── ImadsPlatform.scala
```

---

## Clojure / ClojureScript

Project: `imads-clj/`

### API (identical on CLJ and CLJS)

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

### Source Layout

```
imads-clj/src/
├── cljc/imads/core.cljc       # Shared API (reader conditionals for platform dispatch)
├── clj/imads/platform.clj     # JVM backend via FFM (imads-jvm, JDK 22+)
└── cljs/imads/platform.cljs   # JS backend via WASM (imads-wasm)
```

The `.cljc` file uses `imads.platform` which is resolved to the correct backend
by the Clojure/ClojureScript compiler based on the source path.

---

## Thread Safety

Engine handles are **not** thread-safe across all platforms. Do not call `run`
concurrently on the same engine instance. The internal `AdaptiveExecutor`
manages its own worker threads (JVM/Native targets only; JS is single-threaded).
