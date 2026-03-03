import { Center, Stack, Text } from "@mantine/core";
import { useAtomValue } from "jotai";
import { useEffect, useRef, useState } from "react";
import { events, commands } from "../bindings";
import { botsAtom } from "../stores/botConfigAtom";
import { matchConfigAtom } from "../stores/matchConfigAtom";
import {
	generatePreview,
	useDisplayState,
	useMatchStore,
} from "../stores/matchStore";
import MatchConfigDrawer from "./MatchConfigDrawer";
import MatchToolbar from "./MatchToolbar";
import MazeRenderer from "./MazeRenderer";

type Props = {
	onNavigate: (view: "match" | "bots") => void;
};

export default function MatchView({ onNavigate }: Props) {
	const matchIdRef = useRef<number>(-1);
	const displayState = useDisplayState();
	const bots = useAtomValue(botsAtom);
	const matchConfig = useAtomValue(matchConfigAtom);
	const [configDrawerOpen, setConfigDrawerOpen] = useState(false);

	const viewerMode = useMatchStore((s) => s.viewerMode);
	const playbackSpeed = useMatchStore((s) => s.playbackSpeed);
	const previewError = useMatchStore((s) => s.previewError);
	const player1BotId = useMatchStore((s) => s.player1BotId);
	const player2BotId = useMatchStore((s) => s.player2BotId);

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

	// Generate maze preview when idle
	useEffect(() => {
		if (viewerMode !== "empty") return;
		generatePreview(matchConfig);
	}, [viewerMode, matchConfig]);

	const resolveBotId = (botId: string) => {
		if (botId === "__random__") return { cmd: "__random__", workingDir: null };
		const bot = bots.find((b) => b.id === botId);
		if (!bot) return null;
		return { cmd: bot.command, workingDir: bot.working_dir };
	};

	const handleStart = async () => {
		if (!player1BotId || !player2BotId) return;
		const p1 = resolveBotId(player1BotId);
		const p2 = resolveBotId(player2BotId);
		if (!p1 || !p2) {
			useMatchStore.getState().onError("Selected bot no longer exists.");
			return;
		}
		useMatchStore.setState({ error: null, result: null, disconnection: null });
		const { previewSeed } = useMatchStore.getState();
		const configWithSeed = {
			...matchConfig,
			seed: matchConfig.seed ?? previewSeed,
		};
		const res = await commands.startMatch(
			p1.cmd,
			p2.cmd,
			p1.workingDir,
			p2.workingDir,
			configWithSeed,
		);
		if (res.status === "error") {
			useMatchStore.getState().onError(res.error);
		}
	};

	return (
		<Stack h="100vh" gap={0}>
			<MatchToolbar
				onStart={handleStart}
				onNavigate={onNavigate}
				onOpenConfig={() => setConfigDrawerOpen(true)}
			/>
			<MatchConfigDrawer
				opened={configDrawerOpen}
				onClose={() => setConfigDrawerOpen(false)}
			/>
			<div style={{ flex: 1, overflow: "hidden" }}>
				{displayState ? (
					<MazeRenderer gameState={displayState} />
				) : previewError ? (
					<Center h="100%">
						<Text c="red" size="sm">
							{previewError}
						</Text>
					</Center>
				) : (
					<Center h="100%">
						<Text c="dimmed" size="sm">
							Generating preview…
						</Text>
					</Center>
				)}
			</div>
		</Stack>
	);
}
