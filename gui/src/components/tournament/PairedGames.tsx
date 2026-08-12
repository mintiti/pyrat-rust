import { Group, Paper, SimpleGrid, Stack, Text } from "@mantine/core";
import type {
	FinishedGame,
	MatchFailureRecord,
} from "../../stores/tournamentStore";
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
	exhaustedFailures: MatchFailureRecord[],
	perspectiveId: string,
	paired: boolean,
): string {
	const expectedLegs = paired ? 2 : 1;
	const exhaustedLegs = new Set(
		exhaustedFailures.map((failure) => failure.repetitionIndex),
	).size;
	const terminalLegs = new Set([
		...pair.games.map((game) => game.repetitionIndex),
		...exhaustedFailures.map((failure) => failure.repetitionIndex),
	]).size;
	if (terminalLegs < expectedLegs) {
		return `${terminalLegs}/${expectedLegs} legs terminal`;
	}
	if (exhaustedLegs > 0) {
		return `${pair.games.length} scored · ${exhaustedLegs} exhausted`;
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
	failures,
	perspectiveId,
	paired,
	onOpenGame,
}: {
	tournamentId: number;
	games: FinishedGame[];
	failures: MatchFailureRecord[];
	perspectiveId: string;
	paired: boolean;
	onOpenGame: (matchId: number) => void;
}) {
	const pairsByIndex = new Map(
		groupFinishedGames(games, paired).map((pair) => [pair.pairIndex, pair]),
	);
	const exhaustedByPair = new Map<number, MatchFailureRecord[]>();
	for (const failure of failures.filter((item) => item.exhausted)) {
		const pairIndex = paired
			? Math.floor(failure.repetitionIndex / 2)
			: failure.repetitionIndex;
		exhaustedByPair.set(pairIndex, [
			...(exhaustedByPair.get(pairIndex) ?? []),
			failure,
		]);
		if (!pairsByIndex.has(pairIndex)) {
			pairsByIndex.set(pairIndex, { pairIndex, games: [] });
		}
	}
	const pairs = [...pairsByIndex.values()].sort(
		(a, b) => b.pairIndex - a.pairIndex,
	);
	return (
		<Stack gap="sm">
			{pairs.map((pair) => {
				const exhaustedFailures = exhaustedByPair.get(pair.pairIndex) ?? [];
				return (
					<Paper
						key={pair.pairIndex}
						withBorder
						p="xs"
						radius="md"
						bg={T.panel2}
					>
						<Group justify="space-between" gap="sm" mb="xs">
							<Text size="xs" fw={700} tt="uppercase" c="dimmed">
								{paired
									? `Maze ${pair.pairIndex + 1}`
									: `Game ${pair.pairIndex + 1}`}
							</Text>
							<Text size="xs" ff="monospace">
								{pairVerdict(pair, exhaustedFailures, perspectiveId, paired)}
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
											onOpen={
												matchId === null ? null : () => onOpenGame(matchId)
											}
										/>
									</div>
								);
							})}
							{exhaustedFailures.map((failure) => (
								<Paper
									key={failure.failureKey}
									withBorder
									p="xs"
									radius="sm"
									style={{ borderColor: T.loss }}
								>
									<Text size="xs" c="dimmed" mb={4}>
										{shortId(failure.ratId)} as Rat · exhausted leg
									</Text>
									<Text size="xs">{failure.reason}</Text>
								</Paper>
							))}
						</SimpleGrid>
					</Paper>
				);
			})}
		</Stack>
	);
}
