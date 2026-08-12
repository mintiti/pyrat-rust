import { Box, Group, Text, Title } from "@mantine/core";
import { IconChevronRight } from "@tabler/icons-react";
import type { TournamentLive } from "../../stores/tournamentStore";
import { resultFor, useTournamentStore } from "../../stores/tournamentStore";
import PressableSurface from "./PressableSurface";
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
			<Group justify="space-between" align="baseline" mt="sm" mb="md">
				<Title order={4}>{shortId(botId, live.players)}</Title>
				<Text size="xs" c="dimmed">
					W–L–D · completed/target
				</Text>
			</Group>
			<div>
				{opponents.map((opp) => {
					const games = live.gamesByPair[pairKey(botId, opp)] ?? [];
					const failures = live.failuresByPair[pairKey(botId, opp)] ?? [];
					const exhausted = new Set(
						failures
							.filter((failure) => failure.exhausted)
							.map((failure) => failure.repetitionIndex),
					).size;
					const terminal = games.length + exhausted;
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
						<PressableSurface
							key={opp}
							aria-label={`View ${shortId(botId, live.players)} versus ${shortId(opp, live.players)}. Record: ${w} wins, ${l} losses, ${d} draws. ${terminal} of ${live.gamesPerMatchup} schedule slots terminal.${failures.length > 0 ? ` ${failures.length} failed attempts.` : ""}`}
							style={{ marginBottom: 6 }}
							onClick={() =>
								navigate({ kind: "matchup", a: botId, b: opp, fromBot: botId })
							}
						>
							<Group wrap="nowrap" gap="sm" px="sm" py={8}>
								<Text size="sm" style={{ flex: 1 }}>
									vs {shortId(opp, live.players)}
								</Text>
								<Group gap={3}>
									{games.slice(-5).map((g) => {
										const r = resultFor(g, botId);
										const color =
											r === "W" ? T.win : r === "L" ? T.loss : T.draw;
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
								<Text size="xs" c="dimmed" ff="monospace">
									{w}–{l}–{d} · {terminal}/{live.gamesPerMatchup}
									{failures.length > 0 ? ` · ⚠${failures.length}` : ""}
								</Text>
								<IconChevronRight
									size={15}
									color={T.muted}
									className="pyrat-pressable-surface__chevron"
								/>
							</Group>
						</PressableSurface>
					);
				})}
			</div>
		</Box>
	);
}
