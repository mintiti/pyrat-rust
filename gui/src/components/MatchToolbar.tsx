import { ActionIcon, Badge, Button, Group, Select, Text } from "@mantine/core";
import {
	IconAdjustments,
	IconChevronLeft,
	IconChevronRight,
	IconChevronsLeft,
	IconChevronsRight,
	IconPlayerPause,
	IconPlayerPlay,
	IconSettings,
} from "@tabler/icons-react";
import { useAtomValue } from "jotai";
import { useMemo, useState } from "react";
import type { MatchWinner } from "../bindings/generated";
import { botsAtom } from "../stores/botConfigAtom";
import { matchConfigAtom } from "../stores/matchConfigAtom";
import {
	useCursorDepth,
	useMainlineLength,
	useMatchStore,
} from "../stores/matchStore";
import ConfirmModal from "./common/ConfirmModal";

type Props = {
	onStart: () => void;
	onNavigate: (view: "match" | "bots") => void;
	onOpenConfig: () => void;
};

function winnerLabel(winner: MatchWinner): string {
	switch (winner) {
		case "Player1":
			return "Rat wins!";
		case "Player2":
			return "Python wins!";
		case "Draw":
			return "Draw!";
	}
}

function winnerColor(winner: MatchWinner): string {
	switch (winner) {
		case "Player1":
			return "blue";
		case "Player2":
			return "green";
		case "Draw":
			return "gray";
	}
}

const SPEED_OPTIONS = [
	{ value: "800", label: "0.25x" },
	{ value: "400", label: "0.5x" },
	{ value: "200", label: "1x" },
	{ value: "100", label: "2x" },
	{ value: "40", label: "5x" },
	{ value: "20", label: "10x" },
];

export default function MatchToolbar({
	onStart,
	onNavigate,
	onOpenConfig,
}: Props) {
	const bots = useAtomValue(botsAtom);
	const matchConfig = useAtomValue(matchConfigAtom);
	const player1BotId = useMatchStore((s) => s.player1BotId);
	const player2BotId = useMatchStore((s) => s.player2BotId);
	const viewerMode = useMatchStore((s) => s.viewerMode);
	const playbackSpeed = useMatchStore((s) => s.playbackSpeed);
	const result = useMatchStore((s) => s.result);
	const error = useMatchStore((s) => s.error);
	const disconnection = useMatchStore((s) => s.disconnection);

	const {
		setPlayer1BotId,
		setPlayer2BotId,
		goToStart,
		goToEnd,
		stepForward,
		stepBack,
		togglePlay,
		setPlaybackSpeed,
	} = useMatchStore.getState();

	const cursorDepth = useCursorDepth();
	const totalTurns = useMainlineLength();

	const botOptions = useMemo(
		() => [
			{ value: "__random__", label: "Random Bot" },
			...bots.map((b) => ({ value: b.id, label: b.name })),
		],
		[bots],
	);

	const [confirmOpen, setConfirmOpen] = useState(false);

	const canStart = player1BotId != null && player2BotId != null;
	const hasMatch = viewerMode !== "empty";

	const handleStartClick = () => {
		if (hasMatch) {
			setConfirmOpen(true);
		} else {
			onStart();
		}
	};

	return (
		<Group
			p="xs"
			justify="space-between"
			style={{
				borderBottom: "1px solid var(--mantine-color-dark-4)",
				flexShrink: 0,
			}}
		>
			<Group gap="sm">
				<Select
					size="xs"
					placeholder="Player 1"
					data={botOptions}
					value={player1BotId}
					onChange={setPlayer1BotId}
					style={{ width: 180 }}
					allowDeselect={false}
				/>
				<Select
					size="xs"
					placeholder="Player 2"
					data={botOptions}
					value={player2BotId}
					onChange={setPlayer2BotId}
					style={{ width: 180 }}
					allowDeselect={false}
				/>
				<Button size="xs" onClick={handleStartClick} disabled={!canStart}>
					{!hasMatch ? "Start" : "New Match"}
				</Button>
				<ActionIcon
					variant="subtle"
					size="sm"
					onClick={() => onNavigate("bots")}
					title="Bot settings"
				>
					<IconSettings size={16} />
				</ActionIcon>
				<ActionIcon
					variant="subtle"
					size="sm"
					onClick={onOpenConfig}
					title="Match configuration"
				>
					<IconAdjustments size={16} />
				</ActionIcon>
				<Badge variant="light" size="sm" color="gray">
					{matchConfig.preset === "custom"
						? `${matchConfig.width}×${matchConfig.height}`
						: matchConfig.preset.charAt(0).toUpperCase() +
							matchConfig.preset.slice(1)}
				</Badge>
			</Group>
			{hasMatch && (
				<Group gap={4}>
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={goToStart}
						disabled={cursorDepth === 0}
					>
						<IconChevronsLeft size={16} />
					</ActionIcon>
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={stepBack}
						disabled={cursorDepth === 0}
					>
						<IconChevronLeft size={16} />
					</ActionIcon>
					<ActionIcon variant="subtle" size="sm" onClick={togglePlay}>
						{viewerMode === "playing" ? (
							<IconPlayerPause size={16} />
						) : (
							<IconPlayerPlay size={16} />
						)}
					</ActionIcon>
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={stepForward}
						disabled={cursorDepth >= totalTurns}
					>
						<IconChevronRight size={16} />
					</ActionIcon>
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={goToEnd}
						disabled={cursorDepth >= totalTurns}
					>
						<IconChevronsRight size={16} />
					</ActionIcon>
					<Select
						size="xs"
						data={SPEED_OPTIONS}
						value={String(playbackSpeed)}
						onChange={(v) => v && setPlaybackSpeed(Number(v))}
						allowDeselect={false}
						style={{ width: 80 }}
					/>
					<Text size="xs" c="dimmed" ml={4}>
						Turn {cursorDepth} / {totalTurns}
					</Text>
				</Group>
			)}
			<Group gap="sm">
				{disconnection && (
					<Badge color="yellow" variant="filled" size="lg">
						{disconnection.player === "Player1" ? "Rat" : "Python"}{" "}
						disconnected: {disconnection.reason}
					</Badge>
				)}
				{result && (
					<Badge color={winnerColor(result.winner)} variant="filled" size="lg">
						{winnerLabel(result.winner)} {result.player1_score.toFixed(1)} -{" "}
						{result.player2_score.toFixed(1)} ({result.turns_played}t)
					</Badge>
				)}
				{error && (
					<Text size="xs" c="red">
						{error}
					</Text>
				)}
			</Group>
			<ConfirmModal
				title="Start new match?"
				description="Current match data will be lost."
				opened={confirmOpen}
				onClose={() => setConfirmOpen(false)}
				onConfirm={onStart}
				confirmLabel="New Match"
			/>
		</Group>
	);
}
