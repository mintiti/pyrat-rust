import { Group, Paper, Text } from "@mantine/core";
import type { TournamentLive } from "../../stores/tournamentStore";
import {
	hasFinalTournamentVerdict,
	sortedStandings,
} from "../../stores/tournamentStore";
import { T, shortId } from "./theme";

const ord = (n: number) =>
	`${n}${n === 1 ? "st" : n === 2 ? "nd" : n === 3 ? "rd" : "th"}`;

/** Depth-1 headline. In a gauntlet it answers "how strong is it?" first — the
 * target's rank + Elo ± CI — and becomes the verdict when the tournament
 * finishes. Round-robin has no single subject, so the hero only appears at the
 * end with a summary. */
export default function Hero({ live }: { live: TournamentLive }) {
	const terminal = live.status !== "running";
	const finalVerdict = hasFinalTournamentVerdict(live);
	const minimumGames =
		live.provenance?.interpretation.min_games_per_player ?? null;

	if (!live.target) {
		if (!terminal) return null;
		const scheduleCompleted = live.lifecycle === "completed";
		const scheduleInterrupted =
			live.lifecycle === "stopped" || live.lifecycle === "failed";
		return (
			<Paper
				withBorder
				p="sm"
				radius="md"
				bg={T.panel2}
				style={{ borderColor: finalVerdict ? T.cheeseDim : T.line }}
			>
				<Group gap="md" align="baseline">
					<Text fw={700} size="lg">
						{scheduleCompleted
							? live.terminalOutcome === "completed_with_failures"
								? "Finished with failures"
								: "Finished"
							: scheduleInterrupted
								? "Partial results"
								: "Results · status unknown"}
					</Text>
					<Text size="sm" c="dimmed">
						{finalVerdict
							? `${live.success} successful games · final standings`
							: scheduleCompleted
								? `${live.done}/${live.total} schedule slots · no final rating`
								: scheduleInterrupted
									? `${live.done}/${live.total} schedule slots · not a final ranking`
									: `${live.done}/${live.total} recorded terminal slots · completion unknown`}
					</Text>
				</Group>
			</Paper>
		);
	}

	const rows = sortedStandings(live.standings);
	const mine = rows.find((r) => r.player_id === live.target);
	const rank = mine ? rows.indexOf(mine) + 1 : 0;
	const rating =
		mine !== undefined &&
		!mine.pending &&
		mine.elo !== null &&
		mine.elo_ci_low !== null &&
		mine.elo_ci_high !== null
			? {
					elo: mine.elo,
					low: mine.elo_ci_low,
					high: mine.elo_ci_high,
					games: mine.games,
				}
			: null;

	return (
		<Paper
			withBorder
			p="sm"
			radius="md"
			bg={T.panel2}
			style={finalVerdict ? { borderColor: T.cheeseDim } : undefined}
		>
			<Group gap="lg" align="baseline">
				<Text fw={700} c="yellow">
					{shortId(live.target, live.players)}
				</Text>
				{rating === null ? (
					<Text size="sm" c="dimmed">
						{minimumGames === null
							? `not rated — ${live.ratingReason ?? "saved rating interpretation unavailable"}`
							: terminal
								? `not rated — ${mine?.games ?? 0}/${minimumGames} completed games; ${minimumGames} required`
								: `${mine?.games ?? 0}/${minimumGames} games — rating appears at ${minimumGames}`}
					</Text>
				) : (
					<>
						<Text fw={700} size="xl">
							{terminal && !finalVerdict && (
								<Text span size="xs" c="dimmed">
									provisional{" "}
								</Text>
							)}
							{ord(rank)}{" "}
							<Text span size="sm" c="dimmed">
								of {rows.length}
							</Text>
						</Text>
						<Text fw={700} size="xl" ff="monospace">
							{Math.round(rating.elo)}{" "}
							<Text span size="sm" c="dimmed">
								±{Math.round((rating.high - rating.low) / 2)}
							</Text>
						</Text>
						<Text size="xs" c="dimmed" ml="auto">
							{terminal
								? finalVerdict
									? `final · ${live.success} successful games`
									: live.lifecycle === "completed"
										? `schedule complete · ${live.success} successful games · no final rating`
										: `partial · ${live.done}/${live.total} schedule slots · not final`
								: `${rating.games} of ${live.total} games in`}
						</Text>
					</>
				)}
			</Group>
		</Paper>
	);
}
