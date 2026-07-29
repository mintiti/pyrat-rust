import { useEffect, useState } from "react";
import { commands } from "../../bindings";
import type { GameReplayState } from "../../bindings/generated";

export type GameReplayLoadState =
	| { kind: "loading" }
	| GameReplayState
	| { kind: "error"; reason: string };

/**
 * Shared replay cache keyed by `(tournamentId, matchId)`. A matchup grid
 * mounts N `GameCard`s and the game page mounts one `GameView`, each of which
 * needs the same replay — without a cache that's N IPC calls plus a refetch on
 * every remount (open a card, go back, reopen). A finished match's replay is
 * immutable, so caching the in-flight promise collapses all of that to one
 * call per match. A rejected fetch evicts itself so a later mount can retry.
 */
const cache = new Map<string, Promise<GameReplayState>>();

function cacheKey(tournamentId: number, matchId: number): string {
	return `${tournamentId}:${matchId}`;
}

export function fetchGameReplay(
	tournamentId: number,
	matchId: number,
): Promise<GameReplayState> {
	const key = cacheKey(tournamentId, matchId);
	let pending = cache.get(key);
	if (!pending) {
		pending = commands
			.getGameReplay(tournamentId, matchId)
			.then((r) => {
				if (r.status === "ok") return r.data;
				throw new Error(r.error);
			})
			.catch((e) => {
				cache.delete(key);
				throw e;
			});
		cache.set(key, pending);
	}
	return pending;
}

/** Load a game's replay through the shared cache. Loading, a deliberately
 * missing replay, and a command failure are separate UI states: only the
 * backend's `missing` variant means there is no replay for this match. */
export function useGameReplay(
	tournamentId: number,
	matchId: number | null,
): GameReplayLoadState {
	const [replay, setReplay] = useState<GameReplayLoadState>(() =>
		matchId === null
			? { kind: "missing", reason: "legacy result has no replay id" }
			: { kind: "loading" },
	);
	useEffect(() => {
		let live = true;
		if (matchId === null) {
			setReplay({ kind: "missing", reason: "legacy result has no replay id" });
			return () => {
				live = false;
			};
		}
		setReplay({ kind: "loading" });
		fetchGameReplay(tournamentId, matchId)
			.then((r) => {
				if (live) setReplay(r);
			})
			.catch((error: unknown) => {
				if (!live) return;
				setReplay({
					kind: "error",
					reason: error instanceof Error ? error.message : String(error),
				});
			});
		return () => {
			live = false;
		};
	}, [tournamentId, matchId]);
	return replay;
}
