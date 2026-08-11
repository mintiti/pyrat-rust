import { describe, expect, it } from "vitest";
import {
	type GameShape,
	type GameShapeLimits,
	PRESET_VALUES,
	validate,
} from "./gameConfig";

const limits: GameShapeLimits = {
	min_dimension: 2,
	max_dimension: 64,
	max_turns: 1023,
	max_mud_range: 15,
	max_cheese_count: 509,
};

function validShape(): GameShape {
	return { ...PRESET_VALUES.tiny };
}

describe("game config validation", () => {
	it("uses backend-owned packed-state limits", () => {
		const shape = {
			...validShape(),
			width: 65,
			max_turns: 1024,
			mud_range: 16,
			cheese_count: 510,
		};

		expect(validate(shape, limits)).toMatchObject({
			width: "Max 64",
			max_turns: "Max 1023",
			mud_range: "Max 15",
			cheese_count: "Max 509",
		});
	});

	it("rejects fractional integer controls", () => {
		const errors = validate({ ...validShape(), width: 7.5 }, limits);
		expect(errors.width).toBe("Whole number required");
	});

	it("rejects non-finite densities", () => {
		const errors = validate(
			{ ...validShape(), wall_density: Number.NaN },
			limits,
		);
		expect(errors.wall_density).toBe("Must be between 0 and 1");
	});
});
