import {
	Badge,
	Button,
	Group,
	Text,
	TextInput,
} from "@mantine/core";
import type { MatchOverEvent, MatchWinner } from "../bindings/generated";

type MatchStatus = "idle" | "running" | "finished";

type Props = {
	player1Cmd: string;
	player2Cmd: string;
	onPlayer1CmdChange: (value: string) => void;
	onPlayer2CmdChange: (value: string) => void;
	onStart: () => void;
	status: MatchStatus;
	result: MatchOverEvent | null;
	error: string | null;
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
			<Group gap="sm">
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
		</Group>
	);
}
