package booletoy

import "github.com/consensys/gnark/frontend"

// BooleRatchetCandidate computes 2*a*a*b with two products: (a*a) * (b+b).
func BooleRatchetCandidate(api frontend.API, a, b frontend.Variable) frontend.Variable {
	return api.Mul(api.Mul(a, a), api.Add(b, b))
}
