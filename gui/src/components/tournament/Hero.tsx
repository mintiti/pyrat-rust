import { Group, Paper, Text } from "@mantine/core";
import type { TournamentLive } from "../../stores/tournamentStore";
import { sortedStandings } from "../../stores/tournamentStore";
import { T, shortId } from "./theme";

const ord = (n: number) =>
	`${n}${n === 1 ? "st" : n === 2 ? "nd" : n === 3 ? "rd" : "th"}`;

/** Depth-1 headline. In a gauntlet it answers "how strong is it?" first — the
 * target's rank + Elo ± CI — and becomes the verdict when the tournament
 * finishes. Round-robin has no single subject, so the hero only appears at the
 * end with a summary. */
export default function Hero({ live }: { live: TournamentLive }) {
	const finished = live.status === "finished";

	if (!live.target) {
		if (!finished) return null;
		return (
			<Paper
				withBorder
				p="sm"
				radius="md"
				bg={T.panel2}
				style={{ borderColor: T.cheeseDim }}
			>
				<Group gap="md" align="baseline">
					<Text fw={700} size="lg">
						Finished
					</Text>
					<Text size="sm" c="dimmed">
						{live.done} games · one tournament row in the store
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
			style={finished ? { borderColor: T.cheeseDim } : undefined}
		>
			<Group gap="lg" align="baseline">
				<Text fw={700} c="yellow">
					{shortId(live.target)}
				</Text>
				{!mine || mine.pending ? (
					<Text size="sm" c="dimmed">
						warming up — first games running
					</Text>
				) : (
					<>
						<Text fw={700} size="xl">
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
							{finished
								? `final · ${live.done} games · one tournament row in the store`
								: `${mine.games} of ${live.total} games in`}
						</Text>
					</>
				)}
			</Group>
		</Paper>
	);
}
