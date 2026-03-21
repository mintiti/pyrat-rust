import type { Direction } from "../bindings/generated";
import { DIRECTION_DELTA } from "../renderer/pvArrows";

export const DIR_ARROW: Record<Direction, string> = {
	Up: "\u2191",
	Down: "\u2193",
	Left: "\u2190",
	Right: "\u2192",
	Stay: "\u00b7",
};

/** Reverse lookup: delta → Direction. Built once from DIRECTION_DELTA. */
const DELTA_TO_DIR = new Map<string, Direction>(
	Object.entries(DIRECTION_DELTA).map(([dir, { dx, dy }]) => [
		`${dx},${dy}`,
		dir as Direction,
	]),
);

/** Infer a Direction from a coordinate delta. Returns "Stay" for (0,0) or invalid. */
export function directionFromDelta(dx: number, dy: number): Direction {
	return DELTA_TO_DIR.get(`${dx},${dy}`) ?? "Stay";
}
