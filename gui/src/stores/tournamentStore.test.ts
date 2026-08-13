import { beforeEach, describe, expect, it } from "vitest";
import type {
	FailureKind,
	TimeoutPhase,
	TournamentMatchFailedEvent,
	TournamentMatchFinishedEvent,
	TournamentMatchStartedEvent,
	TournamentProvenance,
	TournamentSnapshot,
	TournamentStartedEvent,
} from "../bindings/generated";
import {
	type TournamentLive,
	botFailures,
	estimateTournamentEta,
	failureBreakdown,
	hasFinalTournamentVerdict,
	useTournamentStore,
} from "./tournamentStore";

function started(
	overrides: Partial<TournamentStartedEvent> = {},
): TournamentStartedEvent {
	return {
		tournament_id: 1,
		name: null,
		format: "round_robin",
		target: null,
		total_games: 4,
		games_per_matchup: 4,
		paired: true,
		max_parallel: 3,
		anchor_id: "a",
		plan_summary: "",
		players: [{ player_id: "a" }, { player_id: "b" }],
		...overrides,
	};
}

beforeEach(() => {
	useTournamentStore.setState(useTournamentStore.getInitialState(), true);
});

function matchStarted(matchId: number): TournamentMatchStartedEvent {
	return {
		tournament_id: 1,
		match_id: matchId,
		player1_id: "a",
		player2_id: "b",
		repetition_index: 0,
	};
}

function matchFinished(matchId: number): TournamentMatchFinishedEvent {
	return {
		tournament_id: 1,
		match_id: matchId,
		player1_id: "a",
		player2_id: "b",
		repetition_index: 0,
		rat_id: "a",
		player1_score: 5,
		player2_score: 3,
	};
}

function matchFailed(
	matchId: number,
	opts: {
		failing?: string | null;
		kind?: FailureKind;
		phase?: TimeoutPhase | null;
	} = {},
): TournamentMatchFailedEvent {
	return {
		tournament_id: 1,
		match_id: matchId,
		player1_id: "a",
		player2_id: "b",
		repetition_index: 0,
		attempt_index: 0,
		rat_id: "a",
		failing_player_id: opts.failing ?? null,
		kind: opts.kind ?? "timeout",
		timeout_phase: opts.phase ?? "move",
		reason: "timeout: move: Player1",
		exhausted: true,
	};
}

function snapshot(
	overrides: Partial<TournamentSnapshot> = {},
): TournamentSnapshot {
	return {
		tournament_id: 2,
		name: "saved tournament",
		format: "round_robin",
		target: null,
		anchor_id: "a",
		running: false,
		lifecycle: "completed",
		terminal: {
			outcome: "completed",
			reason: null,
			at: "2026-07-17 10:01:00",
		},
		rating_readiness: "rateable",
		rating_reason: null,
		paired: true,
		created_at: "2026-07-17 10:00:00",
		started_at: "2026-07-17 10:00:01",
		terminal_at: "2026-07-17 10:01:00",
		last_finished_at: "2026-07-17 10:01:00",
		plan_summary: "all pairs of 2 · 7×7 · 2 games/matchup",
		players: ["a", "b"],
		games_per_matchup: 2,
		progress: {
			planned_slots: 2,
			terminal_slots: 2,
			successful_games: 2,
			exhausted_slots: 0,
			failed_attempts: 0,
			running_matches: 0,
		},
		standings: [],
		games: [],
		failures: [],
		slots: [],
		inspection_warning: null,
		...overrides,
	};
}

function provenance(): TournamentProvenance {
	return {
		tournament_seed: "42",
		game_config_id: "cfg",
		factory: {
			width: 7,
			height: 7,
			max_turns: 100,
			wall_density: 0.7,
			mud_density: 0.1,
			mud_range: 5,
			connected: true,
			symmetric: true,
			player_start: "corners",
			cheese_count: 12,
			cheese_symmetric: true,
		},
		games_per_matchup: 4,
		mazes_per_matchup: 2,
		max_failures_per_pair: 3,
		seat_policy: "paired",
		methodology: {
			timing_mode: "wait",
			move_timeout_ms: 200,
			preprocessing_timeout_ms: 2_000,
			startup_timeout_ms: 120_000,
			configure_timeout_ms: 5_000,
			network_grace_ms: 50,
			max_parallel: 3,
		},
		interpretation: {
			methodology_version: 1,
			anchor_id: "a",
			anchor_elo: 1_000,
			estimator: "bradley_terry_newton",
			estimator_version: 1,
			draw_weight: 0.5,
			prior_games: 2,
			max_iterations: 1_000,
			tolerance: 0.001,
			min_games_per_player: 4,
			uncertainty: "player_minus_anchor_95_percent_normal",
			instance_policy: "shared_maze_per_seat_pair",
		},
		participants: [],
		mutable_files_warning: "mutable files were not frozen",
	};
}

describe("tournamentStore live-row guards", () => {
	beforeEach(() => {
		useTournamentStore.getState().onStarted(started(), 0);
	});

	it("ignores a delayed MatchStarted for an already-finished match", () => {
		const s = useTournamentStore.getState();
		s.onMatchStarted(matchStarted(7));
		expect(useTournamentStore.getState().live?.liveByMatch[7]).toBeDefined();

		s.onMatchFinished(matchFinished(7));
		expect(useTournamentStore.getState().live?.liveByMatch[7]).toBeUndefined();

		// The independent, lossy MatchStarted stream re-delivers match 7 after
		// it finished. Without the guard this re-inserts a zombie "now playing"
		// row at turn 0 that never clears.
		s.onMatchStarted(matchStarted(7));
		expect(useTournamentStore.getState().live?.liveByMatch[7]).toBeUndefined();
	});

	it("clears a failed match's row and tombstones it against a late MatchStarted", () => {
		const s = useTournamentStore.getState();
		s.onMatchStarted(matchStarted(7));
		expect(useTournamentStore.getState().live?.liveByMatch[7]).toBeDefined();

		// A failed match emits no scored event; onMatchFailed must drop the row.
		s.onMatchFailed(matchFailed(7));
		expect(useTournamentStore.getState().live?.liveByMatch[7]).toBeUndefined();

		// A delayed MatchStarted for the now-terminated match must not revive it.
		s.onMatchStarted(matchStarted(7));
		expect(useTournamentStore.getState().live?.liveByMatch[7]).toBeUndefined();
	});

	it("tombstones a failure that arrives before its MatchStarted (order-independent)", () => {
		const s = useTournamentStore.getState();
		// Terminal event first (streams are unordered): no row should ever form.
		s.onMatchFailed(matchFailed(8));
		s.onMatchStarted(matchStarted(8));
		expect(useTournamentStore.getState().live?.liveByMatch[8]).toBeUndefined();
	});

	it("accumulates per-bot failures attributed by failing_player_id", () => {
		const s = useTournamentStore.getState();
		s.onMatchFailed(
			matchFailed(10, { failing: "a", kind: "timeout", phase: "move" }),
		);
		s.onMatchFailed(
			matchFailed(11, { failing: "a", kind: "timeout", phase: "move" }),
		);
		s.onMatchFailed(
			matchFailed(12, { failing: "b", kind: "disconnected", phase: null }),
		);
		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;
		// Attribution follows the resolved failing bot, not the canonical pair.
		expect(botFailures(live, "a")).toHaveLength(2);
		expect(botFailures(live, "b")).toHaveLength(1);
		expect(failureBreakdown(botFailures(live, "a"))).toEqual([
			{ label: "move timeout", count: 2 },
		]);
	});

	it("ignores MatchStarted / NowPlaying once the tournament is not running", () => {
		const s = useTournamentStore.getState();
		s.onFinished({
			tournament_id: 1,
			terminal: {
				outcome: "completed",
				reason: null,
				at: "2026-07-17 10:01:00",
			},
			rating_readiness: "rateable",
		});
		expect(useTournamentStore.getState().live?.status).toBe("finished");

		s.onMatchStarted(matchStarted(9));
		expect(useTournamentStore.getState().live?.liveByMatch[9]).toBeUndefined();

		s.onNowPlaying({
			tournament_id: 1,
			match_id: 9,
			turn: 3,
			player1_score: 1,
			player2_score: 2,
		});
		expect(useTournamentStore.getState().live?.liveByMatch[9]).toBeUndefined();
	});
});

describe("tournamentStore launch ↔ live navigation", () => {
	it("initializes the bot roster once, preserves empty, and sanitizes rescans", () => {
		const store = useTournamentStore.getState();
		store.reconcileLaunchBots(["a", "b"]);
		expect(useTournamentStore.getState().launchDraft).toMatchObject({
			selectedBotIds: ["a", "b"],
			rosterInitialized: true,
		});

		store.updateLaunchDraft({ selectedBotIds: [], target: null });
		store.reconcileLaunchBots(["a", "b"]);
		expect(useTournamentStore.getState().launchDraft.selectedBotIds).toEqual(
			[],
		);

		store.updateLaunchDraft({ selectedBotIds: ["a"], target: "a" });
		store.reconcileLaunchBots(["b"]);
		expect(useTournamentStore.getState().launchDraft).toMatchObject({
			selectedBotIds: [],
			target: null,
		});
	});

	it("keeps edited launch conditions when the launch surface remounts", () => {
		const store = useTournamentStore.getState();
		const recipe = provenance();
		store.initializeLaunchDefaults(recipe.factory, {
			mazes_per_matchup: 2,
			move_timeout_ms: 200,
			preprocessing_timeout_ms: 2_000,
			max_parallel: 3,
		});
		store.updateLaunchDraft({
			name: "after-mud-fix",
			methodology: {
				mazes_per_matchup: 5,
				move_timeout_ms: 450,
				preprocessing_timeout_ms: 2_000,
				max_parallel: 2,
			},
		});
		store.initializeLaunchDefaults(recipe.factory, {
			mazes_per_matchup: 2,
			move_timeout_ms: 200,
			preprocessing_timeout_ms: 2_000,
			max_parallel: 3,
		});

		expect(useTournamentStore.getState().launchDraft).toMatchObject({
			name: "after-mud-fix",
			methodology: { mazes_per_matchup: 5, move_timeout_ms: 450 },
		});
	});

	it("estimates from observed terminal-slot throughput and admits a stall", () => {
		useTournamentStore.getState().onStarted(started({ total_games: 12 }), 0);
		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;
		const observed = {
			...live,
			done: 4,
			startedAt: 0,
			lastProgressAt: 40_000,
			runningMatches: 2,
		};
		expect(estimateTournamentEta(observed, 40_000)).toEqual({
			kind: "ready",
			seconds: 80,
		});
		expect(
			estimateTournamentEta({ ...observed, runningMatches: 0 }, 100_000),
		).toEqual({ kind: "waiting" });
	});

	it("onStarted shows the live screen; showLaunch / showLive toggle it", () => {
		const s = useTournamentStore.getState();
		s.onStarted(started(), 0);
		expect(useTournamentStore.getState().screen).toBe("live");

		// Leave the detail view for setup. The runner remains app-owned.
		s.showLaunch();
		expect(useTournamentStore.getState().screen).toBe("launch");
		expect(useTournamentStore.getState().nav).toEqual({ kind: "overview" });

		// ...and return to the still-running one (chip / banner).
		s.showLive();
		expect(useTournamentStore.getState().screen).toBe("live");
		// The live data is untouched by the nav round-trip.
		expect(useTournamentStore.getState().live?.tournamentId).toBe(1);
	});

	it("keeps active events flowing while a different saved tournament is open", () => {
		const s = useTournamentStore.getState();
		s.onStarted(started(), 0);
		s.openSnapshot(snapshot());

		expect(useTournamentStore.getState()).toMatchObject({
			screen: "live",
			viewing: { tournamentId: 2, origin: "stored" },
			live: { tournamentId: 1, origin: "active", done: 0 },
		});

		s.onMatchFinished(matchFinished(7));
		s.onStandings({
			tournament_id: 1,
			progress: {
				planned_slots: 4,
				terminal_slots: 1,
				successful_games: 1,
				exhausted_slots: 0,
				failed_attempts: 0,
				running_matches: 0,
			},
			rating_readiness: "insufficient_games",
			rating_reason: "not enough successful games",
			standings: [],
		});

		const state = useTournamentStore.getState();
		expect(state.viewing).toMatchObject({ tournamentId: 2, done: 2 });
		expect(state.live).toMatchObject({ tournamentId: 1, done: 1 });
		expect(state.live?.gamesByPair["a|b"]).toHaveLength(1);
	});

	it("keeps event ratings unavailable until durable interpretation is attached", () => {
		const s = useTournamentStore.getState();
		s.onStarted(started(), 0);
		s.onStandings({
			tournament_id: 1,
			progress: {
				planned_slots: 4,
				terminal_slots: 1,
				successful_games: 1,
				exhausted_slots: 0,
				failed_attempts: 0,
				running_matches: 0,
			},
			rating_readiness: "rateable",
			rating_reason: null,
			standings: [
				{
					player_id: "a",
					elo: 1_000,
					elo_ci_low: 1_000,
					elo_ci_high: 1_000,
					games: 1,
					pending: false,
				},
			],
		});

		expect(useTournamentStore.getState().live?.standings[0]).toMatchObject({
			elo: null,
			elo_ci_low: null,
			elo_ci_high: null,
			pending: true,
		});
	});

	it("applies live disconnected readiness once durable interpretation is attached", () => {
		const s = useTournamentStore.getState();
		s.onStarted(started(), 0);
		s.restoreActive(
			snapshot({ tournament_id: 1, running: true, provenance: provenance() }),
		);
		s.onStandings({
			tournament_id: 1,
			progress: {
				planned_slots: 4,
				terminal_slots: 4,
				successful_games: 4,
				exhausted_slots: 0,
				failed_attempts: 0,
				running_matches: 0,
			},
			rating_readiness: "disconnected_graph",
			rating_reason: "successful games do not connect every participant",
			standings: [],
		});

		expect(useTournamentStore.getState().live).toMatchObject({
			ratingReadiness: "disconnected_graph",
			ratingReason: "successful games do not connect every participant",
		});
	});

	it("opens the active row without replacing its event-fed model", () => {
		const s = useTournamentStore.getState();
		s.onStarted(started({ max_parallel: 8 }), 123);
		s.showLaunch();
		s.openSnapshot(snapshot({ tournament_id: 1, running: true }));

		expect(useTournamentStore.getState()).toMatchObject({
			screen: "live",
			viewing: null,
			live: {
				tournamentId: 1,
				origin: "active",
				maxParallel: 8,
				startedAt: 123,
			},
		});
	});

	it("keeps live, restored, and historical projections semantically identical", () => {
		const durable = snapshot({
			tournament_id: 1,
			running: true,
			lifecycle: "running",
			terminal: null,
			terminal_at: null,
			rating_readiness: "insufficient_games",
			rating_reason: "each player needs four successful games",
			provenance: provenance(),
			progress: {
				planned_slots: 4,
				terminal_slots: 2,
				successful_games: 1,
				exhausted_slots: 1,
				failed_attempts: 1,
				running_matches: 0,
			},
			games: [
				{
					attempt_id: 41,
					match_id: 7,
					player1_id: "a",
					player2_id: "b",
					repetition_index: 0,
					rat_id: "a",
					player1_score: 5,
					player2_score: 3,
				},
			],
			failures: [
				{
					attempt_id: 42,
					match_id: 8,
					player1_id: "a",
					player2_id: "b",
					repetition_index: 1,
					attempt_index: 0,
					rat_id: "b",
					failing_player_id: "a",
					kind: "timeout",
					timeout_phase: "move",
					reason: "timeout: move: Player2",
					exhausted: true,
				},
			],
		});

		const project = (): TournamentLive => {
			const live = useTournamentStore.getState().live;
			expect(live).not.toBeNull();
			if (!live) throw new Error("expected active tournament");
			return live;
		};
		const durableProjection = (live: TournamentLive) => ({
			ratingReadiness: live.ratingReadiness,
			ratingReason: live.ratingReason,
			done: live.done,
			total: live.total,
			success: live.success,
			exhausted: live.exhausted,
			failure: live.failure,
			anchorId: live.anchorId,
			provenance: live.provenance,
			gamesByPair: live.gamesByPair,
			failuresByPair: live.failuresByPair,
		});

		const store = useTournamentStore.getState();
		store.onStarted(started(), 0);
		store.onMatchFinished(matchFinished(7));
		store.onMatchFailed({
			...matchFailed(8, { failing: "a", kind: "timeout", phase: "move" }),
			repetition_index: 1,
			rat_id: "b",
			reason: "timeout: move: Player2",
		});
		store.reconcileSnapshot(durable);
		const liveProjection = project();

		useTournamentStore.setState(useTournamentStore.getInitialState(), true);
		useTournamentStore.getState().restoreActive(durable);
		const restoredProjection = project();

		useTournamentStore.setState(useTournamentStore.getInitialState(), true);
		useTournamentStore.getState().openSnapshot({
			...durable,
			running: false,
			lifecycle: "stopped",
			terminal: {
				outcome: "user_stopped",
				reason: "release-proof comparison",
				at: "2026-07-17 10:01:00",
			},
			terminal_at: "2026-07-17 10:01:00",
		});
		const historical = useTournamentStore.getState().viewing;
		expect(historical).not.toBeNull();
		if (!historical) return;
		const historicalProjection = durableProjection(historical);

		expect(durableProjection(restoredProjection)).toEqual(
			durableProjection(liveProjection),
		);
		expect(historicalProjection).toEqual(durableProjection(liveProjection));
	});

	it("reopens legacy saved results even when no replay id was persisted", () => {
		useTournamentStore.getState().openSnapshot(
			snapshot({
				lifecycle: "legacy_unknown",
				terminal: null,
				rating_readiness: "legacy_unknown",
				rating_reason: null,
				progress: {
					planned_slots: 2,
					terminal_slots: 1,
					successful_games: 1,
					exhausted_slots: 0,
					failed_attempts: 0,
					running_matches: 0,
				},
				games: [
					{
						attempt_id: 41,
						match_id: null,
						player1_id: "a",
						player2_id: "b",
						repetition_index: 0,
						rat_id: "a",
						player1_score: 5,
						player2_score: 3,
					},
				],
			}),
		);

		const viewing = useTournamentStore.getState().viewing;
		expect(viewing).toMatchObject({
			origin: "stored",
			status: "partial",
			maxParallel: null,
		});
		expect(viewing?.gamesByPair["a|b"][0]).toMatchObject({
			gameKey: "attempt:41",
			matchId: null,
		});
	});

	it("keeps launch and preparation activity outside the launch view lifetime", () => {
		const s = useTournamentStore.getState();
		s.beginLaunch();
		s.onPreparing({ done: 0, total: 2, current: "a" });
		s.navigate({ kind: "bot", botId: "a" });

		expect(useTournamentStore.getState()).toMatchObject({
			starting: true,
			preparing: { done: 0, total: 2, current: "a" },
		});

		s.onStarted(started(), 123);
		expect(useTournamentStore.getState()).toMatchObject({
			starting: false,
			preparing: null,
		});
	});

	it("keeps a failed warmup explanation until the user edits or retries", () => {
		const s = useTournamentStore.getState();
		s.setLaunchFailure("bot a failed its setup handshake");
		s.openSnapshot(snapshot());
		s.showLaunch();

		expect(useTournamentStore.getState().launchFailure).toBe(
			"bot a failed its setup handshake",
		);

		s.beginLaunch();
		expect(useTournamentStore.getState().launchFailure).toBeNull();
	});

	it("scopes a retained terminal signal to the backend-owned tournament", () => {
		const s = useTournamentStore.getState();
		s.onRuntimeStatus({ phase: "running", tournament_id: 2 });

		s.onFinished({
			tournament_id: 1,
			terminal: {
				outcome: "completed",
				reason: null,
				at: "2026-07-17 10:01:00",
			},
			rating_readiness: "rateable",
		});
		expect(useTournamentStore.getState()).toMatchObject({
			runtimePhase: "running",
			runtimeTournamentId: 2,
		});

		s.onFinished({
			tournament_id: 2,
			terminal: {
				outcome: "completed",
				reason: null,
				at: "2026-07-17 10:01:00",
			},
			rating_readiness: "rateable",
		});
		expect(useTournamentStore.getState()).toMatchObject({
			runtimePhase: "idle",
			runtimeTournamentId: null,
		});
	});

	it("preserves the configured matchup count and executor concurrency", () => {
		useTournamentStore
			.getState()
			.onStarted(started({ games_per_matchup: 16, max_parallel: 8 }), 0);

		expect(useTournamentStore.getState().live).toMatchObject({
			gamesPerMatchup: 16,
			maxParallel: 8,
		});
	});

	it("scopes terminal events and makes their handoff one-shot", () => {
		const s = useTournamentStore.getState();
		s.onStarted(started(), 0);
		const finished = {
			tournament_id: 1,
			terminal: {
				outcome: "completed" as const,
				reason: null,
				at: "2026-07-17 10:01:00",
			},
			rating_readiness: "rateable" as const,
		};

		s.onFinished({ ...finished, tournament_id: 99 });
		expect(useTournamentStore.getState().live?.status).toBe("running");

		s.onFinished(finished);
		expect(useTournamentStore.getState().live).toMatchObject({
			status: "finished",
			endedAt: expect.any(Number),
		});
		expect(useTournamentStore.getState().terminalNotice).toMatchObject({
			tournamentId: 1,
			status: "finished",
		});

		s.showLive();
		expect(useTournamentStore.getState().terminalNotice).toBeNull();
	});

	it("keeps the stop reason and marks its ladder as non-final", () => {
		const s = useTournamentStore.getState();
		s.onStarted(started(), 0);
		s.onAborted({
			tournament_id: 1,
			lifecycle: "stopped",
			reason: "bot process exited",
			terminal: {
				outcome: "user_stopped",
				reason: "bot process exited",
				at: "2026-07-17 10:01:00",
			},
			rating_readiness: "provisional",
		});

		const state = useTournamentStore.getState();
		expect(state.live).toMatchObject({
			status: "aborted",
			abortReason: "bot process exited",
		});
		expect(state.terminalNotice).toMatchObject({
			status: "aborted",
			reason: "bot process exited",
		});
		expect(state.live && hasFinalTournamentVerdict(state.live)).toBe(false);
	});
});
