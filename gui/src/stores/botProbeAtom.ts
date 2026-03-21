import { atom } from "jotai";
import { commands } from "../bindings";
import type { BotProbeResult } from "../bindings/generated";

export type ProbeEntry =
	| { status: "loading" }
	| { status: "ok"; data: BotProbeResult }
	| { status: "error"; error: string };

/** Cache of probe results keyed by agent_id. Probe at most once per session. */
export const probeCacheAtom = atom<Record<string, ProbeEntry>>({});

/** Write-only atom: fires a probe for the given bot if not already cached. */
export const triggerProbeAtom = atom(
	null,
	async (
		get,
		set,
		args: { agentId: string; runCommand: string; workingDir: string },
	) => {
		const cache = get(probeCacheAtom);
		const existing = cache[args.agentId];
		if (existing?.status === "ok" || existing?.status === "loading") return;

		// Mark loading
		set(probeCacheAtom, { ...cache, [args.agentId]: { status: "loading" } });

		const res = await commands.probeBot(
			args.runCommand,
			args.workingDir,
			args.agentId,
		);

		// Re-read cache in case other probes ran concurrently
		const latest = get(probeCacheAtom);
		if (res.status === "ok") {
			set(probeCacheAtom, {
				...latest,
				[args.agentId]: { status: "ok", data: res.data },
			});
		} else {
			set(probeCacheAtom, {
				...latest,
				[args.agentId]: { status: "error", error: res.error },
			});
		}
	},
);
