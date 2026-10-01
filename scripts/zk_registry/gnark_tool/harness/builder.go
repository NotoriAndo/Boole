// Package harness compiles generated wrapper circuits around gnark gadgets with gnark's own R1CS
// builder, exports the constraint system and solver witnesses as JSON, and implements the
// commitment model of the Boole DET generator.
//
// Builders (all wrap gnark's r1cs.NewBuilder; the wrapping is the extension point gnark documents
// for Committer / Rangechecker):
//
//   - production: gnark's builder as gnark compiles the circuit (log-derivative range checks and the
//     Groth16 commitment); used only to record the production constraint count.
//   - symbolic:   range checks through gnark's own non-commitment checker (rangecheck.New on a view
//     without Committer, i.e. bit decomposition); a commitment is a fresh challenge wire computed by a
//     hint.  Used to locate and analyse the challenge-dependent part of the system.
//   - constant:   as symbolic, but the commitment is a fixed constant challenge.  The model of a
//     circuit with a commitment is the union of the constant compiles at N distinct challenges.
//
// Every builder stops the compilation (panic recovered by gnark's parser) once the system exceeds
// the constraint limit, so very large circuits are sized cheaply.
package harness

import (
	"errors"
	"fmt"
	"math/big"
	"reflect"
	"unsafe"

	"github.com/consensys/gnark/constraint"
	"github.com/consensys/gnark/constraint/solver"
	"github.com/consensys/gnark/frontend"
	"github.com/consensys/gnark/frontend/cs/r1cs"
	"github.com/consensys/gnark/std/rangecheck"
)

// Mode selects the builder.
type Mode int

const (
	ModeProduction Mode = iota
	ModeSymbolic
	ModeConstant
)

func (m Mode) String() string {
	switch m {
	case ModeProduction:
		return "production"
	case ModeSymbolic:
		return "symbolic"
	case ModeConstant:
		return "constant"
	}
	return "?"
}

// Recorder collects what happened during one compilation.
type Recorder struct {
	Mode      Mode
	Challenge *big.Int // ModeConstant: the challenge value
	Limit     int      // constraint limit (0: none)

	Commits             int
	BoundaryConstraints int // constraints when the (first) commitment was taken
	BoundaryWires       int // wires (public + secret + internal) when the (first) commitment was taken
	ChallengeWire       int // ModeSymbolic: wire of the challenge (-1 otherwise)
	Committed           int // number of committed variables (first commitment)
	RangeChecks         int // calls of the Rangechecker (model builders)
	RangeCheckBits      int

	sys constraint.R1CS[constraint.U64]
}

// TooLarge is the panic value of the size guard.
type TooLarge struct{ N int }

func (t TooLarge) String() string {
	return fmt.Sprintf("boole-size-guard: more than %d constraints", t.N)
}

func (t TooLarge) Error() string { return t.String() }

// ChallengeHint computes the symbolic challenge from the committed values (a fixed, deterministic
// function; its value only matters for solving the symbolic compile, which the model does not use).
func ChallengeHint(mod *big.Int, inputs []*big.Int, outputs []*big.Int) error {
	acc := big.NewInt(7)
	for _, in := range inputs {
		acc.Mul(acc, big.NewInt(1000003))
		acc.Add(acc, in)
		acc.Mod(acc, mod)
	}
	outputs[0].Set(acc)
	return nil
}

// ExposeHint copies its input; output wires of the wrapper are created by it and constrained equal
// to the gadget's results.
func ExposeHint(_ *big.Int, inputs []*big.Int, outputs []*big.Int) error {
	if len(inputs) != len(outputs) {
		return errors.New("expose hint: input/output count mismatch")
	}
	for i := range inputs {
		outputs[i].Set(inputs[i])
	}
	return nil
}

func init() {
	solver.RegisterHint(ChallengeHint, ExposeHint)
}

// innerSystem reads the constraint system of gnark's r1cs builder (unexported field `cs`), only to
// count constraints and wires; it never modifies it.
func innerSystem(b frontend.Builder[constraint.U64]) (constraint.R1CS[constraint.U64], error) {
	v := reflect.ValueOf(b)
	if v.Kind() != reflect.Ptr {
		return nil, errors.New("unexpected builder kind")
	}
	f := v.Elem().FieldByName("cs")
	if !f.IsValid() {
		return nil, errors.New("builder has no cs field")
	}
	f = reflect.NewAt(f.Type(), unsafe.Pointer(f.UnsafeAddr())).Elem()
	cs, ok := f.Interface().(constraint.R1CS[constraint.U64])
	if !ok {
		return nil, errors.New("builder cs is not an R1CS")
	}
	return cs, nil
}

func nbWires(cs constraint.R1CS[constraint.U64]) int {
	return cs.GetNbPublicVariables() + cs.GetNbSecretVariables() + cs.GetNbInternalVariables()
}

// guarded forwards everything to gnark's builder and enforces the size guard on the API calls that
// add constraints.
type guarded struct {
	frontend.Builder[constraint.U64]
	rec *Recorder
}

func (g *guarded) guard() {
	if g.rec.Limit > 0 && g.rec.sys.GetNbConstraints() > g.rec.Limit {
		panic(TooLarge{N: g.rec.Limit})
	}
}

func (g *guarded) Mul(a, b frontend.Variable, in ...frontend.Variable) frontend.Variable {
	defer g.guard()
	return g.Builder.Mul(a, b, in...)
}
func (g *guarded) MulAcc(a, b, c frontend.Variable) frontend.Variable {
	defer g.guard()
	return g.Builder.MulAcc(a, b, c)
}
func (g *guarded) Div(a, b frontend.Variable) frontend.Variable {
	defer g.guard()
	return g.Builder.Div(a, b)
}
func (g *guarded) DivUnchecked(a, b frontend.Variable) frontend.Variable {
	defer g.guard()
	return g.Builder.DivUnchecked(a, b)
}
func (g *guarded) Inverse(a frontend.Variable) frontend.Variable {
	defer g.guard()
	return g.Builder.Inverse(a)
}
func (g *guarded) ToBinary(a frontend.Variable, n ...int) []frontend.Variable {
	defer g.guard()
	return g.Builder.ToBinary(a, n...)
}
func (g *guarded) Xor(a, b frontend.Variable) frontend.Variable {
	defer g.guard()
	return g.Builder.Xor(a, b)
}
func (g *guarded) Or(a, b frontend.Variable) frontend.Variable {
	defer g.guard()
	return g.Builder.Or(a, b)
}
func (g *guarded) And(a, b frontend.Variable) frontend.Variable {
	defer g.guard()
	return g.Builder.And(a, b)
}
func (g *guarded) Select(b, i1, i2 frontend.Variable) frontend.Variable {
	defer g.guard()
	return g.Builder.Select(b, i1, i2)
}
func (g *guarded) IsZero(a frontend.Variable) frontend.Variable {
	defer g.guard()
	return g.Builder.IsZero(a)
}
func (g *guarded) Cmp(a, b frontend.Variable) frontend.Variable {
	defer g.guard()
	return g.Builder.Cmp(a, b)
}
func (g *guarded) AssertIsEqual(a, b frontend.Variable) {
	defer g.guard()
	g.Builder.AssertIsEqual(a, b)
}
func (g *guarded) AssertIsDifferent(a, b frontend.Variable) {
	defer g.guard()
	g.Builder.AssertIsDifferent(a, b)
}
func (g *guarded) AssertIsBoolean(a frontend.Variable) {
	defer g.guard()
	g.Builder.AssertIsBoolean(a)
}
func (g *guarded) AssertIsLessOrEqual(v, bound frontend.Variable) {
	defer g.guard()
	g.Builder.AssertIsLessOrEqual(v, bound)
}

// productionBuilder: gnark's builder with its own commitment (log-derivative range checks apply,
// since it exposes Committer and not Rangechecker).
type productionBuilder struct{ guarded }

func (b *productionBuilder) Commit(v ...frontend.Variable) (frontend.Variable, error) {
	b.rec.Commits++
	if b.rec.Commits == 1 {
		b.rec.BoundaryConstraints = b.rec.sys.GetNbConstraints()
		b.rec.BoundaryWires = nbWires(b.rec.sys)
		b.rec.Committed = len(v)
	}
	return b.Builder.(frontend.Committer).Commit(v...)
}

// noCommit hides Committer and Rangechecker, so that rangecheck.New returns gnark's own
// non-commitment checker for it.
type noCommit struct{ frontend.API }

// modelBuilder: range checks by gnark's non-commitment checker, commitments as challenge wires
// (symbolic) or fixed challenges (constant).
type modelBuilder struct {
	guarded
	plain frontend.Rangechecker
}

func (b *modelBuilder) Check(v frontend.Variable, bits int) {
	defer b.guard()
	b.rec.RangeChecks++
	b.rec.RangeCheckBits += bits
	b.plain.Check(v, bits)
}

func (b *modelBuilder) Commit(v ...frontend.Variable) (frontend.Variable, error) {
	b.rec.Commits++
	if b.rec.Commits > 1 {
		return nil, errors.New("boole-model: more than one commitment in the circuit (not modelled)")
	}
	b.rec.BoundaryConstraints = b.rec.sys.GetNbConstraints()
	b.rec.BoundaryWires = nbWires(b.rec.sys)
	b.rec.Committed = len(v)
	if b.rec.Mode == ModeConstant {
		return new(big.Int).Set(b.rec.Challenge), nil
	}
	if len(v) == 0 {
		v = []frontend.Variable{0}
	}
	out, err := b.Builder.Compiler().NewHint(ChallengeHint, 1, v...)
	if err != nil {
		return nil, err
	}
	le, ok := b.Builder.Compiler().ToCanonicalVariable(out[0]).(constraint.LinearExpression)
	if !ok || len(le) != 1 {
		return nil, errors.New("boole-model: challenge wire not a single term")
	}
	b.rec.ChallengeWire = int(le[0].VID)
	return out[0], nil
}

// NewBuilder returns a frontend.NewBuilder for the recorder's mode.
func NewBuilder(rec *Recorder) frontend.NewBuilder {
	return func(field *big.Int, cfg frontend.CompileConfig) (frontend.Builder[constraint.U64], error) {
		inner, err := r1cs.NewBuilder[constraint.U64](field, cfg)
		if err != nil {
			return nil, err
		}
		sys, err := innerSystem(inner)
		if err != nil {
			return nil, err
		}
		rec.sys = sys
		rec.ChallengeWire = -1
		g := guarded{Builder: inner, rec: rec}
		if rec.Mode == ModeProduction {
			return &productionBuilder{g}, nil
		}
		return &modelBuilder{guarded: g, plain: rangecheck.New(noCommit{inner})}, nil
	}
}
