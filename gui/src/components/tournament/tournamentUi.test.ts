import { MantineProvider, Slider } from "@mantine/core";
import { type ComponentType, type ReactNode, createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it } from "vitest";
import type {
	TournamentSnapshot,
	TournamentStartedEvent,
} from "../../bindings/generated";
import { useTournamentStore } from "../../stores/tournamentStore";
import HomePage from "../HomePage";
import SettingRow from "../common/SettingRow";
import BotView from "./BotView";
import { tournamentChromeProjection } from "./LiveChip";
import LiveView from "./LiveView";
import PairedGames, { groupFinishedGames } from "./PairedGames";
import PressableSurface from "./PressableSurface";
import Standings from "./Standings";

function render(node: ReactNode): string {
	return renderToStaticMarkup(createElement(MantineProvider, null, node));
}

function started(
	overrides: Partial<TournamentStartedEvent> = {},
): TournamentStartedEvent {
	return {
		tournament_id: 1,
		name: "nightly ladder",
		format: "round_robin",
		target: null,
		total_games: 80,
		games_per_matchup: 16,
		paired: true,
		max_parallel: 8,
		anchor_id: "a",
		plan_summary: "all pairs · paired seats",
		players: [{ player_id: "a" }, { player_id: "b" }],
		...overrides,
	};
}

function savedSnapshot(
	overrides: Partial<TournamentSnapshot> = {},
): TournamentSnapshot {
	return {
		tournament_id: 7,
		name: "yesterday's ladder",
		format: "round_robin",
		target: null,
		anchor_id: "a",
		running: false,
		finished: false,
		paired: true,
		created_at: "2026-07-16 10:00:00",
		last_finished_at: "2026-07-16 10:01:00",
		plan_summary: "all pairs of 2 · 7×7 · 4 games/matchup",
		players: ["a", "b"],
		games_per_matchup: 4,
		done: 1,
		total: 4,
		success: 1,
		failure: 0,
		standings: [],
		games: [],
		failures: [],
		...overrides,
	};
}

beforeEach(() => {
	useTournamentStore.setState(useTournamentStore.getInitialState(), true);
});

describe("tournament UI truth", () => {
	it("renders a stopped tournament as partial and keeps its reason", () => {
		const store = useTournamentStore.getState();
		store.onStarted(started(), Date.now() - 10_000);
		store.onStandings({
			tournament_id: 1,
			done: 47,
			total: 80,
			success: 46,
			failure: 1,
			standings: [],
		});
		store.onAborted(1, "bot process exited");

		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;
		const html = render(createElement(LiveView, { live }));

		expect(html).toContain("Tournament stopped. Results below are partial.");
		expect(html).toContain("Reason: bot process exited");
		expect(html).toContain("partial, not a final ranking");
		expect(html).not.toContain("· done");
	});

	it("gives saved tournaments a back-to-setup path without live-only controls", () => {
		useTournamentStore.getState().openSnapshot(savedSnapshot());
		const saved = useTournamentStore.getState().viewing;
		expect(saved).not.toBeNull();
		if (!saved) return;

		const html = render(createElement(LiveView, { live: saved }));
		expect(html).toContain('aria-label="back to tournament setup"');
		expect(html).toContain("Saved partial results");
		expect(html).not.toContain(">Stop<");
		expect(html).not.toContain("min left");
		expect(html).not.toContain("Now playing");
	});

	it("groups out-of-order seat-swapped legs into one maze", () => {
		const games = [
			{
				gameKey: "odd",
				matchId: 12,
				player1Id: "a",
				player2Id: "b",
				repetitionIndex: 3,
				ratId: "b",
				player1Score: 2,
				player2Score: 2,
			},
			{
				gameKey: "even",
				matchId: 11,
				player1Id: "a",
				player2Id: "b",
				repetitionIndex: 2,
				ratId: "a",
				player1Score: 5,
				player2Score: 3,
			},
		];

		const grouped = groupFinishedGames(games, true);
		expect(grouped).toHaveLength(1);
		expect(grouped[0]?.pairIndex).toBe(1);
		expect(grouped[0]?.games.map((game) => game.repetitionIndex)).toEqual([
			2, 3,
		]);

		const html = render(
			createElement(PairedGames, {
				tournamentId: 7,
				games,
				perspectiveId: "a",
				paired: true,
				onOpenGame: () => undefined,
			}),
		);
		expect(html).toContain("Maze 2");
		expect(html).toContain("a won pair 1½–½");
		expect(html).toContain("a as Rat");
		expect(html).toContain("b as Rat");
	});

	it("keeps legacy repetitions as independent games", () => {
		const games = [
			{
				gameKey: "one",
				matchId: null,
				player1Id: "a",
				player2Id: "b",
				repetitionIndex: 0,
				ratId: "a",
				player1Score: 1,
				player2Score: 0,
			},
			{
				gameKey: "two",
				matchId: null,
				player1Id: "a",
				player2Id: "b",
				repetitionIndex: 1,
				ratId: "a",
				player1Score: 0,
				player2Score: 1,
			},
		];
		expect(groupFinishedGames(games, false)).toHaveLength(2);
	});

	it("uses the configured concurrency in the live ETA", () => {
		useTournamentStore
			.getState()
			.onStarted(started({ total_games: 80, max_parallel: 8 }), Date.now());
		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;

		const html = render(createElement(LiveView, { live }));
		expect(html).toContain("~2 min left");
	});

	it("uses the configured matchup denominator in a bot drill-down", () => {
		const store = useTournamentStore.getState();
		store.onStarted(started({ games_per_matchup: 16 }), Date.now());
		for (let matchId = 1; matchId <= 9; matchId++) {
			store.onMatchFinished({
				tournament_id: 1,
				match_id: matchId,
				player1_id: "a",
				player2_id: "b",
				repetition_index: matchId - 1,
				rat_id: matchId % 2 === 1 ? "a" : "b",
				player1_score: 5,
				player2_score: 3,
			});
		}
		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;

		const html = render(createElement(BotView, { live, botId: "a" }));
		expect(html).toContain("9/16");
		expect(html).not.toContain("9/15");
	});

	it("renders shared navigation surfaces as native buttons", () => {
		const html = render(
			createElement(
				PressableSurface,
				{ "aria-label": "Open matchup", motion: "row" },
				"matchup",
			),
		);

		expect(html).toContain("<button");
		expect(html).toContain('type="button"');
		expect(html).toContain('aria-label="Open matchup"');
		expect(html).toContain('data-motion="row"');
	});

	it("names the three Home actions distinctly", () => {
		const html = render(
			createElement(HomePage, {
				onNavigate: () => undefined,
				onOpenTournaments: () => undefined,
			}),
		);

		expect(html).toContain("Play");
		expect(html).toContain("Analyze");
		expect(html).toContain("Open tournaments");
		expect(html).not.toContain(">Start<");
	});

	it("associates shared setting labels with their controls", () => {
		const TestSettingRow = SettingRow as ComponentType<{
			label: string;
			description?: string;
		}>;
		const html = render(
			createElement(
				TestSettingRow,
				{ label: "Move budget", description: "milliseconds per move" },
				createElement("input", { type: "number" }),
			),
		);
		const labelId = html.match(/<p[^>]*id="([^"]+)"[^>]*>Move budget/)?.[1];

		expect(labelId).toBeTruthy();
		expect(html).toContain(`aria-labelledby="${labelId}"`);
		expect(html).toContain("aria-describedby=");
	});

	it("labels the interactive Mantine Slider thumb, not only its root", () => {
		const SliderSettingRow = SettingRow as ComponentType<{
			label: string;
			description?: string;
			controlTarget?: "root" | "slider-thumb";
		}>;
		const html = render(
			createElement(
				SliderSettingRow,
				{
					label: "Wall density",
					description: "Share of passages that are walls",
					controlTarget: "slider-thumb",
				},
				createElement(Slider, { min: 0, max: 1, value: 0.25 }),
			),
		);
		const labelId = html.match(/<p[^>]*id="([^"]+)"[^>]*>Wall density/)?.[1];
		const descriptionId = html.match(
			/<p[^>]*id="([^"]+)"[^>]*>Share of passages/,
		)?.[1];
		const sliderThumb = html.match(/<div(?=[^>]*role="slider")[^>]*>/)?.[0];

		expect(labelId).toBeTruthy();
		expect(descriptionId).toBeTruthy();
		expect(sliderThumb).toContain(`aria-labelledby="${labelId}"`);
		expect(sliderThumb).toContain(`aria-describedby="${descriptionId}"`);
		expect(sliderThumb).toContain('aria-valuenow="0.25"');
	});

	it("labels an incomplete natural finish as partial in the app handoff", () => {
		const store = useTournamentStore.getState();
		store.onStarted(started({ total_games: 8 }), Date.now());
		store.onStandings({
			tournament_id: 1,
			done: 3,
			total: 8,
			success: 2,
			failure: 1,
			standings: [],
		});
		store.onFinished(1);

		const projection = tournamentChromeProjection(
			useTournamentStore.getState(),
		);
		expect(projection?.status).toBe("partial results");
		expect(projection?.progress).toBe("view partial results");
	});

	it("renders gauntlet form with letters as well as color", () => {
		const store = useTournamentStore.getState();
		store.onStarted(started({ target: "a", total_games: 2 }), Date.now());
		store.onStandings({
			tournament_id: 1,
			done: 1,
			total: 2,
			success: 1,
			failure: 0,
			standings: [
				{
					player_id: "a",
					elo: 1000,
					elo_ci_low: 1000,
					elo_ci_high: 1000,
					games: 1,
					pending: true,
				},
				{
					player_id: "b",
					elo: 980,
					elo_ci_low: 940,
					elo_ci_high: 1020,
					games: 1,
					pending: true,
				},
			],
		});
		store.onMatchFinished({
			tournament_id: 1,
			match_id: 1,
			player1_id: "a",
			player2_id: "b",
			repetition_index: 0,
			rat_id: "a",
			player1_score: 5,
			player2_score: 3,
		});
		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;

		const html = render(createElement(Standings, { live }));
		expect(html).toContain("Recent form: W");
		expect(html).toContain(">W</div>");
	});

	it("states the four-game rating threshold and current count", () => {
		const store = useTournamentStore.getState();
		store.onStarted(
			started({ target: "a", total_games: 8, games_per_matchup: 8 }),
			Date.now(),
		);
		store.onStandings({
			tournament_id: 1,
			done: 2,
			total: 8,
			success: 2,
			failure: 0,
			standings: [
				{
					player_id: "a",
					elo: 1000,
					elo_ci_low: 1000,
					elo_ci_high: 1000,
					games: 2,
					pending: true,
				},
				{
					player_id: "b",
					elo: 1000,
					elo_ci_low: 1000,
					elo_ci_high: 1000,
					games: 2,
					pending: true,
				},
			],
		});
		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;

		const html = render(createElement(LiveView, { live }));
		expect(html).toContain("2/4 games — rating appears at 4");
		expect(html).toContain("2/4 to rating");
		expect(html).not.toContain("warming up");
	});
});
