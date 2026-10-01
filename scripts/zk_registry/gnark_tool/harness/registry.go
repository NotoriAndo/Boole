package harness

import (
	"crypto/elliptic"
	"math/big"
	"math/rand"
	"reflect"
	"strings"

	"github.com/consensys/gnark-crypto/ecc/bls12-377"
	"github.com/consensys/gnark-crypto/ecc/bls12-381"
	"github.com/consensys/gnark-crypto/ecc/bn254"
	"github.com/consensys/gnark-crypto/ecc/bw6-761"
	"github.com/consensys/gnark-crypto/ecc/grumpkin"
	"github.com/consensys/gnark-crypto/ecc/secp256k1"
	"github.com/consensys/gnark-crypto/ecc/secp256r1"
	starkcurve "github.com/consensys/gnark-crypto/ecc/stark-curve"
	"github.com/consensys/gnark/std/algebra/emulated/sw_bls12381"
	"github.com/consensys/gnark/std/algebra/emulated/sw_bn254"
	"github.com/consensys/gnark/std/algebra/emulated/sw_bw6761"
	"github.com/consensys/gnark/std/algebra/emulated/sw_emulated"
	"github.com/consensys/gnark/std/algebra/native/sw_bls12377"
	"github.com/consensys/gnark/std/algebra/native/sw_grumpkin"
	"github.com/consensys/gnark/std/math/emulated"
	"github.com/consensys/gnark/std/math/emulated/emparams"
	"github.com/consensys/gnark/std/math/uints"
)

// EmInfo describes an emulated field (type parameter of emulated.Element).
type EmInfo struct {
	Name      string
	Modulus   *big.Int
	effective func(native *big.Int) (nbLimbs, nbBits uint)
	valueOf   func(v *big.Int) reflect.Value
}

var emRegistry = map[reflect.Type]*EmInfo{}

func regEm[T emulated.FieldParams](name string) {
	var p T
	t := reflect.TypeOf(emulated.Element[T]{})
	emRegistry[t] = &EmInfo{
		Name:    name,
		Modulus: new(big.Int).Set(p.Modulus()),
		effective: func(native *big.Int) (uint, uint) {
			return emulated.GetEffectiveFieldParams[T](native)
		},
		valueOf: func(v *big.Int) reflect.Value { return reflect.ValueOf(emulated.ValueOf[T](v)) },
	}
}

func init() {
	regEm[emparams.BabyBear]("BabyBear")
	regEm[emparams.BLS12377Fp]("BLS12377Fp")
	regEm[emparams.BLS12377Fr]("BLS12377Fr")
	regEm[emparams.BLS12381Fp]("BLS12381Fp")
	regEm[emparams.BLS12381Fr]("BLS12381Fr")
	regEm[emparams.BN254Fp]("BN254Fp")
	regEm[emparams.BN254Fr]("BN254Fr")
	regEm[emparams.BW6761Fp]("BW6761Fp")
	regEm[emparams.BW6761Fr]("BW6761Fr")
	regEm[emparams.Goldilocks]("Goldilocks")
	regEm[emparams.GrumpkinFp]("GrumpkinFp")
	regEm[emparams.GrumpkinFr]("GrumpkinFr")
	regEm[emparams.KoalaBear]("KoalaBear")
	regEm[emparams.Mod1e256]("Mod1e256")
	regEm[emparams.Mod1e4096]("Mod1e4096")
	regEm[emparams.Mod1e512]("Mod1e512")
	regEm[emparams.P256Fp]("P256Fp")
	regEm[emparams.P256Fr]("P256Fr")
	regEm[emparams.P384Fp]("P384Fp")
	regEm[emparams.P384Fr]("P384Fr")
	regEm[emparams.Secp256k1Fp]("Secp256k1Fp")
	regEm[emparams.Secp256k1Fr]("Secp256k1Fr")
	regEm[emparams.STARKCurveFp]("STARKCurveFp")
	regEm[emparams.STARKCurveFr]("STARKCurveFr")

	regPoint[emparams.Secp256k1Fp]("secp256k1 G1 (gnark-crypto)", func(s *big.Int) (*big.Int, *big.Int) {
		_, g := secp256k1.Generators()
		var p secp256k1.G1Affine
		p.ScalarMultiplication(&g, s)
		return p.X.BigInt(new(big.Int)), p.Y.BigInt(new(big.Int))
	}, secp256k1.ID.ScalarField())
	regPoint[emparams.P256Fp]("secp256r1 / P-256 G1 (gnark-crypto)", func(s *big.Int) (*big.Int, *big.Int) {
		_, g := secp256r1.Generators()
		var p secp256r1.G1Affine
		p.ScalarMultiplication(&g, s)
		return p.X.BigInt(new(big.Int)), p.Y.BigInt(new(big.Int))
	}, elliptic.P256().Params().N)
	regPoint[emparams.P384Fp]("P-384 (Go crypto/elliptic)", func(s *big.Int) (*big.Int, *big.Int) {
		return elliptic.P384().ScalarBaseMult(s.Bytes())
	}, elliptic.P384().Params().N)
	regPoint[emparams.STARKCurveFp]("STARK curve G1 (gnark-crypto)", func(s *big.Int) (*big.Int, *big.Int) {
		_, g := starkcurve.Generators()
		var p starkcurve.G1Affine
		p.ScalarMultiplication(&g, s)
		return p.X.BigInt(new(big.Int)), p.Y.BigInt(new(big.Int))
	}, new(big.Int).Set(starkcurve.ID.ScalarField()))
	regPoint[emparams.BN254Fp]("BN254 G1 (gnark-crypto)", func(s *big.Int) (*big.Int, *big.Int) {
		_, _, g, _ := bn254.Generators()
		var p bn254.G1Affine
		p.ScalarMultiplication(&g, s)
		return p.X.BigInt(new(big.Int)), p.Y.BigInt(new(big.Int))
	}, bn254.ID.ScalarField())
	regPoint[emparams.BLS12381Fp]("BLS12-381 G1 (gnark-crypto)", func(s *big.Int) (*big.Int, *big.Int) {
		_, _, g, _ := bls12381.Generators()
		var p bls12381.G1Affine
		p.ScalarMultiplication(&g, s)
		return p.X.BigInt(new(big.Int)), p.Y.BigInt(new(big.Int))
	}, bls12381.ID.ScalarField())
	regPoint[emparams.BW6761Fp]("BW6-761 G1 (gnark-crypto)", func(s *big.Int) (*big.Int, *big.Int) {
		_, _, g, _ := bw6761.Generators()
		var p bw6761.G1Affine
		p.ScalarMultiplication(&g, s)
		return p.X.BigInt(new(big.Int)), p.Y.BigInt(new(big.Int))
	}, bw6761.ID.ScalarField())
	regPoint[emparams.BLS12377Fp]("BLS12-377 G1 (gnark-crypto)", func(s *big.Int) (*big.Int, *big.Int) {
		_, _, g, _ := bls12377.Generators()
		var p bls12377.G1Affine
		p.ScalarMultiplication(&g, s)
		return p.X.BigInt(new(big.Int)), p.Y.BigInt(new(big.Int))
	}, bls12377.ID.ScalarField())
	regPoint[emparams.GrumpkinFp]("Grumpkin G1 (gnark-crypto)", func(s *big.Int) (*big.Int, *big.Int) {
		_, g := grumpkin.Generators()
		var p grumpkin.G1Affine
		p.ScalarMultiplication(&g, s)
		return p.X.BigInt(new(big.Int)), p.Y.BigInt(new(big.Int))
	}, grumpkin.ID.ScalarField())

	// G2 and GT of the emulated pairing packages, native BLS12-377 and Grumpkin points
	regDomain(reflect.TypeOf(sw_bn254.G2Affine{}), "BN254 G2 (gnark-crypto)", func(r *rand.Rand) reflect.Value {
		_, _, _, g := bn254.Generators()
		var q bn254.G2Affine
		q.ScalarMultiplication(&g, randBelow(r, bn254.ID.ScalarField()))
		return reflect.ValueOf(sw_bn254.NewG2Affine(q))
	})
	regDomain(reflect.TypeOf(sw_bn254.GTEl{}), "BN254 GT (pairing of random points, gnark-crypto)", func(r *rand.Rand) reflect.Value {
		_, _, g1, g2 := bn254.Generators()
		var p bn254.G1Affine
		var q bn254.G2Affine
		p.ScalarMultiplication(&g1, randBelow(r, bn254.ID.ScalarField()))
		q.ScalarMultiplication(&g2, randBelow(r, bn254.ID.ScalarField()))
		e, _ := bn254.Pair([]bn254.G1Affine{p}, []bn254.G2Affine{q})
		return reflect.ValueOf(sw_bn254.NewGTEl(e))
	})
	regDomain(reflect.TypeOf(sw_bls12381.G2Affine{}), "BLS12-381 G2 (gnark-crypto)", func(r *rand.Rand) reflect.Value {
		_, _, _, g := bls12381.Generators()
		var q bls12381.G2Affine
		q.ScalarMultiplication(&g, randBelow(r, bls12381.ID.ScalarField()))
		return reflect.ValueOf(sw_bls12381.NewG2Affine(q))
	})
	regDomain(reflect.TypeOf(sw_bls12381.GTEl{}), "BLS12-381 GT (pairing of random points, gnark-crypto)", func(r *rand.Rand) reflect.Value {
		_, _, g1, g2 := bls12381.Generators()
		var p bls12381.G1Affine
		var q bls12381.G2Affine
		p.ScalarMultiplication(&g1, randBelow(r, bls12381.ID.ScalarField()))
		q.ScalarMultiplication(&g2, randBelow(r, bls12381.ID.ScalarField()))
		e, _ := bls12381.Pair([]bls12381.G1Affine{p}, []bls12381.G2Affine{q})
		return reflect.ValueOf(sw_bls12381.NewGTEl(e))
	})
	regDomain(reflect.TypeOf(sw_bw6761.G2Affine{}), "BW6-761 G2 (gnark-crypto)", func(r *rand.Rand) reflect.Value {
		_, _, _, g := bw6761.Generators()
		var q bw6761.G2Affine
		q.ScalarMultiplication(&g, randBelow(r, bw6761.ID.ScalarField()))
		return reflect.ValueOf(sw_bw6761.NewG2Affine(q))
	})
	regDomain(reflect.TypeOf(sw_bw6761.GTEl{}), "BW6-761 GT (pairing of random points, gnark-crypto)", func(r *rand.Rand) reflect.Value {
		_, _, g1, g2 := bw6761.Generators()
		var p bw6761.G1Affine
		var q bw6761.G2Affine
		p.ScalarMultiplication(&g1, randBelow(r, bw6761.ID.ScalarField()))
		q.ScalarMultiplication(&g2, randBelow(r, bw6761.ID.ScalarField()))
		e, _ := bw6761.Pair([]bw6761.G1Affine{p}, []bw6761.G2Affine{q})
		return reflect.ValueOf(sw_bw6761.NewGTEl(e))
	})
	regDomain(reflect.TypeOf(sw_bls12377.G1Affine{}), "BLS12-377 G1 (gnark-crypto, native coordinates)", func(r *rand.Rand) reflect.Value {
		_, _, g, _ := bls12377.Generators()
		var p bls12377.G1Affine
		p.ScalarMultiplication(&g, randBelow(r, bls12377.ID.ScalarField()))
		return reflect.ValueOf(sw_bls12377.NewG1Affine(p))
	})
	regDomain(reflect.TypeOf(sw_bls12377.G2Affine{}), "BLS12-377 G2 (gnark-crypto, native coordinates)", func(r *rand.Rand) reflect.Value {
		_, _, _, g := bls12377.Generators()
		var q bls12377.G2Affine
		q.ScalarMultiplication(&g, randBelow(r, bls12377.ID.ScalarField()))
		return reflect.ValueOf(sw_bls12377.NewG2Affine(q))
	})
	regDomain(reflect.TypeOf(sw_bls12377.GT{}), "BLS12-377 GT (pairing of random points, gnark-crypto)", func(r *rand.Rand) reflect.Value {
		_, _, g1, g2 := bls12377.Generators()
		var p bls12377.G1Affine
		var q bls12377.G2Affine
		p.ScalarMultiplication(&g1, randBelow(r, bls12377.ID.ScalarField()))
		q.ScalarMultiplication(&g2, randBelow(r, bls12377.ID.ScalarField()))
		e, _ := bls12377.Pair([]bls12377.G1Affine{p}, []bls12377.G2Affine{q})
		return reflect.ValueOf(sw_bls12377.NewGTEl(e))
	})
	regDomain(reflect.TypeOf(sw_grumpkin.G1Affine{}), "Grumpkin G1 (gnark-crypto, native coordinates)", func(r *rand.Rand) reflect.Value {
		_, g := grumpkin.Generators()
		var p grumpkin.G1Affine
		p.ScalarMultiplication(&g, randBelow(r, grumpkin.ID.ScalarField()))
		return reflect.ValueOf(sw_grumpkin.NewG1Affine(p))
	})
	regDomain(reflect.TypeOf(uints.U8{}), "byte", func(r *rand.Rand) reflect.Value {
		return reflect.ValueOf(uints.NewU8(uint8(r.Intn(256))))
	})
}

// domain samplers: values of a type that satisfy its documented domain (points on the curve, GT
// elements, bytes)
type domainSampler struct {
	Name string
	fn   func(r *rand.Rand) reflect.Value
}

var domainRegistry = map[reflect.Type]*domainSampler{}

func regDomain(t reflect.Type, name string, fn func(r *rand.Rand) reflect.Value) {
	domainRegistry[t] = &domainSampler{Name: name, fn: fn}
}

func regPoint[T emulated.FieldParams](name string, mul func(s *big.Int) (*big.Int, *big.Int), order *big.Int) {
	t := reflect.TypeOf(sw_emulated.AffinePoint[T]{})
	regDomain(t, name, func(r *rand.Rand) reflect.Value {
		s := randBelow(r, order)
		if s.Sign() == 0 {
			s.SetInt64(1)
		}
		x, y := mul(s)
		return reflect.ValueOf(sw_emulated.AffinePoint[T]{X: emulated.ValueOf[T](x), Y: emulated.ValueOf[T](y)})
	})
}

func randBelow(r *rand.Rand, n *big.Int) *big.Int {
	return new(big.Int).Rand(r, n)
}

// emInfoOf returns the registry entry of an emulated.Element type, nil for other types, and an error
// marker for unregistered Element instantiations.
func emInfoOf(t reflect.Type) (*EmInfo, bool) {
	if info, ok := emRegistry[t]; ok {
		return info, true
	}
	if t.Kind() == reflect.Struct && strings.HasPrefix(t.Name(), "Element[") &&
		t.PkgPath() == "github.com/consensys/gnark/std/math/emulated" {
		return nil, true
	}
	return nil, false
}
