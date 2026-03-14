import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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

// Mantine dark-theme blue-8 ≈ #1971c2, green-8 ≈ #2f9e44
const BLUE: [number, number, number] = [25, 113, 194];
const GREEN: [number, number, number] = [47, 158, 68];
const MUD: [number, number, number] = [139, 90, 43];

// Background lane color (dark-7 area)
const LANE_BG = "var(--mantine-color-dark-7)";

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

		// Walk the mainline (children[0] chain)
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

// ── Component ────────────────────────────────────────────────────

const TOTAL_HEIGHT = 48;
const LANE_HEIGHT = 20;
const P1_Y = 3; // Top of rat lane
const DIVIDER_Y = 24;
const P2_Y = 25; // Top of python lane
const MUD_SWELL = 6; // Max additional height for mud rects
const TICK_INTERVAL = 50;

export default function EventTimeline() {
	const data = useTimelineData();
	const cursorDepth = useMatchStore((s) => s.cursor.length);
	const maxTurns = useMatchStore((s) => s.mazeConfig?.max_turns ?? 300);
	const svgRef = useRef<SVGSVGElement>(null);
	const [dragging, setDragging] = useState(false);

	const goToTurn = useMatchStore.getState().goToTurn;
	const totalTurns = data?.length ?? 0;

	const turnFromClientX = useCallback(
		(clientX: number) => {
			const svg = svgRef.current;
			if (!svg) return 0;
			const rect = svg.getBoundingClientRect();
			const fraction = (clientX - rect.left) / rect.width;
			const turn = Math.round(fraction * maxTurns);
			return Math.max(0, Math.min(turn, totalTurns));
		},
		[maxTurns, totalTurns],
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

	// Clean up dragging state if pointer leaves window
	useEffect(() => {
		if (!dragging) return;
		const handleUp = () => setDragging(false);
		window.addEventListener("pointerup", handleUp);
		return () => window.removeEventListener("pointerup", handleUp);
	}, [dragging]);

	if (!data) return null;

	// Compute mud color + swell for each entry
	const rects: React.ReactNode[] = [];
	const cheeseMarkers: React.ReactNode[] = [];
	const ticks: React.ReactNode[] = [];

	for (let i = 0; i < data.length; i++) {
		const e = data[i];
		const x = e.turn; // 1-indexed turn, but that's fine for positioning

		// Rat lane (P1) — grows upward on mud
		const p1MudFraction = Math.min(e.p1MudTurns / 5, 1);
		const p1Swell = Math.round(p1MudFraction * MUD_SWELL);
		const p1Color =
			e.p1MudTurns > 0
				? lerpRgb(BLUE, MUD, p1MudFraction)
				: lerpRgb(BLUE, BLUE, 0);
		rects.push(
			<rect
				key={`p1-${i}`}
				x={x}
				y={P1_Y - p1Swell}
				width={1}
				height={LANE_HEIGHT + p1Swell}
				fill={p1Color}
			/>,
		);

		// Python lane (P2) — grows downward on mud
		const p2MudFraction = Math.min(e.p2MudTurns / 5, 1);
		const p2Swell = Math.round(p2MudFraction * MUD_SWELL);
		const p2Color =
			e.p2MudTurns > 0
				? lerpRgb(GREEN, MUD, p2MudFraction)
				: lerpRgb(GREEN, GREEN, 0);
		rects.push(
			<rect
				key={`p2-${i}`}
				x={x}
				y={P2_Y}
				width={1}
				height={LANE_HEIGHT + p2Swell}
				fill={p2Color}
			/>,
		);

		// Cheese markers
		if (e.p1ScoreDelta > 0) {
			cheeseMarkers.push(
				<circle
					key={`c1-${i}`}
					cx={x + 0.5}
					cy={P1_Y + LANE_HEIGHT / 2}
					r={1.5}
					fill="#fcc419"
					opacity={e.p1ScoreDelta >= 1 ? 1 : 0.6}
				/>,
			);
		}
		if (e.p2ScoreDelta > 0) {
			cheeseMarkers.push(
				<circle
					key={`c2-${i}`}
					cx={x + 0.5}
					cy={P2_Y + LANE_HEIGHT / 2}
					r={1.5}
					fill="#fcc419"
					opacity={e.p2ScoreDelta >= 1 ? 1 : 0.6}
				/>,
			);
		}
	}

	// Tick marks every TICK_INTERVAL turns
	for (let t = TICK_INTERVAL; t <= maxTurns; t += TICK_INTERVAL) {
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

	// Cursor line
	const cursorX = cursorDepth + 0.5;

	return (
		<div
			style={{
				borderTop: "1px solid var(--mantine-color-dark-4)",
				height: TOTAL_HEIGHT,
				flexShrink: 0,
				background: LANE_BG,
			}}
		>
			{/* biome-ignore lint/a11y/noSvgWithoutTitle: decorative timeline, not informational */}
			<svg
				ref={svgRef}
				viewBox={`0 0 ${maxTurns} ${TOTAL_HEIGHT}`}
				preserveAspectRatio="none"
				width="100%"
				height={TOTAL_HEIGHT}
				style={{ display: "block", cursor: "pointer" }}
				onPointerDown={handlePointerDown}
				onPointerMove={handlePointerMove}
				onPointerUp={handlePointerUp}
			>
				{/* Background lanes */}
				<rect
					x={0}
					y={P1_Y}
					width={maxTurns}
					height={LANE_HEIGHT}
					fill="var(--mantine-color-dark-8)"
				/>
				<rect
					x={0}
					y={P2_Y}
					width={maxTurns}
					height={LANE_HEIGHT}
					fill="var(--mantine-color-dark-8)"
				/>

				{/* Divider */}
				<line
					x1={0}
					y1={DIVIDER_Y}
					x2={maxTurns}
					y2={DIVIDER_Y}
					stroke="var(--mantine-color-dark-4)"
					strokeWidth={0.5}
				/>

				{/* Tick marks */}
				{ticks}

				{/* Turn rects */}
				{rects}

				{/* Cheese markers */}
				{cheeseMarkers}

				{/* Progress edge — faint line at the end of played turns */}
				{totalTurns > 0 && totalTurns < maxTurns && (
					<line
						x1={totalTurns + 1}
						y1={0}
						x2={totalTurns + 1}
						y2={TOTAL_HEIGHT}
						stroke="var(--mantine-color-dark-3)"
						strokeWidth={0.5}
						strokeDasharray="2 2"
					/>
				)}

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
