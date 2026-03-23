import { useCallback, useMemo, useRef, useState } from "react";
import type { Coord, Direction, PlayerSide } from "../bindings/generated";
import { SLOT_PALETTE } from "../lib/botPalette";
import { directionFromDelta } from "../lib/directions";
import type { AssetMap } from "../renderer/assets";
import {
	type LayoutMetrics,
	canvasToGame,
	gameToCellCenter,
	isOnPlayer,
} from "../renderer/layout";
import { DIRECTION_DELTA, wallKey } from "../renderer/pvArrows";
import type { DisplayState } from "../stores/matchStore";
import { useMatchStore } from "../stores/matchStore";

type Props = {
	layout: LayoutMetrics;
	wallSet: Set<string>;
	gameState: DisplayState;
	assetMap: AssetMap;
};

type DragState = {
	player: "player1" | "player2";
	side: PlayerSide;
	validTargets: { direction: Direction; target: Coord }[];
	ghostX: number;
	ghostY: number;
	nearestTarget: Coord | null;
};

function validMoves(
	pos: Coord,
	wallSet: Set<string>,
	mazeW: number,
	mazeH: number,
): { direction: Direction; target: Coord }[] {
	const result: { direction: Direction; target: Coord }[] = [];
	for (const [dir, delta] of Object.entries(DIRECTION_DELTA)) {
		if (dir === "Stay") continue;
		const target: Coord = { x: pos.x + delta.dx, y: pos.y + delta.dy };
		if (target.x < 0 || target.x >= mazeW || target.y < 0 || target.y >= mazeH)
			continue;
		if (wallSet.has(wallKey(pos, target))) continue;
		result.push({ direction: dir as Direction, target });
	}
	return result;
}

function dist2(
	a: { x: number; y: number },
	b: { x: number; y: number },
): number {
	return (a.x - b.x) ** 2 + (a.y - b.y) ** 2;
}

export default function DragOverlay({
	layout,
	wallSet,
	gameState,
	assetMap,
}: Props) {
	const [drag, setDrag] = useState<DragState | null>(null);
	const overlayRef = useRef<HTMLDivElement>(null);
	const stagedMoves = useMatchStore((s) => s.stagedMoves);
	const { stageMove } = useMatchStore.getState();

	const getPointerPos = useCallback(
		(e: React.PointerEvent): { x: number; y: number } | null => {
			const rect = overlayRef.current?.getBoundingClientRect();
			if (!rect) return null;
			return { x: e.clientX - rect.left, y: e.clientY - rect.top };
		},
		[],
	);

	const handlePointerDown = useCallback(
		(e: React.PointerEvent) => {
			const pos = getPointerPos(e);
			if (!pos) return;

			// Hit-test both players. Check player1 (rat) first.
			const players: {
				key: "player1" | "player2";
				side: PlayerSide;
				pos: Coord;
			}[] = [
				{ key: "player1", side: "Player1", pos: gameState.player1.position },
				{ key: "player2", side: "Player2", pos: gameState.player2.position },
			];

			for (const p of players) {
				if (isOnPlayer(pos, p.pos, layout)) {
					const targets = validMoves(
						p.pos,
						wallSet,
						gameState.width,
						gameState.height,
					);
					(e.target as HTMLElement).setPointerCapture(e.pointerId);
					setDrag({
						player: p.key,
						side: p.side,
						validTargets: targets,
						ghostX: pos.x,
						ghostY: pos.y,
						nearestTarget: null,
					});
					return;
				}
			}
		},
		[gameState, layout, wallSet, getPointerPos],
	);

	const handlePointerMove = useCallback(
		(e: React.PointerEvent) => {
			if (!drag) return;
			const pos = getPointerPos(e);
			if (!pos) return;

			// Find nearest valid target cell
			let nearest: Coord | null = null;
			let nearestDist = Number.POSITIVE_INFINITY;
			for (const { target } of drag.validTargets) {
				const center = gameToCellCenter(target, layout);
				const d = dist2(pos, center);
				if (d < nearestDist) {
					nearestDist = d;
					nearest = target;
				}
			}

			// Only highlight if within 1.2 cells of center
			const snapThreshold = (layout.cellSize * 1.2) ** 2;
			if (nearestDist > snapThreshold) nearest = null;

			setDrag((prev) =>
				prev
					? { ...prev, ghostX: pos.x, ghostY: pos.y, nearestTarget: nearest }
					: null,
			);
		},
		[drag, layout, getPointerPos],
	);

	const handlePointerUp = useCallback(
		(e: React.PointerEvent) => {
			if (!drag) return;
			const pos = getPointerPos(e);

			if (pos && drag.nearestTarget) {
				const playerPos =
					drag.player === "player1"
						? gameState.player1.position
						: gameState.player2.position;
				const dx = drag.nearestTarget.x - playerPos.x;
				const dy = drag.nearestTarget.y - playerPos.y;
				const dir = directionFromDelta(dx, dy);
				if (dir !== "Stay") {
					stageMove(drag.player, dir);
				}
			}

			setDrag(null);
		},
		[drag, gameState, stageMove, getPointerPos],
	);

	// Build staged-move arrow data
	const stagedArrows = useMemo(() => {
		const arrows: {
			key: string;
			from: { x: number; y: number };
			to: { x: number; y: number };
			color: string;
		}[] = [];

		const pairs: { key: "player1" | "player2"; side: PlayerSide }[] = [
			{ key: "player1", side: "Player1" },
			{ key: "player2", side: "Player2" },
		];

		for (const { key, side } of pairs) {
			const dir = stagedMoves[key];
			if (!dir) continue;
			const pos =
				key === "player1"
					? gameState.player1.position
					: gameState.player2.position;
			const delta = DIRECTION_DELTA[dir];
			const target: Coord = { x: pos.x + delta.dx, y: pos.y + delta.dy };
			const from = gameToCellCenter(pos, layout);
			const to = gameToCellCenter(target, layout);
			arrows.push({
				key,
				from,
				to,
				color: SLOT_PALETTE[side].saturated,
			});
		}

		return arrows;
	}, [stagedMoves, gameState, layout]);

	return (
		<div
			ref={overlayRef}
			onPointerDown={handlePointerDown}
			onPointerMove={handlePointerMove}
			onPointerUp={handlePointerUp}
			style={{
				position: "absolute",
				top: 0,
				left: 0,
				width: layout.canvasWidth,
				height: layout.canvasHeight,
				cursor: drag ? "grabbing" : "default",
				touchAction: "none",
			}}
		>
			<svg
				width={layout.canvasWidth}
				height={layout.canvasHeight}
				style={{ position: "absolute", top: 0, left: 0, pointerEvents: "none" }}
				role="img"
				aria-label="Move overlay"
			>
				{/* Valid target highlights during drag */}
				{drag?.validTargets.map(({ target }) => {
					const center = gameToCellCenter(target, layout);
					const isNearest =
						drag.nearestTarget?.x === target.x &&
						drag.nearestTarget?.y === target.y;
					return (
						<circle
							key={`${target.x},${target.y}`}
							cx={center.x}
							cy={center.y}
							r={layout.cellSize * 0.25}
							fill={
								isNearest
									? SLOT_PALETTE[drag.side].saturated
									: SLOT_PALETTE[drag.side].pale
							}
						/>
					);
				})}

				{/* Staged move arrows */}
				{stagedArrows.map(({ key, from, to, color }) => (
					<line
						key={key}
						x1={from.x}
						y1={from.y}
						x2={to.x}
						y2={to.y}
						stroke={color}
						strokeWidth={Math.max(3, layout.cellSize * 0.08)}
						strokeLinecap="round"
						markerEnd={`url(#staged-arrow-${key})`}
					/>
				))}

				{/* Arrowhead markers for staged moves */}
				<defs>
					{stagedArrows.map(({ key, color }) => (
						<marker
							key={key}
							id={`staged-arrow-${key}`}
							markerWidth="8"
							markerHeight="8"
							refX="6"
							refY="4"
							orient="auto"
						>
							<path d="M 0 1 L 6 4 L 0 7 Z" fill={color} />
						</marker>
					))}
				</defs>
			</svg>

			{/* Ghost sprite during drag */}
			{drag && (
				<img
					src={
						drag.player === "player1"
							? assetMap.rat.neutral.src
							: assetMap.python.neutral.src
					}
					alt=""
					style={{
						position: "absolute",
						left: drag.ghostX - layout.cellSize * 0.3,
						top: drag.ghostY - layout.cellSize * 0.3,
						width: layout.cellSize * 0.6,
						height: layout.cellSize * 0.6,
						opacity: 0.5,
						pointerEvents: "none",
					}}
				/>
			)}
		</div>
	);
}
