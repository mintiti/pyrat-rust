import { ScrollArea, Text } from "@mantine/core";
import { useEffect, useMemo, useRef } from "react";
import type { Direction } from "../../bindings/generated";
import {
	type GameNode,
	getNodeAtPath,
	useMatchStore,
} from "../../stores/matchStore";
import TurnCell, { type NotationEntry } from "./TurnCell";

/** Depth-first walk of the game tree, producing flat notation entries. */
function buildEntries(root: GameNode): NotationEntry[] {
	const entries: NotationEntry[] = [];

	function walk(node: GameNode, path: number[], variationLevel: number) {
		for (let i = 0; i < node.children.length; i++) {
			const child = node.children[i];
			const childPath = [...path, i];
			if (!child.actions) continue;

			const prevScore1 = node.player1.score;
			const prevScore2 = node.player2.score;
			const scoreDelta =
				child.player1.score - prevScore1 + (child.player2.score - prevScore2);
			const inMud = child.player1.mud_turns > 0 || child.player2.mud_turns > 0;

			const highlight: NotationEntry["highlight"] =
				scoreDelta > 0 ? "cheese" : inMud ? "mud" : null;

			const isVariation = i > 0;
			const level = isVariation ? variationLevel + 1 : variationLevel;

			entries.push({
				path: childPath,
				turn: child.turn,
				actions: child.actions as { player1: Direction; player2: Direction },
				highlight,
				variationLevel: level,
				isVariationStart: isVariation,
			});

			walk(child, childPath, level);
		}
	}

	walk(root, [], 0);
	return entries;
}

function pathEq(a: number[], b: number[]): boolean {
	return a.length === b.length && a.every((v, i) => v === b[i]);
}

export default function NotationPanel() {
	const root = useMatchStore((s) => s.root);
	const cursor = useMatchStore((s) => s.cursor);
	const mainlineDepth = useMatchStore((s) => s.mainlineDepth);
	const goToPath = useMatchStore.getState().goToPath;
	const scrollRef = useRef<HTMLDivElement>(null);
	const currentRef = useRef<HTMLDivElement>(null);

	// mainlineDepth is an extra dep to rebuild entries when mainline grows (root ref changes cover branch additions via Immer)
	// biome-ignore lint/correctness/useExhaustiveDependencies: mainlineDepth triggers rebuild as turns arrive
	const entries = useMemo(() => {
		if (!root) return [];
		return buildEntries(root);
	}, [root, mainlineDepth]);

	// Auto-scroll to current entry when cursor changes
	// biome-ignore lint/correctness/useExhaustiveDependencies: cursor drives scroll position via currentRef
	useEffect(() => {
		if (currentRef.current) {
			currentRef.current.scrollIntoView({ block: "nearest" });
		}
	}, [cursor]);

	if (!root || entries.length === 0) {
		return (
			<ScrollArea
				style={{
					flex: 1,
					borderTop: "1px solid var(--mantine-color-dark-4)",
				}}
				p="sm"
			>
				<Text size="xs" c="dimmed" ta="center">
					No moves yet.
				</Text>
			</ScrollArea>
		);
	}

	// Group variations with left border
	let prevLevel = 0;

	return (
		<ScrollArea
			viewportRef={scrollRef}
			style={{
				flex: 1,
				borderTop: "1px solid var(--mantine-color-dark-4)",
			}}
			p="xs"
		>
			{entries.map((entry, i) => {
				const isCurrent = pathEq(entry.path, cursor);
				const enteringVariation =
					entry.isVariationStart && entry.variationLevel > prevLevel;
				prevLevel = entry.variationLevel;

				return (
					<div
						key={entry.path.join(",")}
						ref={isCurrent ? currentRef : undefined}
						style={
							entry.variationLevel > 0
								? {
										borderLeft: "2px solid #404040",
										paddingLeft: 5,
										marginLeft: Math.max(0, (entry.variationLevel - 1) * 12),
										marginTop: enteringVariation ? 4 : 0,
									}
								: { marginTop: enteringVariation ? 4 : 0 }
						}
					>
						<TurnCell
							entry={entry}
							isCurrent={isCurrent}
							onClick={() => goToPath(entry.path)}
						/>
					</div>
				);
			})}
		</ScrollArea>
	);
}
