# Plattformuebergreifender FFI-Leitfaden (Kotlin, Scala 3, Clojure)

Jede Sprache bietet eine **einheitliche API**, die auf allen unterstuetzten Zielplattformen identisch funktioniert.

| Sprache | JVM (FFM) | JS (WASM) | Native (C FFI) |
|---------|:---------:|:---------:|:--------------:|
| **Kotlin** | `jvmMain` | `jsMain` | `nativeMain` |
| **Scala 3** | `jvm/` | `js/` (Scala.js) | `native/` (Scala Native) |
| **Clojure** | `clj/` | `cljs/` (ClojureScript) | — |

> **Hinweis:** JVM-Ziele wurden von JNI (`imads-jni`, nun entfernt) auf **FFM**
> (Foreign Function & Memory API, JDK 22+) migriert. Das `imads-jvm`-Crate stellt die FFM-Bridge
> bereit. Bestehende nutzerseitige APIs bleiben unveraendert; nur der interne Binding-Mechanismus
> wurde ersetzt.

## Architektur

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

## Native Libraries erstellen

```bash
# FFM shared library (JVM-Ziele — erfordert JDK 22+)
cargo build -p imads-ffi --release

# Compile FFM Java bridge
javac -d imads-jvm/target imads-jvm/src/main/java/io/imads/*.java

# C static/shared library (Kotlin/Native, Scala Native)
cargo build -p imads-ffi --release

# WASM module (JS-Ziele)
cd imads-wasm && cargo component build --release
```

## JVM-FFM-Anforderungen

Die FFM-API erfordert **JDK 22 oder hoeher**. Zur Laufzeit muss die JVM die
`libimads_ffi`-Shared-Library finden koennen:

```bash
# Library-Pfad beim Start uebergeben
java -Djava.library.path=target/release -cp imads-jvm/target your.MainClass

# Oder LD_LIBRARY_PATH / DYLD_LIBRARY_PATH setzen
export LD_LIBRARY_PATH=$PWD/target/release:$LD_LIBRARY_PATH
```

FFM ersetzt JNI vollstaendig. Das `imads-jni`-Crate wurde entfernt.

---

## Kotlin Multiplatform

Projekt: `imads-kotlin/` (Gradle KMP)

### API (identisch auf allen Zielplattformen)

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
    override fun searchDim(): Int? = null  // None = aus Config oder Incumbent ableiten
}
imadsRun(evaluator = evaluator, numConstraints = 2, workers = 4) { println(it) }
```

### Verzeichnisstruktur

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

Projekt: `imads-scala/` (sbt, cross-compiled)

### API (identisch auf allen Zielplattformen)

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
  def searchDim: Option[Int] = None  // None = aus Config oder Incumbent ableiten

Imads.run("balanced", evaluator = Some((eval, 2)), workers = 4) { output =>
  println(s"f_best = ${output.fBest}")
}

// Presets
Imads.presetNames // Seq("legacy_baseline", "balanced", "conservative", "throughput")
```

### Verzeichnisstruktur

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

Projekt: `imads-clj/`

### API (identisch auf CLJ und CLJS)

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

### Verzeichnisstruktur

```
imads-clj/src/
├── cljc/imads/core.cljc       # Shared API (reader conditionals for platform dispatch)
├── clj/imads/platform.clj     # JVM backend via FFM (imads-jvm, JDK 22+)
└── cljs/imads/platform.cljs   # JS backend via WASM (imads-wasm)
```

Die `.cljc`-Datei verwendet `imads.platform`, das vom Clojure/ClojureScript-Compiler
anhand des Quellpfads zum korrekten Backend aufgeloest wird.

---

## Thread Safety

Engine-Handles sind auf keiner Plattform **thread-safe**. Rufen Sie `run` nicht
gleichzeitig auf derselben Engine-Instanz auf. Der interne `AdaptiveExecutor`
verwaltet seine eigenen Worker-Threads (nur bei JVM/Native-Zielen; JS ist single-threaded).
