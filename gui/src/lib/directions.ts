import type { Direction } from "../bindings/generated";

export const DIR_ARROW: Record<Direction, string> = {
	Up: "\u2191",
	Down: "\u2193",
	Left: "\u2190",
	Right: "\u2192",
	Stay: "\u00b7",
};

/** Infer a Direction from a coordinate delta. Returns "Stay" for (0,0) or invalid. */
export function directionFromDelta(dx: number, dy: number): Direction {
	if (dx === 0 && dy === 1) return "Up";
	if (dx === 0 && dy === -1) return "Down";
	if (dx === -1 && dy === 0) return "Left";
	if (dx === 1 && dy === 0) return "Right";
	return "Stay";
}
