import { Accordion, Group, Stack, Text } from "@mantine/core";
import type { PlayerSide } from "../../bindings/generated";
import type { InfoBucket } from "../../stores/botInfo";
import { currentLines } from "../../stores/botInfo";
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

const SUBJECT_LABEL: Record<PlayerSide, string> = {
	Player1: "Rat",
	Player2: "Python",
};

export default function BotSection({
	sender,
	botName,
	color,
	subjects,
}: Props) {
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
					<Text size="xs" c="dimmed">
						{subjects.map(({ subject }) => SUBJECT_LABEL[subject]).join(", ")}
					</Text>
				</Group>
			</Accordion.Control>
			<Accordion.Panel>
				<Stack gap={8}>
					{subjects.map(({ subject, bucket }) => {
						const lines = currentLines(bucket);
						if (lines.length === 0) return null;
						return (
							<div key={subject}>
								{subjects.length > 1 && (
									<Text
										size="xs"
										c="dimmed"
										fw={600}
										mb={4}
										tt="uppercase"
										lts={0.5}
									>
										{SUBJECT_LABEL[subject]}
									</Text>
								)}
								<Stack gap={4}>
									{lines.map((line) => (
										<AnalysisLine
											key={line.multipv}
											line={line}
											color={color}
										/>
									))}
								</Stack>
							</div>
						);
					})}
				</Stack>
			</Accordion.Panel>
		</Accordion.Item>
	);
}
