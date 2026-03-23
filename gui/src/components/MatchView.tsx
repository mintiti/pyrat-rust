import { Stack } from "@mantine/core";
import { useAtomValue } from "jotai";
import { useEffect, useMemo, useRef } from "react";
import { events, commands } from "../bindings";
import type { BotOptionValue } from "../bindings/generated";
import {
	RANDOM_BOT_ID,
	botsAtom,
	discoveredBotsAtom,
	resolveDiscoveredBot,
} from "../stores/botConfigAtom";
import { matchConfigAtom } from "../stores/matchConfigAtom";
import {
	buildAnalysisPosition,
	useCurrentBotInfo,
	useDisplayState,
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
	const discovered = useAtomValue(discoveredBotsAtom);
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
	const analyzing = useMatchStore((s) => s.analyzing);
	const cursor = useMatchStore((s) => s.cursor);
	const cursorKey = useMemo(() => cursor.join(","), [cursor]);

	const {
		onMatchStarted,
		onPreprocessingStarted,
		onTurnPlayed,
		onMatchOver,
		onBotInfo,
		onError,
		onDisconnect,
		advanceCursor,
		goToStart,
		goToEnd,
		stepForwardOrAdvance,
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
			events.preprocessingStartedEvent.listen((e) => {
				if (e.payload.match_id !== matchIdRef.current) return;
				onPreprocessingStarted();
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

	// Cursor-follows-analysis: start analysis on the current position, restart on cursor change
	// biome-ignore lint/correctness/useExhaustiveDependencies: cursorKey is a derived stable dep
	useEffect(() => {
		if (mode !== "step" || matchPhase !== "playing" || !analyzing) return;

		const { root, cursor } = useMatchStore.getState();
		if (!root) return;
		const position = buildAnalysisPosition(root, cursor);
		if (!position) return;

		const timer = setTimeout(() => {
			commands.startAnalysisTurn(position).catch(console.error);
		}, 50);

		return () => {
			clearTimeout(timer);
			commands.stopAnalysisTurn().catch(() => {});
		};
	}, [mode, matchPhase, analyzing, cursorKey]);

	// Keyboard shortcuts
	// biome-ignore lint/correctness/useExhaustiveDependencies: navigation actions are stable refs from getState()
	useEffect(() => {
		const handler = (e: KeyboardEvent) => {
			const state = useMatchStore.getState();
			if (
				state.matchPhase === "idle" ||
				state.matchPhase === "connecting" ||
				state.matchPhase === "preprocessing"
			)
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
				case "ArrowRight":
					e.preventDefault();
					stepForwardOrAdvance();
					break;
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
						state.setAnalyzing(!state.analyzing);
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
			return { cmd: RANDOM_BOT_ID, workingDir: null, agentId: "random" };
		const bot = resolveDiscoveredBot(botId, discovered);
		if (!bot) return null;
		return {
			cmd: bot.run_command,
			workingDir: bot.working_dir,
			agentId: bot.agent_id,
		};
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
		const { player1Options, player2Options } = useMatchStore.getState();
		const toValues = (opts: Record<string, string>): BotOptionValue[] =>
			Object.entries(opts).map(([name, value]) => ({ name, value }));

		const res = await commands.startMatch(
			p1.cmd,
			p2.cmd,
			p1.workingDir,
			p2.workingDir,
			p1.agentId,
			p2.agentId,
			configWithSeed,
			{
				player1: toValues(player1Options),
				player2: toValues(player2Options),
				step_mode: currentMode === "step",
			},
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
					setupPhase={
						matchPhase === "connecting"
							? "connecting"
							: matchPhase === "preprocessing"
								? "preprocessing"
								: null
					}
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
