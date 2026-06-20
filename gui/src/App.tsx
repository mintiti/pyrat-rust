import { AppShell, Box } from "@mantine/core";
import { useState } from "react";
import BotsPage from "./components/BotsPage";
import HomePage from "./components/HomePage";
import MatchView from "./components/MatchView";
import SetupView from "./components/SetupView";
import Sidebar, { type Page } from "./components/Sidebar";
import TournamentsPage from "./components/TournamentsPage";
import LiveChip from "./components/tournament/LiveChip";
import { useTournamentEvents } from "./components/tournament/useTournamentEvents";
import { useMatchStore } from "./stores/matchStore";
import { useTournamentStore } from "./stores/tournamentStore";

export type GameView = "home" | "setup" | "match";

export default function App() {
	// Mounted at the root so the tournament store updates across every tab
	// (the live chip stays current while the user is in Play/Analysis).
	useTournamentEvents();
	const showLive = useTournamentStore((s) => s.showLive);

	const [page, setPage] = useState<Page>("game");
	const [gameView, setGameView] = useState<GameView>("home");

	const handlePageNav = (p: Page) => {
		setPage(p);
		if (p === "game") {
			const phase = useMatchStore.getState().matchPhase;
			setGameView(phase === "idle" ? "home" : "match");
		}
	};

	let content: React.ReactNode;
	if (page === "bots") {
		content = <BotsPage />;
	} else if (page === "tournaments") {
		content = <TournamentsPage />;
	} else if (gameView === "home") {
		content = (
			<HomePage
				onNavigate={setGameView}
				onOpenTournaments={() => handlePageNav("tournaments")}
			/>
		);
	} else if (gameView === "setup") {
		content = (
			<SetupView
				onBack={() => setGameView("home")}
				onStartMatch={() => setGameView("match")}
			/>
		);
	} else {
		content = <MatchView onNewMatch={() => setGameView("setup")} />;
	}

	return (
		<AppShell
			navbar={{ width: "3rem", breakpoint: 0 }}
			styles={{ main: { height: "100vh" } }}
		>
			<AppShell.Navbar>
				<Sidebar active={page} onNavigate={handlePageNav} />
			</AppShell.Navbar>
			<AppShell.Main>
				{content}
				{/* Glanceable from any tab; click jumps to the live view. */}
				<Box pos="fixed" top={10} right={14} style={{ zIndex: 200 }}>
					<LiveChip
						onClick={() => {
							showLive();
							handlePageNav("tournaments");
						}}
					/>
				</Box>
			</AppShell.Main>
		</AppShell>
	);
}
