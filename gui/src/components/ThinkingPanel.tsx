import { Accordion, ScrollArea, Stack } from "@mantine/core";
import { useAtomValue } from "jotai";
import { useMemo } from "react";
import type { PlayerSide } from "../bindings/generated";
import { RANDOM_BOT_ID, botsAtom } from "../stores/botConfigAtom";
import {
	type BotInfoMap,
	type InfoBucket,
	parseBotInfoKey,
} from "../stores/botInfo";
import { useMatchStore } from "../stores/matchStore";
import BotSection from "./thinking/BotSection";

const SENDER_COLOR: Record<PlayerSide, string> = {
	Player1: "blue",
	Player2: "green",
};

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

		const resolveName = (side: PlayerSide): string => {
			const botId = side === "Player1" ? player1BotId : player2BotId;
			if (!botId || botId === RANDOM_BOT_ID) return "Random Bot";
			const bot = bots.find((b) => b.id === botId);
			return bot?.name ?? side;
		};

		return Array.from(grouped.entries()).map(([sender, subjects]) => ({
			sender,
			botName: resolveName(sender),
			color: SENDER_COLOR[sender],
			subjects: subjects.sort((a, b) => a.subject.localeCompare(b.subject)),
		}));
	}, [botInfo, bots, player1BotId, player2BotId]);
}

type Props = {
	botInfo: BotInfoMap;
};

export default function ThinkingPanel({ botInfo }: Props) {
	const groups = useSenderGroups(botInfo);

	if (groups.length === 0) return null;

	const defaultOpen = groups.map((g) => g.sender);

	return (
		<ScrollArea
			style={{
				width: 320,
				flexShrink: 0,
				borderLeft: "1px solid var(--mantine-color-default-border)",
			}}
			p="sm"
		>
			<Stack gap="sm">
				<Accordion
					multiple
					defaultValue={defaultOpen}
					variant="separated"
					styles={{
						item: { borderRadius: "var(--mantine-radius-sm)" },
					}}
				>
					{groups.map((g) => (
						<BotSection
							key={g.sender}
							sender={g.sender}
							botName={g.botName}
							color={g.color}
							subjects={g.subjects}
						/>
					))}
				</Accordion>
			</Stack>
		</ScrollArea>
	);
}
