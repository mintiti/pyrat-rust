import type { BotInfoEvent, PlayerSide } from "../bindings/generated";

// ── Types ────────────────────────────────────────────────────────

/**
 * Info lines from one bot about one player, with subcycle tracking.
 *
 * Vocabulary (borrowed from nibbler's iterative-deepening model):
 * - **cycle** = turn boundary. Each GameNode starts with `botInfo: {}`,
 *   so the tree structure provides cycle isolation implicitly.
 * - **subcycle** = increments on each multipv=1 within a turn.
 *   One subcycle is one pass through PV ranks at a given search depth.
 */
export interface InfoBucket {
	subcycle: number;
	lines: Record<number, BotInfoEvent & { subcycle: number }>; // keyed by multipv
}

/** All bot info for a game position. Keyed by "sender:subject". */
export type BotInfoMap = Record<string, InfoBucket>;

// ── Functions ────────────────────────────────────────────────────

/** Parse a BotInfoMap key back into its sender and subject. */
export function parseBotInfoKey(key: string): {
	sender: PlayerSide;
	subject: PlayerSide;
} {
	const [sender, subject] = key.split(":") as [PlayerSide, PlayerSide];
	return { sender, subject };
}

/** Accumulate a BotInfoEvent into the map. Mutates in place (designed for immer). */
export function accumulateBotInfo(botInfo: BotInfoMap, e: BotInfoEvent): void {
	const key = `${e.sender}:${e.subject}`;
	let bucket = botInfo[key];
	if (!bucket) {
		bucket = { subcycle: 0, lines: {} };
		botInfo[key] = bucket;
	}
	// multipv=1 starts a new subcycle (one pass through PV ranks)
	if (e.multipv === 1) {
		bucket.subcycle++;
	}
	bucket.lines[e.multipv] = { ...e, subcycle: bucket.subcycle };
}

/** True if this line belongs to an older subcycle than the bucket's current one. */
export function isStale(
	bucket: InfoBucket,
	line: { subcycle: number },
): boolean {
	return line.subcycle < bucket.subcycle;
}

/** All lines from the current subcycle, sorted by multipv rank. */
export function currentLines(
	bucket: InfoBucket,
): (BotInfoEvent & { subcycle: number })[] {
	return Object.values(bucket.lines)
		.filter((l) => l.subcycle === bucket.subcycle)
		.sort((a, b) => a.multipv - b.multipv);
}
