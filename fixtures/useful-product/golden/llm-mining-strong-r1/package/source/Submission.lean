import P7.Targets

namespace Miner

theorem solution : P7.FullQuadrivariateFirstRoundSoundness := by
  rcases Ladder.Accepted.polynomialInfrastructure with
    ⟨_, _, _, _, evalSub, subLength⟩
  intro F _ _ degree polynomial prover domain claimed
    hhonestLength hproverLength hclaimed hproverSum hdomainNodup

  have honestEvaluationAll :
      ∀ (p : List (List (List (List F)))) challenge,
        Ladder.Poly.eval (P7.honestFirstRound p) challenge =
          P7.yzwBooleanSumAt p challenge := by
    intro p challenge
    induction p with
    | nil =>
        simp [P7.honestFirstRound, P7.yzwBooleanSumAt,
          P7.evalQuadrivariate, Ladder.Poly.eval]
        grind
    | cons coefficient rest ih =>
        simp only [P7.honestFirstRound, P7.yzwBooleanSumAt,
          P7.evalQuadrivariate, P7.trivariateBooleanSum,
          List.map_cons, Ladder.Poly.eval] at ih ⊢
        grind

  have honestEvaluation :
      ∀ challenge,
        Ladder.Poly.eval (P7.honestFirstRound polynomial) challenge =
          P7.yzwBooleanSumAt polynomial challenge :=
    honestEvaluationAll polynomial

  have differenceLength :
      (Ladder.Poly.coeffSub prover
        (P7.honestFirstRound polynomial)).length ≤ degree + 1 := by
    have hsubLength :=
      subLength F prover (P7.honestFirstRound polynomial)
    grind

  have differenceNonzero :
      Ladder.Poly.Nonzero
        (Ladder.Poly.coeffSub prover
          (P7.honestFirstRound polynomial)) := by
    by_cases hzero :
        Ladder.Poly.eval
          (Ladder.Poly.coeffSub prover
            (P7.honestFirstRound polynomial)) 0 = 0
    · refine ⟨1, ?_⟩
      intro hone
      apply hclaimed
      rw [← hproverSum]
      unfold P7.totalBooleanSum Ladder.Poly.booleanSum
      have hsubZero :=
        evalSub F prover (P7.honestFirstRound polynomial) 0
      have hsubOne :=
        evalSub F prover (P7.honestFirstRound polynomial) 1
      rw [hzero] at hsubZero
      rw [hone] at hsubOne
      grind
    · exact ⟨0, hzero⟩

  have acceptingEqualsBad :
      P7.falseAcceptingChallenges polynomial prover claimed domain =
        Ladder.badChallenges
          (Ladder.Poly.coeffSub prover
            (P7.honestFirstRound polynomial)) domain := by
    unfold P7.falseAcceptingChallenges Ladder.badChallenges
    congr 1
    funext challenge
    simp only [P7.firstRoundVerifier]
    rw [hproverSum]
    simp only [true_and]
    rw [evalSub F prover (P7.honestFirstRound polynomial) challenge]
    rw [honestEvaluation challenge]
    grind

  have badBound :=
    Ladder.Accepted.finiteIdentitySoundness F degree
      (Ladder.Poly.coeffSub prover (P7.honestFirstRound polynomial))
      domain differenceLength differenceNonzero hdomainNodup

  exact ⟨honestEvaluation, acceptingEqualsBad, by
    rw [acceptingEqualsBad]
    exact badBound⟩

end Miner
