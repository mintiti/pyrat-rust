import { Group, Paper, Text } from "@mantine/core";
import { useAtomValue } from "jotai";
import pythonIconUrl from "../assets/sprites/players/python/neutral.png";
import ratIconUrl from "../assets/sprites/players/rat/neutral.png";
import type { MatchWinner } from "../bindings/generated";
import { SLOT_PALETTE } from "../lib/botPalette";
import { botsAtom, resolveBotName } from "../stores/botConfigAtom";
import { useMatchStore } from "../stores/matchStore";

const ACCENT: Record<MatchWinner, string> = {
	Player1: SLOT_PALETTE.Player1.mantine,
	Player2: SLOT_PALETTE.Player2.mantine,
	Draw: "gray",
};

export default function ResultBanner() {
	const result = useMatchStore((s) => s.result);
	const mazeConfig = useMatchStore((s) => s.mazeConfig);
	const player1BotId = useMatchStore((s) => s.player1BotId);
	const player2BotId = useMatchStore((s) => s.player2BotId);
	const bots = useAtomValue(botsAtom);

	if (!result) return null;

	const p1Name = resolveBotName(player1BotId, bots, "Rat");
	const p2Name = resolveBotName(player2BotId, bots, "Python");
	const maxTurns = mazeConfig?.max_turns ?? 300;

	const color = ACCENT[result.winner];

	return (
		<Paper
			px="sm"
			py={6}
			withBorder
			style={{
				borderLeftWidth: 3,
				borderLeftColor: `var(--mantine-color-${color}-5)`,
				flexShrink: 0,
			}}
		>
			<Group gap="sm" wrap="nowrap">
				{result.winner === "Draw" ? (
					<>
						<img src={ratIconUrl} alt="Rat" width={16} height={16} />
						<img src={pythonIconUrl} alt="Python" width={16} height={16} />
						<Text size="sm" fw={600} c="dimmed">
							Draw
						</Text>
						<Text size="sm" c="dimmed">
							{p1Name} vs {p2Name}
						</Text>
					</>
				) : (
					<>
						<img
							src={result.winner === "Player1" ? ratIconUrl : pythonIconUrl}
							alt={result.winner === "Player1" ? "Rat" : "Python"}
							width={16}
							height={16}
						/>
						<Text size="sm" fw={600} c={`${color}.4`}>
							{result.winner === "Player1" ? p1Name : p2Name} wins
						</Text>
					</>
				)}
				<Text size="sm" c="dimmed">
					{result.player1_score.toFixed(1)} - {result.player2_score.toFixed(1)}
				</Text>
				<Text size="xs" c="dimmed">
					{result.turns_played} / {maxTurns} turns
				</Text>
			</Group>
		</Paper>
	);
}
