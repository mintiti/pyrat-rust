import { atom } from "jotai";
import { unwrap } from "jotai/utils";
import { commands } from "../bindings";
import type { MatchConfigParams } from "../bindings/generated";
import { PRESET_VALUES } from "./gameConfig";

// Preset table, detection, and validation now live in the neutral
// `gameConfig` module (shared with the tournament factory form). Re-exported
// here so existing Play imports keep resolving from `matchConfigAtom`.
export {
	CLASSIC_MAZE,
	OPEN_MAZE,
	PRESET_VALUES,
	detectSizePreset,
	detectMazeType,
	validate,
} from "./gameConfig";
export type { GameShape, PresetName } from "./gameConfig";

export const DEFAULT_MATCH_CONFIG: MatchConfigParams = {
	preset: "custom",
	...PRESET_VALUES.medium,
	seed: null,
};

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
