import { Box, Group, Text } from "@mantine/core";
import { IconChevronRight } from "@tabler/icons-react";
import type { StandingRow } from "../../bindings/generated";
import type { TournamentLive } from "../../stores/tournamentStore";
import {
	resultFor,
	sortedStandings,
	useTournamentStore,
} from "../../stores/tournamentStore";
import { T, pairKey, shortId } from "./theme";

const ANCHOR_ELO = 1000;

/** Depth-1 evidence: Elo bars with 95% CI whiskers on a shared axis, a dashed
 * anchor tick, and pressable rows (the navigation into matchups). The gauntlet
 * target row is highlighted but not clickable — its breakdown is the rest of
 * the table. */
export default function Standings({ live }: { live: TournamentLive }) {
	const navigate = useTournamentStore((s) => s.navigate);
	const rows = sortedStandings(live.standings);

	// Shared axis from the rated rows' CI range, always including the anchor.
	const rated = rows.filter((r) => !r.pending);
	let min = ANCHOR_ELO - 80;
	let max = ANCHOR_ELO + 80;
	for (const r of rated) {
		min = Math.min(min, r.elo_ci_low);
		max = Math.max(max, r.elo_ci_high);
	}
	const pad = (max - min) * 0.08 || 40;
	min -= pad;
	max += pad;
	const pct = (v: number) =>
		Math.max(0, Math.min(100, (100 * (v - min)) / (max - min)));

	const onRowClick = (id: string) => {
		if (id === live.target) return; // target row is its own thing
		if (live.target) {
			navigate({ kind: "matchup", a: live.target, b: id });
		} else {
			navigate({ kind: "bot", botId: id });
		}
	};

	return (
		<Box>
			<Text size="xs" tt="uppercase" c="dimmed" fw={700} mb="xs">
				Standings
			</Text>
			<div>
				{rows.map((r, i) => {
					const isTarget = r.player_id === live.target;
					const clickable = !isTarget;
					return (
						<Group
							key={r.player_id}
							wrap="nowrap"
							gap="sm"
							px="sm"
							py={7}
							mb={6}
							style={{
								background: isTarget
									? "linear-gradient(90deg, rgba(240,180,41,.08), var(--mantine-color-dark-5))"
									: T.panel2,
								border: `1px solid ${isTarget ? T.cheeseDim : T.line}`,
								borderRadius: 8,
								cursor: clickable ? "pointer" : "default",
							}}
							onClick={() => onRowClick(r.player_id)}
						>
							<Text size="xs" c="dimmed" w={20} ta="right" ff="monospace">
								{r.pending ? "·" : i + 1}
							</Text>
							<Text
								size="sm"
								fw={isTarget ? 700 : 400}
								c={isTarget ? "yellow" : undefined}
								w={150}
								truncate
							>
								{shortId(r.player_id)}
							</Text>

							{/* bar + whisker track */}
							<Box pos="relative" style={{ flex: 1, height: 18 }}>
								<Box
									pos="absolute"
									top={0}
									bottom={0}
									style={{
										left: `${pct(ANCHOR_ELO)}%`,
										borderLeft: "1px dashed #4a4e5c",
									}}
								/>
								{!r.pending && (
									<>
										<Box
											pos="absolute"
											style={{
												top: 5,
												left: 0,
												width: `${pct(r.elo)}%`,
												height: 8,
												borderRadius: 4,
												background: isTarget ? T.cheese : "#3d4150",
											}}
										/>
										<Whisker lo={pct(r.elo_ci_low)} hi={pct(r.elo_ci_high)} />
									</>
								)}
							</Box>

							<Text size="xs" ff="monospace" w={96} ta="right">
								{r.pending
									? "warming up"
									: `${Math.round(r.elo)} ±${Math.round((r.elo_ci_high - r.elo_ci_low) / 2)}`}
							</Text>
							<Box w={56} style={{ textAlign: "right" }}>
								{clickable && <RowScent live={live} rowId={r.player_id} />}
							</Box>
							<IconChevronRight
								size={15}
								color={clickable ? T.muted : "transparent"}
							/>
						</Group>
					);
				})}
			</div>
			<Text size="xs" c="dimmed" mt="xs">
				Elo, 95% CI · anchor: {shortId(live.anchorId)} = 1000 (dashed)
			</Text>
		</Box>
	);
}

function Whisker({ lo, hi }: { lo: number; hi: number }) {
	return (
		<Box
			pos="absolute"
			style={{
				top: 8.5,
				left: `${lo}%`,
				width: `${Math.max(hi - lo, 0.5)}%`,
				height: 1.5,
				background: T.muted,
				opacity: 0.85,
			}}
		/>
	);
}

/** Destination scent: in a gauntlet, the last-5 form dots from the target's
 * perspective; in round-robin, the bot's overall W-L-D. */
function RowScent({ live, rowId }: { live: TournamentLive; rowId: string }) {
	const target = live.target;
	if (target) {
		const games = live.gamesByPair[pairKey(target, rowId)] ?? [];
		const last5 = games.slice(-5);
		return (
			<Group gap={3} justify="flex-end" wrap="nowrap">
				{last5.map((g) => {
					const res = resultFor(g, target);
					const color = res === "W" ? T.win : res === "L" ? T.loss : T.draw;
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
		);
	}
	let w = 0;
	let l = 0;
	let d = 0;
	for (const [key, games] of Object.entries(live.gamesByPair)) {
		if (!key.split("|").includes(rowId)) continue;
		for (const g of games) {
			const res = resultFor(g, rowId);
			if (res === "W") w++;
			else if (res === "L") l++;
			else d++;
		}
	}
	return (
		<Text size="xs" c="dimmed" ff="monospace">
			{w}–{l}–{d}
		</Text>
	);
}
