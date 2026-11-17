// The smoke model (FORMAL-01): a counter that the formal stages typecheck, test and verify, so the
// toolchain is proven before a model of the product exists. FORMAL-02 drives it with a test-only
// conformance driver and a negative control that skips a value.
module smoke {
  // The counter.
  var count: int

  action init = count' = 0

  // One increment per step.
  action step = count' = count + 1

  // The counter never goes below zero.
  val countNonNegative = count >= 0

  // Every invariant of the model, checked by Apalache and the simulation (ver §6.1).
  val allInvariants = countNonNegative

  // Three steps from the start count to three.
  run countsUpTest = init.then(3.reps(_ => step)).expect(count == 3)
}
