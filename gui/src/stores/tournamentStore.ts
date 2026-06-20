import { create } from "zustand";
import type {
	NowPlayingEvent,
	StandingRow,
	StandingsUpdatedEvent,
	TournamentMatchFinishedEvent,
	TournamentMatchStartedEvent,
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
	/** In-flight matches keyed by match id. */
	liveByMatch: Record<number, LiveMatch>;
	/** Wall-clock start (epoch ms) for the elapsed counter. */
	startedAt: number;
}

interface TournamentStore {
	screen: "launch" | "live";
	live: TournamentLive | null;
	nav: TournamentNav;
	// navigation
	showLaunch: () => void;
	navigate: (nav: TournamentNav) => void;
	back: () => void;
	// event handlers
	onStarted: (e: TournamentStartedEvent, startedAt: number) => void;
	onStandings: (e: StandingsUpdatedEvent) => void;
	onMatchFinished: (e: TournamentMatchFinishedEvent) => void;
	onMatchStarted: (e: TournamentMatchStartedEvent) => void;
	onNowPlaying: (e: NowPlayingEvent) => void;
	onFinished: () => void;
	onAborted: (reason: string) => void;
}

export const useTournamentStore = create<TournamentStore>((set, get) => ({
	screen: "launch",
	live: null,
	nav: { kind: "overview" },

	showLaunch: () => set({ screen: "launch", nav: { kind: "overview" } }),

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

	onStarted: (e, startedAt) =>
		set({
			screen: "live",
			nav: { kind: "overview" },
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
				liveByMatch: {},
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
				},
			};
		});
	},

	onMatchStarted: (e) => {
		const live = get().live;
		if (!live || live.tournamentId !== e.tournament_id) return;
		// `MatchStarted` and `MatchFinished` ride independent, lossy streams, so
		// a delayed `MatchStarted` can arrive after the match already finished.
		// Without these guards it would re-insert the match at turn 0 and nothing
		// would ever remove it again — a zombie "Now playing" row that inflates
		// the live count. Ignore once the tournament is no longer running, and
		// ignore any match id we've already recorded as finished.
		if (live.status !== "running") return;
		const key = pairKey(e.player1_id, e.player2_id);
		if ((live.gamesByPair[key] ?? []).some((g) => g.matchId === e.match_id))
			return;
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
