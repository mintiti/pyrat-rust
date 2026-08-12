import {
	ActionIcon,
	Badge,
	Box,
	Button,
	Group,
	Paper,
	Progress,
	SimpleGrid,
	Text,
} from "@mantine/core";
import { IconArrowLeft } from "@tabler/icons-react";
import { useEffect, useState } from "react";
import type { LiveMatch, TournamentLive } from "../../stores/tournamentStore";
import {
	estimateTournamentEta,
	hasFinalTournamentVerdict,
	useTournamentStore,
} from "../../stores/tournamentStore";
import BotView from "./BotView";
import FailureSummary from "./FailureSummary";
import GameView from "./GameView";
import Hero from "./Hero";
import MatchupView from "./MatchupView";
import ProvenancePanel from "./ProvenancePanel";
import Standings from "./Standings";
import { T, shortId } from "./theme";
import { confirmTournamentStop } from "./tournamentActions";

export default function LiveView({ live }: { live: TournamentLive }) {
	const nav = useTournamentStore((s) => s.nav);
	const back = useTournamentStore((s) => s.back);
	const showLaunch = useTournamentStore((s) => s.showLaunch);
	const stopping = useTournamentStore((s) => s.stopping);
	const requestStop = useTournamentStore((s) => s.requestStop);
	const reconcileFailure = useTournamentStore((s) => s.reconcileFailure);
	const retryReconcile = useTournamentStore((s) => s.retryReconcile);
	const active = live.origin === "active";
	const stop = async () => {
		if (await confirmTournamentStop()) await requestStop();
	};

	// Ticking elapsed clock.
	const [now, setNow] = useState(() => Date.now());
	useEffect(() => {
		if (live.status !== "running") return;
		const id = setInterval(() => setNow(Date.now()), 1000);
		return () => clearInterval(id);
	}, [live.status]);

	const elapsedSec = Math.max(
		0,
		Math.floor(((live.endedAt ?? now) - live.startedAt) / 1000),
	);
	const elapsed = `${Math.floor(elapsedSec / 60)}m${String(elapsedSec % 60).padStart(2, "0")}s`;
	const createdLabel = live.createdAt
		? new Date(`${live.createdAt.replace(" ", "T")}Z`).toLocaleString()
		: null;
	const terminal = live.status !== "running";
	const finalVerdict = hasFinalTournamentVerdict(live);
	const interrupted =
		live.lifecycle === "stopped" || live.lifecycle === "failed";
	const eta = estimateTournamentEta(live, now);
	const etaText =
		eta?.kind === "estimating"
			? "Estimating…"
			: eta?.kind === "waiting"
				? "Waiting for terminal work…"
				: eta?.kind === "ready"
					? eta.seconds < 60
						? "Under 1 min left"
						: `About ${Math.ceil(eta.seconds / 60)} min left`
					: null;
	const staleProjection =
		reconcileFailure?.tournamentId === live.tournamentId
			? reconcileFailure
			: null;

	const inFlight = Object.values(live.liveByMatch);
	const statusBadge =
		stopping && active ? (
			<Badge color="orange" variant="outline">
				stopping
			</Badge>
		) : live.status === "running" ? (
			<Badge color="yellow" variant="outline" leftSection={<PulseDot />}>
				running
			</Badge>
		) : live.lifecycle === "completed" ? (
			<Badge color="green" variant="outline">
				{live.terminalOutcome === "completed_with_failures"
					? "finished with failures"
					: "finished"}
			</Badge>
		) : live.lifecycle === "stopped" ? (
			<Badge color="red" variant="outline">
				stopped
			</Badge>
		) : live.lifecycle === "failed" ? (
			<Badge color="red" variant="outline">
				failed
			</Badge>
		) : (
			<Badge color="orange" variant="outline">
				status unknown
			</Badge>
		);

	const backLabel =
		nav.kind === "game"
			? "back to matchup"
			: nav.kind === "matchup" && nav.fromBot
				? `back to ${shortId(nav.fromBot, live.players)}`
				: nav.kind === "matchup" || nav.kind === "bot"
					? "back to standings"
					: "back to tournament setup";

	return (
		<Box p="lg" style={{ maxWidth: 980, margin: "0 auto" }}>
			<Group gap="sm" align="center" mb={4}>
				<ActionIcon
					variant="default"
					radius="xl"
					onClick={nav.kind === "overview" ? showLaunch : back}
					title={backLabel}
					aria-label={backLabel}
				>
					<IconArrowLeft size={16} />
				</ActionIcon>
				<Text fw={700} size="xl">
					{live.name ?? `#${live.tournamentId}`}
				</Text>
				{statusBadge}
				<Text size="xs" c="dimmed">
					· {active ? elapsed : (createdLabel ?? "saved tournament")}
				</Text>
				<Group gap="xs" ml="auto">
					{active && live.status === "running" && (
						<Button
							size="compact-sm"
							variant="subtle"
							color="red"
							onClick={() => void stop()}
							disabled={stopping}
							loading={stopping}
						>
							{stopping ? "Stopping…" : "Stop and keep completed results"}
						</Button>
					)}
				</Group>
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
				schedule slots · {live.success} successful game
				{live.success === 1 ? "" : "s"}
				{live.exhausted > 0 ? ` · ${live.exhausted} exhausted` : ""} ·{" "}
				{live.planSummary}
				{!terminal
					? ` · ${etaText ?? "Running"}`
					: live.lifecycle === "completed"
						? finalVerdict
							? " · complete · final ranking"
							: ` · schedule complete · ${ratingReadinessLabel(live.ratingReadiness, live.ratingReason)}`
						: interrupted
							? " · partial schedule · not a final ranking"
							: " · execution status unknown · not a final ranking"}
			</Text>
			{live.status === "aborted" && (
				<Paper
					withBorder
					p="sm"
					radius="md"
					mb="md"
					bg={T.panel2}
					style={{ borderColor: T.loss }}
				>
					<Text size="sm" fw={700}>
						{live.lifecycle === "failed"
							? "Tournament failed. Results below are partial."
							: "Tournament stopped. Results below are partial."}
					</Text>
					{live.abortReason && (
						<Text size="xs" c="dimmed" mt={2}>
							Reason: {live.abortReason}
						</Text>
					)}
				</Paper>
			)}
			{live.status === "partial" && (
				<Paper withBorder p="sm" radius="md" mb="md" bg={T.panel2}>
					<Text size="sm" fw={700}>
						Saved tournament status unknown
					</Text>
					<Text size="xs" c="dimmed" mt={2}>
						This row does not carry a durable terminal disposition. Its recorded
						games remain inspectable, but completion is not inferred.
					</Text>
				</Paper>
			)}
			{staleProjection && (
				<Paper withBorder p="sm" radius="md" mb="md" bg={T.panel2}>
					<Group justify="space-between" gap="sm" wrap="nowrap">
						<div>
							<Text size="sm" fw={700}>
								Tournament details may need attention
							</Text>
							<Text size="xs" c="dimmed" mt={2}>
								A refresh or control request failed; the last confirmed state
								remains visible: {staleProjection.reason}
							</Text>
						</div>
						<Button
							size="compact-xs"
							variant="light"
							onClick={() => void retryReconcile(live.tournamentId)}
						>
							Retry
						</Button>
					</Group>
				</Paper>
			)}
			{live.inspectionWarning && (
				<Paper
					withBorder
					p="sm"
					radius="md"
					mb="md"
					bg={T.panel2}
					style={{ borderColor: T.cheeseDim }}
				>
					<Text size="sm" fw={700}>
						Some final positions cannot be inspected
					</Text>
					<Text size="xs" c="dimmed" mt={2}>
						{live.inspectionWarning}
					</Text>
				</Paper>
			)}

			{nav.kind === "overview" && (
				<>
					<Hero live={live} />
					<FailureSummary live={live} />
					{active && (
						<NowPlaying
							matches={inFlight}
							running={live.status === "running"}
							players={live.players}
						/>
					)}
					<Box mt="md">
						<Standings live={live} />
					</Box>
					<ProvenancePanel live={live} />
				</>
			)}
			{nav.kind === "bot" && <BotView live={live} botId={nav.botId} />}
			{nav.kind === "matchup" && (
				<MatchupView live={live} a={nav.a} b={nav.b} fromBot={nav.fromBot} />
			)}
			{nav.kind === "game" && (
				<GameView
					tournamentId={live.tournamentId}
					matchId={nav.matchId}
					players={live.players}
				/>
			)}
		</Box>
	);
}

function ratingReadinessLabel(
	readiness: TournamentLive["ratingReadiness"],
	reason: string | null,
): string {
	switch (readiness) {
		case "rateable":
			return "rating available";
		case "insufficient_games":
			return `not rated: ${reason ?? "insufficient successful games"}`;
		case "disconnected_graph":
			return `not rated: ${reason ?? "disconnected matchup graph"}`;
		case "estimator_failed":
			return `not rated: ${reason ?? "estimator failed"}`;
		case "provisional":
			return "rating provisional";
		case "legacy_unknown":
			return "rating readiness unknown";
	}
}

/** A small pulsing dot — the "something is happening" signal. Exported so the
 * launch screen's "Preparing bots…" line shares the same liveness vocabulary. */
export function PulseDot({ color = T.cheese }: { color?: string }) {
	return (
		<Box
			className="pyrat-pulse-dot"
			w={7}
			h={7}
			style={{
				borderRadius: "50%",
				background: color,
			}}
		/>
	);
}

/** In-flight matches as a compact stacked list (one row each), instead of a
 * run-on line. Each row's ticking turn counter is the per-match liveness. */
function NowPlaying({
	matches,
	running,
	players = [],
}: {
	matches: LiveMatch[];
	running: boolean;
	players?: string[];
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
				<SimpleGrid cols={{ base: 1, sm: 2 }} spacing={6}>
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
								{shortId(m.player1Id, players)} vs{" "}
								{shortId(m.player2Id, players)}
							</Text>
							<Text size="sm" c="dimmed" ff="monospace">
								turn {m.turn}
							</Text>
							<Text size="sm" ff="monospace" w={56} ta="right">
								{m.player1Score}–{m.player2Score}
							</Text>
						</Group>
					))}
				</SimpleGrid>
			)}
		</Box>
	);
}
