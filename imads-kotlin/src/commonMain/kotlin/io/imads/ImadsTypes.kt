package io.imads

/**
 * Environment descriptor for an IMADS run.
 *
 * All fields participate in deterministic hashing for cache keys and reproducibility.
 */
data class ImadsEnv(
    val runId: Long = 1,
    val configHash: Long = 0,
    val dataSnapshotId: Long = 0,
    val rngMasterSeed: Long = 0,
)

/**
 * Result of an IMADS engine run.
 */
data class ImadsOutput(
    /** Best objective value, or null if no feasible solution found. */
    val fBest: Double?,
    /** Best solution in mesh coordinates. */
    val xBest: LongArray,
    val truthEvals: Long,
    val partialSteps: Long,
    val cheapRejects: Long,
    val invalidEvalRejects: Long,
) {
    override fun toString(): String =
        "ImadsOutput(fBest=$fBest, truthEvals=$truthEvals, partialSteps=$partialSteps)"

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is ImadsOutput) return false
        return fBest == other.fBest && xBest.contentEquals(other.xBest)
    }

    override fun hashCode(): Int = 31 * (fBest?.hashCode() ?: 0) + xBest.contentHashCode()
}

/**
 * Custom evaluator interface. All methods must be deterministic.
 */
interface ImadsEvaluator {
    /**
     * Monte Carlo sample. Return [objective, c0, c1, ...].
     *
     * @param x   decision variables
     * @param tau tolerance level (larger = looser)
     * @param smc sample count
     * @param k   0-based sample index
     */
    fun mcSample(x: DoubleArray, tau: Long, smc: Int, k: Int): DoubleArray

    /** Optional cheap constraint gate. Return false to reject without evaluation. */
    fun cheapConstraints(x: DoubleArray): Boolean = true
}
