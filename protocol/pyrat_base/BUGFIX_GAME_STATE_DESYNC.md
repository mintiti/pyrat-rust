# Bug Fix: Game State Desynchronization

## Summary

Fixed a critical bug where commands received during move calculation were being dropped, causing the AI's internal game state to become desynchronized with the server's game state.

## The Bug

**Location:** `pyrat_base/base_ai.py`, method `_handle_go_command()` lines 614-627

**Issue:** When the AI is calculating a move, it runs a command processing loop that only handles `STOP` and `ISREADY` commands. Any other command that arrives during calculation (such as `MOVES`, `TIMEOUT`, `GAMEOVER`) is read from the command queue but never processed - it's permanently dropped.

### How the Desync Occurs

1. AI receives `GO` command and starts calculating in a background thread
2. While calculating, other commands may arrive (especially if AI is slow)
3. The command processing loop reads these commands: `cmd = self._io.read_command(timeout=0.01)`
4. Commands are removed from the queue when read
5. Non-STOP/ISREADY commands are silently ignored (dropped)
6. **Critical:** When a `MOVES` command is dropped, the AI's game state is never updated!
7. On subsequent turns, the AI's game state is out of sync with the server

### When This Happens

This bug is most likely to occur when:
- The AI takes longer to calculate (like the greedy AI with Dijkstra pathfinding)
- The AI times out (server sends `TIMEOUT` and `MOVES` while AI is still computing)
- Network latency causes commands to arrive out of expected order
- The server sends the next turn's commands before the AI finishes the current turn

### Example Scenario

```
Turn N:
1. Server sends: moves rat:UP python:DOWN
2. Server sends: go
3. AI starts calculating (slow Dijkstra search)
4. [AI is still calculating...]
5. Server timeout! Sends: timeout move:STAY
6. Server sends: moves rat:UP python:STAY  (with AI's default STAY)
7. AI's loop reads TIMEOUT command → dropped (not STOP or ISREADY)
8. AI's loop reads MOVES command → dropped (not STOP or ISREADY)
9. AI finishes calculation and sends move
10. **BUG: AI's game state was never updated with the actual moves!**

Turn N+1:
11. AI calculates based on outdated game state
12. AI's internal state is now out of sync with server
```

## The Fix

**Modified files:**
- `pyrat_base/base_ai.py` - Added re-queueing of non-urgent commands
- `pyrat_base/io_handler.py` - Added `requeue_command()` method

**Solution:** Instead of dropping commands, we now re-queue them so they can be processed after the move calculation completes.

### Code Changes

In `_handle_go_command()`:
```python
# Old code (buggy):
if cmd:
    if cmd.type == CommandType.STOP:
        # ... handle STOP
    elif cmd.type == CommandType.ISREADY:
        # ... handle ISREADY
    # BUG: Other commands silently dropped here!

# New code (fixed):
if cmd:
    if cmd.type == CommandType.STOP:
        # ... handle STOP
    elif cmd.type == CommandType.ISREADY:
        # ... handle ISREADY
    else:
        # Re-queue the command for processing after calculation
        self._io.requeue_command(cmd)
```

In `io_handler.py`:
```python
def requeue_command(self, command: Command) -> None:
    """Put a command back into the queue for later processing."""
    self._command_queue.put(command)
```

## Impact

This fix ensures:
- All protocol commands are processed in order
- Game state stays synchronized between AI and server
- MOVES commands are never lost, even during long calculations
- The AI behaves correctly even when it times out
- Greedy AI and other computationally intensive AIs work reliably

## Testing

Added comprehensive test suite in `tests/unit/test_command_requeue_fix.py`:
- Tests that MOVES commands are preserved during calculation
- Tests that multiple commands are re-queued in correct order
- Integration test demonstrating the desync bug scenario
- Verification that ISREADY is still handled immediately (not re-queued)

## Related Issues

This bug particularly affected the greedy AI because:
1. It performs Dijkstra pathfinding, which takes more time
2. Longer calculation time increases likelihood of commands arriving during calculation
3. The greedy AI maintains path state, making desyncs more apparent
4. Users reported "game states going out of sync" specifically with greedy implementation
