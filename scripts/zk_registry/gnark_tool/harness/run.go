package harness

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"os"
	"reflect"
	"sort"
	"strings"
	"time"

	"github.com/consensys/gnark/constraint"
	"github.com/consensys/gnark/constraint/solver"
	"github.com/consensys/gnark/frontend"
	"github.com/consensys/gnark/test"
)

// Spec is a registered wrapper circuit.
type Spec struct {
	ID    string
	Field *big.Int
	Curve string
	New   func() frontend.Circuit
}

var specs = map[string]Spec{}

func Register(s Spec) {
	if _, dup := specs[s.ID]; dup {
		panic("duplicate wrapper id " + s.ID)
	}
	specs[s.ID] = s
}

func IDs() []string {
	out := make([]string, 0, len(specs))
	for k := range specs {
		out = append(out, k)
	}
	sort.Strings(out)
	return out
}

// Options of one run.
type Options struct {
	Limit      int // guard of every compile
	SizePolicy int // model size above which no witnesses are produced
	Samples    int
	MaxPoints  int
	Production bool
}

type Term [2]string // wire (decimal), coefficient (decimal, reduced)

type HintInfo struct {
	Name  string `json:"name"`
	Start int    `json:"start"`
	End   int    `json:"end"`
}

type Compiled struct {
	Mode                string      `json:"mode"`
	Challenge           string      `json:"challenge,omitempty"`
	NbConstraints       int         `json:"n_constraints"`
	NbWires             int         `json:"n_wires"`
	NbPublic            int         `json:"n_public"`
	NbSecret            int         `json:"n_secret"`
	PublicNames         []string    `json:"public_names,omitempty"`
	SecretNames         []string    `json:"secret_names,omitempty"`
	Constraints         [][3][]Term `json:"constraints,omitempty"`
	Outputs             []int       `json:"outputs"`
	OutputPaths         []string    `json:"output_paths"`
	Groups              []Group     `json:"groups"`
	Hints               []HintInfo  `json:"hints,omitempty"`
	Commits             int         `json:"commits"`
	BoundaryConstraints int         `json:"boundary_constraints"`
	BoundaryWires       int         `json:"boundary_wires"`
	ChallengeWire       int         `json:"challenge_wire"`
	Committed           int         `json:"committed"`
	RangeChecks         int         `json:"range_checks"`
	RangeCheckBits      int         `json:"range_check_bits"`
	Secs                float64     `json:"secs"`
	ccs                 constraint.ConstraintSystem
}

type SampleInfo struct {
	Profile    string   `json:"profile"`
	Solved     bool     `json:"solved"`
	Error      string   `json:"error,omitempty"`
	TestEngine string   `json:"test_engine"`
	Domains    []string `json:"domains,omitempty"`
	// Witness of every model compile (symbolic, or constant 0..N-1), decimal values
	Witnesses [][]string `json:"witnesses,omitempty"`
}

type Result struct {
	ID              string       `json:"id"`
	Curve           string       `json:"curve"`
	Field           string       `json:"field"`
	Status          string       `json:"status"` // ok | too-large | compile-error | not-modelled | no-exposure
	Error           string       `json:"error,omitempty"`
	Production      *Compiled    `json:"production,omitempty"`
	ProductionError string       `json:"production_error,omitempty"`
	Symbolic        *Compiled    `json:"symbolic,omitempty"`
	Degree          int          `json:"degree"`
	Points          int          `json:"points"`
	Constant        []*Compiled  `json:"constant,omitempty"`
	ModelSize       int          `json:"model_size"`
	SizeLowerBound  bool         `json:"size_lower_bound"`
	Samples         []SampleInfo `json:"samples,omitempty"`
	GnarkVersion    string       `json:"gnark_version"`
	Secs            float64      `json:"secs"`
}

func hintNames() map[solver.HintID]string {
	out := map[solver.HintID]string{}
	for _, h := range solver.GetRegisteredHints() {
		out[solver.GetHintID(h)] = solver.GetHintName(h)
	}
	return out
}

func systemField(ccs constraint.ConstraintSystem, name string) reflect.Value {
	v := reflect.ValueOf(ccs)
	for v.Kind() == reflect.Ptr || v.Kind() == reflect.Interface {
		v = v.Elem()
	}
	sys := v.FieldByName("System")
	if !sys.IsValid() {
		return reflect.Value{}
	}
	return sys.FieldByName(name)
}

func compile(spec Spec, mode Mode, challenge *big.Int, limit int, export bool) (*Compiled, error) {
	t0 := time.Now()
	rec := &Recorder{Mode: mode, Challenge: challenge, Limit: limit}
	current = &exposure{}
	defer func() { current = nil }()
	ccs, err := frontend.Compile(spec.Field, NewBuilder(rec), spec.New())
	if err != nil {
		msg := err.Error()
		if i := strings.Index(msg, "boole-size-guard: "); i >= 0 {
			return nil, TooLarge{N: limit}
		}
		return nil, err
	}
	if !current.called {
		return nil, errors.New("boole-model: the wrapper did not call Expose")
	}
	c := &Compiled{Mode: mode.String(), NbConstraints: ccs.GetNbConstraints(),
		NbPublic: ccs.GetNbPublicVariables(), NbSecret: ccs.GetNbSecretVariables(),
		Outputs: current.wires, OutputPaths: current.paths, Groups: current.groups,
		Commits: rec.Commits, BoundaryConstraints: rec.BoundaryConstraints, BoundaryWires: rec.BoundaryWires,
		ChallengeWire: rec.ChallengeWire, Committed: rec.Committed, RangeChecks: rec.RangeChecks,
		RangeCheckBits: rec.RangeCheckBits, ccs: ccs}
	if c.Outputs == nil {
		c.Outputs = []int{}
		c.OutputPaths = []string{}
	}
	if c.Groups == nil {
		c.Groups = []Group{}
	}
	if challenge != nil {
		c.Challenge = challenge.String()
	}
	c.NbWires = c.NbPublic + c.NbSecret + ccs.GetNbInternalVariables()
	if export {
		r1cs, ok := ccs.(constraint.R1CS[constraint.U64])
		if !ok {
			return nil, errors.New("compiled system is not an R1CS")
		}
		cons := r1cs.GetR1Cs()
		if len(cons) != c.NbConstraints {
			return nil, fmt.Errorf("GetR1Cs returned %d constraints, system reports %d", len(cons), c.NbConstraints)
		}
		coeff := func(cid int) string { return ccs.ToBigInt(ccs.GetCoefficient(cid)).String() }
		conv := func(le constraint.LinearExpression) []Term {
			out := make([]Term, 0, len(le))
			for _, t := range le {
				out = append(out, Term{fmt.Sprint(t.WireID()), coeff(t.CoeffID())})
			}
			return out
		}
		c.Constraints = make([][3][]Term, len(cons))
		for k, r := range cons {
			c.Constraints[k] = [3][]Term{conv(r.L), conv(r.R), conv(r.O)}
		}
		if pub := systemField(ccs, "Public"); pub.IsValid() {
			c.PublicNames = pub.Interface().([]string)
		}
		if sec := systemField(ccs, "Secret"); sec.IsValid() {
			c.SecretNames = sec.Interface().([]string)
		}
		names := hintNames()
		bps := systemField(ccs, "Blueprints")
		insts := systemField(ccs, "Instructions")
		if bps.IsValid() && insts.IsValid() {
			for i := 0; i < ccs.GetNbInstructions(); i++ {
				bid := insts.Index(i).FieldByName("BlueprintID").Uint()
				bp := bps.Index(int(bid)).Interface()
				inst := ccs.GetInstruction(i)
				if bh, ok := bp.(constraint.BlueprintHint); ok {
					var hm constraint.HintMapping
					bh.DecompressHint(&hm, inst)
					nm := names[hm.HintID]
					if nm == "" {
						nm = fmt.Sprintf("hint#%d", hm.HintID)
					}
					c.Hints = append(c.Hints, HintInfo{Name: nm, Start: int(hm.OutputRange.Start), End: int(hm.OutputRange.End)})
				} else if bs, ok := bp.(constraint.BlueprintSolvable[constraint.U64]); ok {
					n := bs.NbOutputs(inst)
					if n > 0 {
						c.Hints = append(c.Hints, HintInfo{Name: "blueprint " + reflect.TypeOf(bp).String(),
							Start: int(inst.WireOffset), End: int(inst.WireOffset) + n})
					}
				}
			}
		}
	}
	c.Secs = time.Since(t0).Seconds()
	return c, nil
}

// wire / coefficient helpers for the degree analysis
func termWire(t Term) int {
	var w int
	fmt.Sscan(t[0], &w)
	return w
}

// Degree analysis of the challenge-dependent part of a symbolic compile: wires at or above the
// boundary are defined, in constraint order, by constraints in which they appear only on the output
// side (as gnark's builder creates products, accumulations and compressions); the challenge wire has
// degree 1, wires below the boundary degree 0.  A constraint without an undefined wire is a check; the
// result is the largest degree of a check (as a polynomial in the challenge).  Any other shape (a
// solver-computed wire such as an inverse used before it is defined, two undefined wires) means the
// check is not a polynomial identity and the commitment is not modelled.
func Degree(c *Compiled) (int, error) {
	d, _, err := degreeChecks(c)
	return d, err
}

func degreeChecks(c *Compiled) (int, int, error) {
	B := c.BoundaryWires
	deg := map[int]int{}
	if c.ChallengeWire >= 0 {
		deg[c.ChallengeWire] = 1
	}
	lcDeg := func(le []Term, skip int) (int, bool) {
		d := 0
		for _, t := range le {
			w := termWire(t)
			if w == skip {
				continue
			}
			if w < B {
				continue
			}
			dw, ok := deg[w]
			if !ok {
				return 0, false
			}
			if dw > d {
				d = dw
			}
		}
		return d, true
	}
	maxCheck, checks := 0, 0
	for k := c.BoundaryConstraints; k < len(c.Constraints); k++ {
		L, R, O := c.Constraints[k][0], c.Constraints[k][1], c.Constraints[k][2]
		undef := map[int]bool{}
		for _, le := range [][]Term{L, R, O} {
			for _, t := range le {
				w := termWire(t)
				if _, ok := deg[w]; w >= B && !ok {
					undef[w] = true
				}
			}
		}
		switch len(undef) {
		case 0:
			checks++
			dl, _ := lcDeg(L, -1)
			dr, _ := lcDeg(R, -1)
			do, _ := lcDeg(O, -1)
			d := dl + dr
			if len(L) == 0 || len(R) == 0 {
				d = 0
			}
			if do > d {
				d = do
			}
			if d > maxCheck {
				maxCheck = d
			}
		case 1:
			var u int
			for w := range undef {
				u = w
			}
			for _, t := range append(append([]Term{}, L...), R...) {
				if termWire(t) == u {
					return 0, 0, fmt.Errorf("constraint %d uses solver-computed wire %d as a factor (not a polynomial identity in the challenge)", k, u)
				}
			}
			dl, _ := lcDeg(L, -1)
			dr, _ := lcDeg(R, -1)
			do, _ := lcDeg(O, u)
			d := dl + dr
			if do > d {
				d = do
			}
			deg[u] = d
		default:
			return 0, 0, fmt.Errorf("constraint %d introduces %d challenge-dependent wires at once", k, len(undef))
		}
	}
	return maxCheck, checks, nil
}

// solveAll solves every compile of the model on one assignment; the witness of each compile is the
// full wire vector of gnark's solver.
func solveAll(spec Spec, comps []*Compiled, assign frontend.Circuit) (out [][]string, err error) {
	defer func() {
		if r := recover(); r != nil {
			out, err = nil, fmt.Errorf("solver panic: %v", r)
		}
	}()
	wit, err := frontend.NewWitness(assign, spec.Field)
	if err != nil {
		return nil, fmt.Errorf("witness: %w", err)
	}
	out = make([][]string, 0, len(comps))
	for _, c := range comps {
		sol, err := c.ccs.Solve(wit)
		if err != nil {
			return nil, err
		}
		v := reflect.ValueOf(sol)
		for v.Kind() == reflect.Ptr || v.Kind() == reflect.Interface {
			v = v.Elem()
		}
		W := v.FieldByName("W")
		if !W.IsValid() {
			return nil, errors.New("solution has no W vector")
		}
		vals := make([]string, W.Len())
		for i := 0; i < W.Len(); i++ {
			e := W.Index(i).Addr().Interface().(interface{ BigInt(*big.Int) *big.Int })
			vals[i] = e.BigInt(new(big.Int)).String()
		}
		if len(vals) != c.NbWires {
			return nil, fmt.Errorf("solution has %d values, system has %d wires", len(vals), c.NbWires)
		}
		out = append(out, vals)
	}
	return out, nil
}

func seedOf(id string, k int) int64 {
	h := sha256.Sum256([]byte(fmt.Sprintf("%s/%d", id, k)))
	return int64(binary.BigEndian.Uint64(h[:8]) & 0x7fffffffffffffff)
}

func shortErr(err error) string {
	s := err.Error()
	if i := strings.Index(s, "\ngoroutine "); i >= 0 {
		s = s[:i]
	}
	if len(s) > 600 {
		s = s[:600]
	}
	return s
}

// Run compiles, analyses and solves one wrapper.
func Run(id string, opt Options) *Result {
	t0 := time.Now()
	spec, ok := specs[id]
	res := &Result{ID: id, Degree: -1}
	defer func() { res.Secs = time.Since(t0).Seconds() }()
	if !ok {
		res.Status, res.Error = "compile-error", "unknown wrapper id"
		return res
	}
	res.Curve, res.Field = spec.Curve, spec.Field.String()
	sym, err := compile(spec, ModeSymbolic, nil, opt.Limit, true)
	if err != nil {
		if tl, ok := err.(TooLarge); ok {
			res.Status, res.ModelSize, res.SizeLowerBound = "too-large", tl.N+1, true
			return res
		}
		res.Status, res.Error = "compile-error", shortErr(err)
		if strings.Contains(res.Error, "did not call Expose") {
			res.Status = "no-exposure"
		}
		return res
	}
	res.Symbolic = sym
	if opt.Production {
		prod, err := compile(spec, ModeProduction, nil, opt.Limit, false)
		if err != nil {
			res.ProductionError = shortErr(err)
		} else {
			res.Production = prod
		}
	}
	model := []*Compiled{sym}
	res.ModelSize = sym.NbConstraints
	if sym.Commits > 0 {
		d, checks, err := degreeChecks(sym)
		if err != nil {
			res.Status, res.Error = "not-modelled", "commitment: "+err.Error()
			return res
		}
		res.Degree, res.Points = d, d+1
		// every check gives at least one constraint at every challenge
		if lb := sym.BoundaryConstraints + res.Points*checks; lb > opt.SizePolicy {
			res.Status, res.ModelSize, res.SizeLowerBound = "too-large", lb, true
			res.Symbolic.Constraints = nil
			return res
		}
		if res.Points > opt.MaxPoints {
			res.Status, res.Error = "not-modelled", fmt.Sprintf("commitment: challenge degree %d needs more than %d points", d, opt.MaxPoints)
			return res
		}
		model = nil
		res.ModelSize = sym.BoundaryConstraints
		for j := 0; j < res.Points; j++ {
			ch := big.NewInt(int64(j + 2))
			cc, err := compile(spec, ModeConstant, ch, opt.Limit, true)
			if err != nil {
				res.Status, res.Error = "compile-error", "constant challenge compile: "+shortErr(err)
				return res
			}
			if cc.Commits != 1 {
				res.Status, res.Error = "compile-error", "constant challenge compile without a commitment"
				return res
			}
			res.Constant = append(res.Constant, cc)
			model = append(model, cc)
			res.ModelSize += cc.NbConstraints - cc.BoundaryConstraints
		}
	}
	if res.ModelSize > opt.SizePolicy {
		res.Status = "too-large"
		// keep only counts for oversized systems
		res.Symbolic.Constraints = nil
		for _, c := range res.Constant {
			c.Constraints = nil
		}
		return res
	}
	for k := 0; k < opt.Samples; k++ {
		p := ProfileOf(k)
		info := SampleInfo{Profile: ProfileLabel(p)}
		assign := spec.New()
		smp := NewSampler(seedOf(id, k), spec.Field, p)
		if err := smp.Fill(reflect.ValueOf(assign)); err != nil {
			info.Error = shortErr(err)
			res.Samples = append(res.Samples, info)
			continue
		}
		info.Domains = smp.DomainNames()
		ws, err := solveAll(spec, model, assign)
		if err != nil {
			info.Error = shortErr(err)
		} else {
			info.Solved = true
			info.Witnesses = ws
		}
		current = &exposure{test: true}
		terr := testEngine(spec, assign)
		current = nil
		if terr != nil {
			info.TestEngine = "rejected: " + shortErr(terr)
		} else {
			info.TestEngine = "accepted"
		}
		res.Samples = append(res.Samples, info)
	}
	res.Status = "ok"
	return res
}

func testEngine(spec Spec, assign frontend.Circuit) (err error) {
	defer func() {
		if r := recover(); r != nil {
			err = fmt.Errorf("test engine panic: %v", r)
		}
	}()
	return test.IsSolved(spec.New(), assign, spec.Field)
}

// WriteJSON writes the result.
func WriteJSON(path string, v any) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	enc := json.NewEncoder(f)
	return enc.Encode(v)
}
