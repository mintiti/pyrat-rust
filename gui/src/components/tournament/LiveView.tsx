import {
	ActionIcon,
	Badge,
	Box,
	Button,
	Group,
	Progress,
	Stack,
	Text,
} from "@mantine/core";
import { IconArrowLeft } from "@tabler/icons-react";
import { useEffect, useState } from "react";
import { commands } from "../../bindings";
import type { LiveMatch, TournamentLive } from "../../stores/tournamentStore";
import { useTournamentStore } from "../../stores/tournamentStore";
import BotView from "./BotView";
import GameView from "./GameView";
import Hero from "./Hero";
import MatchupView from "./MatchupView";
import Standings from "./Standings";
import { T, shortId } from "./theme";

const EST_SECONDS_PER_GAME = 6.5;
const MAX_PARALLEL = 4;

export default function LiveView({ live }: { live: TournamentLive }) {
	const nav = useTournamentStore((s) => s.nav);
	const back = useTournamentStore((s) => s.back);

	// Ticking elapsed clock.
	const [now, setNow] = useState(() => Date.now());
	useEffect(() => {
		if (live.status !== "running") return;
		const id = setInterval(() => setNow(Date.now()), 1000);
		return () => clearInterval(id);
	}, [live.status]);

	const elapsedSec = Math.floor((now - live.startedAt) / 1000);
	const elapsed = `${Math.floor(elapsedSec / 60)}m${String(elapsedSec % 60).padStart(2, "0")}s`;
	const finished = live.status !== "running";
	const remaining = Math.max(0, live.total - live.done);
	const etaMin = Math.max(
		1,
		Math.ceil((remaining * EST_SECONDS_PER_GAME) / MAX_PARALLEL / 60),
	);

	const inFlight = Object.values(live.liveByMatch);
	const statusBadge =
		live.status === "running" ? (
			<Badge color="yellow" variant="outline" leftSection={<PulseDot />}>
				running
			</Badge>
		) : live.status === "finished" ? (
			<Badge color="green" variant="outline">
				finished
			</Badge>
		) : (
			<Badge color="red" variant="outline">
				stopped
			</Badge>
		);

	const backLabel =
		nav.kind === "game"
			? "back to matchup"
			: nav.kind === "matchup" && nav.fromBot
				? `back to ${shortId(nav.fromBot)}`
				: nav.kind === "matchup" || nav.kind === "bot"
					? "back to standings"
					: "";

	return (
		<Box p="lg" style={{ maxWidth: 980, margin: "0 auto" }}>
			<Group gap="sm" align="center" mb={4}>
				{nav.kind !== "overview" && (
					<ActionIcon
						variant="default"
						radius="xl"
						onClick={back}
						title={backLabel}
					>
						<IconArrowLeft size={16} />
					</ActionIcon>
				)}
				<Text fw={700} size="xl">
					{live.name ?? `#${live.tournamentId}`}
				</Text>
				{statusBadge}
				<Text size="xs" c="dimmed">
					· {elapsed}
				</Text>
				{live.status === "running" && (
					<Button
						size="compact-xs"
						variant="subtle"
						color="red"
						ml="auto"
						onClick={() => commands.stopTournament()}
					>
						Stop
					</Button>
				)}
			</Group>

			<Progress
				value={(100 * live.done) / Math.max(live.total, 1)}
				color="yellow"
				size="sm"
				mb={4}
				striped={live.status === "running"}
				animated={live.status === "running"}
			/>
			<Text size="xs" c="dimmed" mb="md">
				<span style={{ fontFamily: "monospace" }}>
					{live.done}/{live.total}
				</span>{" "}
				games · {live.planSummary}
				{finished ? " · done" : ` · ~${etaMin} min left`}
			</Text>

			{nav.kind === "overview" && (
				<>
					<Hero live={live} />
					<NowPlaying matches={inFlight} running={live.status === "running"} />
					<Box mt="md">
						<Standings live={live} />
					</Box>
				</>
			)}
			{nav.kind === "bot" && <BotView live={live} botId={nav.botId} />}
			{nav.kind === "matchup" && (
				<MatchupView live={live} a={nav.a} b={nav.b} fromBot={nav.fromBot} />
			)}
			{nav.kind === "game" && (
				<GameView tournamentId={live.tournamentId} matchId={nav.matchId} />
			)}
		</Box>
	);
}

/** A small pulsing dot — the "something is happening" signal. */
function PulseDot({ color = T.cheese }: { color?: string }) {
	return (
		<Box
			w={7}
			h={7}
			style={{
				borderRadius: "50%",
				background: color,
				animation: "pulse 1.2s infinite",
			}}
		/>
	);
}

/** In-flight matches as a compact stacked list (one row each), instead of a
 * run-on line. Each row's ticking turn counter is the per-match liveness. */
function NowPlaying({
	matches,
	running,
}: {
	matches: LiveMatch[];
	running: boolean;
}) {
	if (!running && matches.length === 0) return null;
	return (
		<Box mt="md">
			<Text size="xs" tt="uppercase" c="dimmed" fw={700} mb="xs">
				Now playing{matches.length > 0 ? ` (${matches.length})` : ""}
			</Text>
			{matches.length === 0 ? (
				<Text size="sm" c="dimmed">
					waiting for the next matches to start…
				</Text>
			) : (
				<Stack gap={6}>
					{matches.map((m) => (
						<Group
							key={m.matchId}
							gap="sm"
							wrap="nowrap"
							px="sm"
							py={6}
							style={{
								background: T.panel2,
								border: `1px solid ${T.line}`,
								borderRadius: 8,
							}}
						>
							<PulseDot />
							<Text size="sm" style={{ flex: 1 }}>
								{shortId(m.player1Id)} vs {shortId(m.player2Id)}
							</Text>
							<Text size="sm" c="dimmed" ff="monospace">
								turn {m.turn}
							</Text>
							<Text size="sm" ff="monospace" w={56} ta="right">
								{m.player1Score}–{m.player2Score}
							</Text>
						</Group>
					))}
				</Stack>
			)}
		</Box>
	);
}
