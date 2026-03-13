import { AppShell } from "@mantine/core";
import { useState } from "react";
import BotsPage from "./components/BotsPage";
import MatchView from "./components/MatchView";
import Sidebar, { type Page } from "./components/Sidebar";

export default function App() {
	const [page, setPage] = useState<Page>("game");

	return (
		<AppShell
			navbar={{ width: "3rem", breakpoint: 0 }}
			styles={{ main: { height: "100vh" } }}
		>
			<AppShell.Navbar>
				<Sidebar active={page} onNavigate={setPage} />
			</AppShell.Navbar>
			<AppShell.Main>
				{page === "bots" ? <BotsPage /> : <MatchView />}
			</AppShell.Main>
		</AppShell>
	);
}
