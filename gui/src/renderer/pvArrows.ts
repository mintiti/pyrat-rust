import type {
	BotInfoEvent,
	Coord,
	Direction,
	PlayerSide,
	WallEntry,
} from "../bindings/generated";
import type { BotInfoMap } from "../stores/matchStore";
import { currentLines } from "../stores/matchStore";
import { gameToCellCenter } from "./layout";
import type { LayoutMetrics } from "./layout";

// ── Types ────────────────────────────────────────────────────────

export type ArrowSegment = {
	fromX: number;
	fromY: number;
	toX: number;
	toY: number;
};

export type PvArrow = {
	sender: PlayerSide;
	subject: PlayerSide;
	multipv: number;
	segments: ArrowSegment[];
	color: string;
	thickness: number;
	isBest: boolean;
};

export type TargetMarker = {
	cx: number;
	cy: number;
	radius: number;
	color: string;
};

export type PvOverlayData = {
	arrows: PvArrow[];
	targets: TargetMarker[];
};

export type PvOverlayOptions = {
	maxSegments?: number;
	maxLines?: number;
};

// ── Palette ──────────────────────────────────────────────────────

type ColorPair = { saturated: string; pale: string };

const SENDER_PALETTE: Record<PlayerSide, ColorPair> = {
	Player1: {
		saturated: "rgba(66, 135, 245, 0.85)",
		pale: "rgba(66, 135, 245, 0.35)",
	},
	Player2: {
		saturated: "rgba(72, 199, 142, 0.85)",
		pale: "rgba(72, 199, 142, 0.35)",
	},
};

// ── Direction → delta ────────────────────────────────────────────

const DIRECTION_DELTA: Record<Direction, { dx: number; dy: number }> = {
	Up: { dx: 0, dy: 1 },
	Down: { dx: 0, dy: -1 },
	Left: { dx: -1, dy: 0 },
	Right: { dx: 1, dy: 0 },
	Stay: { dx: 0, dy: 0 },
};

// ── Wall set ─────────────────────────────────────────────────────

function wallKey(a: Coord, b: Coord): string {
	// Normalize: smaller coord first
	if (a.x < b.x || (a.x === b.x && a.y < b.y)) {
		return `${a.x},${a.y}|${b.x},${b.y}`;
	}
	return `${b.x},${b.y}|${a.x},${a.y}`;
}

export function buildWallSet(walls: WallEntry[]): Set<string> {
	const set = new Set<string>();
	for (const w of walls) {
		set.add(wallKey(w.from, w.to));
	}
	return set;
}

// ── Path reconstruction ─────────────────────────────────────────

export function reconstructPath(
	start: Coord,
	pv: Direction[],
	wallSet: Set<string>,
	mazeW: number,
	mazeH: number,
	maxSegments: number,
): Coord[] {
	const path: Coord[] = [start];
	let cur = start;

	for (const dir of pv) {
		if (path.length - 1 >= maxSegments) break;

		const delta = DIRECTION_DELTA[dir];
		if (delta.dx === 0 && delta.dy === 0) continue; // Skip STAY

		const next: Coord = { x: cur.x + delta.dx, y: cur.y + delta.dy };

		// Boundary check
		if (next.x < 0 || next.x >= mazeW || next.y < 0 || next.y >= mazeH) break;

		// Wall check
		if (wallSet.has(wallKey(cur, next))) break;

		path.push(next);
		cur = next;
	}

	return path;
}

// ── Thickness tiers ─────────────────────────────────────────────

function arrowThickness(scoreGap: number, cellSize: number): number {
	if (scoreGap < 2.5) return Math.max(4, cellSize * 0.12);
	if (scoreGap < 5.0) return Math.max(3, cellSize * 0.08);
	return Math.max(2, cellSize * 0.05);
}

// ── Main builder ────────────────────────────────────────────────

export function buildPvOverlay(
	botInfo: BotInfoMap,
	player1Pos: Coord,
	player2Pos: Coord,
	walls: WallEntry[],
	mazeW: number,
	mazeH: number,
	layout: LayoutMetrics,
	options?: PvOverlayOptions,
): PvOverlayData {
	const maxSegments = options?.maxSegments ?? 5;
	const maxLines = options?.maxLines ?? 3;

	const ws = buildWallSet(walls);
	const arrows: PvArrow[] = [];
	const targets: TargetMarker[] = [];

	const posFor = (subject: PlayerSide): Coord =>
		subject === "Player1" ? player1Pos : player2Pos;

	for (const [key, bucket] of Object.entries(botInfo)) {
		const lines = currentLines(bucket);
		if (lines.length === 0) continue;

		const [sender, subject] = key.split(":") as [PlayerSide, PlayerSide];
		const palette = SENDER_PALETTE[sender];
		const bestScore = lines[0].score; // multipv=1 is first after sort
		const startPos = posFor(subject);

		for (const line of lines.slice(0, maxLines)) {
			const path = reconstructPath(
				startPos,
				line.pv,
				ws,
				mazeW,
				mazeH,
				maxSegments,
			);

			// Need at least 2 points for a segment
			if (path.length < 2) continue;

			const segments: ArrowSegment[] = [];
			for (let i = 0; i < path.length - 1; i++) {
				const from = gameToCellCenter(path[i], layout);
				const to = gameToCellCenter(path[i + 1], layout);
				segments.push({
					fromX: from.x,
					fromY: from.y,
					toX: to.x,
					toY: to.y,
				});
			}

			const isBest = line.multipv === 1;
			const scoreGap = Math.abs(line.score - bestScore);

			arrows.push({
				sender,
				subject,
				multipv: line.multipv,
				segments,
				color: isBest ? palette.saturated : palette.pale,
				thickness: arrowThickness(scoreGap, layout.cellSize),
				isBest,
			});
		}

		// Target marker for best line
		const bestLine = lines[0];
		if (bestLine.multipv === 1 && bestLine.target) {
			const center = gameToCellCenter(bestLine.target, layout);
			targets.push({
				cx: center.x,
				cy: center.y,
				radius: layout.cellSize * 0.35,
				color: palette.saturated,
			});
		}
	}

	return { arrows, targets };
}
