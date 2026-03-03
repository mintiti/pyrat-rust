import { useMemo } from "react";
import { create } from "zustand";
import type {
	BotDisconnectedEvent,
	BotInfoEvent,
	Coord,
	Direction,
	MatchOverEvent,
	MazeState,
	PlayerState,
	TurnPlayedEvent,
	WallEntry,
	MudEntry,
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
	botInfo: { player1: BotInfoEvent[]; player2: BotInfoEvent[] };
	children: GameNode[];
}

export type ViewerMode = "empty" | "playing" | "paused";

// ── Tree helpers ─────────────────────────────────────────────────

/** Walk the tree following `path` indices. Returns null if the path is invalid. */
export function getNodeAtPath(
	root: GameNode,
	path: number[],
): GameNode | null {
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

/** Depth of the first-child chain (number of turns after root). */
export function mainlineLength(root: GameNode): number {
	let n = 0;
	let node = root;
	while (node.children.length > 0) {
		node = node.children[0];
		n++;
	}
	return n;
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
	player1Cmd: string | null;
	player2Cmd: string | null;
	result: MatchOverEvent | null;
	pendingResult: MatchOverEvent | null;
	error: string | null;
	disconnection: BotDisconnectedEvent | null;

	// Game tree
	root: GameNode | null;
	cursor: number[]; // path into tree, [] = root
	treeVersion: number; // bumped on every tree mutation to trigger selectors

	// Viewer
	viewerMode: ViewerMode;
	playbackSpeed: number; // ms between frames

	// Setters for bot selectors
	setPlayer1Cmd: (cmd: string | null) => void;
	setPlayer2Cmd: (cmd: string | null) => void;

	// Event handlers
	onMatchStarted: (maze: MazeState, matchId: number) => void;
	onTurnPlayed: (e: TurnPlayedEvent) => void;
	onMatchOver: (e: MatchOverEvent) => void;
	onBotInfo: (e: BotInfoEvent) => void;
	onError: (message: string) => void;
	onDisconnect: (e: BotDisconnectedEvent) => void;

	// Apply pending result (called when cursor reaches the end)
	applyPendingResult: () => void;

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

export const useMatchStore = create<MatchState>((set, get) => ({
	// ── Initial state ────────────────────────────────────────
	matchId: null,
	mazeConfig: null,
	player1Cmd: "__random__",
	player2Cmd: "__random__",
	result: null,
	pendingResult: null,
	error: null,
	disconnection: null,
	root: null,
	cursor: [],
	treeVersion: 0,
	viewerMode: "empty",
	playbackSpeed: 200,

	// ── Setters ──────────────────────────────────────────────
	setPlayer1Cmd: (cmd) => set({ player1Cmd: cmd }),
	setPlayer2Cmd: (cmd) => set({ player2Cmd: cmd }),

	// ── Event handlers ───────────────────────────────────────
	onMatchStarted: (maze, matchId) => {
		const root: GameNode = {
			turn: maze.turn,
			player1: maze.player1,
			player2: maze.player2,
			cheese: maze.cheese,
			actions: null,
			botInfo: { player1: [], player2: [] },
			children: [],
		};
		set({
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
			cursor: [],
			viewerMode: "playing",
			result: null,
			pendingResult: null,
			error: null,
			disconnection: null,
		});
	},

	onTurnPlayed: (e) => {
		const { root } = get();
		if (!root) return;

		const newChild: GameNode = {
			turn: e.turn,
			player1: e.player1,
			player2: e.player2,
			cheese: e.cheese,
			actions: { player1: e.player1_action, player2: e.player2_action },
			botInfo: { player1: [], player2: [] },
			children: [],
		};

		// Append to mainline end — mutate in place, bump version to notify selectors.
		const end = getMainlineEnd(root);
		end.children.push(newChild);
		set({ treeVersion: get().treeVersion + 1 });
	},

	onMatchOver: (e) => {
		set({ pendingResult: e });
	},

	onBotInfo: (e) => {
		const { root } = get();
		if (!root) return;

		// Find the node for this turn.
		const path = mainlinePath(e.turn);
		const node = getNodeAtPath(root, path);
		if (!node) return;

		const key = e.player as "player1" | "player2";
		node.botInfo[key].push(e);
		set({ treeVersion: get().treeVersion + 1 });
	},

	onError: (message) => {
		set({ error: message, viewerMode: "empty" });
	},

	onDisconnect: (e) => {
		set({ disconnection: e });
	},

	applyPendingResult: () => {
		const { pendingResult } = get();
		if (!pendingResult) return;
		set({
			result: pendingResult,
			pendingResult: null,
			viewerMode: "paused",
		});
	},

	// ── Navigation ───────────────────────────────────────────
	goToStart: () => {
		set({ cursor: [], viewerMode: "paused" });
	},

	goToEnd: () => {
		const { root } = get();
		if (!root) return;
		const len = mainlineLength(root);
		set({ cursor: mainlinePath(len), viewerMode: "paused" });
	},

	stepForward: () => {
		const { root, cursor } = get();
		if (!root) return;
		const node = getNodeAtPath(root, cursor);
		if (!node || node.children.length === 0) return;
		set({ cursor: [...cursor, 0], viewerMode: "paused" });
	},

	stepBack: () => {
		const { cursor } = get();
		if (cursor.length === 0) return;
		set({ cursor: cursor.slice(0, -1), viewerMode: "paused" });
	},

	goToTurn: (n) => {
		set({ cursor: mainlinePath(n), viewerMode: "paused" });
	},

	togglePlay: () => {
		const { viewerMode } = get();
		set({
			viewerMode: viewerMode === "playing" ? "paused" : "playing",
		});
	},

	setPlaybackSpeed: (ms) => {
		set({ playbackSpeed: ms });
	},

	advanceCursor: () => {
		const { root, cursor } = get();
		if (!root) return;
		const node = getNodeAtPath(root, cursor);
		if (!node || node.children.length === 0) return;
		set({ cursor: [...cursor, 0] });
	},
}));

// ── Derived selector ─────────────────────────────────────────────

/**
 * Compute the MazeState the renderer expects from config + current node.
 *
 * Uses separate subscriptions so the component only re-renders when cursor
 * moves or mazeConfig changes — not on every tree mutation (turn arrival).
 * Root is mutated in place (same reference) so it doesn't trigger re-renders,
 * but the tree is up-to-date when we walk it after a cursor change.
 */
export function useDisplayState(): MazeState | null {
	const mazeConfig = useMatchStore((s) => s.mazeConfig);
	const root = useMatchStore((s) => s.root);
	const cursor = useMatchStore((s) => s.cursor);

	return useMemo(() => {
		if (!mazeConfig || !root) return null;
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
	}, [mazeConfig, root, cursor]);
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
	const root = useMatchStore((s) => s.root);
	const treeVersion = useMatchStore((s) => s.treeVersion);
	return useMemo(() => {
		if (!root) return 0;
		return mainlineLength(root);
		// treeVersion triggers recomputation when the tree is mutated in place
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [root, treeVersion]);
}

/** Current cursor depth (which turn we're viewing). */
export function useCursorDepth(): number {
	return useMatchStore((s) => s.cursor.length);
}
