import { produce } from "immer";
import { useMemo } from "react";
import { create } from "zustand";
import { commands } from "../bindings";
import type {
	AnalysisActions,
	BotDisconnectedEvent,
	BotInfoEvent,
	Coord,
	Direction,
	MatchConfigParams,
	MatchOverEvent,
	MazeState,
	MudEntry,
	PlayerSide,
	PlayerState,
	TurnPlayedEvent,
	WallEntry,
} from "../bindings/generated";
import { RANDOM_BOT_ID } from "./botConfigAtom";
import { type BotInfoMap, accumulateBotInfo } from "./botInfo";

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
	botInfo: BotInfoMap;
	children: GameNode[];
}

export type MatchPhase = "idle" | "connecting" | "playing" | "finished";
export type MatchMode = "auto" | "step";

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
	mode: MatchMode;
	matchPhase: MatchPhase;
	autoplay: boolean;
	playbackSpeed: number; // ms between frames
	showPlayer1Arrows: boolean;
	showPlayer2Arrows: boolean;

	// Setters for bot selectors
	setPlayer1BotId: (cmd: string | null) => void;
	setPlayer2BotId: (cmd: string | null) => void;
	setMode: (mode: MatchMode) => void;

	// Event handlers
	onMatchStarted: (maze: MazeState, matchId: number) => void;
	onTurnPlayed: (e: TurnPlayedEvent) => void;
	onMatchOver: (e: MatchOverEvent) => void;
	onBotInfo: (e: BotInfoEvent) => void;
	onError: (message: string) => void;
	onDisconnect: (e: BotDisconnectedEvent) => void;

	// Actions
	beginConnecting: () => void;
	resetToPreview: () => void;

	toggleArrows: (sender: PlayerSide) => void;

	// Analysis actions (step mode)
	startAnalysis: () => Promise<void>;
	stopAnalysis: () => Promise<{
		player1: Direction;
		player2: Direction;
	} | null>;
	advanceTurn: (actions?: AnalysisActions) => Promise<void>;

	// Navigation
	goToStart: () => void;
	goToEnd: () => void;
	stepForward: () => void;
	stepBack: () => void;
	stepForwardIntoVariation: (idx: number) => void;
	cycleVariation: (delta: number) => void;
	returnToMainline: () => void;
	goToTurn: (n: number) => void;
	togglePlay: () => void;
	goLive: () => void;
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
	error: null as string | null,
	disconnection: null as BotDisconnectedEvent | null,
	mode: "auto" as MatchMode,
	matchPhase: "idle" as MatchPhase,
	autoplay: true,
};

export const useMatchStore = create<MatchState>((set, get) => ({
	// ── Initial state ────────────────────────────────────────
	...IDLE_MATCH,
	player1BotId: RANDOM_BOT_ID,
	player2BotId: RANDOM_BOT_ID,
	previewMaze: null,
	previewSeed: null,
	previewError: null,
	playbackSpeed: 200,
	showPlayer1Arrows: true,
	showPlayer2Arrows: true,

	// ── Setters ──────────────────────────────────────────────
	setPlayer1BotId: (cmd) => set({ player1BotId: cmd }),
	setPlayer2BotId: (cmd) => set({ player2BotId: cmd }),
	setMode: (mode) => set({ mode }),

	// ── Event handlers ───────────────────────────────────────
	onMatchStarted: (maze, matchId) => {
		const { mode } = get();
		const root: GameNode = {
			turn: maze.turn,
			player1: maze.player1,
			player2: maze.player2,
			cheese: maze.cheese,
			actions: null,
			botInfo: {},
			children: [],
		};
		set({
			...IDLE_MATCH,
			mode,
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
			matchPhase: "playing",
			autoplay: mode === "auto",
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
					botInfo: {},
					children: [],
				};

				if (state.mode === "step") {
					const parent = getNodeAtPath(state.root, state.cursor);
					if (!parent) return;
					// Dedup by resulting state (not raw actions — invalid moves become Stay)
					const existingIdx = parent.children.findIndex(
						(c) =>
							c.player1.position.x === e.player1.position.x &&
							c.player1.position.y === e.player1.position.y &&
							c.player1.score === e.player1.score &&
							c.player1.mud_turns === e.player1.mud_turns &&
							c.player2.position.x === e.player2.position.x &&
							c.player2.position.y === e.player2.position.y &&
							c.player2.score === e.player2.score &&
							c.player2.mud_turns === e.player2.mud_turns &&
							c.cheese.length === e.cheese.length,
					);
					if (existingIdx >= 0) {
						state.cursor = [...state.cursor, existingIdx];
					} else {
						parent.children.push(newChild);
						state.cursor = [...state.cursor, parent.children.length - 1];
					}
				} else {
					const end = getMainlineEnd(state.root);
					end.children.push(newChild);
					state.mainlineDepth += 1;
				}
			}),
		);
	},

	onMatchOver: (e) => {
		set({ result: e, matchPhase: "finished" });
	},

	onBotInfo: (e) => {
		set(
			produce((state: MatchState) => {
				if (!state.root) return;
				if (state.mode === "step") {
					const node = getNodeAtPath(state.root, state.cursor);
					if (!node || node.turn !== e.turn) return;
					accumulateBotInfo(node.botInfo, e);
				} else {
					const node = getNodeAtPath(state.root, mainlinePath(e.turn));
					if (!node) return;
					accumulateBotInfo(node.botInfo, e);
				}
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
	beginConnecting: () => {
		set({
			error: null,
			result: null,
			disconnection: null,
			matchPhase: "connecting",
		});
	},

	resetToPreview: () => {
		commands.stopMatch().catch(console.error);
		set(IDLE_MATCH);
	},

	toggleArrows: (sender) => {
		const key =
			sender === "Player1" ? "showPlayer1Arrows" : "showPlayer2Arrows";
		set((s) => ({ [key]: !s[key] }));
	},

	// ── Analysis actions (step mode) ────────────────────────
	startAnalysis: async () => {
		const { mode, matchPhase } = get();
		if (mode !== "step" || matchPhase !== "playing") return;
		const res = await commands.startAnalysisTurn(0);
		if (res.status === "error") console.error("startAnalysis:", res.error);
	},

	stopAnalysis: async () => {
		const res = await commands.stopAnalysisTurn();
		if (res.status === "error") {
			console.error("stopAnalysis:", res.error);
			return null;
		}
		return {
			player1: res.data.player1_action,
			player2: res.data.player2_action,
		};
	},

	advanceTurn: async (actions) => {
		const { mode, matchPhase, root, cursor } = get();
		if (mode !== "step" || matchPhase !== "playing" || !root) return;
		// Guard: cursor must be at a tree tip
		const node = getNodeAtPath(root, cursor);
		if (!node || node.children.length > 0) return;
		const res = await commands.advanceAnalysis(actions ?? null);
		if (res.status === "error") {
			console.error("advanceTurn:", res.error);
			return;
		}
		// Tree mutation + cursor advance happens in onTurnPlayed
		if (res.data.game_over) {
			// Match will end via onMatchOver event
		}
	},

	// ── Navigation ───────────────────────────────────────────
	goToStart: () => {
		set({ cursor: [], autoplay: false });
	},

	goToEnd: () => {
		const { mode, root, cursor, mainlineDepth } = get();
		if (mode === "step" && root) {
			// Follow first child from cursor to the tip of this branch
			const path = [...cursor];
			let node = getNodeAtPath(root, path);
			while (node && node.children.length > 0) {
				path.push(0);
				node = node.children[0];
			}
			set({ cursor: path, autoplay: false });
		} else {
			set({ cursor: mainlinePath(mainlineDepth), autoplay: false });
		}
	},

	stepForward: () => {
		const { root, cursor } = get();
		if (!root) return;
		const node = getNodeAtPath(root, cursor);
		if (!node || node.children.length === 0) return;
		set({ cursor: [...cursor, 0], autoplay: false });
	},

	stepBack: () => {
		const { cursor } = get();
		if (cursor.length === 0) return;
		set({ cursor: cursor.slice(0, -1), autoplay: false });
	},

	stepForwardIntoVariation: (idx) => {
		const { root, cursor } = get();
		if (!root) return;
		const node = getNodeAtPath(root, cursor);
		if (!node || idx < 0 || idx >= node.children.length) return;
		set({ cursor: [...cursor, idx], autoplay: false });
	},

	cycleVariation: (delta) => {
		const { root, cursor } = get();
		if (!root || cursor.length === 0) return;
		const parentPath = cursor.slice(0, -1);
		const parent = getNodeAtPath(root, parentPath);
		if (!parent) return;
		const currentIdx = cursor[cursor.length - 1];
		const newIdx = currentIdx + delta;
		if (newIdx < 0 || newIdx >= parent.children.length) return;
		set({
			cursor: [...parentPath, newIdx],
			autoplay: false,
		});
	},

	returnToMainline: () => {
		const { root, cursor } = get();
		if (!root) return;
		// Follow mainline (first child) to the same depth as cursor
		const path: number[] = [];
		let node: GameNode | null = root;
		for (let i = 0; i < cursor.length; i++) {
			if (!node || node.children.length === 0) break;
			path.push(0);
			node = node.children[0];
		}
		set({ cursor: path, autoplay: false });
	},

	goToTurn: (n) => {
		set({ cursor: mainlinePath(n), autoplay: false });
	},

	togglePlay: () => {
		set((s) => ({ autoplay: !s.autoplay }));
	},

	goLive: () => {
		const { mainlineDepth } = get();
		set({ cursor: mainlinePath(mainlineDepth), autoplay: true });
	},

	setPlaybackSpeed: (ms) => {
		set({ playbackSpeed: ms });
	},

	advanceCursor: () => {
		const { root, cursor, matchPhase } = get();
		if (!root) return;
		const node = getNodeAtPath(root, cursor);
		if (!node || node.children.length === 0) {
			// At tree end: stop autoplay if match is finished (replay done)
			if (matchPhase === "finished") {
				set({ autoplay: false });
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

/** True if the cursor node has no children (at a tree tip). */
export function useIsAtTip(): boolean {
	return useMatchStore((s) => {
		if (!s.root) return true;
		const node = getNodeAtPath(s.root, s.cursor);
		return !node || node.children.length === 0;
	});
}

/** True if all cursor indices are 0 (on the mainline). */
export function useIsOnMainline(): boolean {
	return useMatchStore((s) => s.cursor.every((idx) => idx === 0));
}

/** Number of sibling variations at the cursor's parent. */
export function useVariationCount(): number {
	return useMatchStore((s) => {
		if (!s.root || s.cursor.length === 0) return 0;
		const parent = getNodeAtPath(s.root, s.cursor.slice(0, -1));
		return parent?.children.length ?? 0;
	});
}

/** Index of the current variation (last element of cursor). */
export function useCurrentVariationIndex(): number {
	return useMatchStore((s) =>
		s.cursor.length > 0 ? s.cursor[s.cursor.length - 1] : 0,
	);
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
