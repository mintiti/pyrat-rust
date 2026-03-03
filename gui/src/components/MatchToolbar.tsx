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
import type { MatchWinner } from "../bindings/generated";
import {
	useMatchStore,
	useCursorDepth,
	useMainlineLength,
} from "../stores/matchStore";

type Props = {
	onStart: () => void;
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

const SPEED_OPTIONS = [
	{ value: "800", label: "0.25x" },
	{ value: "400", label: "0.5x" },
	{ value: "200", label: "1x" },
	{ value: "100", label: "2x" },
	{ value: "40", label: "5x" },
	{ value: "20", label: "10x" },
];

export default function MatchToolbar({ onStart }: Props) {
	const player1Cmd = useMatchStore((s) => s.player1Cmd);
	const player2Cmd = useMatchStore((s) => s.player2Cmd);
	const viewerMode = useMatchStore((s) => s.viewerMode);
	const playbackSpeed = useMatchStore((s) => s.playbackSpeed);
	const result = useMatchStore((s) => s.result);
	const error = useMatchStore((s) => s.error);
	const disconnection = useMatchStore((s) => s.disconnection);

	const {
		setPlayer1Cmd,
		setPlayer2Cmd,
		goToStart,
		goToEnd,
		stepForward,
		stepBack,
		togglePlay,
		setPlaybackSpeed,
	} = useMatchStore.getState();

	const cursorDepth = useCursorDepth();
	const totalTurns = useMainlineLength();

	const canStart = player1Cmd != null && player2Cmd != null;
	const hasMatch = viewerMode !== "empty";

	const handleStartClick = () => {
		if (hasMatch) {
			if (!window.confirm("Start a new match? Current match data will be lost."))
				return;
		}
		onStart();
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
					data={BOT_OPTIONS}
					value={player1Cmd}
					onChange={setPlayer1Cmd}
					style={{ width: 180 }}
					allowDeselect={false}
				/>
				<Select
					size="xs"
					placeholder="Player 2"
					data={BOT_OPTIONS}
					value={player2Cmd}
					onChange={setPlayer2Cmd}
					style={{ width: 180 }}
					allowDeselect={false}
				/>
				<Button
					size="xs"
					onClick={handleStartClick}
					disabled={!canStart}
				>
					{!hasMatch ? "Start" : "New Match"}
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
