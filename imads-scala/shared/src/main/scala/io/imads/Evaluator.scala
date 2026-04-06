package io.imads

/** Custom evaluator trait. All methods must be deterministic. */
trait Evaluator:
  /**
   * Monte Carlo sample. Return Array(objective, c0, c1, ...).
   *
   * @param x   decision variables
   * @param tau tolerance level (larger = looser)
   * @param smc sample count
   * @param k   0-based sample index
   */
  def mcSample(x: Array[Double], tau: Long, smc: Int, k: Int): Array[Double]

  /** Optional cheap constraint gate. Return false to reject. */
  def cheapConstraints(x: Array[Double]): Boolean = true
