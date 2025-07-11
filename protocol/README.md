# PyRat Protocol

This directory contains the PyRat Communication Protocol specification and related implementations.

## Contents

- **`spec.md`** - The official PyRat Communication Protocol specification (v1.0)
  - Defines how PyRat engines and AI players communicate
  - Text-based protocol inspired by UCI (Universal Chess Interface)
  - Language-agnostic design allowing AI development in any programming language

## Protocol Overview

The PyRat Communication Protocol enables:
- Language-independent AI development
- Process isolation for stability and parallelism
- Robust error handling and recovery
- Tournament automation
- Real-time progress monitoring

## Key Features

- **Handshake & Initialization**: Standard connection sequence
- **Game Phases**: Preprocessing, gameplay, and postprocessing
- **Time Management**: Configurable time limits for each phase
- **Info Messages**: Optional progress reporting during calculation
- **Interrupt Support**: Stop command for immediate response
- **Options System**: Configurable AI parameters

## Next Steps

- `pyrat_base/` - Python base library for protocol-compliant AIs (coming soon)
- `examples/` - Example AI implementations using the protocol (coming soon)
- `tests/` - Protocol compliance tests (coming soon)

## For AI Developers

To implement a PyRat AI, read `spec.md` and follow the protocol requirements. The protocol is designed to be simple to implement in any language with standard I/O support.
