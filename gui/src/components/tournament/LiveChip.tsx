import { Box, Group, Text } from "@mantine/core";
import { useTournamentStore } from "../../stores/tournamentStore";
import { T } from "./theme";

/** Persistent topbar chip: glanceable tournament progress from any tab. Click
 * to jump to the tournaments tab. Pulses while running, green when done. */
export default function LiveChip({ onClick }: { onClick: () => void }) {
	const live = useTournamentStore((s) => s.live);
	if (!live) return null;

	const done = live.status === "finished";
	const aborted = live.status === "aborted";
	const color = aborted ? T.loss : done ? T.win : T.cheese;

	return (
		<Group
			gap={7}
			px="sm"
			py={4}
			onClick={onClick}
			style={{
				border: `1px solid ${color}`,
				borderRadius: 14,
				cursor: "pointer",
			}}
		>
			<Box
				w={7}
				h={7}
				style={{
					borderRadius: "50%",
					background: color,
					animation: done || aborted ? undefined : "pulse 1.2s infinite",
				}}
			/>
			<Text size="xs">{live.name ?? `#${live.tournamentId}`}</Text>
			<Text size="xs" c="dimmed" ff="monospace">
				{live.done}/{live.total}
			</Text>
		</Group>
	);
}
