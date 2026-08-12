import { useEffect } from "react";
import { events } from "../bindings";
import { useMatchStore } from "../stores/matchStore";

/** Keep the Play/Analysis event receiver at app scope. Navigation may unmount
 * MatchView while its backend match keeps running; event ownership must not
 * disappear with that presentation surface. */
export function useMatchEvents() {
	useEffect(() => {
		const accepts = (matchId: number) =>
			useMatchStore.getState().matchId === matchId;
		const unlisteners = [
			events.matchStartedEvent.listen((event) => {
				useMatchStore
					.getState()
					.onMatchStarted(event.payload.maze, event.payload.match_id);
			}),
			events.preprocessingStartedEvent.listen((event) => {
				if (accepts(event.payload.match_id)) {
					useMatchStore
						.getState()
						.onPreprocessingStarted(event.payload.match_id);
				}
			}),
			events.setupCompleteEvent.listen((event) => {
				if (accepts(event.payload.match_id)) {
					useMatchStore.getState().onSetupComplete(event.payload.match_id);
				}
			}),
			events.turnPlayedEvent.listen((event) => {
				if (accepts(event.payload.match_id)) {
					useMatchStore.getState().onTurnPlayed(event.payload);
				}
			}),
			events.matchOverEvent.listen((event) => {
				if (accepts(event.payload.match_id)) {
					useMatchStore.getState().onMatchOver(event.payload);
				}
			}),
			events.matchErrorEvent.listen((event) => {
				if (accepts(event.payload.match_id)) {
					useMatchStore.getState().onError(event.payload.message);
				}
			}),
			events.botInfoEvent.listen((event) => {
				if (accepts(event.payload.match_id)) {
					useMatchStore.getState().onBotInfo(event.payload);
				}
			}),
		];

		return () => {
			for (const unlisten of unlisteners) {
				unlisten.then((off) => off());
			}
		};
	}, []);
}
