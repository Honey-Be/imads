package io.imads

/** Environment descriptor. All fields participate in deterministic hashing. */
final case class Env(
    runId: Long = 1L,
    configHash: Long = 0L,
    dataSnapshotId: Long = 0L,
    rngMasterSeed: Long = 0L,
)

/** Result of an engine run. */
final case class Output(
    fBest: Option[Double],
    xBest: Array[Long],
    truthEvals: Long,
    partialSteps: Long,
    cheapRejects: Long,
    invalidEvalRejects: Long,
):
  override def toString: String =
    s"Output(fBest=$fBest, truthEvals=$truthEvals, partialSteps=$partialSteps)"
