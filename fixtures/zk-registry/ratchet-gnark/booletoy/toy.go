// Package booletoy is the gnark ratchet test fixture (added to a copy of the gnark module snapshot by the tests).
package booletoy

import "github.com/consensys/gnark/frontend"

// Toy returns 2*a*a*b computed wastefully as a*a*b + a*(a*b): four products.
func Toy(api frontend.API, a, b frontend.Variable) frontend.Variable {
	x := api.Mul(a, a)
	y := api.Mul(a, b)
	return api.Add(api.Mul(x, b), api.Mul(a, y))
}
