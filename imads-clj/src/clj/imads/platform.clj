(ns imads.platform
  "CLJ (JVM) platform backend via JNI."
  (:import [io.imads ImadsNative ImadsJvmEvaluator]))

(defn preset-names []
  (vec (ImadsNative/presetNames)))

(defn config-from-preset [name]
  (let [p (ImadsNative/configFromPreset name)]
    (when (zero? p) (throw (IllegalArgumentException. (str "Unknown preset: " name))))
    p))

(defn config-free [handle]
  (ImadsNative/configFree handle))

(defn engine-new []
  (ImadsNative/engineNew))

(defn engine-free [handle]
  (ImadsNative/engineFree handle))

(defn- unpack-output [^longs packed]
  (let [f (Double/longBitsToDouble (aget packed 0))
        x-len (int (aget packed 1))
        x-best (long-array x-len)]
    (System/arraycopy packed 6 x-best 0 x-len)
    {:f-best (when-not (Double/isNaN f) f)
     :x-best (vec x-best)
     :truth-evals (aget packed 2)
     :partial-steps (aget packed 3)
     :cheap-rejects (aget packed 4)
     :invalid-eval-rejects (aget packed 5)}))

(defn engine-run [engine cfg env-data workers]
  (let [packed (ImadsNative/engineRun
                 engine cfg
                 (long (:run-id env-data))
                 (long (:config-hash env-data))
                 (long (:data-snapshot-id env-data))
                 (long (:rng-master-seed env-data))
                 (int workers))]
    (unpack-output packed)))

(defn engine-run-with-evaluator [engine cfg env-data mc-sample-fn num-constraints cheap-fn workers]
  (let [evaluator (reify ImadsJvmEvaluator
                    (mcSample [_ x tau smc k]
                      (mc-sample-fn (vec x) tau smc k))
                    (cheapConstraints [_ x]
                      (if cheap-fn
                        (boolean (cheap-fn (vec x)))
                        true)))
        packed (ImadsNative/engineRunWithEvaluator
                 engine cfg
                 (long (:run-id env-data))
                 (long (:config-hash env-data))
                 (long (:data-snapshot-id env-data))
                 (long (:rng-master-seed env-data))
                 (int workers)
                 evaluator
                 (int num-constraints))]
    (unpack-output packed)))
