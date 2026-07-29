import { Group, Paper, Text } from "@mantine/core";
import type { TournamentLive } from "../../stores/tournamentStore";
import {
	MIN_GAMES_FOR_RATING,
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

	if (!live.target) {
		if (!terminal) return null;
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
						{finalVerdict ? "Finished" : "Partial results"}
					</Text>
					<Text size="sm" c="dimmed">
						{finalVerdict
							? `${live.done} games · final standings`
							: `${live.done}/${live.total} games · not a final ranking`}
					</Text>
				</Group>
			</Paper>
		);
	}

	const rows = sortedStandings(live.standings);
	const mine = rows.find((r) => r.player_id === live.target);
	const rank = mine ? rows.indexOf(mine) + 1 : 0;

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
					{shortId(live.target)}
				</Text>
				{!mine || mine.pending ? (
					<Text size="sm" c="dimmed">
						{terminal
							? `not rated — ${mine?.games ?? 0}/${MIN_GAMES_FOR_RATING} completed games; ${MIN_GAMES_FOR_RATING} required`
							: `${mine?.games ?? 0}/${MIN_GAMES_FOR_RATING} games — rating appears at ${MIN_GAMES_FOR_RATING}`}
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
							{Math.round(mine.elo)}{" "}
							<Text span size="sm" c="dimmed">
								±{Math.round((mine.elo_ci_high - mine.elo_ci_low) / 2)}
							</Text>
						</Text>
						<Text size="xs" c="dimmed" ml="auto">
							{terminal
								? finalVerdict
									? `final · ${live.done} games`
									: `partial · ${live.done}/${live.total} games · not final`
								: `${mine.games} of ${live.total} games in`}
						</Text>
					</>
				)}
			</Group>
		</Paper>
	);
}
