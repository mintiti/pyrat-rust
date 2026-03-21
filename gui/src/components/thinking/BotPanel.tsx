import { Accordion, ActionIcon, Group, Stack, Text } from "@mantine/core";
import {
	IconPlayerPause,
	IconPlayerPlay,
	IconRoute,
} from "@tabler/icons-react";
import pythonIconUrl from "../../assets/sprites/players/python/neutral.png";
import ratIconUrl from "../../assets/sprites/players/rat/neutral.png";
import type { PlayerSide } from "../../bindings/generated";
import { PLAYER_LABEL } from "../../lib/botPalette";
import type { InfoBucket } from "../../stores/botInfo";
import { currentLines } from "../../stores/botInfo";
import { useMatchStore } from "../../stores/matchStore";
import AnalysisLine from "./AnalysisLine";

type SubjectEntry = {
	subject: PlayerSide;
	bucket: InfoBucket;
};

type Props = {
	sender: PlayerSide;
	botName: string;
	color: string;
	subjects: SubjectEntry[];
};

const SUBJECT_ICON: Record<PlayerSide, string> = {
	Player1: ratIconUrl,
	Player2: pythonIconUrl,
};

function formatNodes(n: number): string {
	if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
	if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
	return String(n);
}

/** Compact summary of the best line from the first subject with data. */
function headerSummary(subjects: SubjectEntry[]): string | null {
	for (const { bucket } of subjects) {
		const lines = currentLines(bucket);
		const best = lines.find((l) => l.multipv === 1);
		if (!best) continue;

		const parts: string[] = [];
		if (best.score !== null) {
			parts.push(String(best.score));
		}
		if (best.depth > 0) parts.push(`d${best.depth}`);
		if (best.nodes > 0) parts.push(formatNodes(best.nodes));
		if (parts.length > 0) return parts.join("  ");
	}
	return null;
}

export default function BotPanel({ sender, botName, color, subjects }: Props) {
	const showArrows = useMatchStore((s) =>
		sender === "Player1" ? s.showPlayer1Arrows : s.showPlayer2Arrows,
	);
	const paused = useMatchStore((s) => s.pausedSenders[sender]);
	const { toggleArrows, togglePauseSender } = useMatchStore.getState();
	const summary = headerSummary(subjects);

	return (
		<Accordion.Item value={sender}>
			<Accordion.Control
				style={{
					borderLeft: `3px solid var(--mantine-color-${color}-5)`,
				}}
			>
				<Group gap="xs" wrap="nowrap">
					<Text size="sm" fw={600}>
						{botName}
					</Text>
					{subjects.map(({ subject }) => (
						<img
							key={subject}
							src={SUBJECT_ICON[subject]}
							alt={PLAYER_LABEL[subject]}
							width={14}
							height={14}
						/>
					))}
					{summary && (
						<Text size="xs" c="dimmed" ff="monospace">
							{summary}
						</Text>
					)}
					<Group ml="auto" gap={2}>
						<ActionIcon
							variant={paused ? "transparent" : "filled"}
							color={color}
							size="xs"
							onClick={(e) => {
								e.stopPropagation();
								togglePauseSender(sender);
							}}
							title={`${paused ? "Resume" : "Pause"} ${PLAYER_LABEL[sender]} analysis feed`}
						>
							{paused ? (
								<IconPlayerPlay size={12} />
							) : (
								<IconPlayerPause size={12} />
							)}
						</ActionIcon>
						<ActionIcon
							variant={showArrows ? "filled" : "subtle"}
							color={color}
							size="xs"
							onClick={(e) => {
								e.stopPropagation();
								toggleArrows(sender);
							}}
							title={`Toggle ${PLAYER_LABEL[sender]} PV arrows`}
						>
							<IconRoute size={12} />
						</ActionIcon>
					</Group>
				</Group>
			</Accordion.Control>
			<Accordion.Panel>
				<Stack gap={8}>
					{subjects.map(({ subject, bucket }) => {
						const lines = currentLines(bucket);
						if (lines.length === 0) return null;
						return (
							<Stack key={subject} gap={4}>
								{lines.map((line) => (
									<AnalysisLine
										key={line.multipv}
										line={line}
										color={color}
										subjectIcon={SUBJECT_ICON[subject]}
									/>
								))}
							</Stack>
						);
					})}
				</Stack>
			</Accordion.Panel>
		</Accordion.Item>
	);
}
