import type {
	Coord,
	Direction,
	PlayerSide,
	WallEntry,
} from "../bindings/generated";
import { SLOT_PALETTE } from "../lib/botPalette";
import type { BotInfoMap } from "../stores/botInfo";
import { currentLines, parseBotInfoKey } from "../stores/botInfo";
import { gameToCellCenter } from "./layout";
import type { LayoutMetrics } from "./layout";

// ── Visual tuning constants ──────────────────────────────────────

export const SENDER_OFFSET_FRACTION = 0.06;
export const TARGET_RADIUS_FRACTION = 0.35;
export const TARGET_STROKE_WIDTH = 3;
export const ARROWHEAD_SIZE_FACTOR = 2.5;

type ThicknessTier = { maxGap: number; minPx: number; fraction: number };

const THICKNESS_TIERS: ThicknessTier[] = [
	{ maxGap: 2.5, minPx: 4, fraction: 0.12 },
	{ maxGap: 5.0, minPx: 3, fraction: 0.08 },
	{ maxGap: Number.POSITIVE_INFINITY, minPx: 2, fraction: 0.05 },
];

// ── Types ────────────────────────────────────────────────────────

export type PvArrow = {
	sender: PlayerSide;
	subject: PlayerSide;
	multipv: number;
	points: { x: number; y: number }[];
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
	visibleSenders?: Set<PlayerSide>;
};

// ── Per-sender pixel offset (disambiguates overlapping arrows) ───

const SENDER_OFFSET: Record<PlayerSide, { dx: number; dy: number }> = {
	Player1: { dx: -1, dy: -1 },
	Player2: { dx: 1, dy: 1 },
};

// ── Direction → delta ────────────────────────────────────────────

export const DIRECTION_DELTA: Record<Direction, { dx: number; dy: number }> = {
	Up: { dx: 0, dy: 1 },
	Down: { dx: 0, dy: -1 },
	Left: { dx: -1, dy: 0 },
	Right: { dx: 1, dy: 0 },
	Stay: { dx: 0, dy: 0 },
};

// ── Wall set ─────────────────────────────────────────────────────

export function wallKey(a: Coord, b: Coord): string {
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
	for (const tier of THICKNESS_TIERS) {
		if (scoreGap < tier.maxGap) {
			return Math.max(tier.minPx, cellSize * tier.fraction);
		}
	}
	const last = THICKNESS_TIERS[THICKNESS_TIERS.length - 1];
	return Math.max(last.minPx, cellSize * last.fraction);
}

// ── Main builder ────────────────────────────────────────────────

export function buildPvOverlay(
	botInfo: BotInfoMap,
	player1Pos: Coord,
	player2Pos: Coord,
	wallSet: Set<string>,
	mazeW: number,
	mazeH: number,
	layout: LayoutMetrics,
	options?: PvOverlayOptions,
): PvOverlayData {
	const maxSegments = options?.maxSegments ?? Number.POSITIVE_INFINITY;
	const maxLines = options?.maxLines ?? 3;
	const visibleSenders = options?.visibleSenders;

	const arrows: PvArrow[] = [];
	const targets: TargetMarker[] = [];

	const posFor = (subject: PlayerSide): Coord =>
		subject === "Player1" ? player1Pos : player2Pos;

	for (const [key, bucket] of Object.entries(botInfo)) {
		const { sender, subject } = parseBotInfoKey(key);

		if (visibleSenders && !visibleSenders.has(sender)) continue;

		const lines = currentLines(bucket);
		if (lines.length === 0) continue;

		const palette = SLOT_PALETTE[sender];
		const bestScore = lines[0].score;
		const startPos = posFor(subject);
		const senderOff = SENDER_OFFSET[sender];
		const offPx = layout.cellSize * SENDER_OFFSET_FRACTION;
		const ox = senderOff.dx * offPx;
		const oy = senderOff.dy * offPx;

		for (const line of lines.slice(0, maxLines)) {
			const path = reconstructPath(
				startPos,
				line.pv,
				wallSet,
				mazeW,
				mazeH,
				maxSegments,
			);

			// Need at least 2 points for a drawable arrow
			if (path.length < 2) continue;

			const points = path.map((coord) => {
				const c = gameToCellCenter(coord, layout);
				return { x: c.x + ox, y: c.y + oy };
			});

			const isBest = line.multipv === 1;
			const scoreGap =
				line.score != null && bestScore != null
					? Math.abs(line.score - bestScore)
					: 0;

			arrows.push({
				sender,
				subject,
				multipv: line.multipv,
				points,
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
				cx: center.x + ox,
				cy: center.y + oy,
				radius: layout.cellSize * TARGET_RADIUS_FRACTION,
				color: palette.saturated,
			});
		}
	}

	// Pre-sort: alternatives first, best lines last (drawn on top)
	arrows.sort((a, b) => b.multipv - a.multipv);

	return { arrows, targets };
}
