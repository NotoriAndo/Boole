package booletoy

import "github.com/consensys/gnark/frontend"

// BooleRatchetCandidate computes a*a*b: smaller, but not the reference's function.
func BooleRatchetCandidate(api frontend.API, a, b frontend.Variable) frontend.Variable {
	return api.Mul(api.Mul(a, a), b)
}
