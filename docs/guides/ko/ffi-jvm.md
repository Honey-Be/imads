# 크로스 플랫폼 FFI 가이드 (Kotlin, Scala 3, Clojure)

각 언어는 지원되는 모든 타겟에서 동일하게 작동하는 **단일 통합 API**를 제공합니다.

| Language | JVM (FFM) | JS (WASM) | Native (C FFI) |
|----------|:---------:|:---------:|:--------------:|
| **Kotlin** | `jvmMain` | `jsMain` | `nativeMain` |
| **Scala 3** | `jvm/` | `js/` (Scala.js) | `native/` (Scala Native) |
| **Clojure** | `clj/` | `cljs/` (ClojureScript) | — |

> **참고:** JVM 타겟은 JNI(`imads-jni`, 현재 제거됨)에서 **FFM**
> (Foreign Function & Memory API, JDK 22+)으로 마이그레이션되었습니다. `imads-jvm` 크레이트가
> FFM 브릿지를 제공합니다. 기존 사용자 대면 API는 변경되지 않았으며, 내부 바인딩 메커니즘만
> 교체되었습니다.

## 아키텍처

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

## 네이티브 라이브러리 빌드

```bash
# FFM 공유 라이브러리 (JVM 타겟 — JDK 22+ 필요)
cargo build -p imads-ffi --release

# FFM Java 브릿지 컴파일
javac -d imads-jvm/target imads-jvm/src/main/java/io/imads/*.java

# C 정적/공유 라이브러리 (Kotlin/Native, Scala Native)
cargo build -p imads-ffi --release

# WASM 모듈 (JS 타겟)
cd imads-wasm && cargo component build --release
```

## JVM FFM 요구 사항

FFM API는 **JDK 22 이상**이 필요합니다. 런타임에 JVM이 `libimads_ffi` 공유 라이브러리를
찾을 수 있어야 합니다:

```bash
# 실행 시 라이브러리 경로 전달
java -Djava.library.path=target/release -cp imads-jvm/target your.MainClass

# 또는 LD_LIBRARY_PATH / DYLD_LIBRARY_PATH 설정
export LD_LIBRARY_PATH=$PWD/target/release:$LD_LIBRARY_PATH
```

FFM은 JNI를 완전히 대체합니다. `imads-jni` 크레이트는 제거되었습니다.

---

## Kotlin Multiplatform

프로젝트: `imads-kotlin/` (Gradle KMP)

### API (모든 타겟에서 동일)

```kotlin
import io.imads.*

// DSL 스타일
imadsRun(preset = "balanced", env = ImadsEnv(runId = 1), workers = 4) { output ->
    println("f_best = ${output.fBest}")
}

// 수동 리소스 관리
ImadsConfig.fromPreset("balanced").use { cfg ->
    ImadsEngine().use { engine ->
        val output = engine.run(cfg, ImadsEnv(runId = 1), workers = 4)
        println(output)
    }
}

// 커스텀 evaluator
val evaluator = object : ImadsEvaluator {
    override fun mcSample(x: DoubleArray, tau: Long, smc: Int, k: Int): DoubleArray {
        val f = x.sumOf { it * it }
        return doubleArrayOf(f, x.sum() - 1.0, x.sum() - 2.0)
    }
    override fun searchDim(): Int? = null  // None = config 또는 incumbent에서 추론
}
imadsRun(evaluator = evaluator, numConstraints = 2, workers = 4) { println(it) }
```

### 소스 레이아웃

```
imads-kotlin/src/
├── commonMain/kotlin/io/imads/    # expect 선언 + 공유 타입
│   ├── ImadsTypes.kt              # ImadsEnv, ImadsOutput, ImadsEvaluator
│   └── ImadsEngine.kt             # expect ImadsConfig, expect ImadsEngine, imadsRun()
├── jvmMain/kotlin/io/imads/       # FFM을 통한 actual 구현 (imads-jvm, JDK 22+)
│   └── ImadsEngine.jvm.kt
├── jsMain/kotlin/io/imads/        # imads-wasm을 통한 actual 구현
│   └── ImadsEngine.js.kt
└── nativeMain/kotlin/io/imads/    # C FFI를 통한 actual 구현 (cinterop)
    └── ImadsEngine.native.kt
```

---

## Scala 3

프로젝트: `imads-scala/` (sbt, 크로스 컴파일)

### API (모든 타겟에서 동일)

```scala
import io.imads.*

// 간단한 실행
Imads.run("balanced", workers = 4) { output =>
  println(s"f_best = ${output.fBest}")
}

// 커스텀 evaluator
val eval = new Evaluator:
  def mcSample(x: Array[Double], tau: Long, smc: Int, k: Int): Array[Double] =
    val f = x.map(xi => xi * xi).sum
    Array(f, x.sum - 1, x.sum - 2)
  def searchDim: Option[Int] = None  // None = config 또는 incumbent에서 추론

Imads.run("balanced", evaluator = Some((eval, 2)), workers = 4) { output =>
  println(s"f_best = ${output.fBest}")
}

// 프리셋
Imads.presetNames // Seq("legacy_baseline", "balanced", "conservative", "throughput")
```

### 소스 레이아웃

```
imads-scala/
├── shared/src/main/scala/io/imads/    # 순수 타입 + API trait
│   ├── Types.scala                     # Env, Output case class
│   ├── Evaluator.scala                 # Evaluator trait
│   └── Imads.scala                     # Imads object + ImadsPlatformOps trait
├── jvm/src/main/scala/io/imads/       # FFM을 통한 ImadsPlatform (JDK 22+)
│   └── ImadsPlatform.scala
├── js/src/main/scala/io/imads/        # Scala.js → WASM을 통한 ImadsPlatform
│   └── ImadsPlatform.scala
└── native/src/main/scala/io/imads/    # Scala Native → C FFI를 통한 ImadsPlatform
    └── ImadsPlatform.scala
```

---

## Clojure / ClojureScript

프로젝트: `imads-clj/`

### API (CLJ와 CLJS에서 동일)

```clojure
(require '[imads.core :as imads])

;; 기본 실행
(imads/run {:preset "balanced"
            :workers 4
            :env {:run-id 1 :config-hash 2
                  :data-snapshot-id 3 :rng-master-seed 4}})
;; => {:f-best 0.0, :x-best [0 0 0], :truth-evals 42, ...}

;; 프리셋
(imads/preset-names)
;; => ["legacy_baseline" "balanced" "conservative" "throughput"]

;; 커스텀 evaluator
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

### 소스 레이아웃

```
imads-clj/src/
├── cljc/imads/core.cljc       # 공유 API (플랫폼 디스패치를 위한 reader conditional 사용)
├── clj/imads/platform.clj     # FFM을 통한 JVM 백엔드 (imads-jvm, JDK 22+)
└── cljs/imads/platform.cljs   # WASM을 통한 JS 백엔드 (imads-wasm)
```

`.cljc` 파일은 `imads.platform`을 사용하며, 이는 소스 경로에 따라 Clojure/ClojureScript 컴파일러가 올바른 백엔드로 해석합니다.

---

## 스레드 안전성

엔진 핸들은 모든 플랫폼에서 스레드에 **안전하지 않습니다**. 동일한 엔진 인스턴스에서 `run`을 동시에 호출하지 마십시오. 내부의 `AdaptiveExecutor`는 자체 워커 스레드를 관리합니다 (JVM/Native 타겟에서만 해당되며, JS는 싱글 스레드입니다).
