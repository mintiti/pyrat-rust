# PyRat Communication Protocol Specification

**Version**: 1.0
**Status**: Draft
**Last Updated**: January 2025

## Table of Contents

1. [Overview](#overview)
2. [Design Principles](#design-principles)
3. [Protocol Messages](#protocol-messages)
   - [Connection Handshake](#connection-handshake)
   - [Synchronization](#synchronization)
   - [Configuration](#configuration)
   - [Game Initialization](#game-initialization)
   - [Preprocessing Phase](#preprocessing-phase)
   - [Turn Communication](#turn-communication)
   - [Ready Check](#ready-check-after-timeout)
   - [Game End](#game-end)
   - [Postprocessing Phase](#postprocessing-phase)
   - [Error Recovery](#error-recovery)
4. [Message Format](#message-format)
5. [Protocol Examples](#protocol-examples)
6. [Implementation Requirements](#implementation-requirements)
7. [Future Extensions](#future-extensions)

## Overview

The PyRat Communication Protocol defines how PyRat engines and AI players communicate. Based on the Universal Chess Interface (UCI) design, this text-based protocol enables AI development in any programming language that supports standard input/output.

## Design Principles

1. **Language Independence**: Any language with stdin/stdout support can implement an AI
2. **Process Isolation**: Each AI runs as a separate process for stability and parallelism
3. **AI State Management**: AIs maintain their own game state from move updates
4. **Fault Tolerance**: Timeouts and crashes are handled gracefully
5. **Forward Compatibility**: Protocol extensions won't break existing implementations
6. **Continuous Input Processing**: AIs must always read stdin, even while computing moves

## Protocol Messages

### Connection Handshake

```
Engine → AI: pyrat
AI → Engine: id name [AI name]
AI → Engine: id author [Author name] (optional)
AI → Engine: option name [name] type [type] default [value] [additional parameters] (optional, repeatable)
AI → Engine: pyratready
```

**Option Types:**
- `check` - Boolean (true/false)
- `spin` - Integer with min/max range
- `combo` - Choice from predefined values
- `string` - Text value
- `button` - Action trigger

**Example Options:**
```
option name SearchDepth type spin default 3 min 1 max 10
option name Strategy type combo default Balanced var Aggressive var Balanced var Defensive
option name Debug type check default false
```

### Synchronization

At any time, the engine can check if the AI is responsive:

```
Engine → AI: isready
AI → Engine: readyok
```

This command must always be answered with "readyok", even if the AI is currently calculating a move. AIs must implement non-blocking stdin reading to handle this.

### Configuration

**Setting Options:**
```
Engine → AI: setoption name [name] value [value]
```

**Debug Mode:**
```
Engine → AI: debug [on|off]
```

When debug is on, AIs should send additional information via `info string` messages.

### Game Initialization

```
Engine → AI: newgame
Engine → AI: maze height:[H] width:[W]
Engine → AI: walls [list of wall positions as (x1,y1)-(x2,y2)]
Engine → AI: mud [list of mud positions as (x1,y1)-(x2,y2):N]
Engine → AI: cheese [list of cheese positions as (x,y)]
Engine → AI: player1 rat (x,y)
Engine → AI: player2 python (x,y)
Engine → AI: youare [rat|python]
Engine → AI: timecontrol move:[milliseconds] preprocessing:[milliseconds] postprocessing:[milliseconds] (optional)
```

### Preprocessing Phase

After game initialization, AIs have preprocessing time to analyze the maze:

```
Engine → AI: startpreprocessing
[AI computes strategy within preprocessing time limit]
AI → Engine: preprocessingdone
```

If AI doesn't respond within time limit:
```
Engine → AI: startpreprocessing
[No response within preprocessing time limit]
Engine → AI: timeout preprocessing
```

### Turn Communication

**Normal Turn:**
```
Engine → AI: moves rat:[MOVE] python:[MOVE]
Engine → AI: go
AI → Engine: move [UP|DOWN|LEFT|RIGHT|STAY]
```

**Info Messages (Optional):**
During calculation, AIs may send progress information:
```
AI → Engine: info [key value pairs]
```

Supported info types:
- `nodes [N]` - Number of nodes/states evaluated
- `depth [D]` - Current search depth
- `time [T]` - Time spent in milliseconds
- `currmove [MOVE]` - Move currently being evaluated
- `currline [MOVE1 MOVE2 ...]` - Current line being analyzed
- `score [S]` - Position evaluation (higher is better for current player)
- `pv [MOVE1 MOVE2 ...]` - Principal variation (best line found)
- `target [x,y]` - Current target cheese being considered
- `string [TEXT]` - Any debug/status message

Examples:
```
info nodes 12345 depth 3 currmove UP
info score 25 pv UP RIGHT RIGHT target (5,3)
info depth 4 time 150 nodes 50000
info string Switching to defensive strategy
```

**Stop Command:**
The engine can interrupt AI calculation:
```
Engine → AI: stop
AI → Engine: move [best move found so far]
```

**Timeout Handling:**
```
Engine → AI: go
[No response within time limit]
Engine → AI: timeout move:STAY
Engine → AI: moves rat:[MOVE] python:STAY
```

### Ready Check (After Timeout)

```
Engine → AI: ready?
AI → Engine: ready
```

If no response to ready check, the engine may kill and restart the AI process.

### Game End

```
Engine → AI: gameover winner:[rat|python|draw] score:[X]-[Y]
```

### Postprocessing Phase

After game ends, AIs have postprocessing time for learning/analysis:

```
Engine → AI: startpostprocessing
[AI analyzes game within postprocessing time limit]
AI → Engine: postprocessingdone
```

If AI doesn't respond within time limit:
```
Engine → AI: startpostprocessing
[No response within postprocessing time limit]
Engine → AI: timeout postprocessing
```

### Error Recovery

After process restart:
```
Engine → AI: pyrat
AI → Engine: id name [AI name]
AI → Engine: pyratready
Engine → AI: recover
Engine → AI: maze height:[H] width:[W]
Engine → AI: walls [...]
Engine → AI: mud [...]
Engine → AI: cheese [remaining cheese only]
Engine → AI: moves_history [list of all moves]
Engine → AI: current_position rat:(x,y) python:(x,y)
Engine → AI: score rat:[X] python:[Y]
Engine → AI: go
```

## Protocol Examples

### Simple Game Flow

```
> pyrat
< id name GreedyBot v1.0
< id author Student Name
< pyratready

> newgame
> maze height:10 width:10
> walls (0,0)-(0,1) (1,1)-(2,1) (3,3)-(3,4)
> mud (5,5)-(5,6):3
> cheese (2,2) (7,8) (4,5)
> player1 rat (9,9)
> player2 python (0,0)
> youare python
> timecontrol move:100 preprocessing:3000 postprocessing:1000

> startpreprocessing
< preprocessingdone

> moves rat:STAY python:STAY
> go
< move UP

> moves rat:LEFT python:UP
> go
< move RIGHT

> moves rat:DOWN python:RIGHT
> gameover winner:rat score:2-1

> startpostprocessing
< postprocessingdone
```

### Timeout and Recovery

```
> moves rat:UP python:LEFT
> go
[... no response for 100ms ...]
> timeout move:STAY
> moves rat:UP python:STAY
> ready?
< ready
> go
< move DOWN
```

## Message Format

### General Rules

1. All communication uses UTF-8 encoded text
2. Commands are line-based (terminated by newline `\n`)
3. Whitespace separates command components
4. Arbitrary whitespace between tokens is allowed (spaces, tabs)
5. Unknown commands must be ignored (not cause errors)
6. Commands received at inappropriate times should be ignored
7. Coordinates use format `(x,y)` with no spaces
8. Wall/mud positions use format `(x1,y1)-(x2,y2)`
9. Lists use space separation

### Timing Requirements

- **Move commands**: Must respond to `go` within specified move time limit
- **Preprocessing**: Must complete within preprocessing time limit if specified
- **Postprocessing**: Must complete within postprocessing time limit if specified
- **Handshake/initialization**: No time limit
- **Ready checks**: Should respond promptly (< 1 second)

## Implementation Requirements

### AI Requirements

1. **Continuous Input Processing**: Must always read stdin, even while calculating moves
2. **Non-blocking I/O**: Implement asynchronous stdin reading to handle commands during computation
3. **Unknown Command Handling**: Ignore unknown commands gracefully (do not crash)
4. **Protocol Compliance**: Respond to all known commands as specified
5. **State Management**: Maintain complete game state from initialization and move updates
6. **Time Limits**: Respond to `go` within specified timeout
7. **Move Validation**: Send only valid moves (UP, DOWN, LEFT, RIGHT, STAY)
8. **Recovery Support**: Handle `recover` command after restart
9. **Interrupt Handling**: Respond to `stop` command immediately with best move found
10. **Synchronization**: Always respond to `isready` with `readyok`, even during calculation

### Engine Requirements

1. **Command Format**: Send commands exactly as specified
2. **Move Reporting**: Always report as "rat:[move] python:[move]"
3. **State Consistency**: Report actual executed moves after validation
4. **Timeout Handling**: Default timeouts to STAY without disqualification
5. **Recovery Protocol**: Support full state recovery via `recover` command

## Future Extensions

### Planned Enhancements

1. **Performance Mode**: Binary protocol (protobuf/msgpack) for high-frequency training
2. **Analysis Output**: `info` commands for GUI visualization of AI thinking
3. **Configuration**: AI-specific options and parameters
4. **Tournament Modes**: Special commands for tournament play

### Protocol Versioning

Future versions will maintain backward compatibility. AIs can declare supported protocol versions:

```
Engine → AI: pyrat
AI → Engine: id name AdvancedBot
AI → Engine: protocol version 1.0 1.1 2.0
AI → Engine: pyratready
```

## Notes

### Design Decisions

- **Text-based**: Human-readable for debugging, universally supported
- **Line-oriented**: Simple parsing, clear message boundaries
- **Stateful AIs**: Reduces communication overhead after initialization
- **Process isolation**: Enables parallelism and fault tolerance

### Common Implementation Patterns

1. **Line-based I/O**: Read stdin line by line, write complete lines to stdout
2. **State object**: Maintain game state that updates with each move
3. **Command parser**: Switch/match on command keywords
4. **Timeout handling**: Use timeouts when reading from stdin during gameplay
