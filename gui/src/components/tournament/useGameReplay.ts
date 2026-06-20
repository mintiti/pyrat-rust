import { useEffect, useState } from "react";
import { commands } from "../../bindings";
import type { GameReplayState } from "../../bindings/generated";

/**
 * Shared replay cache keyed by `(tournamentId, matchId)`. A matchup grid
 * mounts N `GameCard`s and the game page mounts one `GameView`, each of which
 * needs the same replay — without a cache that's N IPC calls plus a refetch on
 * every remount (open a card, go back, reopen). A finished match's replay is
 * immutable, so caching the in-flight promise collapses all of that to one
 * call per match. A rejected fetch evicts itself so a later mount can retry.
 */
const cache = new Map<string, Promise<GameReplayState | null>>();

function cacheKey(tournamentId: number, matchId: number): string {
	return `${tournamentId}:${matchId}`;
}

export function fetchGameReplay(
	tournamentId: number,
	matchId: number,
): Promise<GameReplayState | null> {
	const key = cacheKey(tournamentId, matchId);
	let pending = cache.get(key);
	if (!pending) {
		pending = commands
			.getGameReplay(tournamentId, matchId)
			.then((r) => (r.status === "ok" ? r.data : null))
			.catch((e) => {
				cache.delete(key);
				throw e;
			});
		cache.set(key, pending);
	}
	return pending;
}

/** Load a game's replay through the shared cache. Returns `null` while the
 * fetch is in flight (or after a transient failure); a resolved value is one
 * of the `GameReplayState` variants (`available` / `missing`). */
export function useGameReplay(
	tournamentId: number,
	matchId: number,
): GameReplayState | null {
	const [replay, setReplay] = useState<GameReplayState | null>(null);
	useEffect(() => {
		let live = true;
		fetchGameReplay(tournamentId, matchId)
			.then((r) => {
				if (live) setReplay(r);
			})
			.catch(() => {
				/* evicted from cache; a remount retries */
			});
		return () => {
			live = false;
		};
	}, [tournamentId, matchId]);
	return replay;
}
