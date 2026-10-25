"""Compatibility entry point; the sandbox grants only compile_support/."""
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parent/"compile_support"))
from compile_driver import main
if __name__=="__main__":sys.exit(main(sys.argv[1:]))
