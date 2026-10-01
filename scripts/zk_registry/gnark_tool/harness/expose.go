package harness

import (
	"errors"
	"fmt"
	"math/big"
	"reflect"

	"github.com/consensys/gnark/constraint"
	"github.com/consensys/gnark/frontend"
)

// Group is an emulated output element: Len consecutive output wires (limbs, least significant first,
// Bits each) whose value is taken modulo Modulus.
type Group struct {
	Start   int    `json:"start"`
	Len     int    `json:"len"`
	Bits    int    `json:"bits"`
	Modulus string `json:"modulus"`
	Field   string `json:"field"`
	Path    string `json:"path"`
}

// exposure state of the compilation in progress (one compilation at a time per process)
type exposure struct {
	test   bool // gnark's test engine: outputs are only asserted, not recorded
	wires  []int
	paths  []string
	groups []Group
	called bool
}

var current *exposure

type leaf struct {
	v    frontend.Variable
	path string
}

// flatten collects the circuit variables of a gadget result: values held by frontend.Variable
// (interface) slots, the limbs of emulated elements (padded to the field's limb count; recorded as a
// group), and recursively the exported fields of structs, the elements of slices and arrays, and
// pointer targets.  Go values that are not circuit variables (errors, booleans, integers declared as
// Go integers, gadget objects without exported variables) contribute nothing.
func flatten(native *big.Int, v reflect.Value, path string, out *[]leaf, groups *[]Group, depth int, viaIface bool) error {
	if depth > 64 {
		return errors.New("result nesting too deep")
	}
	if !v.IsValid() {
		return nil
	}
	t := v.Type()
	if t.Kind() == reflect.Interface {
		if v.IsNil() || t.Implements(errorType) && !t.Implements(variableType) {
			return nil
		}
		return flatten(native, v.Elem(), path, out, groups, depth+1, true)
	}
	if isLinearExpression(t) || viaIface && isConstant(v) {
		*out = append(*out, leaf{v.Interface(), path})
		return nil
	}
	switch t.Kind() {
	case reflect.Ptr:
		if v.IsNil() {
			return nil
		}
		return flatten(native, v.Elem(), path, out, groups, depth+1, false)
	case reflect.Slice, reflect.Array:
		if t.Elem().Kind() == reflect.Uint8 {
			return nil // raw bytes (Go values)
		}
		for i := 0; i < v.Len(); i++ {
			if err := flatten(native, v.Index(i), fmt.Sprintf("%s[%d]", path, i), out, groups, depth+1, false); err != nil {
				return err
			}
		}
		return nil
	case reflect.Struct:
		if info, isEl := emInfoOf(t); isEl {
			if info == nil {
				return fmt.Errorf("emulated element of an unregistered field: %s", t.String())
			}
			if !v.CanAddr() {
				cp := reflect.New(t)
				cp.Elem().Set(v)
				v = cp.Elem()
			}
			limbs := v.FieldByName("Limbs")
			if limbs.Len() == 0 {
				if m := v.Addr().MethodByName("Initialize"); m.IsValid() {
					m.Call([]reflect.Value{reflect.ValueOf(native)})
					limbs = v.FieldByName("Limbs")
				}
			}
			nb, bits := info.effective(native)
			n := limbs.Len()
			if int(nb) > n {
				n = int(nb)
			}
			start := len(*out)
			for i := 0; i < n; i++ {
				var lv frontend.Variable = 0
				if i < limbs.Len() {
					lv = limbs.Index(i).Interface()
				}
				*out = append(*out, leaf{lv, fmt.Sprintf("%s.Limbs[%d]", path, i)})
			}
			*groups = append(*groups, Group{Start: start, Len: n, Bits: int(bits), Modulus: info.Modulus.String(),
				Field: info.Name, Path: path})
			return nil
		}
		if t == reflect.TypeOf(big.Int{}) {
			return nil
		}
		for i := 0; i < t.NumField(); i++ {
			f := t.Field(i)
			if !f.IsExported() || f.Tag.Get("gnark") == "-" {
				continue
			}
			if err := flatten(native, v.Field(i), path+"."+f.Name, out, groups, depth+1, false); err != nil {
				return err
			}
		}
		return nil
	default:
		return nil
	}
}

var (
	errorType    = reflect.TypeOf((*error)(nil)).Elem()
	variableType = reflect.TypeOf((*frontend.Variable)(nil)).Elem()
)

// isLinearExpression: the variable representation of gnark's R1CS builder (expr.LinearExpression[E],
// internal to gnark) or a constraint.LinearExpression.
func isLinearExpression(t reflect.Type) bool {
	if t == reflect.TypeOf(constraint.LinearExpression{}) {
		return true
	}
	return t.Kind() == reflect.Slice && t.PkgPath() == "github.com/consensys/gnark/frontend/internal/expr"
}

// isConstant: a constant operand held by a frontend.Variable slot.
func isConstant(v reflect.Value) bool {
	switch v.Interface().(type) {
	case *big.Int, big.Int:
		return true
	}
	switch v.Kind() {
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64,
		reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64, reflect.String:
		return true
	}
	return false
}

// Expose (called with pointers to the gadget's results) makes their circuit variables observable: every variable becomes a new
// wire computed by ExposeHint and constrained equal to it; the new wires are the DET outputs.
func Expose(api frontend.API, results ...any) error {
	if current == nil {
		return errors.New("Expose called outside of a harness compilation")
	}
	if current.called {
		return errors.New("Expose called twice")
	}
	current.called = true
	native := api.Compiler().Field()
	var leaves []leaf
	var groups []Group
	for k, r := range results {
		rv := reflect.ValueOf(r)
		if rv.Kind() != reflect.Ptr {
			return errors.New("Expose takes pointers to the results")
		}
		if err := flatten(native, rv.Elem(), fmt.Sprintf("r%d", k), &leaves, &groups, 0, false); err != nil {
			return err
		}
	}
	current.groups = groups
	if len(leaves) == 0 {
		return nil
	}
	ins := make([]frontend.Variable, len(leaves))
	for i, l := range leaves {
		ins[i] = l.v
	}
	outs, err := api.Compiler().NewHint(ExposeHint, len(ins), ins...)
	if err != nil {
		return err
	}
	for i := range outs {
		api.AssertIsEqual(outs[i], ins[i])
		if current.test {
			continue
		}
		le, ok := api.Compiler().ToCanonicalVariable(outs[i]).(constraint.LinearExpression)
		if !ok || len(le) != 1 {
			return errors.New("exposed output is not a single wire")
		}
		current.wires = append(current.wires, int(le[0].VID))
		current.paths = append(current.paths, leaves[i].path)
	}
	return nil
}
