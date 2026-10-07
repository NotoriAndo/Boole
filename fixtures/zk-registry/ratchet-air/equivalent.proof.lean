  have hm : (Cand.MsgEq : Cand.Msg → Cand.Msg → Prop) = ZkDet.sp1_v6_8_1_001_Toy.MsgEq := rfl
  have hb : Cand.sp1Byte = ZkDet.sp1_v6_8_1_001_Toy.sp1Byte := rfl
  intro f x y
  constructor
  · rintro ⟨w, ⟨h1, h2, h3⟩, ha, hf, hin, hout⟩
    have e : w 2 = w 3 := sub_eq_zero.mp h3
    refine ⟨![w 0, w 1, w 2, w 4], ⟨?_, ?_⟩, ?_, ?_, ?_, ?_⟩
    · simpa using h1
    · simp only [Matrix.cons_val]
      linear_combination h3 + h2
    · simpa [Cand.Assumptions, ZkDet.sp1_v6_8_1_001_Toy.Assumptions, hb] using ha
    · simpa [Cand.Fixed, ZkDet.sp1_v6_8_1_001_Toy.Fixed] using hf
    · simpa [Cand.BusEq, Cand.In, ZkDet.sp1_v6_8_1_001_Toy.BusEq, ZkDet.sp1_v6_8_1_001_Toy.In, hm] using hin
    · simpa [Cand.BusEq, Cand.Out, ZkDet.sp1_v6_8_1_001_Toy.BusEq, ZkDet.sp1_v6_8_1_001_Toy.Out, hm, e] using hout
  · rintro ⟨w, ⟨h1, h2⟩, ha, hf, hin, hout⟩
    refine ⟨![w 0, w 1, w 2, w 0 * w 1, w 3], ⟨?_, ?_, ?_⟩, ?_, ?_, ?_, ?_⟩
    · simpa using h1
    · simp
    · simp only [Matrix.cons_val]
      linear_combination h2
    · simpa [Cand.Assumptions, ZkDet.sp1_v6_8_1_001_Toy.Assumptions, hb] using ha
    · simpa [Cand.Fixed, ZkDet.sp1_v6_8_1_001_Toy.Fixed] using hf
    · simpa [Cand.BusEq, Cand.In, ZkDet.sp1_v6_8_1_001_Toy.BusEq, ZkDet.sp1_v6_8_1_001_Toy.In, hm] using hin
    · simpa [Cand.BusEq, Cand.Out, ZkDet.sp1_v6_8_1_001_Toy.BusEq, ZkDet.sp1_v6_8_1_001_Toy.Out, hm] using hout
