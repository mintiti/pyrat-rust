import { Box, Button, Group, Stack, Text, Title } from "@mantine/core";
import { toDisplayState } from "../../stores/matchStore";
import MazeRenderer from "../MazeRenderer";
import { T, shortId } from "./theme";
import { useGameReplay } from "./useGameReplay";

/** Depth 3: one game. The final position large, the verdict, and a (stubbed)
 * entry to the full replay — which is the parked gui-next replay-loading item. */
export default function GameView({
	tournamentId,
	matchId,
}: {
	tournamentId: number;
	matchId: number;
}) {
	const replay = useGameReplay(tournamentId, matchId);

	if (!replay) {
		return (
			<Text size="sm" c="dimmed" mt="sm">
				loading…
			</Text>
		);
	}
	if (replay.kind === "missing") {
		return (
			<Box mt="sm">
				<Title order={4} mb="sm">
					Game unavailable
				</Title>
				<Text size="sm" c="dimmed">
					{replay.reason}
				</Text>
			</Box>
		);
	}

	const winner = replay.winner;
	const verdictColor =
		winner === null ? T.draw : winner === replay.player1_id ? T.rat : T.python;

	return (
		<Box mt="sm">
			<Title order={4} mb="md">
				{shortId(replay.player1_id)} vs {shortId(replay.player2_id)}
			</Title>
			<Group align="flex-start" gap="xl">
				<Box w={320} h={300}>
					<MazeRenderer
						gameState={toDisplayState(replay.final_state)}
						staticBoard
						hideScoreStrip
					/>
				</Box>
				<Stack gap="sm" pt={4}>
					<Text fw={700} size="lg" c={verdictColor}>
						{winner === null ? "draw" : `${shortId(winner)} wins`}
					</Text>
					<Text size="xl" ff="monospace">
						{replay.player1_score} – {replay.player2_score}
					</Text>
					<Text size="sm" c="dimmed">
						{replay.turns} turns · final position
					</Text>
					{/* Full replay is the parked gui-next replay-loading item. */}
					<Button color="yellow" size="sm" variant="light" disabled>
						Open replay (coming soon)
					</Button>
				</Stack>
			</Group>
		</Box>
	);
}
