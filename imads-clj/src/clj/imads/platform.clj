(ns imads.platform
  "CLJ (JVM) platform backend via FFM (Foreign Function & Memory)."
  (:import [java.lang.foreign
            Arena FunctionDescriptor Linker MemoryLayout MemorySegment
            SymbolLookup ValueLayout ValueLayout$OfDouble ValueLayout$OfInt
            ValueLayout$OfLong]))

;; ---- FFM setup: load imads_jvm native library and create downcall handles ----

(defonce ^:private linker (Linker/nativeLinker))

(defonce ^:private lib-lookup
  (SymbolLookup/libraryLookup
    (System/mapLibraryName "imads_jvm")
    (Arena/global)))

(defn- find-symbol ^MemorySegment [^String name]
  (-> (.find lib-lookup name)
      (.orElseThrow (reify java.util.function.Supplier
                      (get [_] (UnsatisfiedLinkError. (str "Missing symbol: " name)))))))

;; Struct layouts

(defonce ^:private env-layout
  (MemoryLayout/structLayout
    (.withName ValueLayout/JAVA_LONG "run_id")
    (.withName ValueLayout/JAVA_LONG "config_hash")
    (.withName ValueLayout/JAVA_LONG "data_snapshot_id")
    (.withName ValueLayout/JAVA_LONG "rng_master_seed")))

(defonce ^:private stats-layout
  (MemoryLayout/structLayout
    (.withName ValueLayout/JAVA_LONG "truth_evals")
    (.withName ValueLayout/JAVA_LONG "partial_steps")
    (.withName ValueLayout/JAVA_LONG "cheap_rejects")
    (.withName ValueLayout/JAVA_LONG "invalid_eval_rejects")
    (.withName ValueLayout/JAVA_LONG "_pad0")
    (.withName ValueLayout/JAVA_LONG "_pad1")
    (.withName ValueLayout/JAVA_LONG "_pad2")
    (.withName ValueLayout/JAVA_LONG "_pad3")))

(defonce ^:private output-layout
  (MemoryLayout/structLayout
    (.withName ValueLayout/JAVA_DOUBLE "f_best")
    (.withName ValueLayout/JAVA_INT "f_best_valid")
    (MemoryLayout/paddingLayout 4)
    (.withName ValueLayout/ADDRESS "x_best_ptr")
    (.withName ValueLayout/JAVA_LONG "x_best_len")
    (.withName stats-layout "stats")))

(defonce ^:private stats-offset
  (.byteOffset output-layout
    (into-array MemoryLayout$PathElement
      [(MemoryLayout$PathElement/groupElement "stats")])))

;; Downcall handles

(defonce ^:private h-config-from-preset
  (.downcallHandle linker
    (find-symbol "imads_config_from_preset")
    (FunctionDescriptor/of ValueLayout/ADDRESS ValueLayout/ADDRESS)))

(defonce ^:private h-config-free
  (.downcallHandle linker
    (find-symbol "imads_config_free")
    (FunctionDescriptor/ofVoid ValueLayout/ADDRESS)))

(defonce ^:private h-engine-new
  (.downcallHandle linker
    (find-symbol "imads_engine_new")
    (FunctionDescriptor/of ValueLayout/ADDRESS)))

(defonce ^:private h-engine-free
  (.downcallHandle linker
    (find-symbol "imads_engine_free")
    (FunctionDescriptor/ofVoid ValueLayout/ADDRESS)))

(defonce ^:private h-engine-run
  (.downcallHandle linker
    (find-symbol "imads_engine_run")
    (FunctionDescriptor/of output-layout ValueLayout/ADDRESS ValueLayout/ADDRESS env-layout ValueLayout/JAVA_INT)))

;; ---- Output extraction ----

(defn- extract-output [^MemorySegment seg]
  (let [f-best (.get seg ValueLayout/JAVA_DOUBLE 0)
        f-best-valid (.get seg ValueLayout/JAVA_INT 8)
        x-best-ptr (.get seg ValueLayout/ADDRESS 16)
        addr-size (.byteSize ValueLayout/ADDRESS)
        x-best-len (int (.get seg ValueLayout/JAVA_LONG (+ 16 addr-size)))
        x-best (if (and (pos? x-best-len)
                        (not= x-best-ptr (MemorySegment/NULL)))
                 (let [sized (.reinterpret x-best-ptr (* x-best-len (.byteSize ValueLayout/JAVA_LONG)))]
                   (long-array (map #(.getAtIndex sized ValueLayout/JAVA_LONG (long %))
                                    (range x-best-len))))
                 (long-array 0))
        so stats-offset]
    {:f-best (when-not (zero? f-best-valid) f-best)
     :x-best (vec x-best)
     :truth-evals (.get seg ValueLayout/JAVA_LONG so)
     :partial-steps (.get seg ValueLayout/JAVA_LONG (+ so 8))
     :cheap-rejects (.get seg ValueLayout/JAVA_LONG (+ so 16))
     :invalid-eval-rejects (.get seg ValueLayout/JAVA_LONG (+ so 24))}))

;; ---- Public API ----

(defn preset-names []
  ["legacy_baseline" "balanced" "conservative" "throughput"])

(defn config-from-preset [name]
  (let [arena (Arena/ofConfined)]
    (try
      (let [c-name (.allocateFrom arena ^String name)
            p (.invoke h-config-from-preset (object-array [c-name]))]
        (when (= p (MemorySegment/NULL))
          (throw (IllegalArgumentException. (str "Unknown preset: " name))))
        p)
      (finally (.close arena)))))

(defn config-free [^MemorySegment handle]
  (when-not (= handle (MemorySegment/NULL))
    (.invoke h-config-free (object-array [handle]))))

(defn engine-new []
  (.invoke h-engine-new (object-array [])))

(defn engine-free [^MemorySegment handle]
  (when-not (= handle (MemorySegment/NULL))
    (.invoke h-engine-free (object-array [handle]))))

(defn engine-run [engine cfg env-data workers]
  (let [arena (Arena/ofConfined)]
    (try
      (let [env-seg (.allocate arena env-layout)]
        (.set env-seg ValueLayout/JAVA_LONG 0 (long (:run-id env-data 1)))
        (.set env-seg ValueLayout/JAVA_LONG 8 (long (:config-hash env-data 0)))
        (.set env-seg ValueLayout/JAVA_LONG 16 (long (:data-snapshot-id env-data 0)))
        (.set env-seg ValueLayout/JAVA_LONG 24 (long (:rng-master-seed env-data 0)))
        (let [out-seg (.invoke h-engine-run (object-array [engine cfg env-seg (int workers)]))]
          (extract-output out-seg)))
      (finally (.close arena)))))

(defn engine-run-with-evaluator
  "Custom evaluator support via FFM is not yet implemented (requires upcall stubs).
   Falls back to toy evaluator."
  ([engine cfg env-data _mc-sample-fn _num-constraints _cheap-fn workers]
   (engine-run-with-evaluator engine cfg env-data _mc-sample-fn _num-constraints _cheap-fn nil workers))
  ([engine cfg env-data _mc-sample-fn _num-constraints _cheap-fn _search-dim-val workers]
   ;; TODO: Custom evaluator via FFM requires upcall stubs for callback function pointers.
   (binding [*err* *err*]
     (.println *err* "Warning: custom evaluator not yet supported via FFM; using toy evaluator"))
   (engine-run engine cfg env-data workers)))
