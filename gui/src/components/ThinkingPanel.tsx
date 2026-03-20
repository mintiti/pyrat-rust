import { Accordion, Center, ScrollArea, Stack, Text } from "@mantine/core";
import { useAtomValue } from "jotai";
import { useMemo } from "react";
import type { PlayerSide } from "../bindings/generated";
import { SLOT_PALETTE } from "../lib/botPalette";
import { botsAtom, resolveBotName } from "../stores/botConfigAtom";
import {
	type BotInfoMap,
	type InfoBucket,
	parseBotInfoKey,
} from "../stores/botInfo";
import { useMatchStore } from "../stores/matchStore";
import BotPanel from "./thinking/BotPanel";

type SenderGroup = {
	sender: PlayerSide;
	botName: string;
	color: string;
	subjects: { subject: PlayerSide; bucket: InfoBucket }[];
};

function useSenderGroups(botInfo: BotInfoMap): SenderGroup[] {
	const bots = useAtomValue(botsAtom);
	const player1BotId = useMatchStore((s) => s.player1BotId);
	const player2BotId = useMatchStore((s) => s.player2BotId);

	return useMemo(() => {
		const grouped = new Map<
			PlayerSide,
			{ subject: PlayerSide; bucket: InfoBucket }[]
		>();

		for (const [key, bucket] of Object.entries(botInfo)) {
			const { sender, subject } = parseBotInfoKey(key);
			let list = grouped.get(sender);
			if (!list) {
				list = [];
				grouped.set(sender, list);
			}
			list.push({ subject, bucket });
		}

		// Fixed order: Player1 always first, Player2 always second.
		const SIDES: PlayerSide[] = ["Player1", "Player2"];
		return SIDES.filter((side) => grouped.has(side)).map((sender) => ({
			sender,
			botName: resolveBotName(
				sender === "Player1" ? player1BotId : player2BotId,
				bots,
				sender,
			),
			color: SLOT_PALETTE[sender].mantine,
			subjects: (grouped.get(sender) ?? []).sort((a, b) =>
				a.subject.localeCompare(b.subject),
			),
		}));
	}, [botInfo, bots, player1BotId, player2BotId]);
}

type Props = {
	botInfo: BotInfoMap;
};

export default function ThinkingPanel({ botInfo }: Props) {
	const groups = useSenderGroups(botInfo);
	const matchPhase = useMatchStore((s) => s.matchPhase);

	const emptyMessage =
		matchPhase === "finished"
			? "No analysis for this turn."
			: "Waiting for bot analysis...";

	return (
		<ScrollArea
			style={{
				flex: 1,
				minHeight: 0,
			}}
			p="sm"
		>
			{groups.length === 0 ? (
				<Center h="100%">
					<Text size="sm" c="dimmed">
						{emptyMessage}
					</Text>
				</Center>
			) : (
				<Stack gap="sm">
					<Accordion
						multiple
						defaultValue={["Player1", "Player2"]}
						variant="separated"
						styles={{
							item: { borderRadius: "var(--mantine-radius-sm)" },
						}}
					>
						{groups.map((g) => (
							<BotPanel
								key={g.sender}
								sender={g.sender}
								botName={g.botName}
								color={g.color}
								subjects={g.subjects}
							/>
						))}
					</Accordion>
				</Stack>
			)}
		</ScrollArea>
	);
}
