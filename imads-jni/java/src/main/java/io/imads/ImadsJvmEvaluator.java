package io.imads;

/**
 * JVM-side evaluator interface called from JNI native code.
 *
 * <p>This is the JVM-specific evaluator contract. Language-specific wrappers
 * adapt the common evaluator interface to this one.
 */
public interface ImadsJvmEvaluator {
    /** Return [objective, c0, c1, ...]. Must be deterministic. */
    double[] mcSample(double[] x, long tau, int smc, int k);

    /** Optional cheap constraint gate. Return false to reject. */
    default boolean cheapConstraints(double[] x) { return true; }
}
