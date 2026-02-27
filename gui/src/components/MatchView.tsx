import { Center, Stack, Text } from "@mantine/core";
import { useEffect, useState } from "react";
import { commands, events } from "../bindings";
import type { MatchOverEvent, MazeState } from "../bindings/generated";
import MazeRenderer from "./MazeRenderer";
import MatchToolbar from "./MatchToolbar";

type MatchStatus = "idle" | "running" | "finished";

export default function MatchView() {
	const [player1Cmd, setPlayer1Cmd] = useState("");
	const [player2Cmd, setPlayer2Cmd] = useState("");
	const [mazeState, setMazeState] = useState<MazeState | null>(null);
	const [status, setStatus] = useState<MatchStatus>("idle");
	const [result, setResult] = useState<MatchOverEvent | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		const unlisteners = [
			events.matchStartedEvent.listen((e) => {
				setMazeState(e.payload);
				setStatus("running");
				setResult(null);
				setError(null);
			}),
			events.turnPlayedEvent.listen((e) => {
				setMazeState((prev) => {
					if (!prev) return prev;
					return {
						...prev,
						turn: e.payload.turn,
						player1: e.payload.player1,
						player2: e.payload.player2,
						cheese: e.payload.cheese,
					};
				});
			}),
			events.matchOverEvent.listen((e) => {
				setResult(e.payload);
				setStatus("finished");
			}),
			events.matchErrorEvent.listen((e) => {
				setError(e.payload.message);
				setStatus("idle");
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
		const res = await commands.startMatch(player1Cmd, player2Cmd);
		if (res.status === "error") {
			setError(res.error);
		}
	};

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
			/>
			<div style={{ flex: 1, overflow: "hidden" }}>
				{mazeState ? (
					<MazeRenderer gameState={mazeState} />
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
