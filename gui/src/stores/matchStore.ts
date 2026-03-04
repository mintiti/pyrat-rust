import { produce } from "immer";
import { useMemo } from "react";
import { create } from "zustand";
import { commands } from "../bindings";
import type {
	BotDisconnectedEvent,
	BotInfoEvent,
	Coord,
	Direction,
	MatchConfigParams,
	MatchOverEvent,
	MazeState,
	MudEntry,
	PlayerState,
	TurnPlayedEvent,
	WallEntry,
} from "../bindings/generated";

// ── Types ────────────────────────────────────────────────────────

/** Static maze geometry — set once on match start, never changes. */
export interface MazeConfig {
	width: number;
	height: number;
	walls: WallEntry[];
	mud: MudEntry[];
	max_turns: number;
	total_cheese: number;
}

/** One position in the game tree. */
export interface GameNode {
	turn: number;
	player1: PlayerState;
	player2: PlayerState;
	cheese: Coord[];
	actions: { player1: Direction; player2: Direction } | null;
	botInfo: { Player1: BotInfoEvent[]; Player2: BotInfoEvent[] };
	children: GameNode[];
}

export type ViewerMode = "previewing" | "live" | "reviewing";

// ── Tree helpers ─────────────────────────────────────────────────

/** Walk the tree following `path` indices. Returns null if the path is invalid. */
export function getNodeAtPath(root: GameNode, path: number[]): GameNode | null {
	let node = root;
	for (const idx of path) {
		if (idx < 0 || idx >= node.children.length) return null;
		node = node.children[idx];
	}
	return node;
}

/** Walk to the deepest first child (mainline end). */
export function getMainlineEnd(root: GameNode): GameNode {
	let node = root;
	while (node.children.length > 0) {
		node = node.children[0];
	}
	return node;
}

/** Build a cursor path of `n` zeros (follow mainline for n steps). */
function mainlinePath(n: number): number[] {
	return new Array(n).fill(0);
}

// ── Store ────────────────────────────────────────────────────────

interface MatchState {
	// Match metadata
	matchId: number | null;
	mazeConfig: MazeConfig | null;
	player1BotId: string | null;
	player2BotId: string | null;
	result: MatchOverEvent | null;
	pendingResult: MatchOverEvent | null;
	error: string | null;
	disconnection: BotDisconnectedEvent | null;

	// Game tree
	root: GameNode | null;
	cursor: number[]; // path into tree, [] = root
	mainlineDepth: number; // number of turns appended, drives useMainlineLength

	// Preview (idle state — no match running)
	previewMaze: MazeState | null;
	previewSeed: number | null;
	previewError: string | null;

	// Viewer
	viewerMode: ViewerMode;
	playbackSpeed: number; // ms between frames

	// Setters for bot selectors
	setPlayer1BotId: (cmd: string | null) => void;
	setPlayer2BotId: (cmd: string | null) => void;

	// Event handlers
	onMatchStarted: (maze: MazeState, matchId: number) => void;
	onTurnPlayed: (e: TurnPlayedEvent) => void;
	onMatchOver: (e: MatchOverEvent) => void;
	onBotInfo: (e: BotInfoEvent) => void;
	onError: (message: string) => void;
	onDisconnect: (e: BotDisconnectedEvent) => void;

	// Actions
	resetToPreview: () => void;

	// Navigation
	goToStart: () => void;
	goToEnd: () => void;
	stepForward: () => void;
	stepBack: () => void;
	goToTurn: (n: number) => void;
	togglePlay: () => void;
	setPlaybackSpeed: (ms: number) => void;

	// Auto-advance (called by the interval timer)
	advanceCursor: () => void;
}

/** Fields that get wiped when returning to idle/preview state. */
const IDLE_MATCH = {
	matchId: null as number | null,
	mazeConfig: null as MazeConfig | null,
	root: null as GameNode | null,
	cursor: [] as number[],
	mainlineDepth: 0,
	result: null as MatchOverEvent | null,
	pendingResult: null as MatchOverEvent | null,
	error: null as string | null,
	disconnection: null as BotDisconnectedEvent | null,
	viewerMode: "previewing" as ViewerMode,
};

export const useMatchStore = create<MatchState>((set, get) => ({
	// ── Initial state ────────────────────────────────────────
	...IDLE_MATCH,
	player1BotId: "__random__",
	player2BotId: "__random__",
	previewMaze: null,
	previewSeed: null,
	previewError: null,
	playbackSpeed: 200,

	// ── Setters ──────────────────────────────────────────────
	setPlayer1BotId: (cmd) => set({ player1BotId: cmd }),
	setPlayer2BotId: (cmd) => set({ player2BotId: cmd }),

	// ── Event handlers ───────────────────────────────────────
	onMatchStarted: (maze, matchId) => {
		const root: GameNode = {
			turn: maze.turn,
			player1: maze.player1,
			player2: maze.player2,
			cheese: maze.cheese,
			actions: null,
			botInfo: { Player1: [], Player2: [] },
			children: [],
		};
		set({
			...IDLE_MATCH,
			matchId,
			mazeConfig: {
				width: maze.width,
				height: maze.height,
				walls: maze.walls,
				mud: maze.mud,
				max_turns: maze.max_turns,
				total_cheese: maze.total_cheese,
			},
			root,
			viewerMode: "live",
		});
	},

	onTurnPlayed: (e) => {
		set(
			produce((state: MatchState) => {
				if (!state.root) return;

				const newChild: GameNode = {
					turn: e.turn,
					player1: e.player1,
					player2: e.player2,
					cheese: e.cheese,
					actions: {
						player1: e.player1_action,
						player2: e.player2_action,
					},
					botInfo: { Player1: [], Player2: [] },
					children: [],
				};

				const end = getMainlineEnd(state.root);
				end.children.push(newChild);
				state.mainlineDepth += 1;
			}),
		);
	},

	onMatchOver: (e) => {
		const { cursor, mainlineDepth } = get();
		if (cursor.length >= mainlineDepth) {
			set({ result: e, pendingResult: null, viewerMode: "reviewing" });
		} else {
			set({ pendingResult: e });
		}
	},

	onBotInfo: (e) => {
		set(
			produce((state: MatchState) => {
				if (!state.root) return;
				const node = getNodeAtPath(state.root, mainlinePath(e.turn));
				if (!node) return;
				node.botInfo[e.player].push(e);
			}),
		);
	},

	onError: (message) => {
		set({ ...IDLE_MATCH, error: message });
	},

	onDisconnect: (e) => {
		set({ disconnection: e });
	},

	// ── Actions ─────────────────────────────────────────────
	resetToPreview: () => {
		commands.stopMatch().catch(console.error);
		set(IDLE_MATCH);
	},

	// ── Navigation ───────────────────────────────────────────
	goToStart: () => {
		set({ cursor: [], viewerMode: "reviewing" });
	},

	goToEnd: () => {
		const { mainlineDepth } = get();
		set({ cursor: mainlinePath(mainlineDepth), viewerMode: "reviewing" });
	},

	stepForward: () => {
		const { root, cursor } = get();
		if (!root) return;
		const node = getNodeAtPath(root, cursor);
		if (!node || node.children.length === 0) return;
		set({ cursor: [...cursor, 0], viewerMode: "reviewing" });
	},

	stepBack: () => {
		const { cursor } = get();
		if (cursor.length === 0) return;
		set({ cursor: cursor.slice(0, -1), viewerMode: "reviewing" });
	},

	goToTurn: (n) => {
		set({ cursor: mainlinePath(n), viewerMode: "reviewing" });
	},

	togglePlay: () => {
		const { viewerMode } = get();
		set({
			viewerMode: viewerMode === "live" ? "reviewing" : "live",
		});
	},

	setPlaybackSpeed: (ms) => {
		set({ playbackSpeed: ms });
	},

	advanceCursor: () => {
		const { root, cursor } = get();
		if (!root) return;
		const node = getNodeAtPath(root, cursor);
		if (!node || node.children.length === 0) {
			const { pendingResult } = get();
			if (pendingResult) {
				set({
					result: pendingResult,
					pendingResult: null,
					viewerMode: "reviewing",
				});
			}
			return;
		}
		set({ cursor: [...cursor, 0] });
	},
}));

// ── Derived selector ─────────────────────────────────────────────

/**
 * Compute the MazeState the renderer expects from config + current node.
 *
 * Subscribes to mazeConfig and cursor only. Root is read via getState() so
 * immer-produced new references on every turn don't cause re-renders during
 * live playback. Cursor changes trigger the memo, and getState() always
 * returns the latest tree.
 */
export function useDisplayState(): MazeState | null {
	const mazeConfig = useMatchStore((s) => s.mazeConfig);
	const cursor = useMatchStore((s) => s.cursor);
	const previewMaze = useMatchStore((s) => s.previewMaze);

	return useMemo(() => {
		if (mazeConfig) {
			const root = useMatchStore.getState().root;
			if (!root) return previewMaze;
			const node = getNodeAtPath(root, cursor) ?? root;
			return {
				width: mazeConfig.width,
				height: mazeConfig.height,
				walls: mazeConfig.walls,
				mud: mazeConfig.mud,
				max_turns: mazeConfig.max_turns,
				total_cheese: mazeConfig.total_cheese,
				turn: node.turn,
				player1: node.player1,
				player2: node.player2,
				cheese: node.cheese,
			};
		}
		return previewMaze;
	}, [mazeConfig, cursor, previewMaze]);
}

/** Current node's bot info, or null if at root with nothing. */
export function useCurrentBotInfo() {
	return useMatchStore((s) => {
		if (!s.root) return null;
		const node = getNodeAtPath(s.root, s.cursor);
		return node?.botInfo ?? null;
	});
}

/** Number of turns in the mainline (for "Turn X / Y" display). */
export function useMainlineLength(): number {
	return useMatchStore((s) => s.mainlineDepth);
}

/** Current cursor depth (which turn we're viewing). */
export function useCursorDepth(): number {
	return useMatchStore((s) => s.cursor.length);
}

// ── Preview generation ──────────────────────────────────────────

let previewVersion = 0;

function randomSeed(): number {
	return Math.floor(Math.random() * 2 ** 32);
}

/** Generate a maze preview for the given config. Stale responses are discarded. */
export async function generatePreview(
	config: MatchConfigParams,
	seedOverride?: number,
) {
	const version = ++previewVersion;
	const seed = seedOverride ?? config.seed ?? randomSeed();

	const res = await commands.getGameState({ ...config, seed });
	if (version !== previewVersion) return; // stale

	if (res.status === "ok") {
		useMatchStore.setState({
			previewMaze: res.data,
			previewSeed: seed,
			previewError: null,
		});
	} else {
		useMatchStore.setState({
			previewMaze: null,
			previewSeed: null,
			previewError: res.error,
		});
	}
}

/** Re-roll: generate preview with a fresh random seed, ignoring config.seed. */
export async function rerollPreview(config: MatchConfigParams) {
	return generatePreview(config, randomSeed());
}
