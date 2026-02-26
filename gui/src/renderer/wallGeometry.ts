import type { Wall } from "../types/game";
import type { LayoutMetrics } from "./layout";

export type WallSegment = {
	x: number;
	y: number;
	horizontal: boolean;
};

export type Corner = {
	x: number;
	y: number;
};

/**
 * Compute wall segment pixel positions from the wall list + border walls.
 *
 * Internal walls come from the game's wall list.
 * Border walls are added along the maze perimeter.
 *
 * A wall between two cells that differ in x → vertical segment at the shared edge.
 * A wall between two cells that differ in y → horizontal segment at the shared edge.
 */
export function computeWallSegments(
	walls: Wall[],
	layout: LayoutMetrics,
	mazeW: number,
	mazeH: number,
): WallSegment[] {
	const { mazeX, mazeY, cellSize } = layout;
	const segments: WallSegment[] = [];

	// Internal walls
	for (const wall of walls) {
		const [fx, fy] = wall.from;
		const [tx, ty] = wall.to;

		if (fx !== tx) {
			// Vertical wall between horizontally adjacent cells
			const edgeX = mazeX + Math.max(fx, tx) * cellSize;
			// Y in canvas: invert game Y
			const canvasY = mazeY + (mazeH - 1 - fy) * cellSize;
			segments.push({ x: edgeX, y: canvasY, horizontal: false });
		} else {
			// Horizontal wall between vertically adjacent cells
			const canvasX = mazeX + fx * cellSize;
			const edgeY = mazeY + (mazeH - Math.max(fy, ty)) * cellSize;
			segments.push({ x: canvasX, y: edgeY, horizontal: true });
		}
	}

	// Border walls — top and bottom rows
	for (let x = 0; x < mazeW; x++) {
		// Top border
		segments.push({ x: mazeX + x * cellSize, y: mazeY, horizontal: true });
		// Bottom border
		segments.push({
			x: mazeX + x * cellSize,
			y: mazeY + mazeH * cellSize,
			horizontal: true,
		});
	}

	// Border walls — left and right columns
	for (let y = 0; y < mazeH; y++) {
		// Left border
		segments.push({
			x: mazeX,
			y: mazeY + y * cellSize,
			horizontal: false,
		});
		// Right border
		segments.push({
			x: mazeX + mazeW * cellSize,
			y: mazeY + y * cellSize,
			horizontal: false,
		});
	}

	return segments;
}

/**
 * Compute corner positions. A corner goes at every grid intersection point
 * where at least one wall segment touches.
 */
export function computeCorners(
	wallSegments: WallSegment[],
	layout: LayoutMetrics,
	mazeW: number,
	mazeH: number,
): Corner[] {
	const { mazeX, mazeY, cellSize } = layout;
	const cornerSet = new Set<string>();

	const addCorner = (x: number, y: number) => {
		cornerSet.add(`${x},${y}`);
	};

	for (const seg of wallSegments) {
		if (seg.horizontal) {
			// Horizontal wall: corners at left and right ends
			addCorner(seg.x, seg.y);
			addCorner(seg.x + cellSize, seg.y);
		} else {
			// Vertical wall: corners at top and bottom ends
			addCorner(seg.x, seg.y);
			addCorner(seg.x, seg.y + cellSize);
		}
	}

	// All 4 outer corners of the maze are always present
	addCorner(mazeX, mazeY);
	addCorner(mazeX + mazeW * cellSize, mazeY);
	addCorner(mazeX, mazeY + mazeH * cellSize);
	addCorner(mazeX + mazeW * cellSize, mazeY + mazeH * cellSize);

	return Array.from(cornerSet).map((key) => {
		const [x, y] = key.split(",").map(Number);
		return { x, y };
	});
}
