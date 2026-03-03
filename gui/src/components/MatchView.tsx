import { Center, Stack, Text } from "@mantine/core";
import { useEffect, useRef } from "react";
import { events, commands } from "../bindings";
import { useDisplayState, useMatchStore } from "../stores/matchStore";
import MatchToolbar from "./MatchToolbar";
import MazeRenderer from "./MazeRenderer";

export default function MatchView() {
	const matchIdRef = useRef<number>(-1);
	const displayState = useDisplayState();

	const viewerMode = useMatchStore((s) => s.viewerMode);
	const playbackSpeed = useMatchStore((s) => s.playbackSpeed);
	const player1Cmd = useMatchStore((s) => s.player1Cmd);
	const player2Cmd = useMatchStore((s) => s.player2Cmd);

	const {
		onMatchStarted,
		onTurnPlayed,
		onMatchOver,
		onBotInfo,
		onError,
		onDisconnect,
		advanceCursor,
	} = useMatchStore.getState();

	// Event listeners — wire Tauri events to store actions
	useEffect(() => {
		const unlisteners = [
			events.matchStartedEvent.listen((e) => {
				matchIdRef.current = e.payload.match_id;
				onMatchStarted(e.payload.maze, e.payload.match_id);
			}),
			events.turnPlayedEvent.listen((e) => {
				if (e.payload.match_id !== matchIdRef.current) return;
				onTurnPlayed(e.payload);
			}),
			events.matchOverEvent.listen((e) => {
				if (e.payload.match_id !== matchIdRef.current) return;
				onMatchOver(e.payload);
			}),
			events.matchErrorEvent.listen((e) => {
				if (e.payload.match_id !== matchIdRef.current) return;
				onError(e.payload.message);
			}),
			events.botDisconnectedEvent.listen((e) => {
				if (e.payload.match_id !== matchIdRef.current) return;
				onDisconnect(e.payload);
			}),
			events.botInfoEvent.listen((e) => {
				if (e.payload.match_id !== matchIdRef.current) return;
				onBotInfo(e.payload);
			}),
		];

		return () => {
			for (const p of unlisteners) {
				p.then((unlisten) => unlisten());
			}
		};
	}, []);

	// Auto-advance cursor during playback
	useEffect(() => {
		if (viewerMode !== "playing") return;
		const id = setInterval(() => {
			advanceCursor();
		}, playbackSpeed);
		return () => clearInterval(id);
	}, [viewerMode, playbackSpeed]);

	const handleStart = async () => {
		if (!player1Cmd || !player2Cmd) return;
		useMatchStore.setState({ error: null, result: null, disconnection: null });
		const res = await commands.startMatch(player1Cmd, player2Cmd);
		if (res.status === "error") {
			useMatchStore.getState().onError(res.error);
		}
	};

	return (
		<Stack h="100vh" gap={0}>
			<MatchToolbar onStart={handleStart} />
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
