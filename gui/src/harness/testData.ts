import type { MazeState } from "../types/game";

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
	maxTurns: 100,
	totalCheese: 5,
	walls: [
		// Vertical wall between (1,3) and (2,3)
		{ from: [1, 3], to: [2, 3] },
		// Vertical wall between (3,1) and (4,1)
		{ from: [3, 1], to: [4, 1] },
		// Horizontal wall between (4,2) and (4,3)
		{ from: [4, 2], to: [4, 3] },
		// A few more to make it interesting
		{ from: [0, 3], to: [0, 4] },
		{ from: [5, 0], to: [6, 0] },
	],
	mud: [
		{ from: [2, 0], to: [3, 0], cost: 3 },
		{ from: [1, 1], to: [1, 2], cost: 2 },
	],
	cheese: [
		[2, 2],
		[4, 2],
		[3, 4],
		[1, 0],
		[5, 3],
	],
	player1: { position: [0, 0], score: 0 },
	player2: { position: [6, 4], score: 0 },
};
