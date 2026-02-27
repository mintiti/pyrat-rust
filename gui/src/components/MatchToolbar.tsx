import {
	ActionIcon,
	Badge,
	Button,
	Group,
	Text,
	TextInput,
} from "@mantine/core";
import {
	IconChevronLeft,
	IconChevronRight,
	IconChevronsLeft,
	IconChevronsRight,
	IconPlayerPause,
	IconPlayerPlay,
} from "@tabler/icons-react";
import type {
	BotDisconnectedEvent,
	MatchOverEvent,
	MatchWinner,
} from "../bindings/generated";
import type { MatchStatus } from "./MatchView";

type Props = {
	player1Cmd: string;
	player2Cmd: string;
	onPlayer1CmdChange: (value: string) => void;
	onPlayer2CmdChange: (value: string) => void;
	onStart: () => void;
	status: MatchStatus;
	result: MatchOverEvent | null;
	error: string | null;
	disconnection: BotDisconnectedEvent | null;
	currentTurn: number;
	totalTurns: number;
	isPlaying: boolean;
	onGoToStart: () => void;
	onGoToEnd: () => void;
	onStepForward: () => void;
	onStepBack: () => void;
	onTogglePlay: () => void;
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

export default function MatchToolbar({
	player1Cmd,
	player2Cmd,
	onPlayer1CmdChange,
	onPlayer2CmdChange,
	onStart,
	status,
	result,
	error,
	disconnection,
	currentTurn,
	totalTurns,
	isPlaying,
	onGoToStart,
	onGoToEnd,
	onStepForward,
	onStepBack,
	onTogglePlay,
}: Props) {
	const canStart = player1Cmd.trim() !== "" && player2Cmd.trim() !== "";

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
				<TextInput
					size="xs"
					placeholder="Player 1 command"
					value={player1Cmd}
					onChange={(e) => onPlayer1CmdChange(e.currentTarget.value)}
					style={{ width: 220 }}
					disabled={status === "running"}
				/>
				<TextInput
					size="xs"
					placeholder="Player 2 command"
					value={player2Cmd}
					onChange={(e) => onPlayer2CmdChange(e.currentTarget.value)}
					style={{ width: 220 }}
					disabled={status === "running"}
				/>
				<Button
					size="xs"
					onClick={onStart}
					disabled={!canStart}
					loading={status === "running"}
				>
					Start
				</Button>
			</Group>
			{status !== "idle" && (
				<Group gap={4}>
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={onGoToStart}
						disabled={currentTurn <= -1}
					>
						<IconChevronsLeft size={16} />
					</ActionIcon>
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={onStepBack}
						disabled={currentTurn <= -1}
					>
						<IconChevronLeft size={16} />
					</ActionIcon>
					<ActionIcon variant="subtle" size="sm" onClick={onTogglePlay}>
						{isPlaying ? (
							<IconPlayerPause size={16} />
						) : (
							<IconPlayerPlay size={16} />
						)}
					</ActionIcon>
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={onStepForward}
						disabled={currentTurn >= totalTurns - 1}
					>
						<IconChevronRight size={16} />
					</ActionIcon>
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={onGoToEnd}
						disabled={currentTurn >= totalTurns - 1}
					>
						<IconChevronsRight size={16} />
					</ActionIcon>
					<Text size="xs" c="dimmed" ml={4}>
						Turn {currentTurn + 1} / {totalTurns}
					</Text>
				</Group>
			)}
			<Group gap="sm">
				{disconnection && (
					<Badge color="yellow" variant="filled" size="lg">
						{disconnection.player} disconnected: {disconnection.reason}
					</Badge>
				)}
				{result && (
					<Badge
						color={winnerColor(result.winner)}
						variant="filled"
						size="lg"
					>
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
		</Group>
	);
}
