package harness

import (
	"fmt"
	"math/big"
	"math/rand"
	"reflect"
	"sort"
)

// Profiles of sampled values (native variables and emulated elements).
var profileNames = []string{"uniform", "bit", "byte", "u16", "u32", "u64", "zero", "one"}

// Sampler fills the input fields of a fresh wrapper circuit with an assignment.  Values of types
// with a documented domain (curve points, GT elements, bytes) come from gnark-crypto through the
// domain registry; emulated elements are canonical values below their modulus (or small values,
// by profile); native variables follow the profile.
type Sampler struct {
	rng       *rand.Rand
	native    *big.Int
	profile   int // -1: a random profile per leaf
	DomainHit map[string]int
}

func NewSampler(seed int64, native *big.Int, profile int) *Sampler {
	return &Sampler{rng: rand.New(rand.NewSource(seed)), native: native, profile: profile, DomainHit: map[string]int{}}
}

func (s *Sampler) leafProfile() int {
	if s.profile >= 0 {
		return s.profile
	}
	return s.rng.Intn(len(profileNames))
}

func (s *Sampler) value(p int, bound *big.Int) *big.Int {
	var v *big.Int
	switch profileNames[p] {
	case "uniform":
		v = new(big.Int).Rand(s.rng, bound)
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
	case "zero":
		v = big.NewInt(0)
	case "one":
		v = big.NewInt(1)
	}
	if v.Cmp(bound) >= 0 {
		v.Mod(v, bound)
	}
	return v
}

// Fill assigns every circuit variable reachable from v (a pointer to the circuit struct).
func (s *Sampler) Fill(v reflect.Value) error {
	return s.fill(v, "", 0)
}

func (s *Sampler) fill(v reflect.Value, path string, depth int) error {
	if depth > 64 {
		return fmt.Errorf("%s: nesting too deep", path)
	}
	t := v.Type()
	if d, ok := domainRegistry[t]; ok && v.CanSet() {
		v.Set(d.fn(s.rng))
		s.DomainHit[d.Name]++
		return nil
	}
	if info, isEl := emInfoOf(t); isEl && v.CanSet() {
		if info == nil {
			return fmt.Errorf("%s: emulated element of an unregistered field %s", path, t)
		}
		v.Set(info.valueOf(s.value(s.leafProfile(), info.Modulus)))
		return nil
	}
	switch t.Kind() {
	case reflect.Interface:
		if v.CanSet() && t == variableType {
			v.Set(reflect.ValueOf(s.value(s.leafProfile(), s.native)))
		}
		return nil
	case reflect.Ptr:
		if v.IsNil() {
			return nil
		}
		return s.fill(v.Elem(), path, depth+1)
	case reflect.Slice, reflect.Array:
		for i := 0; i < v.Len(); i++ {
			if err := s.fill(v.Index(i), fmt.Sprintf("%s[%d]", path, i), depth+1); err != nil {
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
			if err := s.fill(v.Field(i), path+"."+f.Name, depth+1); err != nil {
				return err
			}
		}
		return nil
	}
	return nil
}

// DomainNames lists the domain samplers used.
func (s *Sampler) DomainNames() []string {
	out := make([]string, 0, len(s.DomainHit))
	for k := range s.DomainHit {
		out = append(out, k)
	}
	sort.Strings(out)
	return out
}

// ProfileName of an attempt: attempts 0..31 cycle through the global profiles, later attempts draw a
// profile per leaf.
func ProfileOf(attempt int) int {
	if attempt < 32 {
		return attempt % len(profileNames)
	}
	return -1
}

func ProfileLabel(p int) string {
	if p < 0 {
		return "mixed"
	}
	return profileNames[p]
}
