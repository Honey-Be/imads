(ns imads.platform
  "CLJ (JVM) platform backend via FFM (Foreign Function & Memory)."
  (:import [java.lang.foreign
            Arena FunctionDescriptor Linker MemoryLayout MemorySegment
            SymbolLookup ValueLayout ValueLayout$OfDouble ValueLayout$OfInt
            ValueLayout$OfLong]
           [java.lang.invoke MethodHandles MethodType]))

;; ---- Java interfaces for FFM upcall targets ----

(definterface IMcSampleCallback
  (^void invoke [^java.lang.foreign.MemorySegment x
                 ^long dim ^long tau ^int smc ^int k
                 ^java.lang.foreign.MemorySegment fOut
                 ^java.lang.foreign.MemorySegment cOut
                 ^long m
                 ^java.lang.foreign.MemorySegment userData]))

(definterface ICheapConstraintsCallback
  (^int invoke [^java.lang.foreign.MemorySegment x
                ^long dim
                ^java.lang.foreign.MemorySegment userData]))

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

(defonce ^:private vtable-layout
  (MemoryLayout/structLayout
    (.withName ValueLayout/ADDRESS "cheap_constraints")
    (.withName ValueLayout/ADDRESS "mc_sample")
    (.withName ValueLayout/JAVA_LONG "num_constraints")
    (.withName ValueLayout/JAVA_LONG "search_dim")
    (.withName ValueLayout/ADDRESS "user_data")))

(defonce ^:private stats-offset
  (.byteOffset output-layout
    (into-array MemoryLayout$PathElement
      [(MemoryLayout$PathElement/groupElement "stats")])))

;; Callback function descriptors

(defonce ^:private cheap-constraints-fd
  (FunctionDescriptor/of ValueLayout/JAVA_INT
    ValueLayout/ADDRESS ValueLayout/JAVA_LONG ValueLayout/ADDRESS))

(defonce ^:private mc-sample-fd
  (FunctionDescriptor/ofVoid
    ValueLayout/ADDRESS ValueLayout/JAVA_LONG ValueLayout/JAVA_LONG
    ValueLayout/JAVA_INT ValueLayout/JAVA_INT
    ValueLayout/ADDRESS ValueLayout/ADDRESS ValueLayout/JAVA_LONG
    ValueLayout/ADDRESS))

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

(defonce ^:private h-engine-run-with-evaluator
  (.downcallHandle linker
    (find-symbol "imads_engine_run_with_evaluator")
    (FunctionDescriptor/of output-layout ValueLayout/ADDRESS ValueLayout/ADDRESS env-layout ValueLayout/JAVA_INT vtable-layout)))

;; ---- Upcall stub creation ----

(defn- create-mc-sample-stub
  "Create an FFM upcall stub for the mc_sample callback."
  [mc-sample-fn ^Arena arena]
  (let [callback (reify IMcSampleCallback
                   (invoke [_ x dim tau smc k fOut cOut m _userData]
                     (let [x-sized (.reinterpret x (* dim (.byteSize ValueLayout/JAVA_DOUBLE)))
                           x-arr (double-array (map #(.getAtIndex x-sized ValueLayout/JAVA_DOUBLE (long %))
                                                    (range dim)))
                           result (mc-sample-fn x-arr tau (int smc) (int k))
                           f-out-sized (.reinterpret fOut (.byteSize ValueLayout/JAVA_DOUBLE))]
                       (.set f-out-sized ValueLayout/JAVA_DOUBLE 0 (double (first result)))
                       (let [c-out-sized (.reinterpret cOut (* m (.byteSize ValueLayout/JAVA_DOUBLE)))]
                         (doseq [j (range m)]
                           (.setAtIndex c-out-sized ValueLayout/JAVA_DOUBLE (long j)
                                        (double (if (< (inc j) (count result))
                                                  (nth result (inc j))
                                                  0.0))))))))
        mh (-> (MethodHandles/lookup)
               (.findVirtual IMcSampleCallback "invoke"
                 (MethodType/methodType Void/TYPE
                   (into-array Class [MemorySegment Long/TYPE Long/TYPE
                                      Integer/TYPE Integer/TYPE
                                      MemorySegment MemorySegment Long/TYPE MemorySegment])))
               (.bindTo callback))]
    (.upcallStub linker mh mc-sample-fd arena)))

(defn- create-cheap-constraints-stub
  "Create an FFM upcall stub for the cheap_constraints callback."
  [cheap-fn ^Arena arena]
  (let [callback (reify ICheapConstraintsCallback
                   (invoke [_ x dim _userData]
                     (let [x-sized (.reinterpret x (* dim (.byteSize ValueLayout/JAVA_DOUBLE)))
                           x-arr (double-array (map #(.getAtIndex x-sized ValueLayout/JAVA_DOUBLE (long %))
                                                    (range dim)))]
                       (if (and cheap-fn (cheap-fn x-arr)) (int 1) (int 0)))))
        mh (-> (MethodHandles/lookup)
               (.findVirtual ICheapConstraintsCallback "invoke"
                 (MethodType/methodType Integer/TYPE
                   (into-array Class [MemorySegment Long/TYPE MemorySegment])))
               (.bindTo callback))]
    (.upcallStub linker mh cheap-constraints-fd arena)))

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
  "Run the engine with a custom evaluator via FFM upcall stubs."
  ([engine cfg env-data mc-sample-fn num-constraints cheap-fn workers]
   (engine-run-with-evaluator engine cfg env-data mc-sample-fn num-constraints cheap-fn nil workers))
  ([engine cfg env-data mc-sample-fn num-constraints cheap-fn search-dim-val workers]
   (let [arena (Arena/ofShared)]
     (try
       (let [env-seg (.allocate arena env-layout)]
         (.set env-seg ValueLayout/JAVA_LONG 0 (long (:run-id env-data 1)))
         (.set env-seg ValueLayout/JAVA_LONG 8 (long (:config-hash env-data 0)))
         (.set env-seg ValueLayout/JAVA_LONG 16 (long (:data-snapshot-id env-data 0)))
         (.set env-seg ValueLayout/JAVA_LONG 24 (long (:rng-master-seed env-data 0)))
         (let [mc-stub (create-mc-sample-stub mc-sample-fn arena)
               cheap-stub (create-cheap-constraints-stub cheap-fn arena)
               vtable-seg (.allocate arena vtable-layout)]
           (.set vtable-seg ValueLayout/ADDRESS 0 cheap-stub)
           (.set vtable-seg ValueLayout/ADDRESS 8 mc-stub)
           (.set vtable-seg ValueLayout/JAVA_LONG 16 (long num-constraints))
           (.set vtable-seg ValueLayout/JAVA_LONG 24 (long (or search-dim-val 0)))
           (.set vtable-seg ValueLayout/ADDRESS 32 (MemorySegment/NULL))
           (let [out-seg (.invoke h-engine-run-with-evaluator
                           (object-array [engine cfg env-seg (int workers) vtable-seg]))]
             (extract-output out-seg))))
       (finally (.close arena))))))
