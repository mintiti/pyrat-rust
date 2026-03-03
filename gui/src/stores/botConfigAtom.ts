import { atom } from "jotai";
import { unwrap } from "jotai/utils";
import { commands } from "../bindings";
import type { BotConfigEntry } from "../bindings/generated";

export type BotConfig = BotConfigEntry;

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
