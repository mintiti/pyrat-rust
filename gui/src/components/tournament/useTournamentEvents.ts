import { useEffect } from "react";
import { events } from "../../bindings";
import { useTournamentStore } from "../../stores/tournamentStore";

/** Subscribe the tournament store to the backend event stream. Mount exactly
 * once, at the App root — NOT inside the tournaments tab. `TournamentStarted`
 * is fire-and-forget with no replay, so a subscription that only exists while
 * the tab is mounted would miss the start of a tournament launched from one
 * tab while another is in front. Verdict-bearing events (standings, match
 * finished) come from the lossless backend path; now-playing is the lossy
 * 4 Hz liveness stream. */
export function useTournamentEvents() {
	useEffect(() => {
		const {
			onStarted,
			onStandings,
			onMatchFinished,
			onMatchStarted,
			onNowPlaying,
			onFinished,
			onAborted,
		} = useTournamentStore.getState();

		const unlisteners = [
			events.tournamentStartedEvent.listen((e) =>
				onStarted(e.payload, Date.now()),
			),
			events.standingsUpdatedEvent.listen((e) => onStandings(e.payload)),
			events.tournamentMatchFinishedEvent.listen((e) =>
				onMatchFinished(e.payload),
			),
			events.tournamentMatchStartedEvent.listen((e) =>
				onMatchStarted(e.payload),
			),
			events.nowPlayingEvent.listen((e) => onNowPlaying(e.payload)),
			events.tournamentFinishedEvent.listen(() => onFinished()),
			events.tournamentAbortedEvent.listen((e) => onAborted(e.payload.reason)),
		];

		return () => {
			for (const u of unlisteners) u.then((off) => off());
		};
	}, []);
}
