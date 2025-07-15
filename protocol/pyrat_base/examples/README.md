# PyRat AI Examples

This directory contains example AI implementations using the `pyrat_base` library.

## Available Examples

### dummy_ai.py
The simplest possible AI - always returns STAY.
- Demonstrates basic PyRatAI structure
- Minimal implementation

### random_ai.py
Makes random effective moves (moves that change position).
- Shows how to get effective moves from game state
- Demonstrates debug logging
- Uses random selection

### greedy_ai.py
Sophisticated greedy AI that finds the cheese reachable in minimum time.
- Uses Dijkstra's algorithm for optimal pathfinding
- Properly accounts for both walls AND mud delays
- Demonstrates preprocessing phase
- Shows info messages during calculation
- Makes optimal decisions based on actual time cost

## Running the Examples

Each example can be run as a standalone script that will communicate via the PyRat protocol:

```bash
python dummy_ai.py
python random_ai.py
python greedy_ai.py
```

These scripts expect to receive protocol commands on stdin and will send responses to stdout.

## Creating Your Own AI

To create your own AI:

1. Import the necessary modules:
   ```python
   from pyrat_base import PyRatAI, ProtocolState
   from pyrat_engine.game import Direction
   ```

2. Create a class that inherits from PyRatAI:
   ```python
   class MyAI(PyRatAI):
       def get_move(self, state: ProtocolState) -> Direction:
           # Your strategy here
           return Direction.UP
   ```

3. Run your AI:
   ```python
   if __name__ == "__main__":
       ai = MyAI("MyBot v1.0", "Your Name")
       ai.run()
   ```
