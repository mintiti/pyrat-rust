import { ActionIcon, Badge, Button, Group, Select, Text } from "@mantine/core";
import {
	IconChevronLeft,
	IconChevronRight,
	IconChevronsLeft,
	IconChevronsRight,
	IconPlayerPause,
	IconPlayerPlay,
	IconRoute,
} from "@tabler/icons-react";
import { useState } from "react";
import type { MatchWinner } from "../bindings/generated";
import {
	useCursorDepth,
	useMainlineLength,
	useMatchStore,
} from "../stores/matchStore";
import ConfirmModal from "./common/ConfirmModal";

type Props = {
	onNewMatch: () => void;
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

export default function MatchToolbar({ onNewMatch }: Props) {
	const viewerMode = useMatchStore((s) => s.viewerMode);
	const playbackSpeed = useMatchStore((s) => s.playbackSpeed);
	const result = useMatchStore((s) => s.result);
	const error = useMatchStore((s) => s.error);
	const disconnection = useMatchStore((s) => s.disconnection);
	const showP1Arrows = useMatchStore((s) => s.showPlayer1Arrows);
	const showP2Arrows = useMatchStore((s) => s.showPlayer2Arrows);

	const {
		goToStart,
		goToEnd,
		stepForward,
		stepBack,
		togglePlay,
		setPlaybackSpeed,
		resetToPreview,
		toggleArrows,
	} = useMatchStore.getState();

	const cursorDepth = useCursorDepth();
	const totalTurns = useMainlineLength();

	const [confirmOpen, setConfirmOpen] = useState(false);

	const hasMatch = viewerMode !== "previewing";

	const handleNewMatchClick = () => {
		if (hasMatch) {
			setConfirmOpen(true);
		} else {
			onNewMatch();
		}
	};

	const handleConfirm = () => {
		setConfirmOpen(false);
		resetToPreview();
		onNewMatch();
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
				<Button size="xs" variant="light" onClick={handleNewMatchClick}>
					New Match
				</Button>
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
						{viewerMode === "live" ? (
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
					<ActionIcon
						variant={showP1Arrows ? "filled" : "subtle"}
						color="blue"
						size="sm"
						onClick={() => toggleArrows("Player1")}
						title="Toggle Rat PV arrows"
					>
						<IconRoute size={14} />
					</ActionIcon>
					<ActionIcon
						variant={showP2Arrows ? "filled" : "subtle"}
						color="green"
						size="sm"
						onClick={() => toggleArrows("Player2")}
						title="Toggle Python PV arrows"
					>
						<IconRoute size={14} />
					</ActionIcon>
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
				title="Return to setup?"
				description="Current match data will be lost."
				opened={confirmOpen}
				onClose={() => setConfirmOpen(false)}
				onConfirm={handleConfirm}
				confirmLabel="Confirm"
			/>
		</Group>
	);
}
