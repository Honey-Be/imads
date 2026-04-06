(ns imads.platform
  "CLJS platform backend via WASM (imads-wasm npm package).

  Requires the 'imads-wasm' npm package built with --target bundler.
  Works with shadow-cljs (:npm-module, :esm), or any bundler that resolves
  npm packages via package.json exports (Webpack 5+, Vite)."
  (:require ["imads-wasm" :as wasm]))

(defn preset-names []
  (js->clj (.presetNames (.-EngineConfig wasm))))

(defn config-from-preset [name]
  (.fromPreset (.-EngineConfig wasm) name))

(defn config-free [handle]
  (.free handle))

(defn engine-new []
  (new (.-Engine wasm)))

(defn engine-free [handle]
  (.free handle))

(defn- extract-output [out]
  {:f-best (let [v (.-fBest out)] (when-not (nil? v) v))
   :x-best (vec (.-xBest out))
   :truth-evals (.-truthEvals out)
   :partial-steps (.-partialSteps out)
   :cheap-rejects (.-cheapRejects out)
   :invalid-eval-rejects (.-invalidEvalRejects out)})

(defn engine-run [engine cfg env-data workers]
  (let [env (new (.-Env wasm)
                 (:run-id env-data)
                 (:config-hash env-data)
                 (:data-snapshot-id env-data)
                 (:rng-master-seed env-data))]
    (extract-output (.run engine cfg env))))

(defn engine-run-with-evaluator [engine cfg env-data mc-sample-fn num-constraints cheap-fn workers]
  (let [env (new (.-Env wasm)
                 (:run-id env-data)
                 (:config-hash env-data)
                 (:data-snapshot-id env-data)
                 (:rng-master-seed env-data))
        js-mc (fn [x tau k]
                (clj->js (mc-sample-fn (js->clj (Array/from x)) tau 0 k)))
        js-cheap (when cheap-fn
                   (fn [x]
                     (boolean (cheap-fn (js->clj (Array/from x))))))]
    (extract-output
      (.runWithEvaluator engine cfg env js-mc num-constraints js-cheap))))
