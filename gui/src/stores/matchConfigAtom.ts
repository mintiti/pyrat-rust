import { atom } from "jotai";
import { unwrap } from "jotai/utils";
import { commands } from "../bindings";
import type { MatchConfigParams } from "../bindings/generated";

// ---------------------------------------------------------------------------
// Preset values — mirrors engine's GameConfig::preset() table
// ---------------------------------------------------------------------------

export type PresetName =
	| "tiny"
	| "small"
	| "medium"
	| "large"
	| "huge"
	| "open"
	| "custom";

type PresetValues = Omit<MatchConfigParams, "preset" | "seed">;

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

export const PRESET_VALUES: Record<
	Exclude<PresetName, "custom">,
	PresetValues
> = {
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

export const DEFAULT_MATCH_CONFIG: MatchConfigParams = {
	preset: "custom",
	...PRESET_VALUES.medium,
	seed: null,
};

// ---------------------------------------------------------------------------
// Preset detection — pure functions for matching current values to presets
// ---------------------------------------------------------------------------

/** Derive which size preset matches, or null if custom. */
export function detectSizePreset(d: MatchConfigParams): PresetName | null {
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
export function detectMazeType(
	d: MatchConfigParams,
): "classic" | "open" | null {
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

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

export function validate(c: MatchConfigParams): Record<string, string> {
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
	if (c.mud_density > 0 && c.mud_range < 2)
		errors.mud_range = "Min 2 when mud > 0";
	return errors;
}

// ---------------------------------------------------------------------------
// Jotai atoms — same pattern as botConfigAtom.ts
// ---------------------------------------------------------------------------

const baseConfigAtom = atom<MatchConfigParams | Promise<MatchConfigParams>>(
	commands
		.loadMatchConfig()
		.then((res) => (res.status === "ok" ? res.data : DEFAULT_MATCH_CONFIG)),
);

/** Writable atom — persists to disk on every write. */
export const asyncMatchConfigAtom = atom(
	(get) => get(baseConfigAtom),
	async (_get, set, config: MatchConfigParams) => {
		set(baseConfigAtom, config);
		await commands.saveMatchConfig(config);
	},
);

/** Synchronous read atom — returns DEFAULT until initial load completes. */
export const matchConfigAtom = unwrap(
	asyncMatchConfigAtom,
	(prev) => prev ?? DEFAULT_MATCH_CONFIG,
);
