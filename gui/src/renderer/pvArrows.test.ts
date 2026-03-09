import { describe, expect, it } from "vitest";
import type {
	BotInfoEvent,
	Coord,
	Direction,
	WallEntry,
} from "../bindings/generated";
import type { BotInfoMap } from "../stores/matchStore";
import { accumulateBotInfo } from "../stores/matchStore";
import type { LayoutMetrics } from "./layout";
import { buildPvOverlay, buildWallSet, reconstructPath } from "./pvArrows";

// ── Helpers ──────────────────────────────────────────────────────

function info(
	overrides: Partial<BotInfoEvent> &
		Pick<BotInfoEvent, "sender" | "subject" | "multipv">,
): BotInfoEvent {
	return {
		match_id: 1,
		turn: 0,
		target: null,
		depth: 1,
		nodes: 0,
		score: 0,
		pv: [],
		message: "",
		...overrides,
	};
}

const fakeLayout: LayoutMetrics = {
	cellSize: 40,
	mazeX: 0,
	mazeY: 0,
	mazeW: 5,
	mazeH: 5,
	wallThickness: 4,
	cornerSize: 5,
	canvasWidth: 200,
	canvasHeight: 200,
};

// ── buildWallSet ─────────────────────────────────────────────────

describe("buildWallSet", () => {
	it("creates bidirectional entries", () => {
		const walls: WallEntry[] = [{ from: { x: 0, y: 0 }, to: { x: 1, y: 0 } }];
		const set = buildWallSet(walls);
		// Both directions should hit the same normalized key
		expect(set.has("0,0|1,0")).toBe(true);
		expect(set.size).toBe(1);
	});

	it("handles empty walls", () => {
		expect(buildWallSet([]).size).toBe(0);
	});
});

// ── reconstructPath ──────────────────────────────────────────────

describe("reconstructPath", () => {
	const emptyWalls = new Set<string>();

	it("walks a straight line", () => {
		const pv: Direction[] = ["Right", "Right", "Right"];
		const path = reconstructPath({ x: 0, y: 0 }, pv, emptyWalls, 5, 5, 10);
		expect(path).toEqual([
			{ x: 0, y: 0 },
			{ x: 1, y: 0 },
			{ x: 2, y: 0 },
			{ x: 3, y: 0 },
		]);
	});

	it("stops at boundary", () => {
		const pv: Direction[] = ["Right", "Right", "Right", "Right", "Right"];
		const path = reconstructPath({ x: 3, y: 0 }, pv, emptyWalls, 5, 5, 10);
		// x=3 → x=4 is fine, x=4 → x=5 is out of bounds
		expect(path).toEqual([
			{ x: 3, y: 0 },
			{ x: 4, y: 0 },
		]);
	});

	it("stops at wall", () => {
		const walls: WallEntry[] = [{ from: { x: 1, y: 0 }, to: { x: 2, y: 0 } }];
		const wallSet = buildWallSet(walls);
		const pv: Direction[] = ["Right", "Right", "Right"];
		const path = reconstructPath({ x: 0, y: 0 }, pv, wallSet, 5, 5, 10);
		// Stops before the wall between (1,0) and (2,0)
		expect(path).toEqual([
			{ x: 0, y: 0 },
			{ x: 1, y: 0 },
		]);
	});

	it("respects maxSegments", () => {
		const pv: Direction[] = ["Right", "Right", "Right", "Right"];
		const path = reconstructPath({ x: 0, y: 0 }, pv, emptyWalls, 10, 10, 2);
		// maxSegments=2 means 3 points (start + 2 steps)
		expect(path).toEqual([
			{ x: 0, y: 0 },
			{ x: 1, y: 0 },
			{ x: 2, y: 0 },
		]);
	});

	it("skips STAY directions", () => {
		const pv: Direction[] = ["Right", "Stay", "Stay", "Up"];
		const path = reconstructPath({ x: 0, y: 0 }, pv, emptyWalls, 5, 5, 10);
		expect(path).toEqual([
			{ x: 0, y: 0 },
			{ x: 1, y: 0 },
			{ x: 1, y: 1 },
		]);
	});

	it("returns just start for empty PV", () => {
		const path = reconstructPath({ x: 2, y: 3 }, [], emptyWalls, 5, 5, 10);
		expect(path).toEqual([{ x: 2, y: 3 }]);
	});

	it("returns just start for all-STAY PV", () => {
		const pv: Direction[] = ["Stay", "Stay"];
		const path = reconstructPath({ x: 0, y: 0 }, pv, emptyWalls, 5, 5, 10);
		expect(path).toEqual([{ x: 0, y: 0 }]);
	});
});

// ── buildPvOverlay ───────────────────────────────────────────────

describe("buildPvOverlay", () => {
	const p1Pos: Coord = { x: 0, y: 0 };
	const p2Pos: Coord = { x: 4, y: 4 };
	const noWalls: WallEntry[] = [];

	it("returns empty for null/empty botInfo", () => {
		const result = buildPvOverlay({}, p1Pos, p2Pos, noWalls, 5, 5, fakeLayout);
		expect(result.arrows).toEqual([]);
		expect(result.targets).toEqual([]);
	});

	it("builds arrows for a single line", () => {
		const botInfo: BotInfoMap = {};
		accumulateBotInfo(
			botInfo,
			info({
				sender: "Player1",
				subject: "Player1",
				multipv: 1,
				pv: ["Right", "Right"],
				score: 5,
				target: { x: 2, y: 0 },
			}),
		);

		const result = buildPvOverlay(
			botInfo,
			p1Pos,
			p2Pos,
			noWalls,
			5,
			5,
			fakeLayout,
		);

		expect(result.arrows).toHaveLength(1);
		expect(result.arrows[0].isBest).toBe(true);
		expect(result.arrows[0].segments).toHaveLength(2);
		expect(result.targets).toHaveLength(1);
	});

	it("handles two senders analyzing the same subject", () => {
		const botInfo: BotInfoMap = {};
		accumulateBotInfo(
			botInfo,
			info({
				sender: "Player1",
				subject: "Player1",
				multipv: 1,
				pv: ["Right"],
				score: 3,
			}),
		);
		accumulateBotInfo(
			botInfo,
			info({
				sender: "Player2",
				subject: "Player1",
				multipv: 1,
				pv: ["Up"],
				score: 4,
			}),
		);

		const result = buildPvOverlay(
			botInfo,
			p1Pos,
			p2Pos,
			noWalls,
			5,
			5,
			fakeLayout,
		);

		// Both senders produce arrows
		expect(result.arrows).toHaveLength(2);
		const senders = result.arrows.map((a) => a.sender);
		expect(senders).toContain("Player1");
		expect(senders).toContain("Player2");
	});

	it("skips lines with empty or all-STAY PV", () => {
		const botInfo: BotInfoMap = {};
		accumulateBotInfo(
			botInfo,
			info({
				sender: "Player1",
				subject: "Player1",
				multipv: 1,
				pv: ["Stay", "Stay"],
				score: 0,
			}),
		);

		const result = buildPvOverlay(
			botInfo,
			p1Pos,
			p2Pos,
			noWalls,
			5,
			5,
			fakeLayout,
		);

		// No arrow since path has only the start point
		expect(result.arrows).toHaveLength(0);
	});

	it("marks alternatives as not best with pale color", () => {
		const botInfo: BotInfoMap = {};
		accumulateBotInfo(
			botInfo,
			info({
				sender: "Player1",
				subject: "Player1",
				multipv: 1,
				pv: ["Right"],
				score: 10,
			}),
		);
		accumulateBotInfo(
			botInfo,
			info({
				sender: "Player1",
				subject: "Player1",
				multipv: 2,
				pv: ["Up"],
				score: 5,
			}),
		);

		const result = buildPvOverlay(
			botInfo,
			p1Pos,
			p2Pos,
			noWalls,
			5,
			5,
			fakeLayout,
		);

		expect(result.arrows).toHaveLength(2);
		const best = result.arrows.find((a) => a.isBest);
		const alt = result.arrows.find((a) => !a.isBest);
		expect(best?.color).toContain("0.85");
		expect(alt?.color).toContain("0.35");
	});
});
