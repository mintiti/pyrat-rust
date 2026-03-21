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
	hasBranch: boolean;
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
const NEUTRAL_CSS = `rgb(${NEUTRAL.join(",")})`;
const MUD: [number, number, number] = [160, 110, 55];

// ── Data hook ────────────────────────────────────────────────────

function useTimelineData(): TurnEntry[] | null {
	const cursor = useMatchStore((s) => s.cursor);
	const mainlineDepth = useMatchStore((s) => s.mainlineDepth);

	// biome-ignore lint/correctness/useExhaustiveDependencies: mainlineDepth triggers recomputation as turns arrive; root is read via getState() to avoid re-renders
	return useMemo(() => {
		const root = useMatchStore.getState().root;
		if (!root) return null;

		const entries: TurnEntry[] = [];
		let node: GameNode = root;
		let prevP1Score = root.player1.score;
		let prevP2Score = root.player2.score;

		// Phase 1: walk cursor path
		for (const idx of cursor) {
			if (idx < 0 || idx >= node.children.length) break;
			const child = node.children[idx];
			entries.push({
				turn: child.turn,
				p1MudTurns: child.player1.mud_turns,
				p2MudTurns: child.player2.mud_turns,
				p1ScoreDelta: child.player1.score - prevP1Score,
				p2ScoreDelta: child.player2.score - prevP2Score,
				hasBranch: child.children.length > 1,
			});
			prevP1Score = child.player1.score;
			prevP2Score = child.player2.score;
			node = child;
		}

		// Phase 2: extend along children[0] past cursor
		while (node.children.length > 0) {
			const child = node.children[0];
			entries.push({
				turn: child.turn,
				p1MudTurns: child.player1.mud_turns,
				p2MudTurns: child.player2.mud_turns,
				p1ScoreDelta: child.player1.score - prevP1Score,
				p2ScoreDelta: child.player2.score - prevP2Score,
				hasBranch: child.children.length > 1,
			});
			prevP1Score = child.player1.score;
			prevP2Score = child.player2.score;
			node = child;
		}

		return entries.length > 0 ? entries : null;
	}, [cursor, mainlineDepth]);
}

// ── Constants ────────────────────────────────────────────────────

export const TIMELINE_HEIGHT = 48;
const P1_MID = 13;
const P2_MID = 35;
const BASE_THICKNESS = 5;
const MUD_SWELL_MAX = 8;
const TICK_INTERVAL = 50;
const ICON_SIZE = 16;
const ICON_GAP = 8;
const CHEESE_H = 10;

// ── Lane helper ──────────────────────────────────────────────────

function buildLaneRect(
	key: string,
	x: number,
	midY: number,
	mudTurns: number,
): React.ReactNode {
	const mudFraction = Math.min(mudTurns / 5, 1);
	const thickness = BASE_THICKNESS + Math.round(mudFraction * MUD_SWELL_MAX);
	const color = mudTurns > 0 ? lerpRgb(NEUTRAL, MUD, mudFraction) : NEUTRAL_CSS;
	return (
		<rect
			key={key}
			x={x}
			y={midY - thickness / 2}
			width={1}
			height={thickness}
			fill={color}
		/>
	);
}

function buildCheeseMarker(
	key: string,
	x: number,
	midY: number,
	scoreDelta: number,
): React.ReactNode | null {
	if (scoreDelta <= 0) return null;
	return (
		<rect
			key={key}
			x={x}
			y={midY - CHEESE_H / 2}
			width={1}
			height={CHEESE_H}
			fill="#fcc419"
			opacity={scoreDelta >= 1 ? 1 : 0.6}
		/>
	);
}

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

	// Memoize SVG elements — only rebuild when data changes, not on cursor moves
	const { rects, cheeseMarkers, ticks, branchMarkers } = useMemo(() => {
		if (!data)
			return { rects: [], cheeseMarkers: [], ticks: [], branchMarkers: [] };

		const r: React.ReactNode[] = [];
		const c: React.ReactNode[] = [];

		for (let i = 0; i < data.length; i++) {
			const e = data[i];
			const x = e.turn;

			r.push(buildLaneRect(`p1-${i}`, x, P1_MID, e.p1MudTurns));
			r.push(buildLaneRect(`p2-${i}`, x, P2_MID, e.p2MudTurns));

			const c1 = buildCheeseMarker(`c1-${i}`, x, P1_MID, e.p1ScoreDelta);
			const c2 = buildCheeseMarker(`c2-${i}`, x, P2_MID, e.p2ScoreDelta);
			if (c1) c.push(c1);
			if (c2) c.push(c2);
		}

		const t: React.ReactNode[] = [];
		for (let tick = TICK_INTERVAL; tick <= data.length; tick += TICK_INTERVAL) {
			t.push(
				<line
					key={`tick-${tick}`}
					x1={tick}
					y1={0}
					x2={tick}
					y2={3}
					stroke="var(--mantine-color-dark-3)"
					strokeWidth={0.5}
				/>,
			);
		}

		const b: React.ReactNode[] = [];
		for (let i = 0; i < data.length; i++) {
			if (data[i].hasBranch) {
				b.push(
					<circle
						key={`br-${i}`}
						cx={data[i].turn}
						cy={4}
						r={1.5}
						fill="var(--mantine-color-blue-5)"
						opacity={0.7}
					/>,
				);
			}
		}

		return { rects: r, cheeseMarkers: c, ticks: t, branchMarkers: b };
	}, [data]);

	if (!data || !layout) return null;

	const mazePixelW = layout.cellSize * layout.mazeW;
	const showIcons = layout.mazeX > ICON_SIZE + ICON_GAP;
	const cursorX = cursorDepth + 0.5;

	return (
		<div
			style={{
				height: TIMELINE_HEIGHT,
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
				viewBox={`0 0 ${viewW} ${TIMELINE_HEIGHT}`}
				preserveAspectRatio="none"
				style={{
					display: "block",
					cursor: "pointer",
					position: "absolute",
					left: layout.mazeX,
					width: mazePixelW,
					height: TIMELINE_HEIGHT,
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

				{/* Branch markers */}
				{branchMarkers}

				{/* Cursor line */}
				<line
					x1={cursorX}
					y1={0}
					x2={cursorX}
					y2={TIMELINE_HEIGHT}
					stroke="white"
					strokeWidth={1}
					opacity={0.8}
				/>
			</svg>
		</div>
	);
}
