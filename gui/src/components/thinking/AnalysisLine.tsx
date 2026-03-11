import { Badge, Group, Stack, Text } from "@mantine/core";
import type { BotInfoEvent, Direction } from "../../bindings/generated";

const DIR_ARROW: Record<Direction, string> = {
	Up: "↑",
	Down: "↓",
	Left: "←",
	Right: "→",
	Stay: "·",
};

/** Run-length encode PV arrows: →→→ becomes →×3 */
function formatPv(pv: Direction[], max = 16): string {
	const capped = pv.slice(0, max);
	const runs: { arrow: string; count: number }[] = [];
	for (const d of capped) {
		const arrow = DIR_ARROW[d];
		const last = runs[runs.length - 1];
		if (last && last.arrow === arrow) {
			last.count++;
		} else {
			runs.push({ arrow, count: 1 });
		}
	}
	const parts = runs.map(({ arrow, count }) =>
		count > 1 ? `${arrow}×${count}` : arrow,
	);
	const result = parts.join(" ");
	return pv.length > max ? `${result} …` : result;
}

function formatNodes(n: number): string {
	if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
	if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
	return String(n);
}

type Props = {
	line: BotInfoEvent & { subcycle: number };
	color: string;
};

export default function AnalysisLine({ line, color }: Props) {
	const hasScore = line.score !== 0;
	const hasPv = line.pv.length > 0;
	const hasTarget = line.target !== null;
	const hasDepth = line.depth > 0;
	const hasNodes = line.nodes > 0;
	const hasMessage = line.message !== "";
	const hasMeta = hasTarget || hasDepth || hasNodes || hasMessage;

	if (!hasScore && !hasPv && !hasMeta) return null;

	return (
		<Stack
			gap={0}
			py={4}
			pl={8}
			style={{
				borderLeft: `2px solid var(--mantine-color-${color}-4)`,
			}}
		>
			{/* Row 1: rank + score + arrows */}
			<Group gap={6} wrap="nowrap">
				{line.multipv > 1 && (
					<Text size="xs" c="dimmed" fw={500} style={{ flexShrink: 0 }}>
						{line.multipv}.
					</Text>
				)}
				{hasScore && (
					<Badge
						size="sm"
						variant="light"
						color={line.score > 0 ? "teal" : "red"}
						style={{ flexShrink: 0 }}
					>
						{line.score > 0 ? "+" : ""}
						{line.score}
					</Badge>
				)}
				{hasPv && (
					<Text
						size="xs"
						ff="monospace"
						style={{
							whiteSpace: "nowrap",
							overflow: "hidden",
							textOverflow: "ellipsis",
						}}
					>
						{formatPv(line.pv)}
					</Text>
				)}
			</Group>

			{/* Row 2: target, depth, nodes, message */}
			{hasMeta && (
				<Group gap={8} wrap="nowrap" mt={2}>
					{hasTarget && (
						<Text size="xs" c="dimmed" style={{ flexShrink: 0 }}>
							({line.target?.x}, {line.target?.y})
						</Text>
					)}
					{hasDepth && (
						<Text size="xs" c="dimmed" style={{ flexShrink: 0 }}>
							d{line.depth}
						</Text>
					)}
					{hasNodes && (
						<Text size="xs" c="dimmed" style={{ flexShrink: 0 }}>
							{formatNodes(line.nodes)}
						</Text>
					)}
					{hasMessage && (
						<Text size="xs" c="dimmed" fs="italic" truncate>
							{line.message}
						</Text>
					)}
				</Group>
			)}
		</Stack>
	);
}
