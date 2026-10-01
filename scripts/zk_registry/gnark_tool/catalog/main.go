// boole-gnark-catalog: type information for the Boole gnark DET generator.
//
//	catalog -dir MODULE_DIR -targets TARGETS.json -out CATALOG.json PATTERN...
//
// Loads the packages matching the patterns (with their tests) by go/packages and writes, for every
// target {path, symbol, line} of TARGETS.json: its declaration (function, method or type) with the
// signature as type trees; the named types reachable from those signatures (underlying type,
// exported fields, type parameters); every exported top-level function of the loaded packages
// (constructor and provider candidates); the concrete instantiations of generic types and functions
// written in the repository (with file, line and whether the file is a test); and the call sites of
// every target (constant argument values, the receiver type at the call).
package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"go/ast"
	"go/printer"
	"go/token"
	"go/types"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"golang.org/x/tools/go/packages"
)

type TypeRef struct {
	K     string     `json:"k"` // named | ptr | slice | array | basic | tparam | iface | func | map | chan | struct | other
	Pkg   string     `json:"pkg,omitempty"`
	Name  string     `json:"name,omitempty"`
	Args  []*TypeRef `json:"args,omitempty"`
	Elem  *TypeRef   `json:"elem,omitempty"`
	Len   int64      `json:"len,omitempty"`
	Str   string     `json:"str,omitempty"`
	Alias string     `json:"alias,omitempty"`
}

type Param struct {
	Name string   `json:"name"`
	Type *TypeRef `json:"type"`
}

type TParam struct {
	Name       string `json:"name"`
	Constraint string `json:"constraint"`
}

type Sig struct {
	TParams  []TParam `json:"tparams,omitempty"`
	Params   []Param  `json:"params"`
	Results  []Param  `json:"results"`
	Variadic bool     `json:"variadic"`
}

type Decl struct {
	Kind      string   `json:"kind"` // func | method | type
	Pkg       string   `json:"pkg"`
	PkgName   string   `json:"pkg_name"`
	Name      string   `json:"name"`
	Recv      *TypeRef `json:"recv,omitempty"`     // method receiver (as declared)
	RecvPtr   bool     `json:"recv_ptr,omitempty"` // pointer receiver
	RecvTP    []TParam `json:"recv_tparams,omitempty"`
	Sig       *Sig     `json:"sig,omitempty"`
	Type      *TypeRef `json:"type,omitempty"` // kind type: the named type
	File      string   `json:"file"`
	Line      int      `json:"line"`
	Sha       string   `json:"sha,omitempty"` // sha256 of the comment-free declaration text
	Doc       string   `json:"doc,omitempty"`
	HasDefine bool     `json:"has_define,omitempty"`
	Callees   []string `json:"callees,omitempty"` // functions/methods called in the body (pkg.Name or pkg.Type.Name)
}

type Field struct {
	Name     string   `json:"name"`
	Type     *TypeRef `json:"type"`
	Tag      string   `json:"tag,omitempty"`
	Embedded bool     `json:"embedded,omitempty"`
}

type NamedInfo struct {
	Pkg        string   `json:"pkg"`
	Name       string   `json:"name"`
	TParams    []TParam `json:"tparams,omitempty"`
	Underlying *TypeRef `json:"underlying"`
	Fields     []Field  `json:"fields,omitempty"`
	Methods    []string `json:"methods,omitempty"`
	HasDefine  bool     `json:"has_define"`
}

type Instance struct {
	Generic string     `json:"generic"` // pkg.Name
	Args    []*TypeRef `json:"args"`
	File    string     `json:"file"`
	Line    int        `json:"line"`
	Test    bool       `json:"test"`
}

type Arg struct {
	Const string   `json:"const,omitempty"`
	Type  *TypeRef `json:"type,omitempty"`
	Expr  string   `json:"expr,omitempty"`
	// identifier arguments: the call that defined the identifier in the calling function (its callee
	// key and constant arguments), e.g. a hasher obtained from mimc.NewMiMC(api)
	Origin     string   `json:"origin,omitempty"`
	OriginArgs []string `json:"origin_args,omitempty"`
}

type Call struct {
	Target string     `json:"target"`
	File   string     `json:"file"`
	Line   int        `json:"line"`
	Test   bool       `json:"test"`
	Recv   *TypeRef   `json:"recv,omitempty"`
	TArgs  []*TypeRef `json:"targs,omitempty"`
	Args   []Arg      `json:"args"`
	Caller string     `json:"caller,omitempty"`
}

type Target struct {
	Path   string `json:"path"`
	Symbol string `json:"symbol"`
	Line   int    `json:"line"`
}

type TargetOut struct {
	Target
	Found bool   `json:"found"`
	Why   string `json:"why,omitempty"`
	Key   string `json:"key,omitempty"`
	Decl  *Decl  `json:"decl,omitempty"`
}

// FuncIndex: every function and method declared in non-test files of the module: sha256 of its
// comment-free source text and the functions it calls (content keys and decomposition).
type FuncIndex struct {
	Sha     string   `json:"sha"`
	File    string   `json:"file"`
	Line    int      `json:"line"`
	Callees []string `json:"callees,omitempty"`
}

type Catalog struct {
	Module    string                `json:"module"`
	Index     map[string]*FuncIndex `json:"index"`
	Dir       string                `json:"dir"`
	Targets   []TargetOut           `json:"targets"`
	Named     map[string]*NamedInfo `json:"named"`
	Funcs     []*Decl               `json:"funcs"`
	Instances []Instance            `json:"instances"`
	Calls     []Call                `json:"calls"`
	Errors    []string              `json:"errors,omitempty"`
}

var (
	dir     string
	fset    *token.FileSet
	named   = map[string]*NamedInfo{}
	pending []*types.TypeName
)

func rel(p string) string {
	r, err := filepath.Rel(dir, p)
	if err != nil {
		return p
	}
	return filepath.ToSlash(r)
}

func isTest(file string) bool { return strings.HasSuffix(file, "_test.go") }

func tref(t types.Type) *TypeRef {
	switch x := t.(type) {
	case *types.Alias:
		r := tref(types.Unalias(x))
		if obj := x.Obj(); obj != nil && obj.Pkg() != nil {
			r.Alias = obj.Pkg().Path() + "." + obj.Name()
		}
		return r
	case *types.Named:
		obj := x.Obj()
		r := &TypeRef{K: "named", Name: obj.Name()}
		if obj.Pkg() != nil {
			r.Pkg = obj.Pkg().Path()
			note(x.Origin().Obj())
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
	case *types.Signature:
		return &TypeRef{K: "func", Str: types.TypeString(x, qual)}
	case *types.Map:
		return &TypeRef{K: "map", Str: types.TypeString(x, qual)}
	case *types.Chan:
		return &TypeRef{K: "chan", Str: types.TypeString(x, qual)}
	case *types.Struct:
		return &TypeRef{K: "struct", Str: types.TypeString(x, qual)}
	}
	return &TypeRef{K: "other", Str: types.TypeString(t, qual)}
}

func qual(p *types.Package) string { return p.Path() }

func note(obj *types.TypeName) {
	key := obj.Pkg().Path() + "." + obj.Name()
	if _, ok := named[key]; ok {
		return
	}
	named[key] = nil
	pending = append(pending, obj)
}

func tparams(l *types.TypeParamList) []TParam {
	var out []TParam
	for i := 0; l != nil && i < l.Len(); i++ {
		tp := l.At(i)
		out = append(out, TParam{Name: tp.Obj().Name(), Constraint: types.TypeString(tp.Constraint(), qual)})
	}
	return out
}

func sigOf(s *types.Signature) *Sig {
	out := &Sig{Variadic: s.Variadic(), TParams: tparams(s.TypeParams())}
	for i := 0; i < s.Params().Len(); i++ {
		v := s.Params().At(i)
		out.Params = append(out.Params, Param{Name: v.Name(), Type: tref(v.Type())})
	}
	for i := 0; i < s.Results().Len(); i++ {
		v := s.Results().At(i)
		out.Results = append(out.Results, Param{Name: v.Name(), Type: tref(v.Type())})
	}
	if out.Params == nil {
		out.Params = []Param{}
	}
	if out.Results == nil {
		out.Results = []Param{}
	}
	return out
}

func hasDefine(t types.Type) bool {
	ms := types.NewMethodSet(types.NewPointer(t))
	for i := 0; i < ms.Len(); i++ {
		if ms.At(i).Obj().Name() == "Define" {
			return true
		}
	}
	return false
}

func describeNamed(obj *types.TypeName) *NamedInfo {
	t := obj.Type()
	info := &NamedInfo{Pkg: obj.Pkg().Path(), Name: obj.Name()}
	if n, ok := t.(*types.Named); ok {
		info.TParams = tparams(n.TypeParams())
		for i := 0; i < n.NumMethods(); i++ {
			if n.Method(i).Exported() {
				info.Methods = append(info.Methods, n.Method(i).Name())
			}
		}
	}
	u := t.Underlying()
	info.Underlying = tref(u)
	if st, ok := u.(*types.Struct); ok {
		for i := 0; i < st.NumFields(); i++ {
			f := st.Field(i)
			info.Fields = append(info.Fields, Field{Name: f.Name(), Type: tref(f.Type()), Tag: st.Tag(i), Embedded: f.Embedded()})
		}
	}
	info.HasDefine = hasDefine(t)
	return info
}

func docOf(cg *ast.CommentGroup) string {
	if cg == nil {
		return ""
	}
	s := cg.Text()
	if len(s) > 1200 {
		s = s[:1200]
	}
	return s
}

func funcKey(f *types.Func) string {
	f = f.Origin()
	sig := f.Type().(*types.Signature)
	if f.Pkg() == nil {
		return f.Name()
	}
	if r := sig.Recv(); r != nil {
		rt := r.Type()
		if p, ok := rt.(*types.Pointer); ok {
			rt = p.Elem()
		}
		if n, ok := types.Unalias(rt).(*types.Named); ok {
			return f.Pkg().Path() + "." + n.Obj().Name() + "." + f.Name()
		}
	}
	return f.Pkg().Path() + "." + f.Name()
}

func main() {
	flag.StringVar(&dir, "dir", ".", "module directory")
	targetsPath := flag.String("targets", "", "targets JSON")
	out := flag.String("out", "catalog.json", "output")
	flag.Parse()
	var targets []Target
	if *targetsPath != "" {
		b, err := os.ReadFile(*targetsPath)
		if err != nil {
			panic(err)
		}
		if err := json.Unmarshal(b, &targets); err != nil {
			panic(err)
		}
	}
	absDir, _ := filepath.Abs(dir)
	dir = absDir
	fset = token.NewFileSet()
	cfg := &packages.Config{Dir: dir, Fset: fset, Tests: true,
		Mode: packages.NeedName | packages.NeedFiles | packages.NeedSyntax | packages.NeedTypes |
			packages.NeedTypesInfo | packages.NeedImports | packages.NeedModule}
	pkgs, err := packages.Load(cfg, flag.Args()...)
	if err != nil {
		panic(err)
	}
	cat := &Catalog{Dir: dir, Named: map[string]*NamedInfo{}, Index: map[string]*FuncIndex{}}
	for _, p := range pkgs {
		for _, e := range p.Errors {
			cat.Errors = append(cat.Errors, e.Error())
		}
		if p.Module != nil && cat.Module == "" {
			cat.Module = p.Module.Path
		}
	}
	// declarations by (file, name); funcs of the non-test variants; instances and calls of every variant
	type declAt struct {
		decl *Decl
		obj  types.Object
	}
	decls := map[string][]declAt{} // rel file -> decls
	targetPkgs := map[string]bool{}
	targetObjs := map[string]bool{}
	seenFile := map[string]bool{}
	seenFunc := map[string]bool{}
	var allPkgs []*packages.Package
	packages.Visit(pkgs, nil, func(p *packages.Package) {
		if p.Module == nil || p.Module.Path != cat.Module || p.TypesInfo == nil {
			return
		}
		allPkgs = append(allPkgs, p)
	})
	sort.Slice(allPkgs, func(i, j int) bool { return allPkgs[i].ID < allPkgs[j].ID })
	for _, p := range allPkgs {
		for _, f := range p.Syntax {
			fname := rel(fset.Position(f.Pos()).Filename)
			first := !seenFile[fname]
			seenFile[fname] = true
			if !first {
				continue
			}
			for _, d := range f.Decls {
				switch dd := d.(type) {
				case *ast.FuncDecl:
					obj, _ := p.TypesInfo.Defs[dd.Name].(*types.Func)
					if obj == nil {
						continue
					}
					sig := obj.Type().(*types.Signature)
					decl := &Decl{Pkg: p.PkgPath, PkgName: p.Name, Name: obj.Name(), Sig: sigOf(sig), File: fname,
						Line: fset.Position(dd.Pos()).Line, Doc: docOf(dd.Doc), Kind: "func"}
					if r := sig.Recv(); r != nil {
						decl.Kind = "method"
						rt := r.Type()
						if pt, ok := rt.(*types.Pointer); ok {
							decl.RecvPtr = true
							rt = pt.Elem()
						}
						decl.Recv = tref(rt)
						if n, ok := types.Unalias(rt).(*types.Named); ok {
							decl.RecvTP = tparams(n.Origin().TypeParams())
						}
						decl.Sig.TParams = nil
					}
					if dd.Body != nil {
						seen := map[string]bool{}
						ast.Inspect(dd.Body, func(n ast.Node) bool {
							ce, ok := n.(*ast.CallExpr)
							if !ok {
								return true
							}
							var id *ast.Ident
							switch fn := ast.Unparen(ce.Fun).(type) {
							case *ast.Ident:
								id = fn
							case *ast.SelectorExpr:
								id = fn.Sel
							case *ast.IndexExpr:
								if s, ok := fn.X.(*ast.SelectorExpr); ok {
									id = s.Sel
								} else if i, ok := fn.X.(*ast.Ident); ok {
									id = i
								}
							case *ast.IndexListExpr:
								if s, ok := fn.X.(*ast.SelectorExpr); ok {
									id = s.Sel
								} else if i, ok := fn.X.(*ast.Ident); ok {
									id = i
								}
							}
							if id == nil {
								return true
							}
							if fo, ok := p.TypesInfo.Uses[id].(*types.Func); ok && fo.Pkg() != nil {
								k := funcKey(fo)
								if !seen[k] {
									seen[k] = true
									decl.Callees = append(decl.Callees, k)
								}
							}
							return true
						})
					}
					decl.Sha = textSha(dd)
					if !isTest(fname) {
						cat.Index[funcKey(obj)] = &FuncIndex{Sha: decl.Sha, File: fname, Line: decl.Line, Callees: decl.Callees}
					}
					decls[fname] = append(decls[fname], declAt{decl, obj})
					if obj.Exported() && !isTest(fname) && decl.Kind == "func" {
						k := p.PkgPath + "." + obj.Name()
						if !seenFunc[k] {
							seenFunc[k] = true
							cat.Funcs = append(cat.Funcs, decl)
						}
					}
				case *ast.GenDecl:
					for _, s := range dd.Specs {
						ts, ok := s.(*ast.TypeSpec)
						if !ok {
							continue
						}
						obj, _ := p.TypesInfo.Defs[ts.Name].(*types.TypeName)
						if obj == nil {
							continue
						}
						doc := ts.Doc
						if doc == nil {
							doc = dd.Doc
						}
						decl := &Decl{Kind: "type", Pkg: p.PkgPath, PkgName: p.Name, Name: obj.Name(), Type: tref(obj.Type()),
							File: fname, Line: fset.Position(ts.Pos()).Line, Doc: docOf(doc), HasDefine: hasDefine(obj.Type()),
							Sha: textSha(ts)}
						decls[fname] = append(decls[fname], declAt{decl, obj})
					}
				}
			}
		}
	}
	// targets
	for _, t := range targets {
		to := TargetOut{Target: t}
		name := t.Symbol
		recv := ""
		if i := strings.LastIndex(name, "."); i >= 0 {
			recv, name = name[:i], name[i+1:]
		}
		var best *declAt
		for i := range decls[t.Path] {
			d := &decls[t.Path][i]
			if d.decl.Name != name {
				continue
			}
			if recv != "" && (d.decl.Kind != "method" || d.decl.Recv == nil || d.decl.Recv.Name != recv) {
				continue
			}
			if recv == "" && d.decl.Kind == "method" {
				continue
			}
			if best == nil || abs(d.decl.Line-t.Line) < abs(best.decl.Line-t.Line) {
				best = d
			}
		}
		if best == nil {
			to.Why = "no declaration " + t.Symbol + " in " + t.Path
		} else {
			to.Found, to.Decl = true, best.decl
			targetPkgs[best.decl.Pkg] = true
			if f, ok := best.obj.(*types.Func); ok {
				to.Key = funcKey(f)
				targetObjs[to.Key] = true
			} else {
				to.Key = best.decl.Pkg + "." + best.decl.Name
				note(best.obj.(*types.TypeName))
			}
		}
		cat.Targets = append(cat.Targets, to)
	}
	// instances and calls
	seenInst := map[string]bool{}
	seenCall := map[string]bool{}
	for _, p := range allPkgs {
		info := p.TypesInfo
		for id, inst := range info.Instances {
			pos := fset.Position(id.Pos())
			fname := rel(pos.Filename)
			obj := info.Uses[id]
			if obj == nil || obj.Pkg() == nil {
				continue
			}
			gen := obj.Pkg().Path() + "." + obj.Name()
			if fo, ok := obj.(*types.Func); ok {
				gen = funcKey(fo)
			}
			var args []*TypeRef
			concrete := true
			for i := 0; i < inst.TypeArgs.Len(); i++ {
				a := inst.TypeArgs.At(i)
				if hasTypeParam(a) {
					concrete = false
					break
				}
				args = append(args, tref(a))
			}
			if !concrete {
				continue
			}
			b, _ := json.Marshal(args)
			k := fmt.Sprintf("%s|%s|%s|%d", gen, b, fname, pos.Line)
			if seenInst[k] {
				continue
			}
			seenInst[k] = true
			cat.Instances = append(cat.Instances, Instance{Generic: gen, Args: args, File: fname, Line: pos.Line, Test: isTest(fname)})
		}
		for _, f := range p.Syntax {
			fname := rel(fset.Position(f.Pos()).Filename)
			// calls of targets
			var stack []ast.Node
			ast.Inspect(f, func(n ast.Node) bool {
				if n == nil {
					stack = stack[:len(stack)-1]
					return true
				}
				stack = append(stack, n)
				ce, ok := n.(*ast.CallExpr)
				if !ok {
					return true
				}
				var id *ast.Ident
				var sel *ast.SelectorExpr
				fun := ast.Unparen(ce.Fun)
				if ix, ok := fun.(*ast.IndexExpr); ok {
					fun = ix.X
				} else if ix, ok := fun.(*ast.IndexListExpr); ok {
					fun = ix.X
				}
				switch fn := fun.(type) {
				case *ast.Ident:
					id = fn
				case *ast.SelectorExpr:
					id, sel = fn.Sel, fn
				}
				if id == nil {
					return true
				}
				fo, ok := info.Uses[id].(*types.Func)
				if !ok {
					return true
				}
				key := funcKey(fo)
				if !targetObjs[key] && (fo.Pkg() == nil || !targetPkgs[fo.Pkg().Path()]) {
					return true
				}
				pos := fset.Position(ce.Pos())
				ck := fmt.Sprintf("%s|%s|%d|%d", key, fname, pos.Line, pos.Column)
				if seenCall[ck] {
					return true
				}
				seenCall[ck] = true
				call := Call{Target: key, File: fname, Line: pos.Line, Test: isTest(fname)}
				if sel != nil {
					if s, ok := info.Selections[sel]; ok {
						call.Recv = tref(s.Recv())
					}
				}
				if inst, ok := info.Instances[id]; ok {
					for i := 0; i < inst.TypeArgs.Len(); i++ {
						call.TArgs = append(call.TArgs, tref(inst.TypeArgs.At(i)))
					}
				}
				var encl *ast.FuncDecl
				for i := len(stack) - 1; i >= 0; i-- {
					if fd, ok := stack[i].(*ast.FuncDecl); ok {
						encl = fd
						break
					}
				}
				for _, a := range ce.Args {
					tv := info.Types[a]
					arg := Arg{Expr: exprString(a)}
					if tv.Value != nil {
						arg.Const = tv.Value.ExactString()
					}
					if tv.Type != nil {
						arg.Type = tref(tv.Type)
					}
					if encl != nil {
						arg.Origin, arg.OriginArgs = origin(info, encl, a)
					}
					call.Args = append(call.Args, arg)
				}
				if encl != nil {
					call.Caller = encl.Name.Name
				}
				cat.Calls = append(cat.Calls, call)
				return true
			})
		}
	}
	// named type closure
	for len(pending) > 0 {
		obj := pending[0]
		pending = pending[1:]
		named[obj.Pkg().Path()+"."+obj.Name()] = describeNamed(obj)
	}
	for k, v := range named {
		if v != nil {
			cat.Named[k] = v
		}
	}
	sort.Slice(cat.Instances, func(i, j int) bool {
		a, b := cat.Instances[i], cat.Instances[j]
		if a.Generic != b.Generic {
			return a.Generic < b.Generic
		}
		if a.File != b.File {
			return a.File < b.File
		}
		return a.Line < b.Line
	})
	sort.Slice(cat.Calls, func(i, j int) bool {
		a, b := cat.Calls[i], cat.Calls[j]
		if a.Target != b.Target {
			return a.Target < b.Target
		}
		if a.File != b.File {
			return a.File < b.File
		}
		return a.Line < b.Line
	})
	sort.Slice(cat.Funcs, func(i, j int) bool {
		return cat.Funcs[i].Pkg+"."+cat.Funcs[i].Name < cat.Funcs[j].Pkg+"."+cat.Funcs[j].Name
	})
	fo, err := os.Create(*out)
	if err != nil {
		panic(err)
	}
	defer fo.Close()
	enc := json.NewEncoder(fo)
	if err := enc.Encode(cat); err != nil {
		panic(err)
	}
}

// textSha: sha256 of the declaration printed without comments.
func textSha(n ast.Node) string {
	var b strings.Builder
	if fd, ok := n.(*ast.FuncDecl); ok {
		cp := *fd
		cp.Doc = nil
		n = &cp
	}
	if err := printer.Fprint(&b, token.NewFileSet(), n); err != nil {
		return ""
	}
	h := sha256.Sum256([]byte(b.String()))
	return hex.EncodeToString(h[:])
}

func exprString(e ast.Expr) string {
	var b strings.Builder
	if err := printer.Fprint(&b, fset, e); err != nil {
		return ""
	}
	out := b.String()
	if len(out) > 160 {
		out = out[:160]
	}
	return out
}

// origin: for an identifier argument (possibly &ident), the call expression assigned to it in the
// enclosing function.
func origin(info *types.Info, fd *ast.FuncDecl, a ast.Expr) (string, []string) {
	a = ast.Unparen(a)
	if u, ok := a.(*ast.UnaryExpr); ok && u.Op == token.AND {
		a = ast.Unparen(u.X)
	}
	id, ok := a.(*ast.Ident)
	if !ok {
		return "", nil
	}
	obj := info.Uses[id]
	if obj == nil {
		return "", nil
	}
	var key string
	var cargs []string
	ast.Inspect(fd, func(n ast.Node) bool {
		if key != "" {
			return false
		}
		as, ok := n.(*ast.AssignStmt)
		if !ok || len(as.Rhs) != 1 {
			return true
		}
		ce, ok := ast.Unparen(as.Rhs[0]).(*ast.CallExpr)
		if !ok {
			return true
		}
		match := false
		for _, l := range as.Lhs {
			if li, ok := l.(*ast.Ident); ok && (info.Defs[li] == obj || info.Uses[li] == obj) {
				match = true
			}
		}
		if !match {
			return true
		}
		fun := ast.Unparen(ce.Fun)
		if ix, ok := fun.(*ast.IndexExpr); ok {
			fun = ix.X
		} else if ix, ok := fun.(*ast.IndexListExpr); ok {
			fun = ix.X
		}
		var cid *ast.Ident
		switch fn := fun.(type) {
		case *ast.Ident:
			cid = fn
		case *ast.SelectorExpr:
			cid = fn.Sel
		}
		if cid == nil {
			return true
		}
		if fo, ok := info.Uses[cid].(*types.Func); ok {
			key = funcKey(fo)
			for _, x := range ce.Args {
				if tv := info.Types[x]; tv.Value != nil {
					cargs = append(cargs, tv.Value.ExactString())
				} else {
					cargs = append(cargs, exprString(x))
				}
			}
		}
		return true
	})
	return key, cargs
}

func abs(x int) int {
	if x < 0 {
		return -x
	}
	return x
}

func hasTypeParam(t types.Type) bool {
	switch x := t.(type) {
	case *types.TypeParam:
		return true
	case *types.Alias:
		return hasTypeParam(types.Unalias(x))
	case *types.Named:
		if ta := x.TypeArgs(); ta != nil {
			for i := 0; i < ta.Len(); i++ {
				if hasTypeParam(ta.At(i)) {
					return true
				}
			}
		}
		return false
	case *types.Pointer:
		return hasTypeParam(x.Elem())
	case *types.Slice:
		return hasTypeParam(x.Elem())
	case *types.Array:
		return hasTypeParam(x.Elem())
	case *types.Map:
		return hasTypeParam(x.Key()) || hasTypeParam(x.Elem())
	}
	return false
}
