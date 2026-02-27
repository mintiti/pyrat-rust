import { Center, Stack, Text } from "@mantine/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { commands, events } from "../bindings";
import type {
	BotDisconnectedEvent,
	MatchOverEvent,
	MazeState,
	TurnPlayedEvent,
} from "../bindings/generated";
import MazeRenderer from "./MazeRenderer";
import MatchToolbar from "./MatchToolbar";

export type MatchStatus = "idle" | "running" | "finished";

export default function MatchView() {
	const [player1Cmd, setPlayer1Cmd] = useState("");
	const [player2Cmd, setPlayer2Cmd] = useState("");
	const [status, setStatus] = useState<MatchStatus>("idle");
	const [result, setResult] = useState<MatchOverEvent | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [disconnection, setDisconnection] =
		useState<BotDisconnectedEvent | null>(null);

	// Accumulate-then-navigate replay state
	const [baseMaze, setBaseMaze] = useState<MazeState | null>(null);
	const [turns, setTurns] = useState<TurnPlayedEvent[]>([]);
	const [currentTurn, setCurrentTurn] = useState(-1);
	const [isPlaying, setIsPlaying] = useState(false);
	const [pendingResult, setPendingResult] = useState<MatchOverEvent | null>(
		null,
	);

	const matchIdRef = useRef<number>(-1);

	// Ref so the auto-advance interval can read current length without re-creating
	const turnsLenRef = useRef(0);
	turnsLenRef.current = turns.length;

	// Derive rendered state from baseMaze + turns[cursor]
	const displayState = useMemo(() => {
		if (!baseMaze) return null;
		if (currentTurn < 0 || turns.length === 0) return baseMaze;
		const t = turns[currentTurn];
		if (!t) return baseMaze;
		return {
			...baseMaze,
			turn: t.turn,
			player1: t.player1,
			player2: t.player2,
			cheese: t.cheese,
		};
	}, [baseMaze, turns, currentTurn]);

	// Auto-advance cursor at ~5fps when playing
	useEffect(() => {
		if (!isPlaying) return;
		const id = setInterval(() => {
			setCurrentTurn((prev) => {
				const maxIdx = turnsLenRef.current - 1;
				if (prev >= maxIdx) return prev;
				return prev + 1;
			});
		}, 200);
		return () => clearInterval(id);
	}, [isPlaying]);

	// Apply result once cursor reaches the last turn
	useEffect(() => {
		if (!pendingResult) return;
		if (currentTurn >= 0 && currentTurn >= turns.length - 1) {
			setResult(pendingResult);
			setPendingResult(null);
			setStatus("finished");
			setIsPlaying(false);
		}
	}, [currentTurn, turns.length, pendingResult]);

	// Event listeners — pure accumulation, no pacing
	useEffect(() => {
		const unlisteners = [
			events.matchStartedEvent.listen((e) => {
				matchIdRef.current = e.payload.match_id;
				setBaseMaze(e.payload.maze);
				setTurns([]);
				setCurrentTurn(-1);
				setIsPlaying(true);
				setStatus("running");
				setResult(null);
				setPendingResult(null);
				setError(null);
				setDisconnection(null);
			}),
			events.turnPlayedEvent.listen((e) => {
				if (e.payload.match_id !== matchIdRef.current) return;
				setTurns((prev) => [...prev, e.payload]);
			}),
			events.matchOverEvent.listen((e) => {
				if (e.payload.match_id !== matchIdRef.current) return;
				setPendingResult(e.payload);
			}),
			events.matchErrorEvent.listen((e) => {
				if (e.payload.match_id !== matchIdRef.current) return;
				setError(e.payload.message);
				setStatus("idle");
			}),
			events.botDisconnectedEvent.listen((e) => {
				if (e.payload.match_id !== matchIdRef.current) return;
				setDisconnection(e.payload);
			}),
		];

		return () => {
			for (const p of unlisteners) {
				p.then((unlisten) => unlisten());
			}
		};
	}, []);

	const handleStart = async () => {
		setError(null);
		setResult(null);
		setDisconnection(null);
		const res = await commands.startMatch(player1Cmd, player2Cmd);
		if (res.status === "error") {
			setError(res.error);
		}
	};

	// Navigation callbacks
	const onGoToStart = useCallback(() => {
		setCurrentTurn(-1);
		setIsPlaying(false);
	}, []);

	const onGoToEnd = useCallback(() => {
		setCurrentTurn(turns.length - 1);
		setIsPlaying(false);
	}, [turns.length]);

	const onStepForward = useCallback(() => {
		setCurrentTurn((prev) => Math.min(prev + 1, turns.length - 1));
		setIsPlaying(false);
	}, [turns.length]);

	const onStepBack = useCallback(() => {
		setCurrentTurn((prev) => Math.max(prev - 1, -1));
		setIsPlaying(false);
	}, []);

	const onTogglePlay = useCallback(() => {
		setIsPlaying((prev) => !prev);
	}, []);

	return (
		<Stack h="100vh" gap={0}>
			<MatchToolbar
				player1Cmd={player1Cmd}
				player2Cmd={player2Cmd}
				onPlayer1CmdChange={setPlayer1Cmd}
				onPlayer2CmdChange={setPlayer2Cmd}
				onStart={handleStart}
				status={status}
				result={result}
				error={error}
				disconnection={disconnection}
				currentTurn={currentTurn}
				totalTurns={turns.length}
				isPlaying={isPlaying}
				onGoToStart={onGoToStart}
				onGoToEnd={onGoToEnd}
				onStepForward={onStepForward}
				onStepBack={onStepBack}
				onTogglePlay={onTogglePlay}
			/>
			<div style={{ flex: 1, overflow: "hidden" }}>
				{displayState ? (
					<MazeRenderer gameState={displayState} />
				) : (
					<Center h="100%">
						<Text c="dimmed" size="sm">
							Enter bot commands and click Start to begin a match.
						</Text>
					</Center>
				)}
			</div>
		</Stack>
	);
}
