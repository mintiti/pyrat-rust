import { Box, Group, Text, Title } from "@mantine/core";
import { IconChevronRight } from "@tabler/icons-react";
import type { TournamentLive } from "../../stores/tournamentStore";
import { resultFor, useTournamentStore } from "../../stores/tournamentStore";
import { T, pairKey, shortId } from "./theme";

/** Depth 1.5 (round-robin only): one bot's matchups. The standings row for a
 * round-robin bot fans out to every opponent, so this rung sits between
 * standings and a single matchup. */
export default function BotView({
	live,
	botId,
}: { live: TournamentLive; botId: string }) {
	const navigate = useTournamentStore((s) => s.navigate);
	const opponents = live.players.filter((p) => p !== botId);

	return (
		<Box>
			<Title order={4} mt="sm" mb="md">
				{shortId(botId)}
			</Title>
			<div>
				{opponents.map((opp) => {
					const games = live.gamesByPair[pairKey(botId, opp)] ?? [];
					let w = 0;
					let l = 0;
					let d = 0;
					for (const g of games) {
						const r = resultFor(g, botId);
						if (r === "W") w++;
						else if (r === "L") l++;
						else d++;
					}
					return (
						<Group
							key={opp}
							wrap="nowrap"
							gap="sm"
							px="sm"
							py={8}
							mb={6}
							style={{
								background: T.panel2,
								border: `1px solid ${T.line}`,
								borderRadius: 8,
								cursor: "pointer",
							}}
							onClick={() =>
								navigate({ kind: "matchup", a: botId, b: opp, fromBot: botId })
							}
						>
							<Text size="sm" style={{ flex: 1 }}>
								vs {shortId(opp)}
							</Text>
							<Group gap={3}>
								{games.slice(-5).map((g) => {
									const r = resultFor(g, botId);
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
							<Text size="xs" c="dimmed" ff="monospace">
								{w}–{l}–{d} · {games.length}/15
							</Text>
							<IconChevronRight size={15} color={T.muted} />
						</Group>
					);
				})}
			</div>
		</Box>
	);
}
