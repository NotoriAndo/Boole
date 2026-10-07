package booletoy

import (
	"math/big"

	"github.com/consensys/gnark/constraint/solver"
	"github.com/consensys/gnark/frontend"
)

func init() {
	solver.RegisterHint(booleSquareHint)
}

// booleSquareHint computes a*a outside the circuit.
func booleSquareHint(mod *big.Int, in []*big.Int, out []*big.Int) error {
	out[0].Mul(in[0], in[0])
	out[0].Mod(out[0], mod)
	return nil
}

// BooleRatchetCandidate takes a*a from an unconstrained hint: the solver fills it correctly, so the simulation screen
// passes, but nothing in the constraints pins the hint down.
func BooleRatchetCandidate(api frontend.API, a, b frontend.Variable) frontend.Variable {
	sq, err := api.Compiler().NewHint(booleSquareHint, 1, a)
	if err != nil {
		panic(err)
	}
	return api.Mul(sq[0], api.Add(b, b))
}
