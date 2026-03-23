import type { WallEntry } from "../bindings/generated";
import type { DisplayState } from "../stores/matchStore";
import type { AssetMap } from "./assets";
import type { LayoutMetrics } from "./layout";
import { gameToCanvas } from "./layout";
import type { TileAssignment } from "./tileMap";
import {
	type Corner,
	type WallSegment,
	computeCorners,
	computeWallSegments,
} from "./wallGeometry";

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
	hideScoreStrip?: boolean;
};

/** Pre-computed static geometry — walls and corners don't change per turn. */
export type StaticGeometry = {
	wallSegments: WallSegment[];
	corners: Corner[];
};

/** Compute static geometry from walls + layout. Cache this per maze config + layout. */
export function computeStaticGeometry(
	walls: WallEntry[],
	layout: LayoutMetrics,
): StaticGeometry {
	const wallSegments = computeWallSegments(walls, layout);
	const corners = computeCorners(wallSegments, layout);
	return { wallSegments, corners };
}

export function buildDrawInstructions(
	state: DisplayState,
	layout: LayoutMetrics,
	assets: AssetMap,
	tileMap: TileAssignment[][],
	staticGeo: StaticGeometry,
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
			const { x, y } = gameToCanvas({ x: gx, y: gy }, layout);
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
		const isVertical = mud.from.x === mud.to.x;

		const fromCanvas = gameToCanvas(mud.from, layout);
		const toCanvas = gameToCanvas(mud.to, layout);
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
				const { x, y } = gameToCanvas({ x: gx, y: gy }, layout);
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
	const { wallThickness } = layout;
	const halfThick = wallThickness / 2;

	for (const seg of staticGeo.wallSegments) {
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
	const { cornerSize } = layout;
	const halfCorner = cornerSize / 2;

	for (const c of staticGeo.corners) {
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
		const { x, y } = gameToCanvas(pos, layout);
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
	const p1Canvas = gameToCanvas(state.player1.position, layout);
	sprites.push({
		image: assets.rat.neutral,
		dx: p1Canvas.x + playerOffset,
		dy: p1Canvas.y + playerOffset,
		dw: playerDim,
		dh: playerDim,
	});

	// Python (player 2)
	const p2Canvas = gameToCanvas(state.player2.position, layout);
	sprites.push({
		image: assets.python.neutral,
		dx: p2Canvas.x + playerOffset,
		dy: p2Canvas.y + playerOffset,
		dw: playerDim,
		dh: playerDim,
	});

	// 9. Score strip — text counts above the maze
	const mazePixelW = cellSize * width;
	const marginTop = layout.mazeY;
	const totalCheese = state.total_cheese;

	if (!options.hideScoreStrip && totalCheese > 0 && marginTop > 4) {
		const iconSize = Math.floor(marginTop * 0.6);
		const stripY = layout.mazeY - iconSize - Math.floor(iconSize * 0.3);
		const fontSize = Math.max(10, Math.floor(marginTop * 0.4));
		const textY = stripY + iconSize / 2 + fontSize / 3;

		const formatScore = (score: number): string => {
			const display = score % 1 !== 0 ? score.toFixed(1) : String(score);
			return `${display} / ${totalCheese}`;
		};

		// Rat: left-aligned
		sprites.push({
			image: assets.rat.neutral,
			dx: layout.mazeX,
			dy: stripY,
			dw: iconSize,
			dh: iconSize,
		});
		texts.push({
			text: formatScore(state.player1.score),
			x: layout.mazeX + iconSize + 4,
			y: textY,
			fontSize,
			color: "#ffffff",
			align: "left",
		});

		// Python: right-aligned
		const rightEdge = layout.mazeX + mazePixelW;
		sprites.push({
			image: assets.python.neutral,
			dx: rightEdge - iconSize,
			dy: stripY,
			dw: iconSize,
			dh: iconSize,
		});
		texts.push({
			text: formatScore(state.player2.score),
			x: rightEdge - iconSize - 4,
			y: textY,
			fontSize,
			color: "#ffffff",
			align: "right",
		});
	}

	return {
		background: "#1a1a2e",
		sprites,
		texts,
	};
}
