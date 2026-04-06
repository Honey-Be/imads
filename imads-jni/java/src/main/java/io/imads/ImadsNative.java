package io.imads;

/**
 * Low-level JNI bridge. Not for direct use — use the Kotlin/Scala/Clojure wrappers instead.
 */
public final class ImadsNative {
    static { System.loadLibrary("imads_jni"); }
    private ImadsNative() {}

    public static native long configFromPreset(String name);
    public static native String[] presetNames();
    public static native void configFree(long ptr);

    public static native long engineNew();
    public static native void engineFree(long ptr);

    /** Returns packed long[]: [f_bits, x_len, truth_evals, partial_steps, cheap_rejects, invalid_eval_rejects, x0, x1, ...] */
    public static native long[] engineRun(
            long enginePtr, long cfgPtr,
            long runId, long configHash, long dataSnapshotId, long rngMasterSeed,
            int workers);

    public static native long[] engineRunWithEvaluator(
            long enginePtr, long cfgPtr,
            long runId, long configHash, long dataSnapshotId, long rngMasterSeed,
            int workers, Object evaluator, int numConstraints);
}
