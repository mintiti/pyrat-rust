## Summary

Fixed a critical bug where commands arriving during move calculation were being silently dropped, causing the AI's internal game state to become desynchronized with the server's game state.

## The Problem

**Location:** `pyrat_base/base_ai.py`, method `_handle_go_command()` (lines 614-627)

When the AI is calculating a move, it runs a command processing loop that only handles `STOP` and `ISREADY` commands. Any other command that arrives during calculation is read from the command queue but **never processed** - it's permanently dropped.

### How the Desync Occurred

1. AI receives `GO` command and starts calculating in a background thread
2. While calculating, other commands arrive from the server (especially likely when AI is slow or times out)
3. The command processing loop reads these commands from the queue
4. Commands are removed from the queue when read
5. Non-STOP/ISREADY commands are silently ignored (dropped)
6. **Critical:** When a `MOVES` command is dropped, the AI's game state is never updated with the actual executed moves
7. On subsequent turns, the AI's game state is out of sync with the server

### Example Scenario

```
Turn N:
1. Server → AI: "moves rat:UP python:DOWN"
2. Server → AI: "go"
3. AI starts calculating (slow Dijkstra search in greedy AI)
4. [AI still calculating...]
5. [Server timeout!]
6. Server → AI: "timeout move:STAY"          ← Dropped! 💥
7. Server → AI: "moves rat:UP python:STAY"   ← Dropped! 💥
8. AI finishes calculation and sends move
9. **BUG: AI's game state was never updated with the actual moves!**

Turn N+1:
10. AI calculates based on outdated/incorrect game state
11. AI's internal state is now permanently out of sync with server
```

### When This Bug Occurs

This bug is most likely to manifest when:
- The AI takes longer to calculate (like the **greedy AI** with Dijkstra pathfinding)
- The AI times out (server sends `TIMEOUT` and `MOVES` while AI is still computing)
- Network latency causes commands to arrive in unexpected order
- The server sends the next turn's commands before the AI finishes the current turn

This particularly affected the **greedy AI implementation** due to its computationally intensive pathfinding.

## The Fix

### Code Changes

**1. Added command re-queueing in `base_ai.py`:**

```python
# In _handle_go_command() command processing loop
if cmd:
    if cmd.type == CommandType.STOP:
        # ... handle STOP (interrupt calculation)
    elif cmd.type == CommandType.ISREADY:
        # ... handle ISREADY (synchronization check)
    else:
        # NEW: Re-queue the command for processing after calculation
        self._io.requeue_command(cmd)
```

**2. Added `requeue_command()` method to `io_handler.py`:**

```python
def requeue_command(self, command: Command) -> None:
    """Put a command back into the queue for later processing.

    This ensures commands arriving during move calculation aren't lost.
    """
    self._command_queue.put(command)
```

### What This Fixes

✅ All protocol commands are processed in correct order
✅ Game state stays synchronized between AI and server
✅ MOVES commands are never lost, even during long calculations
✅ AI behaves correctly even when it times out
✅ Greedy AI and other computationally intensive AIs work reliably

## Testing

Added comprehensive test coverage across two test files:

### `test_io_handler.py` (IOHandler tests)
- ✅ `test_requeue_command()` - Basic re-queueing functionality
- ✅ `test_requeue_command_preserves_order()` - Multiple commands maintain order
- ✅ `test_requeue_during_calculation()` - Re-queueing while calculation runs

### `test_base_ai_protocol_bugs.py` (Protocol bug tests)
- ✅ `test_commands_not_dropped_during_calculation()` - MOVES commands preserved
- ✅ `test_game_state_sync_preserved_with_requeue()` - Integration test demonstrating the desync bug and fix
- ✅ `test_isready_handled_immediately_not_requeued()` - ISREADY handled correctly

## Files Changed

- `protocol/pyrat_base/pyrat_base/base_ai.py` - Re-queue non-urgent commands
- `protocol/pyrat_base/pyrat_base/io_handler.py` - Add `requeue_command()` method
- `protocol/pyrat_base/tests/unit/test_io_handler.py` - Tests for IOHandler
- `protocol/pyrat_base/tests/unit/test_base_ai_protocol_bugs.py` - Tests for base AI
- `protocol/pyrat_base/BUGFIX_GAME_STATE_DESYNC.md` - Detailed bug documentation

## Impact

This fix resolves the game state synchronization issues reported with the greedy AI and ensures all AIs maintain correct state even under timeout conditions or slow calculations.

## Related Issues

Fixes game state desynchronization in protocol module, particularly affecting greedy AI implementation.
