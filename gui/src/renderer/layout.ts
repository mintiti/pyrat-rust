import type { Coord } from "../bindings/generated";

export type LayoutMetrics = {
	cellSize: number;
	mazeX: number;
	mazeY: number;
	mazeW: number;
	mazeH: number;
	wallThickness: number;
	cornerSize: number;
	canvasWidth: number;
	canvasHeight: number;
};

const MIN_TOP_MARGIN = 28;

export function computeLayout(
	containerW: number,
	containerH: number,
	mazeW: number,
	mazeH: number,
): LayoutMetrics {
	let cellSize = Math.floor(
		Math.min(containerW / mazeW, containerH / mazeH) * 0.9,
	);
	let mazePixelW = cellSize * mazeW;
	let mazePixelH = cellSize * mazeH;
	let mazeX = Math.floor((containerW - mazePixelW) / 2);
	let mazeY = Math.floor((containerH - mazePixelH) / 2);

	// Ensure minimum top margin for the score strip
	if (mazeY < MIN_TOP_MARGIN) {
		const availableH = containerH - MIN_TOP_MARGIN;
		cellSize = Math.floor(
			Math.min(containerW / mazeW, availableH / mazeH) * 0.9,
		);
		mazePixelW = cellSize * mazeW;
		mazePixelH = cellSize * mazeH;
		mazeX = Math.floor((containerW - mazePixelW) / 2);
		mazeY = Math.max(MIN_TOP_MARGIN, Math.floor((containerH - mazePixelH) / 2));
	}

	const wallThickness = Math.max(1, Math.floor(cellSize / 7));
	const cornerSize = Math.max(1, Math.floor(wallThickness * 1.2));

	return {
		cellSize,
		mazeX,
		mazeY,
		mazeW,
		mazeH,
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
): { x: number; y: number } {
	return {
		x: layout.mazeX + coord.x * layout.cellSize,
		y: layout.mazeY + (layout.mazeH - 1 - coord.y) * layout.cellSize,
	};
}

/** Convert a game coordinate to the center of its cell in canvas pixels. */
export function gameToCellCenter(
	coord: Coord,
	layout: LayoutMetrics,
): { x: number; y: number } {
	const tl = gameToCanvas(coord, layout);
	return { x: tl.x + layout.cellSize / 2, y: tl.y + layout.cellSize / 2 };
}
