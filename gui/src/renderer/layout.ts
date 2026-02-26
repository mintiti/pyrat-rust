import type { Coord } from "../bindings/generated";

export type LayoutMetrics = {
	cellSize: number;
	mazeX: number;
	mazeY: number;
	wallThickness: number;
	cornerSize: number;
	canvasWidth: number;
	canvasHeight: number;
};

export function computeLayout(
	containerW: number,
	containerH: number,
	mazeW: number,
	mazeH: number,
): LayoutMetrics {
	const cellSize = Math.floor(
		Math.min(containerW / mazeW, containerH / mazeH) * 0.9,
	);
	const mazePixelW = cellSize * mazeW;
	const mazePixelH = cellSize * mazeH;
	const mazeX = Math.floor((containerW - mazePixelW) / 2);
	const mazeY = Math.floor((containerH - mazePixelH) / 2);
	const wallThickness = Math.max(1, Math.floor(cellSize / 7));
	const cornerSize = Math.max(1, Math.floor(wallThickness * 1.2));

	return {
		cellSize,
		mazeX,
		mazeY,
		wallThickness,
		cornerSize,
		canvasWidth: containerW,
		canvasHeight: containerH,
	};
}

/** Convert a game coordinate to canvas pixel position (top-left of the cell). */
export function gameToCanvas(
	coord: Coord,
	layout: LayoutMetrics,
	mazeHeight: number,
): { x: number; y: number } {
	return {
		x: layout.mazeX + coord.x * layout.cellSize,
		y: layout.mazeY + (mazeHeight - 1 - coord.y) * layout.cellSize,
	};
}
