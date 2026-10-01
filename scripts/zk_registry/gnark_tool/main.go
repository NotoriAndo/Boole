// boole-gnarkx: compiles generated wrapper circuits around gnark gadgets with gnark's R1CS builder
// and exports the constraint systems, the commitment analysis and solver witnesses as JSON.
//
//	gnarkx list                       registered wrapper ids
//	gnarkx run ID OUT.json [flags]    compile, analyse and solve one wrapper
//
// The wrappers package is generated per repository by scripts/zk_registry/gnark_det.py.
package main

import (
	"flag"
	"fmt"
	"os"
	"runtime/debug"

	"github.com/consensys/gnark/logger"

	"boole.local/gnarkx/harness"
	_ "boole.local/gnarkx/wrappers"
)

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: gnarkx list | run ID OUT.json [flags]")
		os.Exit(2)
	}
	logger.Disable()
	switch os.Args[1] {
	case "list":
		for _, id := range harness.IDs() {
			fmt.Println(id)
		}
	case "run":
		fs := flag.NewFlagSet("run", flag.ExitOnError)
		limit := fs.Int("limit", 100000, "constraint guard of every compile")
		policy := fs.Int("size-policy", 2000, "model size above which no witnesses are produced")
		samples := fs.Int("samples", 48, "assignments to sample")
		maxPoints := fs.Int("max-points", 64, "largest number of challenge points")
		prod := fs.Bool("production", true, "also compile with gnark's production builder")
		if len(os.Args) < 4 {
			fmt.Fprintln(os.Stderr, "usage: gnarkx run ID OUT.json [flags]")
			os.Exit(2)
		}
		_ = fs.Parse(os.Args[4:])
		res := harness.Run(os.Args[2], harness.Options{Limit: *limit, SizePolicy: *policy, Samples: *samples,
			MaxPoints: *maxPoints, Production: *prod})
		if bi, ok := debug.ReadBuildInfo(); ok {
			for _, d := range bi.Deps {
				if d.Path == "github.com/consensys/gnark" {
					res.GnarkVersion = d.Version
				}
			}
		}
		if err := harness.WriteJSON(os.Args[3], res); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		fmt.Println(res.Status)
	default:
		fmt.Fprintln(os.Stderr, "unknown command")
		os.Exit(2)
	}
}
