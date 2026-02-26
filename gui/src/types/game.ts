/** [x, y] coordinate in game space, (0,0) at bottom-left */
export type Coordinate = [number, number];

export type Wall = { from: Coordinate; to: Coordinate };
export type MudPassage = { from: Coordinate; to: Coordinate; cost: number };

export type PlayerState = {
	position: Coordinate;
	score: number;
};

export type MazeState = {
	width: number;
	height: number;
	turn: number;
	maxTurns: number;
	walls: Wall[];
	mud: MudPassage[];
	cheese: Coordinate[];
	player1: PlayerState;
	player2: PlayerState;
	totalCheese: number;
};
