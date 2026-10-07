package harness

// Simulation runner of the gnark ratchet screen (scripts/zk_registry/ratchet_gnark.py).  It compiles the
// wrapper's model exactly as Run does (the symbolic compile; with a commitment, the constant compiles at D + 1
// fixed challenges), then for every input vector -- structured edge vectors first, then seeded random vectors --
// fills a fresh assignment, solves every compile of the model with gnark's solver and writes one JSON line:
// accepted or the solver's error, the values of the exposed output wires and, for the first vectors, the solver
// witnesses of every compile (re-checked against the model by the Python evaluator).  The reference and a
// candidate share the wrapper circuit type, so the same seed gives both the same assignments.

import (
	"bufio"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"math/big"
	"math/rand"
	"os"
	"reflect"

	"github.com/consensys/gnark/frontend"
)

// SimOptions of one simulation run.
type SimOptions struct {
	Limit     int    // constraint guard of every compile
	MaxPoints int    // largest number of challenge points
	N         int    // seeded random vectors after the edge vectors
	Witnesses int    // vectors (from the first) whose solver witnesses are written
	Seed      string // vector seed (the problem's simulation seed)
}

// SimHeader is the first line of a simulation output.
type SimHeader struct {
	ID            string   `json:"id"`
	Curve         string   `json:"curve"`
	Field         string   `json:"field"`
	Status        string   `json:"status"` // ok | compile-error | not-modelled
	Error         string   `json:"error,omitempty"`
	Commits       int      `json:"commits"`
	Degree        int      `json:"degree"`
	Points        int      `json:"points"`
	BoundaryWires int      `json:"boundary_wires"`
	NbWires       []int    `json:"n_wires"`
	NbPublic      int      `json:"n_public"`
	NbSecret      int      `json:"n_secret"`
	PublicNames   []string `json:"public_names"`
	SecretNames   []string `json:"secret_names"`
	Outputs       []int    `json:"outputs"`
	OutputPaths   []string `json:"output_paths"`
	Groups        []Group  `json:"groups"`
	EdgeVectors   int      `json:"edge_vectors"`
	Vectors       int      `json:"vectors"`
	Profiles      []string `json:"profiles"`
	Seed          string   `json:"seed"`
}

// SimLine is the result of one input vector.
type SimLine struct {
	K         int        `json:"k"`
	Label     string     `json:"label"`
	OK        bool       `json:"ok"`
	Error     string     `json:"error,omitempty"`
	Outputs   []string   `json:"outputs,omitempty"`
	Witnesses [][]string `json:"witnesses,omitempty"`
}

// value profiles of the screen: structured values first (every leaf at one of them in the edge vectors)
var simProfiles = []string{"zero", "one", "two", "max", "half", "bit", "byte", "u16", "u32", "u64", "uniform"}
var simEdge = []string{"zero", "one", "two", "max", "half"}
var simRandomModes = []string{"mixed", "mixed", "mixed", "edge-mixed", "uniform", "u64", "u32", "u16", "byte", "bit"}

type simSampler struct {
	rng    *rand.Rand
	native *big.Int
	leaf   func(top string) string
}

func simSeed(seed string, label string) int64 {
	h := sha256.Sum256([]byte(seed + "|" + label))
	return int64(binary.BigEndian.Uint64(h[:8]) & 0x7fffffffffffffff)
}

func (s *simSampler) value(prof string, bound *big.Int) *big.Int {
	var v *big.Int
	switch prof {
	case "zero":
		v = big.NewInt(0)
	case "one":
		v = big.NewInt(1)
	case "two":
		v = big.NewInt(2)
	case "max":
		v = new(big.Int).Sub(bound, big.NewInt(1))
	case "half":
		v = new(big.Int).Rsh(new(big.Int).Sub(bound, big.NewInt(1)), 1)
	case "bit":
		v = big.NewInt(int64(s.rng.Intn(2)))
	case "byte":
		v = big.NewInt(int64(s.rng.Intn(256)))
	case "u16":
		v = big.NewInt(int64(s.rng.Intn(1 << 16)))
	case "u32":
		v = new(big.Int).SetUint64(uint64(s.rng.Uint32()))
	case "u64":
		v = new(big.Int).SetUint64(s.rng.Uint64())
	default:
		v = new(big.Int).Rand(s.rng, bound)
	}
	if v.Sign() < 0 {
		v.SetInt64(0)
	}
	if v.Cmp(bound) >= 0 {
		v.Mod(v, bound)
	}
	return v
}

// fill assigns every circuit variable reachable from v; top is the top-level field of the wrapper the leaf
// belongs to.  Values of types with a documented domain (curve points, GT elements, bytes) come from the domain
// registry (valid values whatever the profile); emulated elements get canonical values below their modulus.
func (s *simSampler) fill(v reflect.Value, top string, depth int) error {
	if depth > 64 {
		return fmt.Errorf("%s: nesting too deep", top)
	}
	t := v.Type()
	if d, ok := domainRegistry[t]; ok && v.CanSet() {
		v.Set(d.fn(s.rng))
		return nil
	}
	if info, isEl := emInfoOf(t); isEl && v.CanSet() {
		if info == nil {
			return fmt.Errorf("%s: emulated element of an unregistered field %s", top, t)
		}
		v.Set(info.valueOf(s.value(s.leaf(top), info.Modulus)))
		return nil
	}
	switch t.Kind() {
	case reflect.Interface:
		if v.CanSet() && t == variableType {
			v.Set(reflect.ValueOf(s.value(s.leaf(top), s.native)))
		}
		return nil
	case reflect.Ptr:
		if v.IsNil() {
			return nil
		}
		return s.fill(v.Elem(), top, depth+1)
	case reflect.Slice, reflect.Array:
		for i := 0; i < v.Len(); i++ {
			if err := s.fill(v.Index(i), top, depth+1); err != nil {
				return err
			}
		}
		return nil
	case reflect.Struct:
		for i := 0; i < t.NumField(); i++ {
			f := t.Field(i)
			if !f.IsExported() || f.Tag.Get("gnark") == "-" {
				continue
			}
			name := top
			if depth == 1 {
				name = f.Name
			}
			if err := s.fill(v.Field(i), name, depth+1); err != nil {
				return err
			}
		}
		return nil
	}
	return nil
}

type simVector struct {
	label string
	leaf  func(rng *rand.Rand) func(top string) string
}

func topFields(c frontend.Circuit) []string {
	v := reflect.ValueOf(c)
	for v.Kind() == reflect.Ptr {
		v = v.Elem()
	}
	var out []string
	t := v.Type()
	for i := 0; i < t.NumField(); i++ {
		f := t.Field(i)
		if f.IsExported() && f.Tag.Get("gnark") != "-" {
			out = append(out, f.Name)
		}
	}
	return out
}

// simVectors: every leaf at one edge profile; with two or more input fields, each field at its maximum (others
// zero) and at zero (others maximal); then n random vectors, each seeded by its own index (prefix-stable).
func simVectors(fields []string, n int) []simVector {
	var out []simVector
	for _, p := range simEdge {
		p := p
		out = append(out, simVector{"edge: all " + p, func(*rand.Rand) func(string) string {
			return func(string) string { return p }
		}})
	}
	if len(fields) > 1 {
		for _, f := range fields {
			f := f
			out = append(out, simVector{"edge: " + f + " max, others zero", func(*rand.Rand) func(string) string {
				return func(top string) string {
					if top == f {
						return "max"
					}
					return "zero"
				}
			}})
			out = append(out, simVector{"edge: " + f + " zero, others max", func(*rand.Rand) func(string) string {
				return func(top string) string {
					if top == f {
						return "zero"
					}
					return "max"
				}
			}})
		}
	}
	for j := 0; j < n; j++ {
		out = append(out, simVector{fmt.Sprintf("random #%d", j), func(rng *rand.Rand) func(string) string {
			mode := simRandomModes[rng.Intn(len(simRandomModes))]
			switch mode {
			case "mixed":
				return func(string) string { return simProfiles[rng.Intn(len(simProfiles))] }
			case "edge-mixed":
				return func(string) string { return simEdge[rng.Intn(len(simEdge))] }
			}
			return func(string) string { return mode }
		}})
	}
	return out
}

// Simulate compiles the model of a wrapper and writes the screen's JSON lines to out.
func Simulate(id string, out string, opt SimOptions) error {
	f, err := os.Create(out)
	if err != nil {
		return err
	}
	defer f.Close()
	w := bufio.NewWriter(f)
	defer w.Flush()
	enc := json.NewEncoder(w)
	hdr := SimHeader{ID: id, Degree: -1, Profiles: simProfiles, Seed: opt.Seed}
	spec, ok := specs[id]
	if !ok {
		hdr.Status, hdr.Error = "compile-error", "unknown wrapper id"
		return enc.Encode(hdr)
	}
	hdr.Curve, hdr.Field = spec.Curve, spec.Field.String()
	sym, err := compile(spec, ModeSymbolic, nil, opt.Limit, true)
	if err != nil {
		hdr.Status, hdr.Error = "compile-error", shortErr(err)
		return enc.Encode(hdr)
	}
	model := []*Compiled{sym}
	hdr.Commits = sym.Commits
	if sym.Commits > 0 {
		d, _, err := degreeChecks(sym)
		if err != nil {
			hdr.Status, hdr.Error = "not-modelled", "commitment: "+err.Error()
			return enc.Encode(hdr)
		}
		if d+1 > opt.MaxPoints {
			hdr.Status, hdr.Error = "not-modelled", fmt.Sprintf("commitment: challenge degree %d needs more than %d points", d, opt.MaxPoints)
			return enc.Encode(hdr)
		}
		hdr.Degree, hdr.Points, hdr.BoundaryWires = d, d+1, sym.BoundaryWires
		model = nil
		for j := 0; j < d+1; j++ {
			cc, err := compile(spec, ModeConstant, big.NewInt(int64(j+2)), opt.Limit, false)
			if err != nil {
				hdr.Status, hdr.Error = "compile-error", "constant challenge compile: "+shortErr(err)
				return enc.Encode(hdr)
			}
			model = append(model, cc)
		}
	}
	for _, c := range model {
		hdr.NbWires = append(hdr.NbWires, c.NbWires)
	}
	hdr.NbPublic, hdr.NbSecret = sym.NbPublic, sym.NbSecret
	hdr.PublicNames, hdr.SecretNames = sym.PublicNames, sym.SecretNames
	hdr.Outputs, hdr.OutputPaths, hdr.Groups = sym.Outputs, sym.OutputPaths, sym.Groups
	vecs := simVectors(topFields(spec.New()), opt.N)
	hdr.Vectors = len(vecs)
	hdr.EdgeVectors = len(vecs) - opt.N
	hdr.Status = "ok"
	if err := enc.Encode(hdr); err != nil {
		return err
	}
	for k, vec := range vecs {
		rng := rand.New(rand.NewSource(simSeed(opt.Seed, vec.label)))
		smp := &simSampler{rng: rng, native: spec.Field, leaf: vec.leaf(rng)}
		line := SimLine{K: k, Label: vec.label}
		assign := spec.New()
		if err := smp.fill(reflect.ValueOf(assign), "", 0); err != nil {
			line.Error = "input: " + shortErr(err)
		} else if ws, err := solveAll(spec, model, assign); err != nil {
			line.Error = shortErr(err)
		} else {
			line.OK = true
			for _, o := range model[0].Outputs {
				line.Outputs = append(line.Outputs, ws[0][o])
			}
			if k < opt.Witnesses {
				line.Witnesses = ws
			}
		}
		if err := enc.Encode(line); err != nil {
			return err
		}
	}
	return nil
}
