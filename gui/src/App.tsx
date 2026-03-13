import { AppShell } from "@mantine/core";
import { useState } from "react";
import BotsPage from "./components/BotsPage";
import HomePage from "./components/HomePage";
import MatchView from "./components/MatchView";
import Sidebar, { type Page } from "./components/Sidebar";

export type GameView = "home" | "match";

export default function App() {
	const [page, setPage] = useState<Page>("game");
	const [gameView, setGameView] = useState<GameView>("home");

	const handlePageNav = (p: Page) => {
		setPage(p);
		if (p === "game") setGameView("home");
	};

	let content: React.ReactNode;
	if (page === "bots") {
		content = <BotsPage />;
	} else if (gameView === "home") {
		content = <HomePage onNavigate={setGameView} />;
	} else {
		content = <MatchView />;
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
