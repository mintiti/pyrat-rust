import { useState } from "react";
import BotsPage from "./components/BotsPage";
import MatchView from "./components/MatchView";

type View = "match" | "bots";

export default function App() {
	const [view, setView] = useState<View>("match");

	if (view === "bots") {
		return <BotsPage onNavigate={setView} />;
	}
	return <MatchView onNavigate={setView} />;
}
