import { useEffect } from "react";
import { events, commands } from "../../bindings";
import { useTournamentStore } from "../../stores/tournamentStore";
import { createTournamentReconcileQueue } from "./tournamentAsync";

/** Subscribe the tournament store to the backend event stream. Mount exactly
 * once, at the App root — NOT inside the tournaments tab. `TournamentStarted`
 * is fire-and-forget with no replay, so a subscription that only exists while
 * the tab is mounted would miss the start of a tournament launched from one
 * tab while another is in front. Lifecycle events keep the UI immediate, and
 * standings counts trigger a durable SQLite reconciliation if any per-game
 * detail was missed. Now-playing remains the deliberately lossy 4 Hz stream. */
export function useTournamentEvents() {
	useEffect(() => {
		const {
			onPreparing,
			onStarted,
			onStandings,
			onMatchFinished,
			onMatchFailed,
			onMatchStarted,
			onNowPlaying,
			onFinished,
			onAborted,
			reconcileSnapshot,
		} = useTournamentStore.getState();
		let mounted = true;

		const reconcileQueue = createTournamentReconcileQueue(
			async (tournamentId) => {
				const result = await commands.getTournamentSnapshot(tournamentId);
				if (mounted && result.status === "ok") {
					reconcileSnapshot(result.data);
				}
			},
			() => mounted,
		);
		const reconcile = (tournamentId: number) => {
			reconcileQueue.request(tournamentId);
		};

		const detailCount = (tournamentId: number) => {
			const live = useTournamentStore.getState().live;
			if (!live || live.tournamentId !== tournamentId) return 0;
			const successes = Object.values(live.gamesByPair).reduce(
				(total, games) => total + games.length,
				0,
			);
			const failures = Object.values(live.failuresByPair).reduce(
				(total, attempts) => total + attempts.length,
				0,
			);
			return successes + failures;
		};

		const unlisteners = [
			events.tournamentPreparingEvent.listen((e) => onPreparing(e.payload)),
			events.tournamentStartedEvent.listen((e) =>
				onStarted(e.payload, Date.now()),
			),
			events.standingsUpdatedEvent.listen((e) => {
				onStandings(e.payload);
				if (
					detailCount(e.payload.tournament_id) <
					e.payload.progress.successful_games +
						e.payload.progress.failed_attempts
				) {
					void reconcile(e.payload.tournament_id);
				}
			}),
			events.tournamentMatchFinishedEvent.listen((e) =>
				onMatchFinished(e.payload),
			),
			events.tournamentMatchFailedEvent.listen((e) => onMatchFailed(e.payload)),
			events.tournamentMatchStartedEvent.listen((e) =>
				onMatchStarted(e.payload),
			),
			events.nowPlayingEvent.listen((e) => onNowPlaying(e.payload)),
			events.tournamentFinishedEvent.listen((e) => {
				onFinished(e.payload);
				void reconcile(e.payload.tournament_id);
			}),
			events.tournamentAbortedEvent.listen((e) => {
				onAborted(e.payload);
				void reconcile(e.payload.tournament_id);
			}),
		];

		return () => {
			mounted = false;
			reconcileQueue.clear();
			for (const u of unlisteners) u.then((off) => off());
		};
	}, []);
}
