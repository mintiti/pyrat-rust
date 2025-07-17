# PyRat Replay Format Specification

**Version**: 1.0
**Status**: Draft
**Last Updated**: July 2025

## Overview

The PyRat Replay Format (PRF) is a text-based format for recording PyRat games, inspired by chess's Portable Game Notation (PGN). The format is designed for easy reading and writing by humans and simple parsing by programs.

## Design Principles

1. **Human-readable**: Plain text format that humans can read and edit
2. **Machine-parseable**: Simple structure for programmatic processing
3. **Self-contained**: Contains all information needed to replay the game
4. **Compact**: Efficient representation without sacrificing readability
5. **Extensible**: Support for comments and variations

## File Format

Replay files use `.pyrat` extension with UTF-8 encoding.

### Overall Structure

```
[Event "PyRat Tournament 2025"]
[Site "University Lab"]
[Date "2025.01.15"]
[Round "1"]
[Rat "GreedyBot v1.0"]
[Python "RandomBot v2.1"]
[Result "1-0"]
[MazeHeight "10"]
[MazeWidth "10"]
[TimeControl "100+3000+1000"]

{Initial maze configuration}
W:(0,0)-(0,1) (1,1)-(2,1) (3,3)-(3,4)
M:(5,5)-(5,6):3 (2,2)-(3,2):2
C:(2,2) (7,8) (4,5)
R:(9,9)
P:(0,0)

{Game moves}
1. S/U (5ms/12ms)
2. L/R (89ms/45ms) {Python collects cheese at (0,1)}
3. L/U (15ms/8ms)
4. D/R (22ms/95ms) {Rat collects cheese at (7,8)}
...
42. U/S (10ms/15ms) {Rat wins by score 3-2}
```

### Tag Pairs Section

Required tags (Seven Tag Roster):
- `[Event "name"]` - Tournament or match name
- `[Site "location"]` - Where the game was played
- `[Date "YYYY.MM.DD"]` - Date of the game
- `[Round "number"]` - Round number (use "-" if not applicable)
- `[Rat "name"]` - Name of the Rat AI
- `[Python "name"]` - Name of the Python AI
- `[Result "score"]` - Game result: "1-0" (Rat wins), "0-1" (Python wins), "1/2-1/2" (draw)

Required PyRat-specific tags:
- `[MazeHeight "number"]` - Maze height
- `[MazeWidth "number"]` - Maze width
- `[TimeControl "move+preprocessing+postprocessing"]` - Time limits in milliseconds

Optional tags:
- `[RatAuthor "name"]` - Author of Rat AI
- `[PythonAuthor "name"]` - Author of Python AI
- `[ReplayID "UUID"]` - Unique identifier
- `[Termination "reason"]` - How game ended (score_threshold, all_cheese, max_turns, timeout)
- `[FinalScore "rat-python"]` - Final cheese count (e.g., "3-2")
- `[TotalTurns "number"]` - Number of turns played

### Initial State Section

After tag pairs, the initial maze configuration:

```
W:<wall_list>     - Walls as (x1,y1)-(x2,y2) pairs
M:<mud_list>      - Mud as (x1,y1)-(x2,y2):value
C:<cheese_list>   - Cheese positions as (x,y)
R:<rat_position>  - Rat starting position
P:<python_pos>    - Python starting position
```

### Movetext Section

Moves are recorded as:

```
<turn>. <rat_move>/<python_move> (<rat_time>ms/<python_time>ms) {optional comment}
```

- **Turn number**: Sequential from 1
- **Moves**: S=STAY, U=UP, D=DOWN, L=LEFT, R=RIGHT
- **Times**: Time taken in milliseconds (optional)
- **Comments**: In curly braces for game events

Special notations:
- `*` indicates timeout (e.g., `L/*` means Python timed out)
- `!` indicates preprocessing done
- `?` indicates postprocessing done

### Comments and Annotations

- `{comment}` - Inline comments about specific moves
- `;comment` - Rest-of-line comments
- `#comment` - Analysis or meta-comments

### Example with All Features

```
[Event "CS101 PyRat Championship"]
[Site "Online"]
[Date "2025.01.15"]
[Round "3"]
[Rat "AlphaPyRat"]
[Python "GreedyMCTS"]
[Result "0-1"]
[MazeHeight "15"]
[MazeWidth "21"]
[TimeControl "100+5000+2000"]
[RatAuthor "Alice Smith"]
[PythonAuthor "Bob Jones"]
[Termination "score_threshold"]
[FinalScore "3-4"]
[TotalTurns "67"]

; Initial maze setup for game 3
W:(0,0)-(0,1) (0,1)-(0,2) (1,0)-(2,0)
M:(10,7)-(10,8):3 (5,5)-(6,5):2
C:(2,2) (18,12) (10,7) (5,5) (15,9) (3,12) (17,2)
R:(20,14)
P:(0,0)

! {Both players completed preprocessing}

1. S/R (5ms/8ms) {Opening moves}
2. L/U (12ms/15ms)
3. L/R (8ms/22ms)
; Python heading for nearest cheese
4. D/R (15ms/18ms)
5. D/R (14ms/16ms) {Python enters mud at (5,5)}
6. D/S (12ms/0ms) {Python stuck in mud}
7. D/S (11ms/0ms) {Python still stuck}
8. L/R (13ms/25ms) {Python collects cheese at (5,5)}

# Turning point of the game
15. U/* (8ms/100ms) {Python timeout, defaults to STAY}
16. U/D (7ms/82ms) {Both players collect cheese at (10,7)! 0.5 points each}

...

67. R/U (9ms/11ms) {Python wins by reaching 4 cheese first}

? {Both players completed postprocessing}
```

## Parsing Rules

1. **Line-oriented**: Each tag pair or move on its own line
2. **Whitespace**: Spaces in movetext are not significant
3. **Case**: Move letters are uppercase, tags are case-sensitive
4. **Order**: Tag pairs first, then initial state, then moves
5. **Unknown tags**: Should be preserved but can be ignored

## Minimal Valid Replay

```
[Event "?"]
[Site "?"]
[Date "????.??.??"]
[Round "-"]
[Rat "?"]
[Python "?"]
[Result "*"]
[MazeHeight "10"]
[MazeWidth "10"]
[TimeControl "100+0+0"]

W:
M:
C:(5,5)
R:(9,9)
P:(0,0)

1. S/S
```

## Implementation Notes

1. **Streaming**: Games can be written move-by-move during play
2. **Recovery**: Can resume from any turn by replaying moves
3. **Validation**: Parsers should validate move legality
4. **Extensibility**: Unknown tags and comments should be preserved

## Advantages of PRF Format

- Can be read without any tools
- Easy to edit manually for testing
- Git-friendly (text diffs work well)
- Compact without compression
- Standard text processing tools work (grep, sed, etc.)
- Can embed analysis and commentary
