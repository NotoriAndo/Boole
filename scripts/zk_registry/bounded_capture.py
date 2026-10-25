"""Compatibility import for the canonical compile-support capture module."""
import sys
from .compile_support import bounded_capture as _implementation
sys.modules[__name__] = _implementation
