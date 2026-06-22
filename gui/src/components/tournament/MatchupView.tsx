import { Box, Group, SimpleGrid, Text, Title } from "@mantine/core";
import { IconAlertTriangle } from "@tabler/icons-react";
import type {
	MatchFailureRecord,
	TournamentLive,
} from "../../stores/tournamentStore";
import {
	failureBreakdown,
	resultFor,
	useTournamentStore,
} from "../../stores/tournamentStore";
import GameCard from "./GameCard";
import { T, pairKey, shortId } from "./theme";

type Props = {
	live: TournamentLive;
	a: string;
	b: string;
	fromBot?: string;
};

/** Depth 2: one matchup. Record + form dots, the live game if one's running,
 * and the grid of finished-game cards. */
export default function MatchupView({ live, a, b, fromBot }: Props) {
	const navigate = useTournamentStore((s) => s.navigate);
	const games = live.gamesByPair[pairKey(a, b)] ?? [];
	// Perspective: the gauntlet target, else the first-named bot.
	const persp =
		live.target && (a === live.target || b === live.target) ? live.target : a;

	let w = 0;
	let l = 0;
	let d = 0;
	for (const g of games) {
		const r = resultFor(g, persp);
		if (r === "W") w++;
		else if (r === "L") l++;
		else d++;
	}

	const liveMatch = Object.values(live.liveByMatch).find(
		(m) => pairKey(m.player1Id, m.player2Id) === pairKey(a, b),
	);

	const failures = live.failuresByPair[pairKey(a, b)] ?? [];

	return (
		<Box>
			<Title order={4} mt="sm">
				{shortId(a)} vs {shortId(b)}
			</Title>
			<Group gap="sm" mb="md" mt={4}>
				<Text size="sm" c="dimmed" ff="monospace">
					{w}–{l}–{d}
				</Text>
				<Text size="sm" c="dimmed">
					· {games.length}/{live.gamesPerMatchup} games
				</Text>
				<Group gap={3}>
					{games.slice(-5).map((g) => {
						const r = resultFor(g, persp);
						const color = r === "W" ? T.win : r === "L" ? T.loss : T.draw;
						return (
							<Box
								key={g.matchId}
								w={7}
								h={7}
								style={{ borderRadius: "50%", background: color }}
							/>
						);
					})}
				</Group>
			</Group>

			{failures.length > 0 && <MatchupFailures failures={failures} />}

			{liveMatch && (
				<Group
					gap="xs"
					mb="md"
					p="xs"
					style={{
						background: T.panel2,
						border: `1px solid ${T.line}`,
						borderRadius: 8,
					}}
				>
					<Box
						w={6}
						h={6}
						style={{ borderRadius: "50%", background: T.cheese }}
					/>
					<Text size="sm">game in progress</Text>
					<Text size="sm" c="dimmed" ff="monospace">
						turn {liveMatch.turn} · {liveMatch.player1Score}–
						{liveMatch.player2Score}
					</Text>
				</Group>
			)}

			{games.length === 0 ? (
				<Text size="sm" c="dimmed">
					no finished games yet
				</Text>
			) : (
				<SimpleGrid cols={{ base: 3, sm: 4, md: 6 }} spacing="sm">
					{[...games].reverse().map((g) => (
						<GameCard
							key={g.matchId}
							tournamentId={live.tournamentId}
							game={g}
							perspectiveId={persp}
							onOpen={() =>
								navigate({ kind: "game", a, b, matchId: g.matchId, fromBot })
							}
						/>
					))}
				</SimpleGrid>
			)}
		</Box>
	);
}

/** Per-pair failure breakdown, attributed to the implicated bot where known
 * (a null `failingPlayerId` is shown unattributed). Calm by design — only
 * renders when there are failures. */
function MatchupFailures({ failures }: { failures: MatchFailureRecord[] }) {
	const byBot = new Map<string | null, MatchFailureRecord[]>();
	for (const f of failures) {
		byBot.set(f.failingPlayerId, [...(byBot.get(f.failingPlayerId) ?? []), f]);
	}
	const text = [...byBot.entries()]
		.map(([bot, fs]) => {
			const br = failureBreakdown(fs)
				.map((b) => `${b.count} ${b.label}`)
				.join(", ");
			return bot ? `${shortId(bot)}: ${br}` : br;
		})
		.join(" · ");
	return (
		<Group gap={6} mb="md" mt={-6} wrap="nowrap" style={{ color: T.muted }}>
			<IconAlertTriangle size={12} />
			<Text size="xs" c="dimmed">
				{text}
			</Text>
		</Group>
	);
}
