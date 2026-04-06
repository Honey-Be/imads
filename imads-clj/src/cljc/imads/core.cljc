(ns imads.core
  "Cross-platform IMADS wrapper. Identical API on CLJ (JNI) and CLJS (WASM).

  Usage:
    (require '[imads.core :as imads])

    ;; Basic run
    (imads/run {:preset \"balanced\" :workers 4
                :env {:run-id 1 :config-hash 2
                      :data-snapshot-id 3 :rng-master-seed 4}})

    ;; Custom evaluator
    (imads/run {:preset \"balanced\" :workers 4
                :env {:run-id 1}
                :evaluator {:mc-sample (fn [x tau smc k]
                                         (let [f (reduce + (map #(* % %) x))]
                                           #?(:clj  (double-array (cons f (repeat 2 0.0)))
                                              :cljs (clj->js (cons f (repeat 2 0.0))))))
                            :num-constraints 2}})"
  (:require [imads.platform :as platform]))

(defn preset-names
  "Return available preset names."
  []
  (platform/preset-names))

(defn run
  "Run the IMADS engine. Returns a result map.

  Options:
    :preset           — preset name (default \"balanced\")
    :workers          — parallel workers (default 1, ignored on CLJS)
    :env              — map with :run-id, :config-hash, :data-snapshot-id, :rng-master-seed
    :evaluator        — optional map with :mc-sample fn, :num-constraints, and optional :cheap-constraints fn"
  [{:keys [preset workers env evaluator]
    :or {preset "balanced" workers 1}}]
  (let [cfg (platform/config-from-preset preset)]
    (try
      (let [engine (platform/engine-new)]
        (try
          (let [env-data (merge {:run-id 1 :config-hash 0
                                 :data-snapshot-id 0 :rng-master-seed 0}
                                env)
                output (if evaluator
                         (platform/engine-run-with-evaluator
                           engine cfg env-data
                           (:mc-sample evaluator)
                           (:num-constraints evaluator)
                           (:cheap-constraints evaluator)
                           workers)
                         (platform/engine-run engine cfg env-data workers))]
            output)
          (finally (platform/engine-free engine))))
      (finally (platform/config-free cfg)))))
