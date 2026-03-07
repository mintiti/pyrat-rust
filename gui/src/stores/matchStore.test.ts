import { describe, expect, it } from "vitest";
import type { BotInfoEvent } from "../bindings/generated";
import { type BotInfoMap, accumulateBotInfo } from "./matchStore";

// ── Helpers ──────────────────────────────────────────────────────

/** Minimal BotInfoEvent with only the fields accumulation cares about. */
function info(
	overrides: Partial<BotInfoEvent> &
		Pick<BotInfoEvent, "sender" | "player" | "multipv" | "depth">,
): BotInfoEvent {
	return {
		match_id: 1,
		turn: 0,
		target: null,
		nodes: 0,
		score: 0,
		pv: [],
		message: "",
		...overrides,
	};
}

/** Shorthand: lines still present in a bucket, as [multipv, depth] pairs. */
function lineEntries(map: BotInfoMap, key: string): [number, number][] {
	const bucket = map[key];
	if (!bucket) return [];
	return Object.entries(bucket.lines).map(([mpv, line]) => [
		Number(mpv),
		line.depth,
	]);
}

/** Which lines in a bucket are stale (batch < bucket.batch). */
function staleMultipvs(map: BotInfoMap, key: string): number[] {
	const bucket = map[key];
	if (!bucket) return [];
	return Object.entries(bucket.lines)
		.filter(([, line]) => line.batch < bucket.batch)
		.map(([mpv]) => Number(mpv));
}

// ── Normal bots (1 bot per player) ──────────────────────────────

describe("normal bots: iterative deepening (single PV)", () => {
	it("first info creates bucket at batch 1", () => {
		const map: BotInfoMap = {};
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 3 }),
		);

		const bucket = map["Player1:Player1"];
		expect(bucket).toBeDefined();
		expect(bucket.batch).toBe(1);
		expect(bucket.lines[1].depth).toBe(3);
	});

	it("each multipv=1 bumps batch and overwrites", () => {
		const map: BotInfoMap = {};
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 3 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 5 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 7 }),
		);

		const bucket = map["Player1:Player1"];
		expect(bucket.batch).toBe(3);
		expect(bucket.lines[1].depth).toBe(7);
		// Only one line — each overwrote the last.
		expect(Object.keys(bucket.lines)).toEqual(["1"]);
	});
});

describe("normal bots: multi-PV at same depth", () => {
	it("multipv=1 bumps batch, subsequent lines share it", () => {
		const map: BotInfoMap = {};
		const key = "Player1:Player1";
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 5 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 2, depth: 5 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 3, depth: 5 }),
		);

		const bucket = map[key];
		expect(bucket.batch).toBe(1);
		expect(lineEntries(map, key)).toEqual([
			[1, 5],
			[2, 5],
			[3, 5],
		]);
		// All lines share the same batch.
		expect(bucket.lines[1].batch).toBe(1);
		expect(bucket.lines[2].batch).toBe(1);
		expect(bucket.lines[3].batch).toBe(1);
	});
});

describe("normal bots: depth increase narrows search", () => {
	it("old higher-ranked lines become stale after new multipv=1", () => {
		const map: BotInfoMap = {};
		const key = "Player1:Player1";

		// Depth 5: three lines.
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 5 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 2, depth: 5 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 3, depth: 5 }),
		);

		// Depth 7: only best line (search narrowed).
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 7 }),
		);

		const bucket = map[key];
		expect(bucket.batch).toBe(2);
		// Line 1 is current (batch 2), lines 2 and 3 are stale (batch 1).
		expect(bucket.lines[1].batch).toBe(2);
		expect(bucket.lines[1].depth).toBe(7);
		expect(staleMultipvs(map, key)).toEqual([2, 3]);
	});

	it("stale lines get overwritten when new depth sends them", () => {
		const map: BotInfoMap = {};
		const key = "Player1:Player1";

		// Depth 5: lines 1,2,3.
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 5 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 2, depth: 5 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 3, depth: 5 }),
		);

		// Depth 7: full set again.
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 7 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 2, depth: 7 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 3, depth: 7 }),
		);

		const bucket = map[key];
		expect(bucket.batch).toBe(2);
		expect(staleMultipvs(map, key)).toEqual([]);
		expect(lineEntries(map, key)).toEqual([
			[1, 7],
			[2, 7],
			[3, 7],
		]);
	});
});

// ── Hivemind (1 bot controls both players) ──────────────────────

describe("hivemind: one sender, two subjects", () => {
	it("creates separate buckets per subject", () => {
		const map: BotInfoMap = {};

		// Sender is P1 by convention; info about both players.
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 3 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player2", multipv: 1, depth: 3 }),
		);

		expect(Object.keys(map).sort()).toEqual([
			"Player1:Player1",
			"Player1:Player2",
		]);
	});

	it("batch counters are independent per subject", () => {
		const map: BotInfoMap = {};

		// P1 analysis deepens three times.
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 3 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 5 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 7 }),
		);

		// P2 analysis: only one depth so far.
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player2", multipv: 1, depth: 4 }),
		);

		expect(map["Player1:Player1"].batch).toBe(3);
		expect(map["Player1:Player2"].batch).toBe(1);
	});
});

// ── Cross-analysis (two senders, same subject) ──────────────────

describe("cross-analysis: two bots analyzing same player", () => {
	it("separate senders get separate buckets", () => {
		const map: BotInfoMap = {};

		// Bot A (plays P1) analyzes P2.
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player2", multipv: 1, depth: 5 }),
		);
		// Bot B (plays P2) also analyzes P2 (self-analysis).
		accumulateBotInfo(
			map,
			info({ sender: "Player2", player: "Player2", multipv: 1, depth: 8 }),
		);

		expect(Object.keys(map).sort()).toEqual([
			"Player1:Player2",
			"Player2:Player2",
		]);
		expect(map["Player1:Player2"].lines[1].depth).toBe(5);
		expect(map["Player2:Player2"].lines[1].depth).toBe(8);
	});

	it("updates to one sender don't affect the other", () => {
		const map: BotInfoMap = {};

		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player2", multipv: 1, depth: 5 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player2", player: "Player2", multipv: 1, depth: 5 }),
		);

		// Sender P1 deepens.
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player2", multipv: 1, depth: 10 }),
		);

		// P2's self-analysis untouched.
		expect(map["Player1:Player2"].batch).toBe(2);
		expect(map["Player2:Player2"].batch).toBe(1);
		expect(map["Player2:Player2"].lines[1].depth).toBe(5);
	});
});

// ── Both bots active simultaneously ─────────────────────────────

describe("two normal bots: interleaved updates", () => {
	it("independent buckets accumulate correctly", () => {
		const map: BotInfoMap = {};

		// Bot 1 (P1): depth 3, lines 1-2.
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 3 }),
		);
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 2, depth: 3 }),
		);

		// Bot 2 (P2): depth 5, line 1.
		accumulateBotInfo(
			map,
			info({ sender: "Player2", player: "Player2", multipv: 1, depth: 5 }),
		);

		// Bot 1 deepens to depth 5, only best line.
		accumulateBotInfo(
			map,
			info({ sender: "Player1", player: "Player1", multipv: 1, depth: 5 }),
		);

		// Bot 2 sends more lines.
		accumulateBotInfo(
			map,
			info({ sender: "Player2", player: "Player2", multipv: 2, depth: 5 }),
		);

		// P1: batch=2, line 1 current (depth 5), line 2 stale (depth 3).
		expect(map["Player1:Player1"].batch).toBe(2);
		expect(map["Player1:Player1"].lines[1].depth).toBe(5);
		expect(staleMultipvs(map, "Player1:Player1")).toEqual([2]);

		// P2: batch=1, both lines current.
		expect(map["Player2:Player2"].batch).toBe(1);
		expect(staleMultipvs(map, "Player2:Player2")).toEqual([]);
		expect(lineEntries(map, "Player2:Player2")).toEqual([
			[1, 5],
			[2, 5],
		]);
	});
});
