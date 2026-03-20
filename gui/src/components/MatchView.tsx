import { Stack } from "@mantine/core";
import { useAtomValue } from "jotai";
import { useEffect, useRef } from "react";
import { events, commands } from "../bindings";
import { RANDOM_BOT_ID, botsAtom } from "../stores/botConfigAtom";
import { matchConfigAtom } from "../stores/matchConfigAtom";
import {
	getNodeAtPath,
	useCurrentBotInfo,
	useDisplayState,
	useIsAtTip,
	useMatchStore,
} from "../stores/matchStore";
import MatchToolbar from "./MatchToolbar";
import MazeColumn from "./MazeColumn";
import ResultBanner from "./ResultBanner";
import ThinkingPanel from "./ThinkingPanel";
import NotationPanel from "./notation/NotationPanel";

type Props = {
	onNewMatch: () => void;
};

export default function MatchView({ onNewMatch }: Props) {
	const matchIdRef = useRef<number>(-1);
	const hasAutoStarted = useRef(false);
	const displayState = useDisplayState();
	const bots = useAtomValue(botsAtom);
	const matchConfig = useAtomValue(matchConfigAtom);
	const matchConfigRef = useRef(matchConfig);
	matchConfigRef.current = matchConfig;

	const botInfo = useCurrentBotInfo();
	const mode = useMatchStore((s) => s.mode);
	const matchPhase = useMatchStore((s) => s.matchPhase);
	const autoplay = useMatchStore((s) => s.autoplay);
	const playbackSpeed = useMatchStore((s) => s.playbackSpeed);
	const previewError = useMatchStore((s) => s.previewError);
	const player1BotId = useMatchStore((s) => s.player1BotId);
	const player2BotId = useMatchStore((s) => s.player2BotId);
	const isAtTip = useIsAtTip();

	const {
		onMatchStarted,
		onTurnPlayed,
		onMatchOver,
		onBotInfo,
		onError,
		onDisconnect,
		advanceCursor,
		startAnalysis,
		stopAnalysis,
		advanceTurn,
		goToStart,
		goToEnd,
		stepForward,
		stepBack,
		cycleVariation,
		togglePlay,
		goLive,
		clearStagedMoves,
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

	// Auto-advance cursor during playback (disabled in step mode)
	// biome-ignore lint/correctness/useExhaustiveDependencies: advanceCursor is a stable ref from getState()
	useEffect(() => {
		if (!autoplay || mode === "step") return;
		const id = setInterval(() => {
			advanceCursor();
		}, playbackSpeed);
		return () => clearInterval(id);
	}, [autoplay, playbackSpeed, mode]);

	// Reactive analysis: auto-start analysis when cursor lands on a tree tip in step mode
	// biome-ignore lint/correctness/useExhaustiveDependencies: startAnalysis is a stable ref from getState()
	useEffect(() => {
		if (mode !== "step" || matchPhase !== "playing" || !isAtTip) return;
		const timer = setTimeout(() => {
			startAnalysis();
		}, 50);
		return () => clearTimeout(timer);
	}, [mode, matchPhase, isAtTip]);

	// Keyboard shortcuts
	// biome-ignore lint/correctness/useExhaustiveDependencies: navigation actions are stable refs from getState()
	useEffect(() => {
		const handler = (e: KeyboardEvent) => {
			const state = useMatchStore.getState();
			if (state.matchPhase === "idle" || state.matchPhase === "connecting")
				return;

			const tag = (e.target as HTMLElement)?.tagName;
			if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;

			switch (e.key) {
				case "Escape":
					clearStagedMoves();
					break;
				case "ArrowLeft":
					e.preventDefault();
					stepBack();
					break;
				case "ArrowRight": {
					e.preventDefault();
					const s = useMatchStore.getState();
					if (s.mode === "step" && s.root) {
						const node = getNodeAtPath(s.root, s.cursor);
						if (!node || node.children.length === 0) {
							advanceTurn();
							break;
						}
					}
					stepForward();
					break;
				}
				case "ArrowUp":
					if (state.mode === "step") {
						e.preventDefault();
						cycleVariation(-1);
					}
					break;
				case "ArrowDown":
					if (state.mode === "step") {
						e.preventDefault();
						cycleVariation(1);
					}
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
					if (state.mode === "step") {
						state.analyzing ? stopAnalysis() : startAnalysis();
					} else {
						togglePlay();
					}
					break;
				case "l":
					if (state.matchPhase === "playing") {
						e.preventDefault();
						goLive();
					}
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
		useMatchStore.getState().beginConnecting();
		const { previewSeed, mode: currentMode } = useMatchStore.getState();
		const cfg = matchConfigRef.current;
		const configWithSeed = {
			...cfg,
			seed: cfg.seed ?? previewSeed,
		};
		const res = await commands.startMatch(
			p1.cmd,
			p2.cmd,
			p1.workingDir,
			p2.workingDir,
			configWithSeed,
			currentMode === "step" ? true : null,
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
			{matchPhase === "finished" && <ResultBanner />}
			<div style={{ flex: 1, overflow: "hidden", display: "flex" }}>
				<MazeColumn
					connecting={matchPhase === "connecting"}
					displayState={displayState}
					previewError={previewError}
					hasMatch={hasMatch}
				/>
				{hasMatch && (
					<div
						style={{
							width: 320,
							flexShrink: 0,
							display: "flex",
							flexDirection: "column",
							borderLeft: "1px solid var(--mantine-color-dark-4)",
							overflow: "hidden",
						}}
					>
						<ThinkingPanel botInfo={botInfo ?? {}} />
						<NotationPanel />
					</div>
				)}
			</div>
		</Stack>
	);
}
