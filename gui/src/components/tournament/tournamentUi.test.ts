import { MantineProvider, Slider } from "@mantine/core";
import { type ComponentType, type ReactNode, createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it } from "vitest";
import type {
	TournamentProvenance,
	TournamentSnapshot,
	TournamentStartedEvent,
} from "../../bindings/generated";
import { useTournamentStore } from "../../stores/tournamentStore";
import HomePage from "../HomePage";
import SettingRow from "../common/SettingRow";
import BotView from "./BotView";
import { tournamentChromeProjection } from "./LiveChip";
import LiveView from "./LiveView";
import MatchupView from "./MatchupView";
import PairedGames, { groupFinishedGames } from "./PairedGames";
import PressableSurface from "./PressableSurface";
import Standings from "./Standings";
import { shortId } from "./theme";

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
		lifecycle: "stopped",
		terminal: {
			outcome: "user_stopped",
			reason: "stopped before the schedule completed",
			at: "2026-07-16 10:01:00",
		},
		rating_readiness: "insufficient_games",
		rating_reason: "not enough successful games",
		paired: true,
		created_at: "2026-07-16 10:00:00",
		started_at: "2026-07-16 10:00:01",
		terminal_at: "2026-07-16 10:01:00",
		last_finished_at: "2026-07-16 10:01:00",
		plan_summary: "all pairs of 2 · 7×7 · 4 games/matchup",
		players: ["a", "b"],
		games_per_matchup: 4,
		progress: {
			planned_slots: 4,
			terminal_slots: 1,
			successful_games: 1,
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

function provenance(
	overrides: Partial<TournamentProvenance> = {},
): TournamentProvenance {
	return {
		tournament_seed: "42",
		game_config_id: "cfg-42",
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
		games_per_matchup: 8,
		mazes_per_matchup: 4,
		max_failures_per_pair: 2,
		seat_policy: "paired",
		methodology: {
			timing_mode: "wait",
			move_timeout_ms: 200,
			preprocessing_timeout_ms: 2_000,
			startup_timeout_ms: 120_000,
			configure_timeout_ms: 5_000,
			network_grace_ms: 50,
			max_parallel: 8,
		},
		interpretation: {
			methodology_version: 1,
			anchor_id: "a",
			anchor_elo: 1_000,
			estimator: "bradley_terry_newton",
			estimator_version: 1,
			draw_weight: 0.5,
			prior_games: 2,
			max_iterations: 100,
			tolerance: 1e-7,
			min_games_per_player: 4,
			uncertainty: "player_minus_anchor_95_percent_normal",
			instance_policy: "shared_maze_per_seat_pair",
		},
		participants: [
			{
				player_id: "a",
				agent_id: "a",
				display_name: "A",
				command: "cargo run --release",
				working_dir: "/bots/a",
				options: { kind: "defaults" },
				declared_version: "1.0",
				fingerprint: {
					kind: "bot_manifest_sha256",
					value: "abc123",
				},
			},
		],
		mutable_files_warning: "mutable files were not frozen",
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
			progress: {
				planned_slots: 80,
				terminal_slots: 47,
				successful_games: 46,
				exhausted_slots: 1,
				failed_attempts: 1,
				running_matches: 0,
			},
			rating_readiness: "rateable",
			rating_reason: null,
			standings: [],
		});
		store.onAborted({
			tournament_id: 1,
			lifecycle: "stopped",
			reason: "bot process exited",
			terminal: {
				outcome: "user_stopped",
				reason: "bot process exited",
				at: "2026-07-16 10:01:00",
			},
			rating_readiness: "provisional",
		});

		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;
		const html = render(createElement(LiveView, { live }));

		expect(html).toContain("Tournament stopped. Results below are partial.");
		expect(html).toContain("Reason: bot process exited");
		expect(html).toContain("partial schedule · not a final ranking");
		expect(html).not.toContain("· done");
	});

	it("gives saved tournaments a back-to-setup path without live-only controls", () => {
		useTournamentStore.getState().openSnapshot(savedSnapshot());
		const saved = useTournamentStore.getState().viewing;
		expect(saved).not.toBeNull();
		if (!saved) return;

		const html = render(createElement(LiveView, { live: saved }));
		expect(html).toContain('aria-label="back to tournament setup"');
		expect(html).toContain("Tournament stopped. Results below are partial.");
		expect(html).toContain("Reason: stopped before the schedule completed");
		expect(html).not.toContain(">Stop<");
		expect(html).not.toContain("min left");
		expect(html).not.toContain("Now playing");
	});

	it("reopens infrastructure failure with its exact disposition and reason", () => {
		useTournamentStore.getState().openSnapshot(
			savedSnapshot({
				lifecycle: "failed",
				terminal: {
					outcome: "infrastructure_failure",
					reason: "session event channel closed",
					at: "2026-07-16 10:01:00",
				},
			}),
		);
		const saved = useTournamentStore.getState().viewing;
		expect(saved).not.toBeNull();
		if (!saved) return;

		const html = render(createElement(LiveView, { live: saved }));
		expect(html).toContain(">failed<");
		expect(html).toContain("Tournament failed. Results below are partial.");
		expect(html).toContain("Reason: session event channel closed");
		expect(html).not.toContain("Tournament stopped.");
	});

	it("keeps the last confirmed view when durable reconciliation fails", () => {
		const store = useTournamentStore.getState();
		store.onStarted(started(), Date.now() - 10_000);
		store.setReconcileFailure(1, "database is temporarily busy");
		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;

		expect(useTournamentStore.getState().reconcileFailure).toEqual({
			tournamentId: 1,
			reason: "database is temporarily busy",
		});
	});

	it("does not infer partial completion for a legacy-unknown row", () => {
		useTournamentStore.getState().openSnapshot(
			savedSnapshot({
				lifecycle: "legacy_unknown",
				terminal: null,
				terminal_at: null,
				rating_readiness: "legacy_unknown",
				rating_reason: null,
			}),
		);
		const saved = useTournamentStore.getState().viewing;
		expect(saved).not.toBeNull();
		if (!saved) return;

		const html = render(createElement(LiveView, { live: saved }));
		expect(html).toContain("Saved tournament status unknown");
		expect(html).toContain("completion is not inferred");
		expect(html).toContain("execution status unknown");
		expect(html).not.toContain("Saved partial results");
	});

	it("keeps completed-with-failures separate from rating readiness", () => {
		useTournamentStore.getState().openSnapshot(
			savedSnapshot({
				lifecycle: "completed",
				terminal: {
					outcome: "completed_with_failures",
					reason: "one schedule slot exhausted its retry budget",
					at: "2026-07-16 10:01:00",
				},
				rating_readiness: "insufficient_games",
				rating_reason: "each player needs four successful games",
				progress: {
					planned_slots: 4,
					terminal_slots: 4,
					successful_games: 3,
					exhausted_slots: 1,
					failed_attempts: 1,
					running_matches: 0,
				},
			}),
		);
		const saved = useTournamentStore.getState().viewing;
		expect(saved).not.toBeNull();
		if (!saved) return;

		const html = render(createElement(LiveView, { live: saved }));
		expect(html).toContain("finished with failures");
		expect(html).toContain("1 failed attempt");
		expect(html).toContain("1 exhausted schedule slot");
		expect(html).toContain("Failure details are refreshing");
		expect(html).toContain("schedule complete");
		expect(html).toContain(
			"not rated: each player needs four successful games",
		);
		expect(html).not.toContain("Saved partial results");
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
				failures: [],
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

	it("renders an exhausted paired leg as terminal rather than pending", () => {
		const games = [
			{
				gameKey: "success",
				matchId: 11,
				player1Id: "a",
				player2Id: "b",
				repetitionIndex: 0,
				ratId: "a",
				player1Score: 5,
				player2Score: 3,
			},
		];
		const failures = [
			{
				failureKey: "attempt:12",
				matchId: 12,
				player1Id: "a",
				player2Id: "b",
				repetitionIndex: 1,
				attemptIndex: 0,
				ratId: "b",
				failingPlayerId: "b",
				kind: "timeout" as const,
				timeoutPhase: "move" as const,
				reason: "timeout: move: Player1",
				exhausted: true,
			},
		];

		const html = render(
			createElement(PairedGames, {
				tournamentId: 7,
				games,
				failures,
				perspectiveId: "a",
				paired: true,
				onOpenGame: () => undefined,
			}),
		);
		expect(html).toContain("1 scored · 1 exhausted");
		expect(html).toContain("b as Rat");
		expect(html).toContain("a as Python");
		expect(html).toContain("attempt 1 exhausted this leg");
		expect(html).toContain("Copy failure evidence");
		expect(html).not.toContain("1/2 legs terminal");
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

	it("starts the live ETA in an honest estimating state", () => {
		useTournamentStore
			.getState()
			.onStarted(started({ total_games: 80, max_parallel: 8 }), Date.now());
		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;

		const html = render(createElement(LiveView, { live }));
		expect(html).toContain("Estimating…");
		expect(html).not.toContain("min left");
	});

	it("makes the consequential stop action explicit and immediately non-repeatable", () => {
		const store = useTournamentStore.getState();
		store.onStarted(started(), Date.now());
		const liveBeforeStop = useTournamentStore.getState().live;
		expect(liveBeforeStop).not.toBeNull();
		if (!liveBeforeStop) return;
		const html = render(createElement(LiveView, { live: liveBeforeStop }));
		expect(html).toContain("Stop and keep completed results");

		store.onStopping({ tournament_id: 1 });
		expect(useTournamentStore.getState()).toMatchObject({
			stopping: true,
			runtimePhase: "stopping",
			runtimeTournamentId: 1,
		});
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
			progress: {
				planned_slots: 8,
				terminal_slots: 3,
				successful_games: 2,
				exhausted_slots: 1,
				failed_attempts: 1,
				running_matches: 0,
			},
			rating_readiness: "insufficient_games",
			rating_reason: "not enough successful games",
			standings: [],
		});
		store.onFinished({
			tournament_id: 1,
			terminal: {
				outcome: "completed_with_failures",
				reason: "one schedule slot exhausted its retry budget",
				at: "2026-07-16 10:01:00",
			},
			rating_readiness: "insufficient_games",
		});

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
			progress: {
				planned_slots: 2,
				terminal_slots: 1,
				successful_games: 1,
				exhausted_slots: 0,
				failed_attempts: 0,
				running_matches: 0,
			},
			rating_readiness: "insufficient_games",
			rating_reason: "not enough successful games",
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
			progress: {
				planned_slots: 8,
				terminal_slots: 2,
				successful_games: 2,
				exhausted_slots: 0,
				failed_attempts: 0,
				running_matches: 0,
			},
			rating_readiness: "insufficient_games",
			rating_reason: "not enough successful games",
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
		const afterStandings = useTournamentStore.getState().live;
		if (!afterStandings) return;
		useTournamentStore.setState({
			live: { ...afterStandings, provenance: provenance() },
		});
		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;

		const html = render(createElement(LiveView, { live }));
		expect(html).toContain("2/4 games — rating appears at 4");
		expect(html).toContain("2/4 to rating");
		expect(html).not.toContain("warming up");
	});

	it("keeps namespace-colliding participant ids visibly distinct", () => {
		const players = ["team-one/greedy", "team-two/greedy", "plain/random"];
		expect(shortId(players[0], players)).toBe("team-one/greedy");
		expect(shortId(players[1], players)).toBe("team-two/greedy");
		expect(shortId(players[2], players)).toBe("random");

		const store = useTournamentStore.getState();
		store.onStarted(
			started({
				players: players.map((player_id) => ({ player_id })),
				anchor_id: players[0],
			}),
			Date.now(),
		);
		const live = useTournamentStore.getState().live;
		expect(live).not.toBeNull();
		if (!live) return;
		const html = render(
			createElement(MatchupView, {
				live,
				a: players[0],
				b: players[1],
			}),
		);
		expect(html).toContain("team-one/greedy");
		expect(html).toContain("team-two/greedy");
	});

	it("renders the durable launch and rating recipe without inventing it", () => {
		const store = useTournamentStore.getState();
		store.openSnapshot(
			savedSnapshot({
				provenance: provenance(),
				anchor_id: "a",
			}),
		);
		const live = useTournamentStore.getState().viewing;
		expect(live).not.toBeNull();
		if (!live) return;
		const html = render(createElement(LiveView, { live }));
		expect(html).toContain("Conditions &amp; methodology");
		expect(html).toContain("cfg-42");
		expect(html).toContain("cargo run --release");
		expect(html).toContain("95% normal interval for player − anchor");
		expect(html).toContain("mutable files were not frozen");
	});

	it("keeps a run-level final-position evidence warning after reopen", () => {
		useTournamentStore.getState().openSnapshot(
			savedSnapshot({
				inspection_warning:
					"Final-position evidence is unavailable for 2 successful games.",
			}),
		);
		const live = useTournamentStore.getState().viewing;
		expect(live).not.toBeNull();
		if (!live) return;

		const html = render(createElement(LiveView, { live }));
		expect(html).toContain("Some final positions cannot be inspected");
		expect(html).toContain(
			"Final-position evidence is unavailable for 2 successful games.",
		);
	});
});
