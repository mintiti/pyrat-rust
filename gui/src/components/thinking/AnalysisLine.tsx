import { Group, Stack, Text } from "@mantine/core";
import { memo } from "react";
import type { BotInfoEvent, Direction } from "../../bindings/generated";
import { DIR_ARROW } from "../../lib/directions";

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
	subjectIcon: string;
};

export default memo(
	function AnalysisLine({ line, color, subjectIcon }: Props) {
		const hasPv = line.pv.length > 0;
		const hasDepth = line.depth > 0;
		const hasNodes = line.nodes > 0;
		const hasMessage = line.message !== "";
		const hasMeta = hasDepth || hasNodes || hasMessage;

		const showScore = line.score !== null;

		if (!showScore && !hasPv && !hasMeta) return null;

		return (
			<Stack gap={0} py={4} pl={8}>
				{/* Row 1: subject icon + rank + score bubble + arrows */}
				<Group gap={6} wrap="nowrap">
					<img
						src={subjectIcon}
						alt=""
						width={12}
						height={12}
						style={{ flexShrink: 0 }}
					/>
					{line.multipv > 1 && (
						<Text size="xs" c="dimmed" fw={500} style={{ flexShrink: 0 }}>
							{line.multipv}.
						</Text>
					)}
					{showScore && (
						<Text
							size="xs"
							fw={700}
							ff="monospace"
							style={{
								flexShrink: 0,
								width: "3.5rem",
								textAlign: "center",
								borderRadius: "var(--mantine-radius-sm)",
								background: `var(--mantine-color-${color}-light)`,
								color: `var(--mantine-color-${color}-light-color)`,
								padding: "1px 4px",
							}}
						>
							{line.score}
						</Text>
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

				{/* Row 2: depth, nodes, message */}
				{hasMeta && (
					<Group gap={8} wrap="nowrap" mt={2}>
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
	},
	(prev, next) =>
		prev.line.subcycle === next.line.subcycle &&
		prev.line.multipv === next.line.multipv &&
		prev.line.depth === next.line.depth &&
		prev.line.nodes === next.line.nodes &&
		prev.line.score === next.line.score &&
		prev.color === next.color,
);
