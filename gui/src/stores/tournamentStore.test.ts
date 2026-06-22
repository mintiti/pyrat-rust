import { beforeEach, describe, expect, it } from "vitest";
import type {
	FailureKind,
	TimeoutPhase,
	TournamentMatchFailedEvent,
	TournamentMatchFinishedEvent,
	TournamentMatchStartedEvent,
	TournamentStartedEvent,
} from "../bindings/generated";
import {
	botFailures,
	failureBreakdown,
	useTournamentStore,
} from "./tournamentStore";

function started(): TournamentStartedEvent {
	return {
		tournament_id: 1,
		name: null,
		format: "round_robin",
		target: null,
		total_games: 4,
		games_per_matchup: 4,
		anchor_id: "a",
		plan_summary: "",
		players: [{ player_id: "a" }, { player_id: "b" }],
	};
}

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
		failing_player_id: opts.failing ?? null,
		kind: opts.kind ?? "timeout",
		timeout_phase: opts.phase ?? "move",
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
		s.onFinished();
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

		// Leave the live view to start another tournament...
		s.showLaunch();
		expect(useTournamentStore.getState().screen).toBe("launch");
		expect(useTournamentStore.getState().nav).toEqual({ kind: "overview" });

		// ...and return to the still-running one (chip / banner).
		s.showLive();
		expect(useTournamentStore.getState().screen).toBe("live");
		// The live data is untouched by the nav round-trip.
		expect(useTournamentStore.getState().live?.tournamentId).toBe(1);
	});
});
