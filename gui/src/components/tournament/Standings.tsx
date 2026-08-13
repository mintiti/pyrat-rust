import { Box, Group, Text, Tooltip } from "@mantine/core";
import { IconAlertTriangle, IconChevronRight } from "@tabler/icons-react";
import type { StandingRow } from "../../bindings/generated";
import type { TournamentLive } from "../../stores/tournamentStore";
import {
	botFailures,
	failureBreakdown,
	resultFor,
	sortedStandings,
	useTournamentStore,
} from "../../stores/tournamentStore";
import PressableSurface from "./PressableSurface";
import { T, pairKey, shortId } from "./theme";

type RatedStanding = StandingRow & {
	elo: number;
	elo_ci_low: number;
	elo_ci_high: number;
};

function hasRating(row: StandingRow): row is RatedStanding {
	return (
		!row.pending &&
		row.elo !== null &&
		row.elo_ci_low !== null &&
		row.elo_ci_high !== null
	);
}

/** Depth-1 evidence: Elo bars with uncertainty whiskers on a shared axis, a dashed
 * anchor tick, and pressable rows (the navigation into matchups). The gauntlet
 * target row is highlighted but not clickable — its breakdown is the rest of
 * the table. */
export default function Standings({ live }: { live: TournamentLive }) {
	const navigate = useTournamentStore((s) => s.navigate);
	const rows = sortedStandings(live.standings);
	const anchorElo = live.provenance?.interpretation.anchor_elo ?? null;
	const minimumGames =
		live.provenance?.interpretation.min_games_per_player ?? null;
	const axisCenter = anchorElo ?? rows.find(hasRating)?.elo ?? 0;

	// Shared axis from the rated rows' CI range, always including the anchor.
	const rated = rows.filter(hasRating);
	let min = axisCenter - 80;
	let max = axisCenter + 80;
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
					const rated = hasRating(r);
					const isTarget = r.player_id === live.target;
					const clickable = !isTarget;
					const health = healthSummary(live, r.player_id);
					const standingEvidence = !rated
						? minimumGames === null
							? `not rated; ${live.ratingReason ?? "rating interpretation was not recorded"}`
							: live.status === "running"
								? `${r.games} of ${minimumGames} games completed; rating appears at ${minimumGames}`
								: `not rated; ${r.games} of ${minimumGames} games completed; ${minimumGames} required`
						: `rank ${i + 1}, Elo ${Math.round(r.elo)} plus or minus ${Math.round((r.elo_ci_high - r.elo_ci_low) / 2)}`;
					const form = live.target
						? gauntletForm(live, r.player_id)
								.map(({ result }) => result)
								.join(", ")
						: "";
					const accessibleEvidence = `${shortId(r.player_id, live.players)}, ${standingEvidence}${form ? `. Recent form: ${form}` : ""}${health ? `. ${health}` : ""}`;
					const row = (
						<Group wrap="nowrap" gap="sm" px="sm" py={7}>
							<Text size="xs" c="dimmed" w={20} ta="right" ff="monospace">
								{rated ? `#${i + 1}` : "·"}
							</Text>
							<Text
								size="sm"
								fw={isTarget ? 700 : 400}
								c={isTarget ? "yellow" : undefined}
								w={150}
								truncate
							>
								{shortId(r.player_id, live.players)}
							</Text>

							{/* Fixed-width health slot — kept constant so the bars
							    below stay left-aligned across rows. */}
							<Box w={34} style={{ textAlign: "right" }}>
								<HealthMarker live={live} botId={r.player_id} />
							</Box>

							{/* bar + whisker track */}
							<Box pos="relative" style={{ flex: 1, height: 18 }}>
								{anchorElo !== null && (
									<Box
										pos="absolute"
										top={0}
										bottom={0}
										style={{
											left: `${pct(anchorElo)}%`,
											borderLeft: "1px dashed #4a4e5c",
										}}
									/>
								)}
								{rated && (
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

							<Text size="xs" ff="monospace" w={104} ta="right">
								{!rated
									? minimumGames === null
										? "not rated"
										: live.status === "running"
											? `${r.games}/${minimumGames} games`
											: `${r.games}/${minimumGames} · not rated`
									: `${Math.round(r.elo)} ±${Math.round((r.elo_ci_high - r.elo_ci_low) / 2)}`}
							</Text>
							<Box w={72} style={{ textAlign: "right" }}>
								{clickable && <RowScent live={live} rowId={r.player_id} />}
							</Box>
							<IconChevronRight
								size={15}
								color={clickable ? T.muted : "transparent"}
								className={
									clickable ? "pyrat-pressable-surface__chevron" : undefined
								}
							/>
						</Group>
					);
					if (isTarget) {
						return (
							<Box
								key={r.player_id}
								title={health || undefined}
								mb={6}
								style={{
									background:
										"linear-gradient(90deg, rgba(240,180,41,.08), var(--mantine-color-dark-5))",
									border: `1px solid ${T.cheeseDim}`,
									borderRadius: 8,
								}}
							>
								{row}
							</Box>
						);
					}
					const destination = live.target
						? `View ${shortId(live.target, live.players)} versus ${shortId(r.player_id, live.players)}`
						: `View ${shortId(r.player_id, live.players)} matchups`;
					return (
						<PressableSurface
							key={r.player_id}
							aria-label={`${destination}. ${accessibleEvidence}`}
							title={health || undefined}
							onClick={() => onRowClick(r.player_id)}
							style={{ marginBottom: 6 }}
						>
							{row}
						</PressableSurface>
					);
				})}
			</div>
			<Text size="xs" c="dimmed" mt="xs">
				{live.anchorId !== null && anchorElo !== null
					? `Elo estimate · 95% range for player − anchor · anchor: ${shortId(live.anchorId, live.players)} = ${anchorElo} (dashed)`
					: "Elo unavailable · saved rating interpretation was not recorded"}
			</Text>
		</Box>
	);
}

/** A quiet "this bot is struggling" marker: a triangle + failure count, with
 * the breakdown on hover. Renders nothing when the bot has no failures, so a
 * healthy tournament shows no clutter. */
function HealthMarker({
	live,
	botId,
}: {
	live: TournamentLive;
	botId: string;
}) {
	const failures = botFailures(live, botId);
	if (failures.length === 0) return null;
	const summary = healthSummary(live, botId);
	return (
		<Tooltip label={summary} withArrow>
			<Group
				role="img"
				aria-label={summary}
				gap={2}
				wrap="nowrap"
				justify="flex-end"
				style={{ color: T.muted }}
			>
				<IconAlertTriangle size={11} />
				<Text size="xs" c="dimmed" ff="monospace">
					{failures.length}
				</Text>
			</Group>
		</Tooltip>
	);
}

function healthSummary(live: TournamentLive, botId: string): string {
	const failures = botFailures(live, botId);
	if (failures.length === 0) return "";
	return `${failures.length} match ${failures.length === 1 ? "failure" : "failures"}: ${failureBreakdown(
		failures,
	)
		.map((breakdown) => `${breakdown.count} ${breakdown.label}`)
		.join(", ")}`;
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
		const form = gauntletForm(live, rowId);
		return (
			<Group
				gap={2}
				justify="flex-end"
				wrap="nowrap"
				role="img"
				aria-label={`Recent form: ${form.map(({ result }) => result).join(", ") || "no games"}`}
			>
				{form.map(({ gameKey, result }) => {
					const color =
						result === "W" ? T.win : result === "L" ? T.loss : T.draw;
					return (
						<Box
							key={gameKey}
							aria-hidden="true"
							w={12}
							h={12}
							style={{
								borderRadius: "50%",
								background: color,
								color: "#15171d",
								display: "grid",
								placeItems: "center",
								fontSize: 8,
								fontWeight: 800,
								lineHeight: 1,
							}}
						>
							{result}
						</Box>
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

function gauntletForm(live: TournamentLive, rowId: string) {
	const target = live.target;
	if (!target || rowId === target) return [];
	const games = live.gamesByPair[pairKey(target, rowId)] ?? [];
	return games.slice(-5).map((game) => ({
		gameKey: game.gameKey,
		result: resultFor(game, target),
	}));
}
