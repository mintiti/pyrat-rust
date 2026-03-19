import { atom } from "jotai";
import { unwrap } from "jotai/utils";
import { commands } from "../bindings";
import type { BotConfigEntry } from "../bindings/generated";

export type BotConfig = BotConfigEntry;

/** Sentinel ID for the built-in random stub bot. Must match STUB_SENTINEL in match_runner.rs. */
export const RANDOM_BOT_ID = "__random__" as const;

/** Internal atom holding the raw data (initially a promise, then plain values). */
const baseBotsAtom = atom<BotConfig[] | Promise<BotConfig[]>>(
	commands
		.loadBotConfigs()
		.then((res) => (res.status === "ok" ? res.data : [])),
);

/**
 * Writable atom — persists to disk on every write.
 * Read from this (async) or from `botsAtom` (sync).
 */
export const asyncBotsAtom = atom(
	(get) => get(baseBotsAtom),
	async (_get, set, configs: BotConfig[]) => {
		set(baseBotsAtom, configs);
		await commands.saveBotConfigs(configs);
	},
);

/**
 * Synchronous read atom — returns [] until initial load completes,
 * then tracks the latest value. Components read this, write asyncBotsAtom.
 */
export const botsAtom = unwrap(asyncBotsAtom, (prev) => prev ?? []);

/** Resolve a bot ID to a display name. */
export function resolveBotName(
	botId: string | null,
	bots: { id: string; name: string }[],
	fallback: string,
): string {
	if (!botId) return fallback;
	if (botId === RANDOM_BOT_ID) return "Random";
	return bots.find((b) => b.id === botId)?.name ?? fallback;
}
