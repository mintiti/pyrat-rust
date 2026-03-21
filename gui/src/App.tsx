import { AppShell } from "@mantine/core";
import { useState } from "react";
import BotsPage from "./components/BotsPage";
import HomePage from "./components/HomePage";
import MatchView from "./components/MatchView";
import SetupView from "./components/SetupView";
import Sidebar, { type Page } from "./components/Sidebar";
import { useMatchStore } from "./stores/matchStore";

export type GameView = "home" | "setup" | "match";

export default function App() {
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
	} else if (gameView === "home") {
		content = <HomePage onNavigate={setGameView} />;
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
			<AppShell.Main>{content}</AppShell.Main>
		</AppShell>
	);
}
