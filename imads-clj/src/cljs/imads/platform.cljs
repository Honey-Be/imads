(ns imads.platform
  "CLJS platform backend via WASM Component Model (imads-wasm npm package).

  The imads-wasm package is built as a WASM Component Model component and
  transpiled to JS via jco. The engine interface is exported as module-level
  functions under the 'engine' namespace."
  (:require ["imads-wasm" :as wasm]))

(def ^:private engine (.-engine wasm))

(defn preset-names []
  ["legacy_baseline" "balanced" "conservative" "throughput"])

(defn config-from-preset [name]
  ;; With Component Model, config is just the preset name (kebab-case).
  (when-not (some #{name} (preset-names))
    (throw (js/Error. (str "Unknown preset: " name))))
  (clojure.string/replace name "_" "-"))

(defn config-free [_handle]
  ;; No-op: Component Model manages resources automatically.
  nil)

(defn engine-new []
  ;; No-op: engine functions are stateless module-level exports.
  nil)

(defn engine-free [_handle]
  nil)

(defn- build-wit-env [env-data]
  #js {:runId (:run-id env-data)
       :configHash (:config-hash env-data)
       :dataSnapshotId (:data-snapshot-id env-data)
       :rngMasterSeed (:rng-master-seed env-data)})

(defn- extract-output [out]
  (let [f-best-arr (.-fBest out)
        f-best (when (and f-best-arr (pos? (.-length f-best-arr)))
                 (aget f-best-arr 0))
        x-best-arr (.-xBest out)
        x-best (if x-best-arr (vec (Array/from x-best-arr)) [])
        stats (.-stats out)]
    {:f-best f-best
     :x-best x-best
     :truth-evals (.-truthEvals stats)
     :partial-steps (.-partialSteps stats)
     :cheap-rejects (.-cheapRejects stats)
     :invalid-eval-rejects (.-invalidEvalRejects stats)}))

(defn engine-run [_engine cfg env-data workers]
  (let [wit-env (build-wit-env env-data)]
    (extract-output (.run engine cfg wit-env workers))))

(defn engine-run-with-evaluator [_engine cfg env-data mc-sample-fn num-constraints cheap-fn workers]
  (let [wit-env (build-wit-env env-data)
        wit-eval #js {:mcSample (fn [x tau smc k]
                                  (clj->js (mc-sample-fn (js->clj (Array/from x)) tau smc k)))
                      :cheapConstraints (fn [x]
                                          (boolean
                                            (if cheap-fn
                                              (cheap-fn (js->clj (Array/from x)))
                                              true)))
                      :numConstraints (fn [] num-constraints)
                      :searchDim (fn [] js/undefined)}]
    (extract-output (.runWithEvaluator engine cfg wit-env workers wit-eval))))
