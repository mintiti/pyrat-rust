import { useTournamentStore } from "../stores/tournamentStore";
import LaunchView from "./tournament/LaunchView";
import LiveView from "./tournament/LiveView";

/** Top-level tournaments tab. Shows the launch screen until a tournament is
 * started, then the live view. (The event subscription lives at the app root
 * so the store updates regardless of which tab is active.) */
export default function TournamentsPage() {
	const screen = useTournamentStore((s) => s.screen);
	const live = useTournamentStore((s) => s.live);

	if (screen === "live" && live) return <LiveView live={live} />;
	return <LaunchView />;
}
