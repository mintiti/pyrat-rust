import { Group, SegmentedControl, Stack, Switch, Text } from "@mantine/core";
import { useState } from "react";
import { commands } from "../bindings";
import type { Coord, MazeState as TauriMazeState } from "../bindings/generated";
import MazeRenderer from "../components/MazeRenderer";
import type { MazeState } from "../types/game";
import { testMazeState } from "./testData";

/** Convert specta's {x,y} Coord objects to [x,y] tuples used by renderer types. */
function convertTauriState(t: TauriMazeState): MazeState {
	const c = (coord: Coord): [number, number] => [coord.x, coord.y];
	return {
		width: t.width,
		height: t.height,
		turn: t.turn,
		maxTurns: t.max_turns,
		totalCheese: t.total_cheese,
		walls: t.walls.map((w) => ({ from: c(w.from), to: c(w.to) })),
		mud: t.mud.map((m) => ({ from: c(m.from), to: c(m.to), cost: m.cost })),
		cheese: t.cheese.map(c),
		player1: { position: c(t.player1.position), score: t.player1.score },
		player2: { position: c(t.player2.position), score: t.player2.score },
	};
}

type Source = "hardcoded" | "tauri";

export default function DevHarness() {
	const [source, setSource] = useState<Source>("hardcoded");
	const [showIndices, setShowIndices] = useState(false);
	const [tauriState, setTauriState] = useState<MazeState | null>(null);
	const [error, setError] = useState<string | null>(null);

	const handleSourceChange = (val: string) => {
		const next = val as Source;
		setSource(next);

		if (next === "tauri" && !tauriState) {
			commands
				.getGameState()
				.then((result) => {
					if (result.status === "ok") {
						setTauriState(convertTauriState(result.data));
					} else {
						setError(result.error);
					}
				})
				.catch((err: unknown) => setError(String(err)));
		}
	};

	const gameState =
		source === "tauri" && tauriState ? tauriState : testMazeState;

	return (
		<Stack h="100vh" gap={0}>
			{/* Toolbar */}
			<Group
				p="xs"
				justify="space-between"
				style={{
					borderBottom: "1px solid var(--mantine-color-dark-4)",
					flexShrink: 0,
				}}
			>
				<Group gap="md">
					<SegmentedControl
						size="xs"
						value={source}
						onChange={handleSourceChange}
						data={[
							{ label: "Hardcoded", value: "hardcoded" },
							{ label: "Tauri", value: "tauri" },
						]}
					/>
					<Switch
						size="xs"
						label="Cell indices"
						checked={showIndices}
						onChange={(e) => setShowIndices(e.currentTarget.checked)}
					/>
				</Group>
				<Group gap="md">
					<Text size="xs" c="dimmed">
						{gameState.width}x{gameState.height}
					</Text>
					<Text size="xs" c="dimmed">
						Turn {gameState.turn}/{gameState.maxTurns}
					</Text>
					<Text size="xs" c="dimmed">
						Cheese: {gameState.cheese.length}/{gameState.totalCheese}
					</Text>
					{error && (
						<Text size="xs" c="red">
							{error}
						</Text>
					)}
				</Group>
			</Group>

			{/* Renderer fills remaining space */}
			<div style={{ flex: 1, overflow: "hidden" }}>
				<MazeRenderer gameState={gameState} showCellIndices={showIndices} />
			</div>
		</Stack>
	);
}
