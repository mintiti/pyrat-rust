import { beforeEach, describe, expect, it } from "vitest";
import type {
	FailureKind,
	TimeoutPhase,
	TournamentMatchFailedEvent,
	TournamentMatchFinishedEvent,
	TournamentMatchStartedEvent,
	TournamentSnapshot,
	TournamentStartedEvent,
} from "../bindings/generated";
import {
	botFailures,
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
		rat_id: "a",
		failing_player_id: opts.failing ?? null,
		kind: opts.kind ?? "timeout",
		timeout_phase: opts.phase ?? "move",
		reason: "timeout: move: Player1",
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
		finished: true,
		paired: true,
		created_at: "2026-07-17 10:00:00",
		last_finished_at: "2026-07-17 10:01:00",
		plan_summary: "all pairs of 2 · 7×7 · 2 games/matchup",
		players: ["a", "b"],
		games_per_matchup: 2,
		done: 2,
		total: 2,
		success: 2,
		failure: 0,
		standings: [],
		games: [],
		failures: [],
		...overrides,
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
		s.onFinished(1);
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
			done: 1,
			total: 4,
			success: 1,
			failure: 0,
			standings: [],
		});

		const state = useTournamentStore.getState();
		expect(state.viewing).toMatchObject({ tournamentId: 2, done: 2 });
		expect(state.live).toMatchObject({ tournamentId: 1, done: 1 });
		expect(state.live?.gamesByPair["a|b"]).toHaveLength(1);
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

	it("reopens legacy saved results even when no replay id was persisted", () => {
		useTournamentStore.getState().openSnapshot(
			snapshot({
				done: 1,
				total: 2,
				finished: false,
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

		s.onFinished(99);
		expect(useTournamentStore.getState().live?.status).toBe("running");

		s.onFinished(1);
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
		s.onAborted(1, "bot process exited");

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
