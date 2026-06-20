import { beforeEach, describe, expect, it } from "vitest";
import type {
	TournamentMatchFinishedEvent,
	TournamentMatchStartedEvent,
	TournamentStartedEvent,
} from "../bindings/generated";
import { useTournamentStore } from "./tournamentStore";

function started(): TournamentStartedEvent {
	return {
		tournament_id: 1,
		name: null,
		format: "round_robin",
		target: null,
		total_games: 4,
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
