import { Center, Stack, Text } from "@mantine/core";
import { useAtomValue } from "jotai";
import { useEffect, useRef } from "react";
import { events, commands } from "../bindings";
import { RANDOM_BOT_ID, botsAtom } from "../stores/botConfigAtom";
import { matchConfigAtom } from "../stores/matchConfigAtom";
import {
	useCurrentBotInfo,
	useDisplayState,
	useMatchStore,
} from "../stores/matchStore";
import MatchToolbar from "./MatchToolbar";
import MazeRenderer from "./MazeRenderer";
import ThinkingPanel from "./ThinkingPanel";

type Props = {
	onNewMatch: () => void;
};

export default function MatchView({ onNewMatch }: Props) {
	const matchIdRef = useRef<number>(-1);
	const hasAutoStarted = useRef(false);
	const displayState = useDisplayState();
	const bots = useAtomValue(botsAtom);
	const matchConfig = useAtomValue(matchConfigAtom);

	const botInfo = useCurrentBotInfo();
	const matchPhase = useMatchStore((s) => s.matchPhase);
	const following = useMatchStore((s) => s.following);
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
		goToStart,
		goToEnd,
		stepForward,
		stepBack,
		togglePlay,
	} = useMatchStore.getState();

	// Event listeners — wire Tauri events to store actions
	// biome-ignore lint/correctness/useExhaustiveDependencies: callbacks from getState() are stable refs
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
	// biome-ignore lint/correctness/useExhaustiveDependencies: advanceCursor is a stable ref from getState()
	useEffect(() => {
		if (!following) return;
		const id = setInterval(() => {
			advanceCursor();
		}, playbackSpeed);
		return () => clearInterval(id);
	}, [following, playbackSpeed]);

	// Keyboard shortcuts
	// biome-ignore lint/correctness/useExhaustiveDependencies: navigation actions are stable refs from getState()
	useEffect(() => {
		const handler = (e: KeyboardEvent) => {
			const phase = useMatchStore.getState().matchPhase;
			if (phase === "idle" || phase === "connecting") return;

			const tag = (e.target as HTMLElement)?.tagName;
			if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;

			switch (e.key) {
				case "ArrowLeft":
					e.preventDefault();
					stepBack();
					break;
				case "ArrowRight":
					e.preventDefault();
					stepForward();
					break;
				case "Home":
					e.preventDefault();
					goToStart();
					break;
				case "End":
					e.preventDefault();
					goToEnd();
					break;
				case " ":
					e.preventDefault();
					togglePlay();
					break;
			}
		};
		window.addEventListener("keydown", handler);
		return () => window.removeEventListener("keydown", handler);
	}, []);

	const resolveBotId = (botId: string) => {
		if (botId === RANDOM_BOT_ID)
			return { cmd: RANDOM_BOT_ID, workingDir: null };
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
		useMatchStore.setState({ matchPhase: "connecting" });
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

	// Auto-start on mount — if we're idle and both bots are selected
	// biome-ignore lint/correctness/useExhaustiveDependencies: intentional one-shot on mount
	useEffect(() => {
		if (hasAutoStarted.current) return;
		if (matchPhase === "idle" && player1BotId && player2BotId) {
			hasAutoStarted.current = true;
			handleStart();
		}
	}, [matchPhase, player1BotId, player2BotId]);

	const hasMatch = matchPhase !== "idle";

	return (
		<Stack h="100%" gap={0}>
			<MatchToolbar onNewMatch={onNewMatch} />
			<div style={{ flex: 1, overflow: "hidden", display: "flex" }}>
				<div style={{ flex: 1, minWidth: 0 }}>
					{matchPhase === "connecting" ? (
						<Center h="100%">
							<Text c="dimmed" size="sm">
								Connecting bots...
							</Text>
						</Center>
					) : displayState ? (
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
								Generating preview...
							</Text>
						</Center>
					)}
				</div>
				{hasMatch && <ThinkingPanel botInfo={botInfo ?? {}} />}
			</div>
		</Stack>
	);
}
