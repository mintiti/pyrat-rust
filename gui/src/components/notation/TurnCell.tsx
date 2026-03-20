import { Text, UnstyledButton } from "@mantine/core";
import type { Direction } from "../../bindings/generated";
import { DIR_ARROW } from "../../lib/directions";

export type NotationEntry = {
	path: number[];
	turn: number;
	actions: { player1: Direction; player2: Direction };
	highlight: "cheese" | "mud" | null;
	variationLevel: number;
	isVariationStart: boolean;
};

type Props = {
	entry: NotationEntry;
	isCurrent: boolean;
	onClick: () => void;
};

const BG_CHEESE = "rgba(252,196,25,0.15)";
const BG_MUD = "rgba(160,110,55,0.15)";
const BG_CURRENT = "rgba(255,255,255,0.08)";
const BG_CURRENT_CHEESE = "rgba(252,196,25,0.28)";
const BG_CURRENT_MUD = "rgba(160,110,55,0.28)";

function background(
	highlight: NotationEntry["highlight"],
	isCurrent: boolean,
): string | undefined {
	if (isCurrent) {
		if (highlight === "cheese") return BG_CURRENT_CHEESE;
		if (highlight === "mud") return BG_CURRENT_MUD;
		return BG_CURRENT;
	}
	if (highlight === "cheese") return BG_CHEESE;
	if (highlight === "mud") return BG_MUD;
	return undefined;
}

export default function TurnCell({ entry, isCurrent, onClick }: Props) {
	const indent = entry.variationLevel * 16;

	return (
		<UnstyledButton
			onClick={onClick}
			style={{
				display: "flex",
				alignItems: "center",
				gap: 6,
				paddingLeft: indent + 6,
				paddingRight: 6,
				paddingTop: 2,
				paddingBottom: 2,
				background: background(entry.highlight, isCurrent),
				borderLeft: isCurrent
					? "2px solid var(--mantine-color-blue-5)"
					: "2px solid transparent",
				borderRadius: "var(--mantine-radius-xs)",
				cursor: "pointer",
			}}
		>
			<Text
				size="xs"
				c="dimmed"
				ff="monospace"
				style={{ width: "2.5em", flexShrink: 0 }}
			>
				{entry.turn}.
			</Text>
			<Text size="xs" ff="monospace" style={{ flexShrink: 0 }}>
				{DIR_ARROW[entry.actions.player1]}
			</Text>
			<Text size="xs" ff="monospace" style={{ flexShrink: 0 }}>
				{DIR_ARROW[entry.actions.player2]}
			</Text>
		</UnstyledButton>
	);
}
