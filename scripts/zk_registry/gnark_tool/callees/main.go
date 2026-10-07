// boole-gnark-callees: the gadget instances a gnark function compiles, for the decomposition of TOO-LARGE records.
//
//	callees -dir MODULE_DIR -roots ROOTS_PKG_DIR -out CALLEES.json
//
// ROOTS_PKG_DIR (inside the module) holds one generated Go file whose package-level variables
// `BooleRoot<i>` are method expressions or function values of the TOO-LARGE records at their recorded type
// arguments (e.g. `(*sw_emulated.Curve[emparams.BN254Fp, emparams.BN254Fr]).ScalarMul`).  The packages are
// built in SSA form with generic functions instantiated; Rapid Type Analysis from each root yields the call
// graph of the instantiated code (static calls, closures, and interface calls resolved by the types the
// reachable code creates).  For every reachable function declared in the module the output lists its key
// (`pkg.Name` or `pkg.Type.Method`, as the catalog), the type arguments of its instance (receiver type
// arguments, then the function's own) as type trees, the type parameter names, and its depth below the root.
package main

import (
	"encoding/json"
	"flag"
	"go/types"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"

	"golang.org/x/tools/go/callgraph/rta"
	"golang.org/x/tools/go/packages"
	"golang.org/x/tools/go/ssa"
	"golang.org/x/tools/go/ssa/ssautil"
)

type TypeRef struct {
	K    string     `json:"k"`
	Pkg  string     `json:"pkg,omitempty"`
	Name string     `json:"name,omitempty"`
	Args []*TypeRef `json:"args,omitempty"`
	Elem *TypeRef   `json:"elem,omitempty"`
	Len  int64      `json:"len,omitempty"`
	Str  string     `json:"str,omitempty"`
}

type Callee struct {
	Key     string              `json:"key"`
	TParams []string            `json:"tparams,omitempty"`
	Env     map[string]*TypeRef `json:"env,omitempty"`
	Label   string              `json:"label"`
	Depth   int                 `json:"depth"`
	Callers []string            `json:"callers,omitempty"` // keys of reachable functions calling it (first 5)
}

type Root struct {
	Var     string    `json:"var"`
	Key     string    `json:"key,omitempty"`
	Error   string    `json:"error,omitempty"`
	Callees []*Callee `json:"callees"`
}

func qual(p *types.Package) string { return p.Path() }

func tref(t types.Type) *TypeRef {
	switch x := t.(type) {
	case *types.Alias:
		return tref(types.Unalias(x))
	case *types.Named:
		obj := x.Obj()
		r := &TypeRef{K: "named", Name: obj.Name()}
		if obj.Pkg() != nil {
			r.Pkg = obj.Pkg().Path()
		}
		if ta := x.TypeArgs(); ta != nil {
			for i := 0; i < ta.Len(); i++ {
				r.Args = append(r.Args, tref(ta.At(i)))
			}
		}
		return r
	case *types.Pointer:
		return &TypeRef{K: "ptr", Elem: tref(x.Elem())}
	case *types.Slice:
		return &TypeRef{K: "slice", Elem: tref(x.Elem())}
	case *types.Array:
		return &TypeRef{K: "array", Len: x.Len(), Elem: tref(x.Elem())}
	case *types.Basic:
		return &TypeRef{K: "basic", Name: x.Name()}
	case *types.TypeParam:
		return &TypeRef{K: "tparam", Name: x.Obj().Name()}
	case *types.Interface:
		return &TypeRef{K: "iface", Str: types.TypeString(x, qual)}
	}
	return &TypeRef{K: "other", Str: types.TypeString(t, qual)}
}

// show: a type tree as Go text with full import paths (aliases resolved, so one type has one text).
func show(t *TypeRef) string {
	switch t.K {
	case "named":
		s := t.Name
		if t.Pkg != "" {
			s = t.Pkg + "." + t.Name
		}
		if len(t.Args) > 0 {
			var a []string
			for _, x := range t.Args {
				a = append(a, show(x))
			}
			s += "[" + strings.Join(a, ", ") + "]"
		}
		return s
	case "ptr":
		return "*" + show(t.Elem)
	case "slice":
		return "[]" + show(t.Elem)
	case "array":
		return "[" + strconv.FormatInt(t.Len, 10) + "]" + show(t.Elem)
	case "basic", "tparam":
		return t.Name
	}
	return t.Str
}

// key of a function as the catalog writes it (generic origin; receiver type name without arguments).
func key(fn *ssa.Function) (string, bool) {
	o := fn
	if fn.Origin() != nil {
		o = fn.Origin()
	}
	obj, ok := o.Object().(*types.Func)
	if !ok || obj == nil || obj.Pkg() == nil || o.Synthetic != "" {
		return "", false
	}
	sig := obj.Type().(*types.Signature)
	if r := sig.Recv(); r != nil {
		rt := r.Type()
		if p, ok := rt.(*types.Pointer); ok {
			rt = p.Elem()
		}
		if n, ok := types.Unalias(rt).(*types.Named); ok {
			return obj.Pkg().Path() + "." + n.Obj().Name() + "." + obj.Name(), true
		}
		return "", false
	}
	return obj.Pkg().Path() + "." + obj.Name(), true
}

func tparamNames(fn *ssa.Function) []string {
	o := fn
	if fn.Origin() != nil {
		o = fn.Origin()
	}
	var out []string
	if rtp := o.Signature.RecvTypeParams(); rtp != nil {
		for i := 0; i < rtp.Len(); i++ {
			out = append(out, rtp.At(i).Obj().Name())
		}
	}
	if tp := o.Signature.TypeParams(); tp != nil {
		for i := 0; i < tp.Len(); i++ {
			out = append(out, tp.At(i).Obj().Name())
		}
	}
	return out
}

func main() {
	dir := flag.String("dir", ".", "module directory")
	roots := flag.String("roots", "", "package directory of the generated roots (inside the module)")
	out := flag.String("out", "callees.json", "output")
	prefix := flag.String("prefix", "", "only functions of packages with this import path prefix")
	flag.Parse()
	absDir, _ := filepath.Abs(*dir)
	cfg := &packages.Config{Dir: absDir, Mode: packages.LoadAllSyntax}
	absRoots, _ := filepath.Abs(*roots)
	rel, err := filepath.Rel(absDir, absRoots)
	if err != nil || strings.HasPrefix(rel, "..") {
		panic("the roots package must be inside the module directory")
	}
	pkgs, err := packages.Load(cfg, "./"+filepath.ToSlash(rel))
	if err != nil {
		panic(err)
	}
	var errs []string
	packages.Visit(pkgs, nil, func(p *packages.Package) {
		for _, e := range p.Errors {
			errs = append(errs, e.Error())
		}
	})
	if len(errs) > 0 {
		os.Stderr.WriteString(strings.Join(errs, "\n") + "\n")
		os.Exit(2)
	}
	prog, spkgs := ssautil.AllPackages(pkgs, ssa.InstantiateGenerics)
	prog.Build()
	rp := spkgs[0]
	// the roots: the function each BooleRoot<i> variable is initialised with (a thunk for method expressions)
	rootFn := map[string]*ssa.Function{}
	for _, b := range rp.Func("init").Blocks {
		for _, ins := range b.Instrs {
			st, ok := ins.(*ssa.Store)
			if !ok {
				continue
			}
			g, ok := st.Addr.(*ssa.Global)
			if !ok || !strings.HasPrefix(g.Name(), "BooleRoot") {
				continue
			}
			if f, ok := st.Val.(*ssa.Function); ok {
				rootFn[g.Name()] = f
			} else if mc, ok := st.Val.(*ssa.MakeClosure); ok {
				rootFn[g.Name()] = mc.Fn.(*ssa.Function)
			}
		}
	}
	names := make([]string, 0, len(rootFn))
	for n := range rootFn {
		names = append(names, n)
	}
	sort.Strings(names)
	var res []Root
	for _, n := range names {
		f := rootFn[n]
		r := Root{Var: n, Callees: []*Callee{}}
		an := rta.Analyze([]*ssa.Function{f}, true)
		if an == nil || an.CallGraph == nil {
			r.Error = "no call graph"
			res = append(res, r)
			continue
		}
		depth := map[*ssa.Function]int{f: 0}
		queue := []*ssa.Function{f}
		callers := map[*ssa.Function][]*ssa.Function{}
		for len(queue) > 0 {
			cur := queue[0]
			queue = queue[1:]
			node := an.CallGraph.Nodes[cur]
			if node == nil {
				continue
			}
			for _, e := range node.Out {
				g := e.Callee.Func
				if g == nil {
					continue
				}
				if len(callers[g]) < 5 {
					callers[g] = append(callers[g], cur)
				}
				if _, ok := depth[g]; ok {
					continue
				}
				depth[g] = depth[cur] + 1
				queue = append(queue, g)
			}
		}
		// the root's own function: the first module function below a synthetic thunk
		for g, d := range depth {
			if k, ok := key(g); ok && d <= 1 && r.Key == "" && strings.HasPrefix(k, *prefix) {
				r.Key = k
			}
		}
		seen := map[string]*Callee{}
		for g, d := range depth {
			k, ok := key(g)
			if !ok || !strings.HasPrefix(k, *prefix) {
				continue
			}
			c := &Callee{Key: k, TParams: tparamNames(g), Depth: d, Env: map[string]*TypeRef{}}
			targs := g.TypeArgs()
			var lab []string
			for i, tp := range c.TParams {
				if i < len(targs) {
					c.Env[tp] = tref(targs[i])
					lab = append(lab, tp+"="+show(c.Env[tp]))
				}
			}
			c.Label = k + "[" + strings.Join(lab, ", ") + "]"
			for _, cl := range callers[g] {
				if ck, ok := key(cl); ok {
					c.Callers = append(c.Callers, ck)
				}
			}
			if old, ok := seen[c.Label]; ok {
				if d < old.Depth {
					old.Depth = d
				}
				continue
			}
			seen[c.Label] = c
			r.Callees = append(r.Callees, c)
		}
		sort.Slice(r.Callees, func(i, j int) bool {
			if r.Callees[i].Depth != r.Callees[j].Depth {
				return r.Callees[i].Depth < r.Callees[j].Depth
			}
			return r.Callees[i].Label < r.Callees[j].Label
		})
		res = append(res, r)
	}
	b, _ := json.MarshalIndent(res, "", " ")
	if err := os.WriteFile(*out, b, 0o644); err != nil {
		panic(err)
	}
}
