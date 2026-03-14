import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import pythonIconUrl from "../assets/sprites/players/python/neutral.png";
import ratIconUrl from "../assets/sprites/players/rat/neutral.png";
import type { LayoutMetrics } from "../renderer/layout";
import type { GameNode } from "../stores/matchStore";
import { useMatchStore } from "../stores/matchStore";

// ── Types ────────────────────────────────────────────────────────

interface TurnEntry {
	turn: number;
	p1MudTurns: number;
	p2MudTurns: number;
	p1ScoreDelta: number;
	p2ScoreDelta: number;
}

// ── Color helpers ────────────────────────────────────────────────

function lerpRgb(
	[r1, g1, b1]: [number, number, number],
	[r2, g2, b2]: [number, number, number],
	t: number,
): string {
	const r = Math.round(r1 + (r2 - r1) * t);
	const g = Math.round(g1 + (g2 - g1) * t);
	const b = Math.round(b1 + (b2 - b1) * t);
	return `rgb(${r},${g},${b})`;
}

const NEUTRAL: [number, number, number] = [130, 130, 145];
const MUD: [number, number, number] = [160, 110, 55];

// ── Data hook ────────────────────────────────────────────────────

function useTimelineData(): TurnEntry[] | null {
	const mainlineDepth = useMatchStore((s) => s.mainlineDepth);

	// biome-ignore lint/correctness/useExhaustiveDependencies: mainlineDepth triggers recomputation as turns arrive; root is read via getState() to avoid re-renders
	return useMemo(() => {
		const root = useMatchStore.getState().root;
		if (!root) return null;

		const entries: TurnEntry[] = [];
		let node: GameNode = root;
		let prevP1Score = root.player1.score;
		let prevP2Score = root.player2.score;

		while (node.children.length > 0) {
			const child = node.children[0];
			entries.push({
				turn: child.turn,
				p1MudTurns: child.player1.mud_turns,
				p2MudTurns: child.player2.mud_turns,
				p1ScoreDelta: child.player1.score - prevP1Score,
				p2ScoreDelta: child.player2.score - prevP2Score,
			});
			prevP1Score = child.player1.score;
			prevP2Score = child.player2.score;
			node = child;
		}

		return entries.length > 0 ? entries : null;
	}, [mainlineDepth]);
}

// ── Constants ────────────────────────────────────────────────────

const TOTAL_HEIGHT = 48;
const P1_MID = 13;
const P2_MID = 35;
const BASE_THICKNESS = 5;
const MUD_SWELL_MAX = 8;
const TICK_INTERVAL = 50;
const ICON_SIZE = 16;
const ICON_GAP = 8;

// ── Component ────────────────────────────────────────────────────

type Props = {
	layout: LayoutMetrics | null;
};

export default function EventTimeline({ layout }: Props) {
	const data = useTimelineData();
	const cursorDepth = useMatchStore((s) => s.cursor.length);
	const svgRef = useRef<SVGSVGElement>(null);
	const [dragging, setDragging] = useState(false);

	const goToTurn = useMatchStore.getState().goToTurn;
	const totalTurns = data?.length ?? 0;

	// ViewBox width adapts to played turns — bars always fill the strip
	const viewW = Math.max(totalTurns + 1, 2);

	const turnFromClientX = useCallback(
		(clientX: number) => {
			const svg = svgRef.current;
			if (!svg) return 0;
			const rect = svg.getBoundingClientRect();
			const fraction = (clientX - rect.left) / rect.width;
			const turn = Math.round(fraction * viewW);
			return Math.max(0, Math.min(turn, totalTurns));
		},
		[viewW, totalTurns],
	);

	const handlePointerDown = useCallback(
		(e: React.PointerEvent) => {
			e.preventDefault();
			setDragging(true);
			(e.target as Element).setPointerCapture(e.pointerId);
			goToTurn(turnFromClientX(e.clientX));
		},
		[turnFromClientX, goToTurn],
	);

	const handlePointerMove = useCallback(
		(e: React.PointerEvent) => {
			if (!dragging) return;
			goToTurn(turnFromClientX(e.clientX));
		},
		[dragging, turnFromClientX, goToTurn],
	);

	const handlePointerUp = useCallback(() => {
		setDragging(false);
	}, []);

	useEffect(() => {
		if (!dragging) return;
		const handleUp = () => setDragging(false);
		window.addEventListener("pointerup", handleUp);
		return () => window.removeEventListener("pointerup", handleUp);
	}, [dragging]);

	if (!data || !layout) return null;

	const mazePixelW = layout.cellSize * layout.mazeW;
	const showIcons = layout.mazeX > ICON_SIZE + ICON_GAP;

	// Build SVG elements
	const rects: React.ReactNode[] = [];
	const cheeseMarkers: React.ReactNode[] = [];
	const ticks: React.ReactNode[] = [];

	for (let i = 0; i < data.length; i++) {
		const e = data[i];
		const x = e.turn;

		// P1 lane — thin rect centered on P1_MID
		const p1MudFraction = Math.min(e.p1MudTurns / 5, 1);
		const p1Thickness =
			BASE_THICKNESS + Math.round(p1MudFraction * MUD_SWELL_MAX);
		const p1Color =
			e.p1MudTurns > 0
				? lerpRgb(NEUTRAL, MUD, p1MudFraction)
				: lerpRgb(NEUTRAL, NEUTRAL, 0);
		rects.push(
			<rect
				key={`p1-${i}`}
				x={x}
				y={P1_MID - p1Thickness / 2}
				width={1}
				height={p1Thickness}
				fill={p1Color}
			/>,
		);

		// P2 lane — thin rect centered on P2_MID
		const p2MudFraction = Math.min(e.p2MudTurns / 5, 1);
		const p2Thickness =
			BASE_THICKNESS + Math.round(p2MudFraction * MUD_SWELL_MAX);
		const p2Color =
			e.p2MudTurns > 0
				? lerpRgb(NEUTRAL, MUD, p2MudFraction)
				: lerpRgb(NEUTRAL, NEUTRAL, 0);
		rects.push(
			<rect
				key={`p2-${i}`}
				x={x}
				y={P2_MID - p2Thickness / 2}
				width={1}
				height={p2Thickness}
				fill={p2Color}
			/>,
		);

		// Cheese markers — rects not circles, since preserveAspectRatio="none" would stretch circles
		const CHEESE_H = 10;
		if (e.p1ScoreDelta > 0) {
			cheeseMarkers.push(
				<rect
					key={`c1-${i}`}
					x={x}
					y={P1_MID - CHEESE_H / 2}
					width={1}
					height={CHEESE_H}
					fill="#fcc419"
					opacity={e.p1ScoreDelta >= 1 ? 1 : 0.6}
				/>,
			);
		}
		if (e.p2ScoreDelta > 0) {
			cheeseMarkers.push(
				<rect
					key={`c2-${i}`}
					x={x}
					y={P2_MID - CHEESE_H / 2}
					width={1}
					height={CHEESE_H}
					fill="#fcc419"
					opacity={e.p2ScoreDelta >= 1 ? 1 : 0.6}
				/>,
			);
		}
	}

	// Tick marks — only within played range
	for (let t = TICK_INTERVAL; t <= totalTurns; t += TICK_INTERVAL) {
		ticks.push(
			<line
				key={`tick-${t}`}
				x1={t}
				y1={0}
				x2={t}
				y2={3}
				stroke="var(--mantine-color-dark-3)"
				strokeWidth={0.5}
			/>,
		);
	}

	const cursorX = cursorDepth + 0.5;

	return (
		<div
			style={{
				height: TOTAL_HEIGHT,
				flexShrink: 0,
				position: "relative",
				background: "var(--mantine-color-dark-7)",
			}}
		>
			{/* Player icons in the gutter */}
			{showIcons && (
				<>
					<img
						src={ratIconUrl}
						alt=""
						style={{
							position: "absolute",
							left: layout.mazeX - ICON_SIZE - ICON_GAP,
							top: P1_MID - ICON_SIZE / 2,
							width: ICON_SIZE,
							height: ICON_SIZE,
							imageRendering: "pixelated",
						}}
					/>
					<img
						src={pythonIconUrl}
						alt=""
						style={{
							position: "absolute",
							left: layout.mazeX - ICON_SIZE - ICON_GAP,
							top: P2_MID - ICON_SIZE / 2,
							width: ICON_SIZE,
							height: ICON_SIZE,
							imageRendering: "pixelated",
						}}
					/>
				</>
			)}

			{/* biome-ignore lint/a11y/noSvgWithoutTitle: decorative timeline, not informational */}
			<svg
				ref={svgRef}
				viewBox={`0 0 ${viewW} ${TOTAL_HEIGHT}`}
				preserveAspectRatio="none"
				style={{
					display: "block",
					cursor: "pointer",
					position: "absolute",
					left: layout.mazeX,
					width: mazePixelW,
					height: TOTAL_HEIGHT,
				}}
				onPointerDown={handlePointerDown}
				onPointerMove={handlePointerMove}
				onPointerUp={handlePointerUp}
			>
				{/* Guide lines */}
				<line
					x1={0}
					y1={P1_MID}
					x2={viewW}
					y2={P1_MID}
					stroke="var(--mantine-color-dark-5)"
					strokeWidth={0.3}
				/>
				<line
					x1={0}
					y1={P2_MID}
					x2={viewW}
					y2={P2_MID}
					stroke="var(--mantine-color-dark-5)"
					strokeWidth={0.3}
				/>

				{/* Tick marks */}
				{ticks}

				{/* Turn rects */}
				{rects}

				{/* Cheese markers */}
				{cheeseMarkers}

				{/* Cursor line */}
				<line
					x1={cursorX}
					y1={0}
					x2={cursorX}
					y2={TOTAL_HEIGHT}
					stroke="white"
					strokeWidth={1}
					opacity={0.8}
				/>
			</svg>
		</div>
	);
}
