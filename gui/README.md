# PyRat GUI

Desktop app for running matches and watching bots play.

<p align="center">
  <img src="../docs/images/match.png" alt="A PyRat match in progress" width="700">
</p>

Part of the [PyRat ecosystem](../README.md). If you're looking to write a bot, see the [SDKs](../sdk/).

## Building

Prerequisites:
- [Rust toolchain](https://rustup.rs/)
- [Node.js](https://nodejs.org/) (v18+)
- [pnpm](https://pnpm.io/)

```bash
cd gui
pnpm install
pnpm build
```

This produces a native app bundle in `src-tauri/target/release/bundle/`:

| Platform | Bundle | Run |
|----------|--------|-----|
| macOS | `bundle/macos/PyRat.app` | `open src-tauri/target/release/bundle/macos/PyRat.app` |
| Linux | `bundle/appimage/PyRat.AppImage` | `./src-tauri/target/release/bundle/appimage/PyRat.AppImage` |
| Windows | `bundle/nsis/PyRat_*.exe` | Run the installer |

The game engine and match host compile as part of the Tauri backend, so there's nothing else to install.

## Adding bots

On the Bots page, add a **scan path** pointing at a directory that contains bots. Every bot it finds shows up in the grid, ready to play. Point it at `botpack/` for the full collection, or at your own project directory. Bots just need a [`bot.toml`](../botpack/#bottoml) in their folder.

Some bots have configurable [options](../sdk/#options) (search depth, strategy, etc.). A gear icon on the setup page lets you tweak them before the match starts.

There's also a built-in random bot for quick testing, no setup needed.

## What's next

The GUI today is a match viewer. The goal is a bot development environment.

### Bot thinking visualization

Why did my bot go right instead of left? Why that cheese and not the closer one? No more guessing. The GUI will show the path your bot planned, the cheese it was targeting, and how deep it searched. Scrub back to any turn and see what it was thinking at that moment.

### Analysis mode

Stop the game on any position. Let both bots think about it as long as they want, watch one change its mind as it searches deeper. Step forward when you've seen enough.

### Human player mode

Play against your own bot. Arrow keys to move. How well does your intuition hold up against the algorithm you wrote?

### Debug overlays

Your bot knows things it can't show you yet: cell values, danger zones, planned routes. Debug overlays let bots draw on the maze: heatmaps, arrows, annotations. See what your bot *sees*, not just what it does.

## Development

### Tech stack

- **Tauri v2** (Rust backend) + **React 19** + **TypeScript** + **Vite**
- **Mantine** for UI components, **Canvas 2D** for maze rendering
- **Zustand** + **Immer** for match state (game tree with cursor-based navigation)
- **Jotai** for persistent config (scan paths, match settings, saved to disk via Tauri commands)
- **tauri-specta** for type-safe IPC (TypeScript bindings generated from Rust types)

### Running in dev mode

```bash
cd gui
pnpm dev           # starts Tauri with hot-reload
```

Backend logging is controlled by `RUST_LOG`:

```bash
RUST_LOG=pyrat_gui=debug,pyrat_host=debug,warn pnpm dev
```

### Architecture

```
┌─────────────────── Tauri (Rust backend) ───────────────────┐
│                                                            │
│   Bot process ──TCP──▶ ┌──────────┐                        │
│   Bot process ──TCP──▶ │   Host   │──MatchEvents──▶ Tauri IPC ──▶ Frontend
│                        └──────────┘                        │
│                          ▲                                 │
│                     Game Engine                            │
│                  (maze gen, rules)                         │
│                                                            │
└────────────────────────────────────────────────────────────┘

┌─────────────────── React (frontend) ───────────────────────┐
│                                                            │
│   Tauri events ──▶ Zustand store ──▶ Renderer ──▶ Canvas   │
│                     (game tree)     (draw instructions)    │
│                       ▲                                    │
│                    cursor                                  │
│                 (current turn)                             │
│                                                            │
└────────────────────────────────────────────────────────────┘
```

The Rust backend embeds the engine and host in-process. Bot subprocesses connect over TCP with FlatBuffers. The host runs the game and streams `MatchEvent`s to the frontend through Tauri IPC.

The frontend accumulates events into a game tree, where each node holds the full game state and the joint action that produced it. A cursor tracks the viewing position, and the renderer derives display state from whatever node the cursor points to. Backend and frontend run at independent speeds: the host might finish all 300 turns while the viewer is still on turn 40 at 1x playback.

### Key files

| File | What it does |
|------|-------------|
| `src-tauri/src/main.rs` | App entry, Tauri builder, tracing init, specta export |
| `src-tauri/src/commands.rs` | Tauri commands: `get_game_state`, `start_match`, `stop_match` |
| `src-tauri/src/match_runner.rs` | Match orchestration: bot launch, TCP, host, event forwarding |
| `src-tauri/src/bot_discovery.rs` | Scan-path persistence and `bot.toml` discovery |
| `src-tauri/src/bot_probe.rs` | Probe a bot process to read its option definitions |
| `src-tauri/src/events.rs` | Tauri event types (specta-derived) |
| `src/App.tsx` | View router (match view vs bot management) |
| `src/stores/matchStore.ts` | Zustand store: game tree, cursor, viewer mode, event handlers |
| `src/stores/botConfigAtom.ts` | Jotai atoms for scan paths and discovered bots |
| `src/stores/botProbeAtom.ts` | Probe cache: per-agent_id option definitions |
| `src/components/BotOptionsPopover.tsx` | Per-slot option editor (gear icon on setup page) |
| `src/renderer/instructions.ts` | Game state → draw instructions (the rendering pipeline) |
| `src/components/MazeCanvas.tsx` | Canvas 2D drawing, DPR-aware |

Run `make check-gui` from the repository root for linting and type-checking, or `make fmt-gui` to format.
