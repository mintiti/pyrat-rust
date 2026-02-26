import type { MazeState } from "../bindings/generated";

/**
 * A small 7x5 maze for dev/testing.
 *
 * Layout (game coords, (0,0) bottom-left):
 *
 *   4 |   |   |   |   |   |   | P2
 *   3 |   | W |   |   |   |   |
 *   2 |   |   | C |   | C |   |
 *   1 |   |   |   | W |   |   |
 *   0 | P1|   |   |   |   |   |
 *       0   1   2   3   4   5   6
 */
export const testMazeState: MazeState = {
	width: 7,
	height: 5,
	turn: 0,
	max_turns: 100,
	total_cheese: 5,
	walls: [
		// Vertical wall between (1,3) and (2,3)
		{ from: { x: 1, y: 3 }, to: { x: 2, y: 3 } },
		// Vertical wall between (3,1) and (4,1)
		{ from: { x: 3, y: 1 }, to: { x: 4, y: 1 } },
		// Horizontal wall between (4,2) and (4,3)
		{ from: { x: 4, y: 2 }, to: { x: 4, y: 3 } },
		// A few more to make it interesting
		{ from: { x: 0, y: 3 }, to: { x: 0, y: 4 } },
		{ from: { x: 5, y: 0 }, to: { x: 6, y: 0 } },
	],
	mud: [
		{ from: { x: 2, y: 0 }, to: { x: 3, y: 0 }, cost: 3 },
		{ from: { x: 1, y: 1 }, to: { x: 1, y: 2 }, cost: 2 },
	],
	cheese: [
		{ x: 2, y: 2 },
		{ x: 4, y: 2 },
		{ x: 3, y: 4 },
		{ x: 1, y: 0 },
		{ x: 5, y: 3 },
	],
	player1: { position: { x: 0, y: 0 }, score: 0 },
	player2: { position: { x: 6, y: 4 }, score: 0 },
};
