/** Monotonic request tokens for async navigation. Starting or invalidating a
 * request makes every older token stale, including across component unmount. */
export function createLatestRequestGuard() {
	let current = 0;

	return {
		begin: () => ++current,
		invalidate: () => {
			current += 1;
		},
		isCurrent: (request: number) => request === current,
	};
}

/** Coalesce durable tournament repairs by tournament id. A repeat request for
 * the in-flight id schedules one more pass, while a different id keeps its own
 * place in the queue instead of being folded into the old run's retry. */
export function createTournamentReconcileQueue(
	reconcile: (tournamentId: number) => Promise<void>,
	isActive: () => boolean,
) {
	const pendingTournamentIds = new Set<number>();
	let draining: Promise<void> | null = null;

	const drain = async () => {
		while (isActive() && pendingTournamentIds.size > 0) {
			const tournamentId = pendingTournamentIds.values().next().value;
			if (tournamentId === undefined) return;
			pendingTournamentIds.delete(tournamentId);
			try {
				await reconcile(tournamentId);
			} catch {
				// Reconciliation is best effort. Keep draining other tournament
				// ids; a later lifecycle event can retry the failed one.
			}
		}
	};

	const ensureDrain = (): Promise<void> => {
		if (draining === null) {
			draining = drain().finally(() => {
				draining = null;
				// A request can arrive after the drain's final loop check but
				// before this continuation runs.
				if (isActive() && pendingTournamentIds.size > 0) {
					void ensureDrain();
				}
			});
		}
		return draining;
	};

	return {
		request(tournamentId: number) {
			pendingTournamentIds.add(tournamentId);
			void ensureDrain();
		},
		clear() {
			pendingTournamentIds.clear();
		},
		async waitForIdle() {
			while (draining !== null) {
				await draining;
			}
		},
	};
}
