import { create } from "zustand";
import { commands } from "../bindings";
import type {
	FailureKind,
	NowPlayingEvent,
	RatingReadiness,
	StandingRow,
	StandingsUpdatedEvent,
	TimeoutPhase,
	TournamentAbortedEvent,
	TournamentFinishedEvent,
	TournamentLifecycleStatus,
	TournamentMatchFailedEvent,
	TournamentMatchFinishedEvent,
	TournamentMatchStartedEvent,
	TournamentPreparingEvent,
	TournamentRuntimeStatus,
	TournamentSnapshot,
	TournamentStartedEvent,
	TournamentStoppingEvent,
} from "../bindings/generated";
import { pairKey } from "../components/tournament/theme";

// ── Types ────────────────────────────────────────────────────────

/** The GUI runner marks non-anchor ratings pending below this sample count. */
export const MIN_GAMES_FOR_RATING = 4;

export type TournamentStatus = "running" | "finished" | "aborted" | "partial";

export interface TournamentTerminalNotice {
	tournamentId: number;
	status: Extract<TournamentStatus, "finished" | "aborted">;
	reason: string | null;
}

/** One finished game, canonical orientation (player1Id is lex-min, and so
 * player1Score is the lex-min player's score regardless of who was Rat — the
 * Rust side canonicalizes seat-order scores before emitting). */
export interface FinishedGame {
	gameKey: string;
	matchId: number | null;
	player1Id: string;
	player2Id: string;
	repetitionIndex: number;
	ratId: string;
	player1Score: number;
	player2Score: number;
}

/** A failed match, attributed to a bot where the failure points at one seat.
 * `failingPlayerId` is resolved Rust-side from the engine seat via the match's
 * orientation, so it's the canonical bot id (or null for structural failures
 * with no single seat). Drives the standings health marker + matchup
 * breakdown. */
export interface MatchFailureRecord {
	failureKey: string;
	matchId: number | null;
	player1Id: string;
	player2Id: string;
	repetitionIndex: number;
	attemptIndex: number;
	ratId: string;
	failingPlayerId: string | null;
	kind: FailureKind;
	timeoutPhase: TimeoutPhase | null;
	reason: string;
	exhausted: boolean;
}

/** A match currently in flight (for the now-playing line + live row). */
export interface LiveMatch {
	matchId: number;
	player1Id: string;
	player2Id: string;
	repetitionIndex: number;
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
	origin: "active" | "stored";
	tournamentId: number;
	name: string | null;
	format: string;
	target: string | null;
	anchorId: string;
	planSummary: string;
	players: string[];
	status: TournamentStatus;
	lifecycle: TournamentLifecycleStatus;
	terminalOutcome: TournamentFinishedEvent["terminal"]["outcome"] | null;
	ratingReadiness: RatingReadiness;
	ratingReason: string | null;
	abortReason: string | null;
	done: number;
	total: number;
	/** Games per matchup (= 2 × mazes); the matchup view reads this instead of
	 * a hardcoded count. */
	gamesPerMatchup: number;
	/** Adjacent repetitions are the two seat-swapped legs of one maze. */
	paired: boolean;
	/** Configured executor concurrency. ETA reads this instead of assuming the
	 * launch default. */
	maxParallel: number | null;
	success: number;
	failure: number;
	exhausted: number;
	runningMatches: number;
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
	/** Captured when the terminal event arrives so a remounted finished view
	 * keeps the real run duration instead of ticking until it is reopened. */
	endedAt: number | null;
	/** Durable creation time for reopened tournaments. */
	createdAt: string | null;
}

interface TournamentStore {
	screen: "launch" | "live";
	/** A read-only stored tournament currently being inspected. Kept separate
	 * from `live`, which is the sole destination for active runner events. */
	viewing: TournamentLive | null;
	live: TournamentLive | null;
	/** The launch command is in its Starting/warmup phase. Kept at app scope so
	 * leaving and returning to the tournament tab cannot make it look idle. */
	starting: boolean;
	/** Cancellation was accepted, but the owning generation is still draining
	 * and therefore still blocks another launch. */
	stopping: boolean;
	/** Backend-owned phase, retained even before a running snapshot has been
	 * reattached after reload. This is the close-safety authority. */
	runtimePhase: TournamentRuntimeStatus["phase"];
	runtimeTournamentId: number | null;
	/** The last launch failure belongs to the app, not the currently mounted
	 * launch view. It therefore survives navigation until the user edits or
	 * retries the launch. */
	launchFailure: string | null;
	/** Transient pre-tournament warmup progress (no tournament id yet). Set by
	 * `onPreparing`, cleared when the tournament starts. Drives the launch
	 * screen's "Preparing bots…" line. `current` is the bot being warmed. */
	preparing: { done: number; total: number; current: string | null } | null;
	/** One-shot cross-tab handoff when a running tournament reaches a terminal
	 * state. The activity chip dismisses it after it is seen or times out. */
	terminalNotice: TournamentTerminalNotice | null;
	nav: TournamentNav;
	// navigation
	showLaunch: () => void;
	showLive: () => void;
	openSnapshot: (snapshot: TournamentSnapshot) => void;
	restoreActive: (snapshot: TournamentSnapshot) => void;
	onRuntimeStatus: (status: TournamentRuntimeStatus) => void;
	reconcileSnapshot: (snapshot: TournamentSnapshot) => void;
	navigate: (nav: TournamentNav) => void;
	back: () => void;
	beginLaunch: () => void;
	setLaunchFailure: (failure: string | null) => void;
	dismissTerminalNotice: () => void;
	// event handlers
	onPreparing: (e: TournamentPreparingEvent) => void;
	onStopping: (e: TournamentStoppingEvent) => void;
	requestStop: () => Promise<void>;
	/** Drop the transient warmup progress (launch start / stop / failed launch),
	 * so a failed or cancelled warmup leaves no stale "Preparing…" behind. */
	clearPreparing: () => void;
	onStarted: (e: TournamentStartedEvent, startedAt: number) => void;
	onStandings: (e: StandingsUpdatedEvent) => void;
	onMatchFinished: (e: TournamentMatchFinishedEvent) => void;
	onMatchFailed: (e: TournamentMatchFailedEvent) => void;
	onMatchStarted: (e: TournamentMatchStartedEvent) => void;
	onNowPlaying: (e: NowPlayingEvent) => void;
	onFinished: (event: TournamentFinishedEvent) => void;
	onAborted: (event: TournamentAbortedEvent) => void;
}

export const useTournamentStore = create<TournamentStore>((set, get) => ({
	screen: "launch",
	viewing: null,
	live: null,
	starting: false,
	stopping: false,
	runtimePhase: "idle",
	runtimeTournamentId: null,
	launchFailure: null,
	preparing: null,
	terminalNotice: null,
	nav: { kind: "overview" },

	showLaunch: () =>
		set({ screen: "launch", viewing: null, nav: { kind: "overview" } }),

	// Return to the live view (e.g. from the launch screen or the LiveChip).
	// Only meaningful when a tournament is live; the chip / banner that call
	// it are shown only then.
	showLive: () =>
		set({
			screen: "live",
			viewing: null,
			nav: { kind: "overview" },
			terminalNotice: null,
		}),

	openSnapshot: (snapshot) => {
		const live = get().live;
		if (live?.tournamentId === snapshot.tournament_id) {
			set({
				screen: "live",
				viewing: null,
				nav: { kind: "overview" },
				terminalNotice: null,
			});
			return;
		}
		set({
			screen: "live",
			viewing: tournamentFromSnapshot(snapshot, "stored"),
			nav: { kind: "overview" },
		});
	},

	// Reattach the event destination after a webview reload. This does not open
	// the page: the app-level chip is the handoff back to the still-running run.
	restoreActive: (snapshot) => {
		if (!snapshot.running) return;
		set((state) => ({
			live:
				state.live?.tournamentId === snapshot.tournament_id
					? mergeTournamentSnapshot(state.live, snapshot)
					: tournamentFromSnapshot(snapshot, "active"),
		}));
	},

	onRuntimeStatus: (status) =>
		set({
			runtimePhase: status.phase,
			runtimeTournamentId: status.tournament_id,
			starting: status.phase === "starting",
			stopping: status.phase === "stopping",
			preparing: status.phase === "starting" ? get().preparing : null,
		}),

	// Durable reconciliation repairs missed terminal lifecycle events without
	// replacing in-flight liveness or navigation. It updates the active object
	// and an independently viewed copy of the same stored tournament, if any.
	reconcileSnapshot: (snapshot) =>
		set((state) => ({
			live:
				state.live?.tournamentId === snapshot.tournament_id
					? mergeTournamentSnapshot(state.live, snapshot)
					: state.live,
			viewing:
				state.viewing?.tournamentId === snapshot.tournament_id
					? tournamentFromSnapshot(snapshot, "stored")
					: state.viewing,
		})),

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

	beginLaunch: () =>
		set({
			screen: "launch",
			viewing: null,
			starting: true,
			stopping: false,
			runtimePhase: "starting",
			runtimeTournamentId: null,
			launchFailure: null,
			preparing: null,
			terminalNotice: null,
		}),

	setLaunchFailure: (launchFailure) => set({ launchFailure }),

	dismissTerminalNotice: () => set({ terminalNotice: null }),

	onPreparing: (e) =>
		set({
			starting: true,
			stopping: false,
			runtimePhase: "starting",
			preparing: { done: e.done, total: e.total, current: e.current },
		}),

	clearPreparing: () =>
		set((state) => ({
			starting: false,
			runtimePhase: state.stopping ? "stopping" : "idle",
			runtimeTournamentId: state.stopping ? state.runtimeTournamentId : null,
			preparing: null,
		})),

	onStopping: (event) =>
		set((state) => ({
			starting: false,
			stopping: true,
			runtimePhase: "stopping",
			runtimeTournamentId: event.tournament_id ?? state.runtimeTournamentId,
			preparing: null,
		})),

	requestStop: async () => {
		if (get().stopping) return;
		get().onStopping({ tournament_id: get().live?.tournamentId ?? null });
		const result = await commands.stopTournament();
		if (result.status === "ok") {
			get().onRuntimeStatus(result.data);
		} else {
			const status = await commands.tournamentStatus();
			if (status.status === "ok") get().onRuntimeStatus(status.data);
			else set({ stopping: false });
		}
	},

	onStarted: (e, startedAt) => {
		const existing = get().live;
		if (existing?.tournamentId === e.tournament_id) {
			set({
				screen: "live",
				viewing: null,
				nav: { kind: "overview" },
				starting: false,
				stopping: false,
				runtimePhase: "running",
				runtimeTournamentId: e.tournament_id,
				launchFailure: null,
				preparing: null,
				terminalNotice: null,
			});
			return;
		}
		set({
			screen: "live",
			viewing: null,
			nav: { kind: "overview" },
			starting: false,
			stopping: false,
			runtimePhase: "running",
			runtimeTournamentId: e.tournament_id,
			launchFailure: null,
			preparing: null,
			terminalNotice: null,
			live: {
				origin: "active",
				tournamentId: e.tournament_id,
				name: e.name,
				format: e.format,
				target: e.target,
				anchorId: e.anchor_id,
				planSummary: e.plan_summary,
				players: e.players.map((p) => p.player_id),
				status: "running",
				lifecycle: "running",
				terminalOutcome: null,
				ratingReadiness: "provisional",
				ratingReason: null,
				abortReason: null,
				done: 0,
				total: e.total_games,
				gamesPerMatchup: e.games_per_matchup,
				paired: e.paired,
				maxParallel: e.max_parallel,
				success: 0,
				failure: 0,
				exhausted: 0,
				runningMatches: 0,
				standings: [],
				gamesByPair: {},
				failuresByPair: {},
				liveByMatch: {},
				terminalMatchIds: {},
				startedAt,
				endedAt: null,
				createdAt: null,
			},
		});
	},

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
							done: e.progress.terminal_slots,
							total: e.progress.planned_slots,
							success: e.progress.successful_games,
							failure: e.progress.failed_attempts,
							exhausted: e.progress.exhausted_slots,
							runningMatches: e.progress.running_matches,
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
			gameKey: `match:${e.match_id}`,
			matchId: e.match_id,
			player1Id: e.player1_id,
			player2Id: e.player2_id,
			repetitionIndex: e.repetition_index,
			ratId: e.rat_id,
			player1Score: e.player1_score,
			player2Score: e.player2_score,
		};
		set((s) => {
			if (!s.live) return {};
			const existing = s.live.gamesByPair[key] ?? [];
			const games = existing.some((item) => item.matchId === e.match_id)
				? existing
				: [...existing, game];
			const { [e.match_id]: _drop, ...liveByMatch } = s.live.liveByMatch;
			return {
				live: {
					...s.live,
					gamesByPair: { ...s.live.gamesByPair, [key]: games },
					liveByMatch,
					runningMatches: Object.keys(liveByMatch).length,
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
			failureKey: `match:${e.match_id}`,
			matchId: e.match_id,
			player1Id: e.player1_id,
			player2Id: e.player2_id,
			repetitionIndex: e.repetition_index,
			attemptIndex: e.attempt_index,
			ratId: e.rat_id,
			failingPlayerId: e.failing_player_id,
			kind: e.kind,
			timeoutPhase: e.timeout_phase,
			reason: e.reason,
			exhausted: e.exhausted,
		};
		set((s) => {
			if (!s.live) return {};
			const { [e.match_id]: _drop, ...liveByMatch } = s.live.liveByMatch;
			const existing = s.live.failuresByPair[key] ?? [];
			const failures = existing.some((item) => item.matchId === e.match_id)
				? existing
				: [...existing, record];
			return {
				live: {
					...s.live,
					liveByMatch,
					runningMatches: Object.keys(liveByMatch).length,
					failuresByPair: {
						...s.live.failuresByPair,
						[key]: failures,
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
				? (() => {
						const liveByMatch = {
							...s.live.liveByMatch,
							[e.match_id]: {
								matchId: e.match_id,
								player1Id: e.player1_id,
								player2Id: e.player2_id,
								repetitionIndex: e.repetition_index,
								turn: 0,
								player1Score: 0,
								player2Score: 0,
							},
						};
						return {
							live: {
								...s.live,
								liveByMatch,
								runningMatches: Object.keys(liveByMatch).length,
							},
						};
					})()
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

	onFinished: (event) => {
		const tournamentId = event.tournament_id;
		const current = get();
		const live = current.live;
		if (
			!live ||
			live.tournamentId !== tournamentId ||
			live.status !== "running"
		) {
			if (current.runtimeTournamentId === tournamentId) {
				set({
					starting: false,
					stopping: false,
					runtimePhase: "idle",
					runtimeTournamentId: null,
				});
			}
			return;
		}
		const endedAt = Date.now();
		set((s) =>
			s.live
				? {
						live: {
							...s.live,
							status: "finished",
							lifecycle: "completed",
							terminalOutcome: event.terminal.outcome,
							ratingReadiness: event.rating_readiness,
							liveByMatch: {},
							runningMatches: 0,
							endedAt,
						},
						terminalNotice: {
							tournamentId,
							status: "finished",
							reason: null,
						},
						starting: false,
						stopping: false,
						runtimePhase: "idle",
						runtimeTournamentId: null,
					}
				: {},
		);
	},

	onAborted: (event) => {
		const tournamentId = event.tournament_id;
		const reason = event.reason;
		const current = get();
		const live = current.live;
		if (
			!live ||
			live.tournamentId !== tournamentId ||
			live.status !== "running"
		) {
			if (current.runtimeTournamentId === tournamentId) {
				set({
					starting: false,
					stopping: false,
					runtimePhase: "idle",
					runtimeTournamentId: null,
				});
			}
			return;
		}
		const endedAt = Date.now();
		set((s) =>
			s.live
				? {
						live: {
							...s.live,
							status: "aborted",
							lifecycle: event.lifecycle,
							terminalOutcome: event.terminal.outcome,
							ratingReadiness: event.rating_readiness,
							abortReason: reason,
							liveByMatch: {},
							runningMatches: 0,
							endedAt,
						},
						terminalNotice: {
							tournamentId,
							status: "aborted",
							reason,
						},
						starting: false,
						stopping: false,
						runtimePhase: "idle",
						runtimeTournamentId: null,
					}
				: {},
		);
	},
}));

function timestampMs(value: string | null): number | null {
	if (!value) return null;
	const normalized = value.includes("T")
		? value
		: `${value.replace(" ", "T")}Z`;
	const parsed = Date.parse(normalized);
	return Number.isFinite(parsed) ? parsed : null;
}

function tournamentFromSnapshot(
	snapshot: TournamentSnapshot,
	origin: TournamentLive["origin"],
): TournamentLive {
	const gamesByPair: Record<string, FinishedGame[]> = {};
	for (const game of snapshot.games) {
		const key = pairKey(game.player1_id, game.player2_id);
		const pairGames = gamesByPair[key] ?? [];
		pairGames.push({
			gameKey: `attempt:${game.attempt_id}`,
			matchId: game.match_id,
			player1Id: game.player1_id,
			player2Id: game.player2_id,
			repetitionIndex: game.repetition_index,
			ratId: game.rat_id,
			player1Score: game.player1_score,
			player2Score: game.player2_score,
		});
		gamesByPair[key] = pairGames;
	}
	for (const games of Object.values(gamesByPair)) {
		games.sort((a, b) => a.repetitionIndex - b.repetitionIndex);
	}

	const failuresByPair: Record<string, MatchFailureRecord[]> = {};
	for (const failure of snapshot.failures) {
		const key = pairKey(failure.player1_id, failure.player2_id);
		const pairFailures = failuresByPair[key] ?? [];
		pairFailures.push({
			failureKey: `attempt:${failure.attempt_id}`,
			matchId: failure.match_id,
			player1Id: failure.player1_id,
			player2Id: failure.player2_id,
			repetitionIndex: failure.repetition_index,
			attemptIndex: failure.attempt_index,
			ratId: failure.rat_id,
			failingPlayerId: failure.failing_player_id,
			kind: failure.kind,
			timeoutPhase: failure.timeout_phase,
			reason: failure.reason,
			exhausted: failure.exhausted,
		});
		failuresByPair[key] = pairFailures;
	}

	const terminalMatchIds: Record<number, true> = {};
	for (const game of snapshot.games) {
		if (game.match_id !== null) terminalMatchIds[game.match_id] = true;
	}
	for (const failure of snapshot.failures) {
		if (failure.match_id !== null) terminalMatchIds[failure.match_id] = true;
	}

	const startedAt =
		timestampMs(snapshot.started_at) ??
		timestampMs(snapshot.created_at) ??
		Date.now();
	const endedAt = snapshot.running
		? null
		: (timestampMs(snapshot.terminal_at) ??
			timestampMs(snapshot.last_finished_at) ??
			startedAt);
	return {
		origin,
		tournamentId: snapshot.tournament_id,
		name: snapshot.name,
		format: snapshot.format,
		target: snapshot.target,
		anchorId: snapshot.anchor_id,
		planSummary: snapshot.plan_summary,
		players: snapshot.players,
		status: snapshot.running
			? "running"
			: snapshot.lifecycle === "completed"
				? "finished"
				: snapshot.lifecycle === "stopped" || snapshot.lifecycle === "failed"
					? "aborted"
					: "partial",
		lifecycle: snapshot.lifecycle,
		terminalOutcome: snapshot.terminal?.outcome ?? null,
		ratingReadiness: snapshot.rating_readiness,
		ratingReason: snapshot.rating_reason,
		abortReason: snapshot.terminal?.reason ?? null,
		done: snapshot.progress.terminal_slots,
		total: snapshot.progress.planned_slots,
		gamesPerMatchup: snapshot.games_per_matchup,
		paired: snapshot.paired,
		maxParallel: null,
		success: snapshot.progress.successful_games,
		failure: snapshot.progress.failed_attempts,
		exhausted: snapshot.progress.exhausted_slots,
		runningMatches: snapshot.progress.running_matches,
		standings: snapshot.standings,
		gamesByPair,
		failuresByPair,
		liveByMatch: {},
		terminalMatchIds,
		startedAt,
		endedAt,
		createdAt: snapshot.created_at,
	};
}

function mergeTournamentSnapshot(
	current: TournamentLive,
	snapshot: TournamentSnapshot,
): TournamentLive {
	const durable = tournamentFromSnapshot(snapshot, current.origin);
	return {
		...durable,
		// Per-turn liveness is intentionally non-durable and remains event-fed.
		liveByMatch: current.liveByMatch,
		startedAt: current.startedAt,
		endedAt: current.endedAt ?? durable.endedAt,
		status: current.status === "running" ? durable.status : current.status,
		abortReason: current.abortReason ?? durable.abortReason,
		runningMatches: snapshot.running
			? current.runningMatches
			: durable.runningMatches,
		maxParallel: current.maxParallel,
		createdAt: current.createdAt ?? durable.createdAt,
	};
}

// ── Selectors / helpers ──────────────────────────────────────────

/** Standings sorted for display: rated rows by Elo desc, pending last. */
export function sortedStandings(rows: StandingRow[]): StandingRow[] {
	return [...rows].sort((a, b) => {
		if (a.pending !== b.pending) return a.pending ? 1 : -1;
		return (
			(b.elo ?? Number.NEGATIVE_INFINITY) - (a.elo ?? Number.NEGATIVE_INFINITY)
		);
	});
}

/** Whether the terminal ladder is complete enough to carry a final verdict.
 * A stopped run, an unfinished plan, or any unrated player is evidence in
 * progress rather than a completed ranking. */
export function hasFinalTournamentVerdict(live: TournamentLive): boolean {
	return (
		live.lifecycle === "completed" &&
		live.done >= live.total &&
		live.ratingReadiness === "rateable" &&
		live.standings.length === live.players.length &&
		live.standings.every(
			(row) =>
				!row.pending &&
				row.elo !== null &&
				row.elo_ci_low !== null &&
				row.elo_ci_high !== null,
		)
	);
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
		case "internal":
			return "tournament error";
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
