"""PyRat - High-performance PyRat game environment

This package provides a fast implementation of the PyRat game,
with both raw game engine access and PettingZoo-compatible interfaces.
"""

from pyrat._rust import PyRatEnv as _RustEnv

# Re-export the Rust environment directly for now
PyRatEnv = _RustEnv

__version__ = "0.1.0"
__all__ = ["PyRatEnv"]
