import type { MazeState } from "../types/game";
import type { AssetMap } from "./assets";
import type { LayoutMetrics } from "./layout";
import { gameToCanvas } from "./layout";
import type { TileAssignment } from "./tileMap";
import { computeCorners, computeWallSegments } from "./wallGeometry";

export type SpriteInstruction = {
	image: HTMLImageElement;
	dx: number;
	dy: number;
	dw: number;
	dh: number;
	rotation?: number; // degrees: 0, 90, 180, 270
	flipX?: boolean;
	flipY?: boolean;
};

export type TextInstruction = {
	text: string;
	x: number;
	y: number;
	fontSize: number;
	color: string;
	align?: CanvasTextAlign;
};

export type DrawInstructions = {
	background: string;
	sprites: SpriteInstruction[];
	texts: TextInstruction[];
};

export type DrawOptions = {
	showCellIndices?: boolean;
};

export function buildDrawInstructions(
	state: MazeState,
	layout: LayoutMetrics,
	assets: AssetMap,
	tileMap: TileAssignment[][],
	options: DrawOptions = {},
): DrawInstructions {
	const sprites: SpriteInstruction[] = [];
	const texts: TextInstruction[] = [];
	const { cellSize } = layout;
	const { width, height } = state;

	// 1. Ground tiles
	for (let gy = 0; gy < height; gy++) {
		for (let gx = 0; gx < width; gx++) {
			const tile = tileMap[gy][gx];
			const { x, y } = gameToCanvas([gx, gy], layout, height);
			sprites.push({
				image: assets.ground[tile.tileIndex],
				dx: x,
				dy: y,
				dw: cellSize,
				dh: cellSize,
				rotation: tile.rotation * 90,
				flipX: tile.flipX,
				flipY: tile.flipY,
			});
		}
	}

	// 2. Mud sprites + 3. Mud weight labels
	for (const mud of state.mud) {
		const [fx, fy] = mud.from;
		const [tx, ty] = mud.to;
		const isVertical = fx === tx;

		// Position between the two cells
		const midGx = (fx + tx) / 2;
		const midGy = (fy + ty) / 2;

		const fromCanvas = gameToCanvas(mud.from, layout, height);
		const toCanvas = gameToCanvas(mud.to, layout, height);
		const midPx = (fromCanvas.x + toCanvas.x) / 2;
		const midPy = (fromCanvas.y + toCanvas.y) / 2;

		sprites.push({
			image: assets.mud,
			dx: midPx,
			dy: midPy,
			dw: cellSize,
			dh: cellSize,
			rotation: isVertical ? 90 : 0,
		});

		// Weight label centered on mud sprite
		const labelSize = Math.max(8, Math.floor(cellSize * 0.17));
		texts.push({
			text: String(mud.cost),
			x: midPx + cellSize / 2,
			y: midPy + cellSize / 2 + labelSize / 3,
			fontSize: labelSize,
			color: "#ffffff",
			align: "center",
		});
	}

	// 4. Cell index numbers
	if (options.showCellIndices) {
		const indexSize = Math.max(6, Math.floor(cellSize * 0.15));
		for (let gy = 0; gy < height; gy++) {
			for (let gx = 0; gx < width; gx++) {
				const { x, y } = gameToCanvas([gx, gy], layout, height);
				texts.push({
					text: `${gx},${gy}`,
					x: x + 3,
					y: y + indexSize + 2,
					fontSize: indexSize,
					color: "rgba(255,255,255,0.5)",
					align: "left",
				});
			}
		}
	}

	// 5. Walls
	const wallSegments = computeWallSegments(state.walls, layout, width, height);
	const { wallThickness } = layout;
	const halfThick = wallThickness / 2;

	for (const seg of wallSegments) {
		if (seg.horizontal) {
			sprites.push({
				image: assets.wall,
				dx: seg.x,
				dy: seg.y - halfThick,
				dw: cellSize,
				dh: wallThickness,
			});
		} else {
			// Specify a horizontal rect centered on the wall's midpoint,
			// then rotate 90° so drawSprite turns it vertical.
			sprites.push({
				image: assets.wall,
				dx: seg.x - cellSize / 2,
				dy: seg.y + (cellSize - wallThickness) / 2,
				dw: cellSize,
				dh: wallThickness,
				rotation: 90,
			});
		}
	}

	// 6. Corners
	const corners = computeCorners(wallSegments, layout, width, height);
	const { cornerSize } = layout;
	const halfCorner = cornerSize / 2;

	for (const c of corners) {
		sprites.push({
			image: assets.corner,
			dx: c.x - halfCorner,
			dy: c.y - halfCorner,
			dw: cornerSize,
			dh: cornerSize,
		});
	}

	// 7. Cheese
	const cheeseDim = Math.floor(cellSize * 0.4);
	const cheeseOffset = (cellSize - cheeseDim) / 2;

	for (const pos of state.cheese) {
		const { x, y } = gameToCanvas(pos, layout, height);
		sprites.push({
			image: assets.cheese,
			dx: x + cheeseOffset,
			dy: y + cheeseOffset,
			dw: cheeseDim,
			dh: cheeseDim,
		});
	}

	// 8. Players
	const playerDim = Math.floor(cellSize * 0.5);
	const playerOffset = (cellSize - playerDim) / 2;

	// Rat (player 1)
	const p1Canvas = gameToCanvas(state.player1.position, layout, height);
	sprites.push({
		image: assets.rat.neutral,
		dx: p1Canvas.x + playerOffset,
		dy: p1Canvas.y + playerOffset,
		dw: playerDim,
		dh: playerDim,
	});

	// Python (player 2)
	const p2Canvas = gameToCanvas(state.player2.position, layout, height);
	sprites.push({
		image: assets.python.neutral,
		dx: p2Canvas.x + playerOffset,
		dy: p2Canvas.y + playerOffset,
		dw: playerDim,
		dh: playerDim,
	});

	return {
		background: "#1a1a2e",
		sprites,
		texts,
	};
}
