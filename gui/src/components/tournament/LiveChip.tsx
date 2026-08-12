import { Box, Group, Text } from "@mantine/core";
import {
	hasFinalTournamentVerdict,
	useTournamentStore,
} from "../../stores/tournamentStore";
import PressableSurface from "./PressableSurface";
import { T, shortId } from "./theme";

export type TournamentChromeDestination = "launch" | "live";

type TournamentChromeState = Pick<
	ReturnType<typeof useTournamentStore.getState>,
	"live" | "starting" | "preparing" | "terminalNotice"
>;

export function tournamentChromeProjection({
	live,
	starting,
	preparing,
	terminalNotice,
}: TournamentChromeState) {
	if (terminalNotice) {
		const noticeLive =
			live?.tournamentId === terminalNotice.tournamentId ? live : null;
		const finalVerdict = noticeLive
			? hasFinalTournamentVerdict(noticeLive)
			: false;
		return {
			name: noticeLive?.name ?? `#${terminalNotice.tournamentId}`,
			status:
				terminalNotice.status === "aborted"
					? noticeLive?.lifecycle === "failed"
						? "failed"
						: "stopped"
					: finalVerdict
						? "finished"
						: "partial results",
			progress: finalVerdict
				? "view result"
				: terminalNotice.status === "finished"
					? "view partial results"
					: "view details",
			color:
				terminalNotice.status === "aborted"
					? T.loss
					: finalVerdict
						? T.win
						: T.cheese,
			pulse: false,
			destination: "live" as const,
			tooltip: terminalNotice.reason ?? undefined,
		};
	}
	if (starting || preparing) {
		return {
			name: "Tournament",
			status: preparing?.current
				? `preparing ${shortId(preparing.current)}`
				: "preparing bots",
			progress: preparing ? `${preparing.done}/${preparing.total}` : null,
			color: T.cheese,
			pulse: true,
			destination: "launch" as const,
			tooltip: undefined,
		};
	}
	if (live?.status === "running") {
		return {
			name: live.name ?? `#${live.tournamentId}`,
			status: "running",
			progress: `${live.done}/${live.total}`,
			color: T.cheese,
			pulse: true,
			destination: "live" as const,
			tooltip: undefined,
		};
	}
	return null;
}

/** App-level activity handoff. Running/preparing state remains glanceable from
 * other pages; terminal state appears only as a short-lived invitation to see
 * the result. The surrounding flow lane is intentional: this never overlays a
 * page's own controls. */
export default function LiveChip({
	onOpenTournament,
}: {
	onOpenTournament: (destination: TournamentChromeDestination) => void;
}) {
	const live = useTournamentStore((s) => s.live);
	const starting = useTournamentStore((s) => s.starting);
	const preparing = useTournamentStore((s) => s.preparing);
	const terminalNotice = useTournamentStore((s) => s.terminalNotice);
	const projection = tournamentChromeProjection({
		live,
		starting,
		preparing,
		terminalNotice,
	});
	if (!projection) return null;
	const { name, status, progress, color, pulse, destination, tooltip } =
		projection;

	const accessibleLabel = [name, status, progress].filter(Boolean).join(", ");

	return (
		<Box
			component="header"
			px="md"
			py={6}
			style={{
				display: "flex",
				justifyContent: "flex-end",
				borderBottom: `1px solid ${T.line}`,
				background: "var(--mantine-color-body)",
				flex: "0 0 auto",
			}}
		>
			<PressableSurface
				variant="bare"
				motion="none"
				className="pyrat-live-chip"
				onClick={() => onOpenTournament(destination)}
				aria-label={accessibleLabel}
				title={tooltip}
				style={{ borderColor: color }}
			>
				<Group gap={7} px="sm" py={4} wrap="nowrap">
					<Box
						className={pulse ? "pyrat-pulse-dot" : undefined}
						w={7}
						h={7}
						style={{
							borderRadius: "50%",
							background: color,
							flex: "0 0 auto",
						}}
					/>
					<Text size="xs" fw={600} truncate="end">
						{name}
					</Text>
					<Text size="xs" c="dimmed">
						{status}
					</Text>
					{progress && (
						<Text size="xs" c="dimmed" ff="monospace">
							{progress}
						</Text>
					)}
				</Group>
			</PressableSurface>
		</Box>
	);
}
