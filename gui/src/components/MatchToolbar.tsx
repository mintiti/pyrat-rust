import {
	ActionIcon,
	Badge,
	Button,
	Group,
	Select,
	Text,
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
	player1Cmd: string | null;
	player2Cmd: string | null;
	onPlayer1CmdChange: (value: string | null) => void;
	onPlayer2CmdChange: (value: string | null) => void;
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

const BOT_OPTIONS = [{ value: "__random__", label: "Random Bot" }];

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
	const canStart = player1Cmd != null && player2Cmd != null;

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
					data={BOT_OPTIONS}
					value={player1Cmd}
					onChange={onPlayer1CmdChange}
					style={{ width: 180 }}
					disabled={status === "running"}
					allowDeselect={false}
				/>
				<Select
					size="xs"
					placeholder="Player 2"
					data={BOT_OPTIONS}
					value={player2Cmd}
					onChange={onPlayer2CmdChange}
					style={{ width: 180 }}
					disabled={status === "running"}
					allowDeselect={false}
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
