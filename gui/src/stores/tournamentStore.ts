import { create } from "zustand";
import type {
	FailureKind,
	NowPlayingEvent,
	StandingRow,
	StandingsUpdatedEvent,
	TimeoutPhase,
	TournamentMatchFailedEvent,
	TournamentMatchFinishedEvent,
	TournamentMatchStartedEvent,
	TournamentPreparingEvent,
	TournamentStartedEvent,
} from "../bindings/generated";
import { pairKey } from "../components/tournament/theme";

// ── Types ────────────────────────────────────────────────────────

export type TournamentStatus = "running" | "finished" | "aborted";

/** One finished game, canonical orientation (player1Id is lex-min, and so
 * player1Score is the lex-min player's score regardless of who was Rat — the
 * Rust side canonicalizes seat-order scores before emitting). */
export interface FinishedGame {
	matchId: number;
	player1Id: string;
	player2Id: string;
	player1Score: number;
	player2Score: number;
}

/** A failed match, attributed to a bot where the failure points at one seat.
 * `failingPlayerId` is resolved Rust-side from the engine seat via the match's
 * orientation, so it's the canonical bot id (or null for structural failures
 * with no single seat). Drives the standings health marker + matchup
 * breakdown. */
export interface MatchFailureRecord {
	matchId: number;
	player1Id: string;
	player2Id: string;
	failingPlayerId: string | null;
	kind: FailureKind;
	timeoutPhase: TimeoutPhase | null;
}

/** A match currently in flight (for the now-playing line + live row). */
export interface LiveMatch {
	matchId: number;
	player1Id: string;
	player2Id: string;
	turn: number;
	player1Score: number;
	player2Score: number;
}

/** Depth-ladder navigation within the live view. */
export type TournamentNav =
	| { kind: "overview" }
	| { kind: "bot"; botId: string } // round-robin depth 1.5
	| { kind: "matchup"; a: string; b: string; fromBot?: string }
	| { kind: "game"; a: string; b: string; matchId: number; fromBot?: string };

export interface TournamentLive {
	tournamentId: number;
	name: string | null;
	format: string;
	target: string | null;
	anchorId: string;
	planSummary: string;
	players: string[];
	status: TournamentStatus;
	abortReason: string | null;
	done: number;
	total: number;
	success: number;
	failure: number;
	standings: StandingRow[];
	/** Finished games keyed by canonical pair key. */
	gamesByPair: Record<string, FinishedGame[]>;
	/** Failed matches keyed by canonical pair key. Mirrors `gamesByPair` so the
	 * matchup view reads its failures directly; the standings marker derives a
	 * per-bot rollup via `botFailures`. */
	failuresByPair: Record<string, MatchFailureRecord[]>;
	/** In-flight matches keyed by match id. */
	liveByMatch: Record<number, LiveMatch>;
	/** Tombstones: match ids that have reached a terminal state (success OR
	 * failure). `onMatchStarted` ignores any id in here, so a delayed/late
	 * `MatchStarted` (lossy stream) can't resurrect a row after the match
	 * already ended — regardless of which stream's event arrives first. */
	terminalMatchIds: Record<number, true>;
	/** Wall-clock start (epoch ms) for the elapsed counter. */
	startedAt: number;
}

interface TournamentStore {
	screen: "launch" | "live";
	live: TournamentLive | null;
	/** Transient pre-tournament warmup progress (no tournament id yet). Set by
	 * `onPreparing`, cleared when the tournament starts. Drives the launch
	 * screen's "Preparing bots…" line. */
	preparing: { done: number; total: number } | null;
	nav: TournamentNav;
	// navigation
	showLaunch: () => void;
	showLive: () => void;
	navigate: (nav: TournamentNav) => void;
	back: () => void;
	// event handlers
	onPreparing: (e: TournamentPreparingEvent) => void;
	onStarted: (e: TournamentStartedEvent, startedAt: number) => void;
	onStandings: (e: StandingsUpdatedEvent) => void;
	onMatchFinished: (e: TournamentMatchFinishedEvent) => void;
	onMatchFailed: (e: TournamentMatchFailedEvent) => void;
	onMatchStarted: (e: TournamentMatchStartedEvent) => void;
	onNowPlaying: (e: NowPlayingEvent) => void;
	onFinished: () => void;
	onAborted: (reason: string) => void;
}

export const useTournamentStore = create<TournamentStore>((set, get) => ({
	screen: "launch",
	live: null,
	preparing: null,
	nav: { kind: "overview" },

	showLaunch: () => set({ screen: "launch", nav: { kind: "overview" } }),

	// Return to the live view (e.g. from the launch screen or the LiveChip).
	// Only meaningful when a tournament is live; the chip / banner that call
	// it are shown only then.
	showLive: () => set({ screen: "live" }),

	navigate: (nav) => set({ nav }),

	back: () =>
		set((s) => {
			switch (s.nav.kind) {
				case "game":
					return {
						nav: {
							kind: "matchup",
							a: s.nav.a,
							b: s.nav.b,
							fromBot: s.nav.fromBot,
						},
					};
				case "matchup":
					return {
						nav: s.nav.fromBot
							? { kind: "bot", botId: s.nav.fromBot }
							: { kind: "overview" },
					};
				case "bot":
					return { nav: { kind: "overview" } };
				default:
					return {};
			}
		}),

	onPreparing: (e) => set({ preparing: { done: e.done, total: e.total } }),

	onStarted: (e, startedAt) =>
		set({
			screen: "live",
			nav: { kind: "overview" },
			preparing: null,
			live: {
				tournamentId: e.tournament_id,
				name: e.name,
				format: e.format,
				target: e.target,
				anchorId: e.anchor_id,
				planSummary: e.plan_summary,
				players: e.players.map((p) => p.player_id),
				status: "running",
				abortReason: null,
				done: 0,
				total: e.total_games,
				success: 0,
				failure: 0,
				standings: [],
				gamesByPair: {},
				failuresByPair: {},
				liveByMatch: {},
				terminalMatchIds: {},
				startedAt,
			},
		}),

	// Standings is the lossless verdict path and sub-ms to render (per the
	// brief). It is NOT wrapped in startTransition: doing so priority-inverts
	// it behind the lossy NowPlaying renders, and onMatchFinished doesn't use
	// it either — keep the verdict path eager and consistent.
	onStandings: (e) => {
		const live = get().live;
		if (!live || live.tournamentId !== e.tournament_id) return;
		set((s) =>
			s.live
				? {
						live: {
							...s.live,
							done: e.done,
							total: e.total,
							success: e.success,
							failure: e.failure,
							standings: e.standings,
						},
					}
				: {},
		);
	},

	onMatchFinished: (e) => {
		const live = get().live;
		if (!live || live.tournamentId !== e.tournament_id) return;
		const key = pairKey(e.player1_id, e.player2_id);
		const game: FinishedGame = {
			matchId: e.match_id,
			player1Id: e.player1_id,
			player2Id: e.player2_id,
			player1Score: e.player1_score,
			player2Score: e.player2_score,
		};
		set((s) => {
			if (!s.live) return {};
			const existing = s.live.gamesByPair[key] ?? [];
			const { [e.match_id]: _drop, ...liveByMatch } = s.live.liveByMatch;
			return {
				live: {
					...s.live,
					gamesByPair: { ...s.live.gamesByPair, [key]: [...existing, game] },
					liveByMatch,
					terminalMatchIds: { ...s.live.terminalMatchIds, [e.match_id]: true },
				},
			};
		});
	},

	// A failed match emits no scored event, so without this its now-playing row
	// would freeze at its last turn until the whole tournament ends. Drop the
	// row and tombstone the id so a late `MatchStarted` can't resurrect it. No
	// `status === "running"` gate: a terminal event must always tombstone.
	// Also record the failure (kind + implicated bot) for the health surface.
	onMatchFailed: (e) => {
		const live = get().live;
		if (!live || live.tournamentId !== e.tournament_id) return;
		const key = pairKey(e.player1_id, e.player2_id);
		const record: MatchFailureRecord = {
			matchId: e.match_id,
			player1Id: e.player1_id,
			player2Id: e.player2_id,
			failingPlayerId: e.failing_player_id,
			kind: e.kind,
			timeoutPhase: e.timeout_phase,
		};
		set((s) => {
			if (!s.live) return {};
			const { [e.match_id]: _drop, ...liveByMatch } = s.live.liveByMatch;
			const existing = s.live.failuresByPair[key] ?? [];
			return {
				live: {
					...s.live,
					liveByMatch,
					failuresByPair: {
						...s.live.failuresByPair,
						[key]: [...existing, record],
					},
					terminalMatchIds: { ...s.live.terminalMatchIds, [e.match_id]: true },
				},
			};
		});
	},

	onMatchStarted: (e) => {
		const live = get().live;
		if (!live || live.tournamentId !== e.tournament_id) return;
		// `MatchStarted` (lossy) and the terminal events (lossless) ride
		// independent streams with no ordering guarantee. Without these guards a
		// delayed `MatchStarted` would re-insert the match at turn 0 and nothing
		// would remove it again — a zombie "Now playing" row that inflates the
		// count. Ignore once the tournament is no longer running, and ignore any
		// id already terminated (success or failure) — the tombstone makes this
		// order-independent: a `MatchFailed` arriving before a late
		// `MatchStarted` still blocks the row.
		if (live.status !== "running") return;
		if (live.terminalMatchIds[e.match_id]) return;
		set((s) =>
			s.live
				? {
						live: {
							...s.live,
							liveByMatch: {
								...s.live.liveByMatch,
								[e.match_id]: {
									matchId: e.match_id,
									player1Id: e.player1_id,
									player2Id: e.player2_id,
									turn: 0,
									player1Score: 0,
									player2Score: 0,
								},
							},
						},
					}
				: {},
		);
	},

	onNowPlaying: (e) => {
		const live = get().live;
		if (!live || live.tournamentId !== e.tournament_id) return;
		if (live.status !== "running") return;
		const cur = live.liveByMatch[e.match_id];
		// `NowPlayingEvent` carries no player ids, so it can't create a row on
		// its own — the row is created by `MatchStarted`. If that hasn't arrived
		// (or the match already finished and was removed), skip; the row
		// self-heals on the next tick once `MatchStarted` lands.
		if (!cur) return;
		set((s) =>
			s.live
				? {
						live: {
							...s.live,
							liveByMatch: {
								...s.live.liveByMatch,
								[e.match_id]: {
									...cur,
									turn: e.turn,
									player1Score: e.player1_score,
									player2Score: e.player2_score,
								},
							},
						},
					}
				: {},
		);
	},

	onFinished: () =>
		set((s) =>
			s.live
				? { live: { ...s.live, status: "finished", liveByMatch: {} } }
				: {},
		),

	onAborted: (reason) =>
		set((s) =>
			s.live
				? {
						live: {
							...s.live,
							status: "aborted",
							abortReason: reason,
							liveByMatch: {},
						},
					}
				: {},
		),
}));

// ── Selectors / helpers ──────────────────────────────────────────

/** Standings sorted for display: rated rows by Elo desc, pending last. */
export function sortedStandings(rows: StandingRow[]): StandingRow[] {
	return [...rows].sort((a, b) => {
		if (a.pending !== b.pending) return a.pending ? 1 : -1;
		return b.elo - a.elo;
	});
}

/** W/L/D from one player's perspective for a finished game. */
export function resultFor(
	game: FinishedGame,
	playerId: string,
): "W" | "L" | "D" {
	const mine =
		game.player1Id === playerId ? game.player1Score : game.player2Score;
	const theirs =
		game.player1Id === playerId ? game.player2Score : game.player1Score;
	if (mine > theirs) return "W";
	if (mine < theirs) return "L";
	return "D";
}

/** Failures attributed to a bot (resolved seat), across all its matchups. */
export function botFailures(
	live: TournamentLive,
	botId: string,
): MatchFailureRecord[] {
	return Object.values(live.failuresByPair)
		.flat()
		.filter((f) => f.failingPlayerId === botId);
}

/** Short human label for a failure, e.g. "move timeout", "disconnected". */
export function failureLabel(
	kind: FailureKind,
	phase: TimeoutPhase | null,
): string {
	switch (kind) {
		case "timeout":
			return phase ? `${phase} timeout` : "timeout";
		case "disconnected":
			return "disconnected";
		case "spawn_failed":
			return "failed to start";
		case "handshake_timeout":
			return "handshake timeout";
		case "protocol_error":
			return "protocol error";
		case "cancelled":
			return "cancelled";
		default:
			return "failed";
	}
}

/** Roll a set of failures up into "{count} {label}" fragments, most-common
 * first — the breakdown shown in the matchup view. */
export function failureBreakdown(
	failures: MatchFailureRecord[],
): { label: string; count: number }[] {
	const byLabel = new Map<string, number>();
	for (const f of failures) {
		const label = failureLabel(f.kind, f.timeoutPhase);
		byLabel.set(label, (byLabel.get(label) ?? 0) + 1);
	}
	return [...byLabel.entries()]
		.map(([label, count]) => ({ label, count }))
		.sort((a, b) => b.count - a.count);
}
