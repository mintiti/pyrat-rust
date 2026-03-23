import { ActionIcon, Badge, Button, Group, Select, Text } from "@mantine/core";
import {
	IconChevronLeft,
	IconChevronRight,
	IconChevronsLeft,
	IconChevronsRight,
	IconPlayerPause,
	IconPlayerPlay,
	IconPlayerTrackNext,
} from "@tabler/icons-react";
import { useAtomValue } from "jotai";
import { useState } from "react";
import { botsAtom, resolveBotName } from "../stores/botConfigAtom";
import {
	useCursorDepth,
	useIsAtTip,
	useMainlineLength,
	useMatchStore,
} from "../stores/matchStore";
import ConfirmModal from "./common/ConfirmModal";

type Props = {
	onNewMatch: () => void;
};

const DISCONNECT_REASONS: Record<string, string> = {
	PeerClosed: "process exited",
	FrameError: "communication error",
	ChannelClosed: "connection dropped",
	HandshakeTimeout: "failed to connect",
	DrainComplete: "disconnected after game",
};

const SPEED_OPTIONS = [
	{ value: "800", label: "0.25x" },
	{ value: "400", label: "0.5x" },
	{ value: "200", label: "1x" },
	{ value: "100", label: "2x" },
	{ value: "40", label: "5x" },
	{ value: "20", label: "10x" },
];

export default function MatchToolbar({ onNewMatch }: Props) {
	const matchPhase = useMatchStore((s) => s.matchPhase);
	const autoplay = useMatchStore((s) => s.autoplay);
	const playbackSpeed = useMatchStore((s) => s.playbackSpeed);
	const error = useMatchStore((s) => s.error);
	const analysisError = useMatchStore((s) => s.analysisError);
	const disconnection = useMatchStore((s) => s.disconnection);
	const player1BotId = useMatchStore((s) => s.player1BotId);
	const player2BotId = useMatchStore((s) => s.player2BotId);
	const mode = useMatchStore((s) => s.mode);
	const mainlineDepth = useMatchStore((s) => s.mainlineDepth);
	const bots = useAtomValue(botsAtom);

	const {
		goToStart,
		goToEnd,
		stepForwardOrAdvance,
		stepBack,
		togglePlay,
		goLive,
		setPlaybackSpeed,
		resetToPreview,
		advanceTurn,
		confirmStagedMoves,
	} = useMatchStore.getState();

	const hasStagedMoves = useMatchStore(
		(s) => s.stagedMoves.player1 !== null || s.stagedMoves.player2 !== null,
	);

	const cursorDepth = useCursorDepth();
	const totalTurns = useMainlineLength();
	const isAtTip = useIsAtTip();

	const [confirmOpen, setConfirmOpen] = useState(false);

	const hasMatch = matchPhase !== "idle";

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
					Back to Setup
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
					{mode === "auto" && (
						<ActionIcon variant="subtle" size="sm" onClick={togglePlay}>
							{autoplay ? (
								<IconPlayerPause size={16} />
							) : (
								<IconPlayerPlay size={16} />
							)}
						</ActionIcon>
					)}
					{mode === "step" && (
						<Button
							size="compact-xs"
							variant="light"
							color={hasStagedMoves ? "green" : undefined}
							leftSection={<IconPlayerTrackNext size={14} />}
							onClick={
								hasStagedMoves
									? () => confirmStagedMoves()
									: () => advanceTurn()
							}
							disabled={!isAtTip || matchPhase !== "playing"}
						>
							{hasStagedMoves ? "Confirm" : "Advance"}
						</Button>
					)}
					{mode === "auto" &&
						matchPhase === "playing" &&
						cursorDepth < mainlineDepth && (
							<Badge
								size="sm"
								color="red"
								variant="filled"
								style={{ cursor: "pointer" }}
								onClick={goLive}
							>
								LIVE
							</Badge>
						)}
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={stepForwardOrAdvance}
						disabled={mode === "auto" ? cursorDepth >= totalTurns : false}
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
					{mode === "auto" && (
						<Select
							size="xs"
							data={SPEED_OPTIONS}
							value={String(playbackSpeed)}
							onChange={(v) => v && setPlaybackSpeed(Number(v))}
							allowDeselect={false}
							style={{ width: 80 }}
						/>
					)}
					<Text size="xs" c="dimmed" ml={4}>
						{matchPhase === "preprocessing"
							? "Preprocessing"
							: mode === "step"
								? `Turn ${cursorDepth}`
								: `Turn ${cursorDepth} / ${totalTurns}`}
					</Text>
				</Group>
			)}
			<Group gap="sm">
				{disconnection && (
					<Badge color="yellow" variant="filled" size="lg">
						{resolveBotName(
							disconnection.player === "Player1" ? player1BotId : player2BotId,
							bots,
							disconnection.player === "Player1" ? "Rat" : "Python",
						)}{" "}
						disconnected:{" "}
						{DISCONNECT_REASONS[disconnection.reason] ?? disconnection.reason}
					</Badge>
				)}
				{analysisError && (
					<Badge color="yellow" variant="filled" size="lg">
						{analysisError}
					</Badge>
				)}
				{error && (
					<Badge color="red" variant="filled" size="lg">
						{error}
					</Badge>
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
