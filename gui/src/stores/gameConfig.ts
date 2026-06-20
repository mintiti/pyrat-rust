// Neutral game-shape config: preset table, preset detection, and validation,
// shared by the Play match config (`matchConfigAtom`) and the tournament
// game-instance factory (`tournament/GameFactoryForm`). Defined here — not on
// the Play atom — so the tournament form isn't secretly tied to the Play flow,
// and the engine-soundness rules (the random-start invariants below) apply in
// both places. Mirrors the engine's `GameConfig::preset()` table.

export type PresetName =
	| "tiny"
	| "small"
	| "medium"
	| "large"
	| "huge"
	| "open"
	| "custom";

/** The game-shape fields shared by Play's `MatchConfigParams` and the
 * tournament `GameFactoryConfig`. Both are structurally assignable to this, so
 * presets / detection / validation are written once and serve both. */
export interface GameShape {
	width: number;
	height: number;
	max_turns: number;
	wall_density: number;
	mud_density: number;
	mud_range: number;
	connected: boolean;
	symmetric: boolean;
	/** "corners" | "random" — kept as `string` so loose-typed callers fit. */
	player_start: string;
	cheese_count: number;
	cheese_symmetric: boolean;
}

export const CLASSIC_MAZE = {
	wall_density: 0.7,
	mud_density: 0.1,
	mud_range: 3,
	connected: true,
	symmetric: true,
	cheese_symmetric: true,
	player_start: "corners" as const,
};

export const OPEN_MAZE = {
	wall_density: 0.0,
	mud_density: 0.0,
	mud_range: 2,
	connected: true,
	symmetric: true,
	cheese_symmetric: true,
	player_start: "corners" as const,
};

export const PRESET_VALUES: Record<Exclude<PresetName, "custom">, GameShape> = {
	tiny: {
		width: 11,
		height: 9,
		max_turns: 150,
		cheese_count: 13,
		...CLASSIC_MAZE,
	},
	small: {
		width: 15,
		height: 13,
		max_turns: 200,
		cheese_count: 21,
		...CLASSIC_MAZE,
	},
	medium: {
		width: 21,
		height: 15,
		max_turns: 300,
		cheese_count: 41,
		...CLASSIC_MAZE,
	},
	large: {
		width: 31,
		height: 21,
		max_turns: 400,
		cheese_count: 85,
		...CLASSIC_MAZE,
	},
	huge: {
		width: 41,
		height: 31,
		max_turns: 500,
		cheese_count: 165,
		...CLASSIC_MAZE,
	},
	open: {
		width: 21,
		height: 15,
		max_turns: 300,
		cheese_count: 41,
		...OPEN_MAZE,
	},
};

/** Derive which size preset matches, or null if custom. */
export function detectSizePreset(d: GameShape): PresetName | null {
	for (const [name, vals] of Object.entries(PRESET_VALUES)) {
		if (name === "open") continue;
		if (
			d.width === vals.width &&
			d.height === vals.height &&
			d.max_turns === vals.max_turns &&
			d.cheese_count === vals.cheese_count
		)
			return name as PresetName;
	}
	return null;
}

/** Derive maze type from density values. */
export function detectMazeType(d: GameShape): "classic" | "open" | null {
	if (
		d.wall_density === OPEN_MAZE.wall_density &&
		d.mud_density === OPEN_MAZE.mud_density
	)
		return "open";
	if (
		d.wall_density === CLASSIC_MAZE.wall_density &&
		d.mud_density === CLASSIC_MAZE.mud_density &&
		d.mud_range === CLASSIC_MAZE.mud_range
	)
		return "classic";
	return null;
}

/** Validation shared by Play and tournament. Returns field → message.
 *
 * The random-start + symmetric-cheese rules mirror the backend
 * (`GameFactoryConfig::to_game_config` / the engine cheese generator): the
 * mirror of a piece can land on a non-mirror random start, and an odd count
 * needs the board center, which a random start can occupy. Enforcing them here
 * means such a config can't be launched — and a Play game can't silently fail
 * to generate either (Play already exposes random starts). */
export function validate(c: GameShape): Record<string, string> {
	const errors: Record<string, string> = {};
	if (c.width < 2) errors.width = "Min 2";
	if (c.height < 2) errors.height = "Min 2";
	if (c.max_turns < 1) errors.max_turns = "Min 1";
	if (c.cheese_count < 1) errors.cheese_count = "Min 1";

	const maxCheese = c.width * c.height - 2;
	if (c.cheese_count > maxCheese) errors.cheese_count = `Max ${maxCheese}`;

	if (
		c.cheese_symmetric &&
		c.cheese_count % 2 === 1 &&
		(c.width % 2 === 0 || c.height % 2 === 0)
	)
		errors.cheese_count = "Odd count + symmetry needs odd board dimensions";

	if (c.player_start === "random" && c.cheese_symmetric) {
		if (c.cheese_count % 2 === 1) {
			errors.cheese_count =
				"Random starts + symmetric cheese need an even count";
		} else {
			const oddBoard = c.width % 2 === 1 && c.height % 2 === 1;
			const cap = c.width * c.height - (oddBoard ? 5 : 4);
			if (c.cheese_count > cap)
				errors.cheese_count = `Max ${cap} (random start + symmetric)`;
		}
	}

	if (c.mud_density > 0 && c.mud_range < 2)
		errors.mud_range = "Min 2 when mud > 0";
	return errors;
}
