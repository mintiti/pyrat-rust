import { Group, Paper, SimpleGrid, Stack, Text } from "@mantine/core";
import type { FinishedGame } from "../../stores/tournamentStore";
import { resultFor } from "../../stores/tournamentStore";
import GameCard from "./GameCard";
import { T, shortId } from "./theme";

export interface FinishedGamePair {
	pairIndex: number;
	games: FinishedGame[];
}

/** Group the paired schedule's `2k` / `2k+1` repetitions into one shared-maze
 * unit. Legacy schedules keep one visible unit per repetition. Input order is
 * irrelevant because live events can arrive out of order. */
export function groupFinishedGames(
	games: FinishedGame[],
	paired: boolean,
): FinishedGamePair[] {
	const grouped = new Map<number, FinishedGame[]>();
	for (const game of games) {
		const pairIndex = paired
			? Math.floor(game.repetitionIndex / 2)
			: game.repetitionIndex;
		grouped.set(pairIndex, [...(grouped.get(pairIndex) ?? []), game]);
	}
	return [...grouped.entries()]
		.map(([pairIndex, pairGames]) => ({
			pairIndex,
			games: pairGames.sort((a, b) => a.repetitionIndex - b.repetitionIndex),
		}))
		.sort((a, b) => b.pairIndex - a.pairIndex);
}

function pointsLabel(points: number): string {
	if (points === 0.5) return "½";
	if (points % 1 === 0.5) return `${Math.floor(points)}½`;
	return String(points);
}

function pairVerdict(
	pair: FinishedGamePair,
	perspectiveId: string,
	paired: boolean,
): string {
	const expectedLegs = paired ? 2 : 1;
	if (pair.games.length < expectedLegs) {
		return `${pair.games.length}/${expectedLegs} legs complete`;
	}
	let mine = 0;
	for (const game of pair.games) {
		const result = resultFor(game, perspectiveId);
		mine += result === "W" ? 1 : result === "D" ? 0.5 : 0;
	}
	const theirs = pair.games.length - mine;
	const opponent =
		pair.games[0]?.player1Id === perspectiveId
			? pair.games[0]?.player2Id
			: pair.games[0]?.player1Id;
	if (!opponent) return "pair complete";
	const score = `${pointsLabel(mine)}–${pointsLabel(theirs)}`;
	if (mine === theirs) return `pair drawn ${score}`;
	return `${shortId(mine > theirs ? perspectiveId : opponent)} won pair ${score}`;
}

export default function PairedGames({
	tournamentId,
	games,
	perspectiveId,
	paired,
	onOpenGame,
}: {
	tournamentId: number;
	games: FinishedGame[];
	perspectiveId: string;
	paired: boolean;
	onOpenGame: (matchId: number) => void;
}) {
	const pairs = groupFinishedGames(games, paired);
	return (
		<Stack gap="sm">
			{pairs.map((pair) => (
				<Paper key={pair.pairIndex} withBorder p="xs" radius="md" bg={T.panel2}>
					<Group justify="space-between" gap="sm" mb="xs">
						<Text size="xs" fw={700} tt="uppercase" c="dimmed">
							{paired
								? `Maze ${pair.pairIndex + 1}`
								: `Game ${pair.pairIndex + 1}`}
						</Text>
						<Text size="xs" ff="monospace">
							{pairVerdict(pair, perspectiveId, paired)}
						</Text>
					</Group>
					<SimpleGrid cols={paired ? { base: 1, xs: 2 } : 1} spacing="xs">
						{pair.games.map((game) => {
							const matchId = game.matchId;
							return (
								<div key={game.gameKey}>
									<Text size="xs" c="dimmed" mb={4}>
										{shortId(game.ratId)} as Rat ·{" "}
										{shortId(
											game.ratId === game.player1Id
												? game.player2Id
												: game.player1Id,
										)}{" "}
										as Python
									</Text>
									<GameCard
										tournamentId={tournamentId}
										game={game}
										perspectiveId={perspectiveId}
										onOpen={matchId === null ? null : () => onOpenGame(matchId)}
									/>
								</div>
							);
						})}
					</SimpleGrid>
				</Paper>
			))}
		</Stack>
	);
}
