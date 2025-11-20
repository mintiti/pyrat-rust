# CLI Test Strategy

## Overview
This document outlines what we test and why, organized by component.

## Test Philosophy
- **Unit tests**: Test individual components in isolation using mocks
- **Integration tests**: Test components working together with real dependencies
- **Avoid testing test utilities**: Mocks are tools, not production code

---

## 1. Move Providers

### What We Need to Verify

**SubprocessMoveProvider (Production Code)**
- ✅ Correctly delegates all method calls to AIProcess
- ✅ Passes constructor parameters correctly
- ✅ Returns values from AIProcess unmodified

**Why**: SubprocessMoveProvider is a thin wrapper - we verify it doesn't break the delegation.

**Test Coverage**: `TestSubprocessMoveProvider::test_delegates_all_methods_to_ai_process`

---

## 2. run_game() Function

### What We Need to Verify

**Core Game Loop Logic**
- ✅ Executes game to completion
- ✅ Calls provider.get_move() each turn
- ✅ Returns correct result tuple (success, winner, scores)

**Error Handling**
- ✅ Handles provider crashes (None + not alive) → fails gracefully
- ✅ Handles provider timeouts (None + still alive) → treats as STAY, continues
- ✅ Returns valid scores even when game fails

**Headless Mode**
- ✅ Works without display object (display=None)
- ✅ No crashes when rendering is skipped

**Why**: run_game() is the core game loop - it must handle all provider scenarios correctly.

**Test Coverage**: `TestRunGameFunction` (6 tests)

---

## 3. GameRunner Class

### What We Need to Verify

**Headless Mode**
- ✅ Accepts headless parameter
- ✅ Sets display=None when headless=True
- ✅ Runs without visualization

**Abstraction Benefit**
- ✅ Allows provider injection for testing (can replace with mocks)

**Why**: GameRunner orchestrates the game - we verify it uses the abstraction correctly.

**Test Coverage**: `TestGameRunnerIntegration` (2 tests)

---

## 4. Display (Existing Tests)

### What We Already Test

**Cell Rendering** (`TestCellContent`)
- Cell combinations (rat, python, cheese, empty)
- ANSI color codes
- Overlapping entities

**Separators** (`TestSeparators`)
- Vertical separators (walls, mud, empty)
- Horizontal separators (walls, mud, empty)
- Unicode characters

**Maze Structure** (`TestMazeStructureBuilding`)
- Wall parsing
- Mud parsing
- Order independence
- Empty maze handling

**Why**: Display is complex with many rendering cases - comprehensive tests ensure correctness.

**Test Coverage**: `test_display.py` (49 tests)

---

## 5. Direction Parsing (Existing Tests)

### What We Already Test

**Direction Name Mapping** (`TestDirectionNameMapping`)
- Enum to string conversion
- Invalid direction handling
- None handling for display

**Direction Parsing** (`TestDirectionParsing`)
- String to enum conversion
- Case sensitivity
- Invalid name defaults
- Roundtrip conversion

**Why**: Protocol communication relies on correct direction parsing.

**Test Coverage**: `test_direction_parsing.py` (23 tests)

---

## Test Organization

### Unit Tests (Fast, Isolated)
```
test_move_providers.py
├── MockMoveProvider (test utility, not tested)
├── TestSubprocessMoveProvider (1 test)
│   └── Uses mocks to verify delegation
└── TestRunGameFunction (6 tests)
    └── Uses MockMoveProvider to test game loop

test_direction_parsing.py (23 tests)
test_display.py (49 tests)
```

### Integration Tests (Slower, Real Components)
```
test_move_providers.py
└── TestGameRunnerIntegration (2 tests)
    └── Uses real GameRunner with temporary AI scripts
```

---

## What We DON'T Test (And Why)

❌ **MockMoveProvider interface**: It's a test utility, not production code
❌ **AIProcess**: Tested separately in its own module
❌ **PyRat game engine**: Tested in engine/python/tests
❌ **Protocol details**: Tested in protocol/pyrat_base/tests

---

## Coverage Summary

| Component              | Unit Tests | Integration Tests | Total |
|------------------------|------------|-------------------|-------|
| SubprocessMoveProvider | 1          | 0                 | 1     |
| run_game()             | 6          | 0                 | 6     |
| GameRunner             | 0          | 2                 | 2     |
| Display                | 49         | 0                 | 49    |
| Direction Parsing      | 23         | 0                 | 23    |
| **Total**              | **79**     | **2**             | **81**|

---

## Future Test Additions

### When Adding DirectMoveProvider
```python
class TestDirectMoveProvider:
    def test_calls_function_directly(self):
        """Verify DirectMoveProvider calls Python function without subprocess."""
        pass
```

### When Adding NetworkMoveProvider
```python
class TestNetworkMoveProvider:
    def test_sends_move_request_over_network(self):
        """Verify NetworkMoveProvider sends requests to remote AI."""
        pass
```

### End-to-End Integration Test
```python
def test_full_game_with_real_ais():
    """Run a complete game between two real AI scripts."""
    # Currently done manually, could automate
    pass
```
