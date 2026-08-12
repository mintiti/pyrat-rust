import { Box, Group, Text, Title } from "@mantine/core";
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
import PairedGames from "./PairedGames";
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

	const liveMatches = Object.values(live.liveByMatch).filter(
		(m) => pairKey(m.player1Id, m.player2Id) === pairKey(a, b),
	);

	const failures = live.failuresByPair[pairKey(a, b)] ?? [];
	const exhaustedLegs = new Set(
		failures
			.filter((failure) => failure.exhausted)
			.map((failure) => failure.repetitionIndex),
	).size;
	const terminalSlots = new Set([
		...games.map((game) => game.repetitionIndex),
		...failures
			.filter((failure) => failure.exhausted)
			.map((failure) => failure.repetitionIndex),
	]).size;

	return (
		<Box>
			<Title order={4} mt="sm">
				{shortId(a)} vs {shortId(b)}
			</Title>
			<Group gap="sm" mb="md" mt={4}>
				<Text size="sm" c="dimmed" ff="monospace">
					W–L–D {w}–{l}–{d}
				</Text>
				<Text size="sm" c="dimmed">
					· {terminalSlots}/{live.gamesPerMatchup} schedule slots ·{" "}
					{games.length} successful
					{exhaustedLegs > 0 ? ` · ${exhaustedLegs} exhausted` : ""}
				</Text>
				<Group gap={3}>
					{games.slice(-5).map((g) => {
						const r = resultFor(g, persp);
						const color = r === "W" ? T.win : r === "L" ? T.loss : T.draw;
						return (
							<Box
								key={g.gameKey}
								w={7}
								h={7}
								style={{ borderRadius: "50%", background: color }}
							/>
						);
					})}
				</Group>
			</Group>

			{failures.length > 0 && <MatchupFailures failures={failures} />}

			{liveMatches.map((liveMatch) => (
				<Group
					key={liveMatch.matchId}
					gap="xs"
					mb="xs"
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
						pair {Math.floor(liveMatch.repetitionIndex / 2) + 1} · turn{" "}
						{liveMatch.turn} · {liveMatch.player1Score}–{liveMatch.player2Score}
					</Text>
				</Group>
			))}

			{games.length === 0 && exhaustedLegs === 0 ? (
				<Text size="sm" c="dimmed">
					no terminal schedule legs yet
				</Text>
			) : (
				<PairedGames
					tournamentId={live.tournamentId}
					games={games}
					failures={failures}
					perspectiveId={persp}
					paired={live.paired}
					onOpenGame={(matchId) =>
						navigate({ kind: "game", a, b, matchId, fromBot })
					}
				/>
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
