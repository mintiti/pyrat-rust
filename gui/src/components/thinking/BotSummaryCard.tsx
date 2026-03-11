import { Group, Paper, Text } from "@mantine/core";
import type { PlayerSide } from "../../bindings/generated";
import type { InfoBucket } from "../../stores/botInfo";
import { currentLines } from "../../stores/botInfo";

type SubjectEntry = {
	subject: PlayerSide;
	bucket: InfoBucket;
};

type Props = {
	botName: string;
	color: string;
	subjects: SubjectEntry[];
};

const SUBJECT_LABEL: Record<PlayerSide, string> = {
	Player1: "Rat",
	Player2: "Python",
};

export default function BotSummaryCard({ botName, color, subjects }: Props) {
	const scores = subjects
		.map(({ subject, bucket }) => {
			const lines = currentLines(bucket);
			if (lines.length === 0) return null;
			return { label: SUBJECT_LABEL[subject], score: lines[0].score };
		})
		.filter(Boolean) as { label: string; score: number }[];

	if (scores.length === 0) return null;

	return (
		<Paper
			p="xs"
			withBorder
			style={{
				borderLeftWidth: 3,
				borderLeftColor: `var(--mantine-color-${color}-5)`,
			}}
		>
			<Group gap="sm" wrap="nowrap">
				<Text size="sm" fw={600} truncate style={{ flex: 1 }}>
					{botName}
				</Text>
				{scores.map(({ label, score }) => (
					<Text key={label} size="xs" c="dimmed" style={{ flexShrink: 0 }}>
						{label}{" "}
						<Text
							span
							size="xs"
							fw={600}
							c={score > 0 ? "teal" : score < 0 ? "red" : undefined}
						>
							{score > 0 ? "+" : ""}
							{score}
						</Text>
					</Text>
				))}
			</Group>
		</Paper>
	);
}
