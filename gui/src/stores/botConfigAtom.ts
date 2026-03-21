import { atom } from "jotai";
import { unwrap } from "jotai/utils";
import { commands } from "../bindings";
import type { DiscoveredBot } from "../bindings/generated";

/** Sentinel ID for the built-in random stub bot. Must match STUB_SENTINEL in match_runner.rs. */
export const RANDOM_BOT_ID = "__random__" as const;

// ---------------------------------------------------------------------------
// Scan paths — persisted list of directories to scan for bot.toml files
// ---------------------------------------------------------------------------

/** Shared promise so both atoms chain off a single IPC call. */
const scanPathsPromise = commands
	.loadScanPaths()
	.then((res) => (res.status === "ok" ? res.data : []));

const baseScanPathsAtom = atom<string[] | Promise<string[]>>(scanPathsPromise);

/** Writable atom — persists scan paths to disk on every write. */
export const asyncScanPathsAtom = atom(
	(get) => get(baseScanPathsAtom),
	async (_get, set, paths: string[]) => {
		set(baseScanPathsAtom, paths);
		await commands.saveScanPaths(paths);
	},
);

/** Synchronous read atom — returns [] until initial load completes. */
export const scanPathsAtom = unwrap(asyncScanPathsAtom, (prev) => prev ?? []);

// ---------------------------------------------------------------------------
// Discovered bots — result of scanning scan paths for bot.toml files
// ---------------------------------------------------------------------------

/** Holds the latest scan results. Initialized by scanning on first load. */
const baseDiscoveredBotsAtom = atom<DiscoveredBot[] | Promise<DiscoveredBot[]>>(
	scanPathsPromise.then((paths) => commands.discoverBots(paths)),
);

/** Read-only sync atom for discovered bots. */
export const discoveredBotsAtom = unwrap(
	baseDiscoveredBotsAtom,
	(prev) => prev ?? [],
);

/** Write-only atom — triggers a re-scan from current scan paths. */
export const refreshBotsAtom = atom(null, async (get, set) => {
	const paths = await get(asyncScanPathsAtom);
	const pathsArr = Array.isArray(paths) ? paths : await paths;
	const bots = await commands.discoverBots(pathsArr);
	set(baseDiscoveredBotsAtom, bots);
});

// ---------------------------------------------------------------------------
// botsAtom — derived { id, name }[] for consumers (SetupView, MatchToolbar, etc.)
// ---------------------------------------------------------------------------

/** Maps discovered bots to the { id, name } shape that consumers expect. */
export const botsAtom = atom((get) => {
	const discovered = get(discoveredBotsAtom);
	return discovered.map((b) => ({ id: b.agent_id, name: b.name }));
});

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/** Resolve a bot agent_id to the full DiscoveredBot record. */
export function resolveDiscoveredBot(
	agentId: string,
	discovered: DiscoveredBot[],
): DiscoveredBot | undefined {
	return discovered.find((b) => b.agent_id === agentId);
}
