import { AppShell, Box } from "@mantine/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirm, message } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { commands } from "./bindings";
import BotsPage from "./components/BotsPage";
import HomePage from "./components/HomePage";
import MatchView from "./components/MatchView";
import SetupView from "./components/SetupView";
import Sidebar, { type Page } from "./components/Sidebar";
import TournamentsPage from "./components/TournamentsPage";
import LiveChip from "./components/tournament/LiveChip";
import { useTournamentEvents } from "./components/tournament/useTournamentEvents";
import { useMatchEvents } from "./components/useMatchEvents";
import { useMatchStore } from "./stores/matchStore";
import {
	hasFinalTournamentVerdict,
	useTournamentStore,
} from "./stores/tournamentStore";

export type GameView = "home" | "setup" | "match";

export default function App() {
	// Mounted at the root so the tournament store updates across every tab
	// (the live chip stays current while the user is in Play/Analysis).
	useTournamentEvents();
	useMatchEvents();
	const showLive = useTournamentStore((s) => s.showLive);
	const live = useTournamentStore((s) => s.live);
	const screen = useTournamentStore((s) => s.screen);
	const viewing = useTournamentStore((s) => s.viewing);
	const restoreActive = useTournamentStore((s) => s.restoreActive);
	const starting = useTournamentStore((s) => s.starting);
	const stopping = useTournamentStore((s) => s.stopping);
	const preparing = useTournamentStore((s) => s.preparing);
	const onRuntimeStatus = useTournamentStore((s) => s.onRuntimeStatus);
	const terminalNotice = useTournamentStore((s) => s.terminalNotice);
	const dismissTerminalNotice = useTournamentStore(
		(s) => s.dismissTerminalNotice,
	);

	const [page, setPage] = useState<Page>("game");
	const [gameView, setGameView] = useState<GameView>("home");

	// The runner belongs to the Tauri app, not the webview. If the webview
	// reloads while it is running, rebuild the active read model from SQLite so
	// future events have an object to update and the return chip reappears.
	useEffect(() => {
		let mounted = true;
		void (async () => {
			try {
				const status = await commands.tournamentStatus();
				if (!mounted || status.status !== "ok") return;
				onRuntimeStatus(status.data);
				if (status.data.tournament_id === null) return;
				if (
					useTournamentStore.getState().live?.tournamentId ===
					status.data.tournament_id
				) {
					return;
				}
				const snapshot = await commands.getTournamentSnapshot(
					status.data.tournament_id,
				);
				if (mounted && snapshot.status === "ok") restoreActive(snapshot.data);
			} catch {
				// Reattachment is best effort. A normally mounted webview receives
				// lifecycle events directly; a later page visit can still open the
				// durable tournament from Recent tournaments.
			}
		})();
		return () => {
			mounted = false;
		};
	}, [onRuntimeStatus, restoreActive]);

	// Closing a non-resumable run is a real product action, not an incidental
	// window event. Confirm it in the webview, then let the backend supervisor
	// drain and persist the application-shutdown reason before destruction.
	useEffect(() => {
		let mounted = true;
		let closing = false;
		const unlisten = getCurrentWindow().onCloseRequested(async (event) => {
			const tournament = useTournamentStore.getState();
			const ownsTournament = tournament.runtimePhase !== "idle";
			if (!ownsTournament) return;

			event.preventDefault();
			if (closing) return;
			const accepted = await confirm(
				"This tournament cannot resume after PyRat closes. Completed games will be kept, and the unfinished schedule will be recorded as stopped. Stop it and close PyRat?",
				{ title: "Tournament still running", kind: "warning" },
			);
			if (!accepted || !mounted) return;

			closing = true;
			tournament.onStopping({
				tournament_id: tournament.live?.tournamentId ?? null,
			});
			const result = await commands.shutdownTournament();
			if (result.status === "error") {
				closing = false;
				const status = await commands.tournamentStatus();
				if (status.status === "ok") tournament.onRuntimeStatus(status.data);
				if (mounted) {
					await message(result.error, {
						title: "Could not stop tournament",
						kind: "error",
					});
				}
				return;
			}
			if (mounted) await getCurrentWindow().destroy();
		});
		return () => {
			mounted = false;
			unlisten.then((off) => off());
		};
	}, []);

	// Keep one mounted announcement lane for background work. Running progress is
	// bucketed so a fast tournament does not flood a screen reader with every
	// completed match, while launch, preparation, and terminal changes remain
	// explicit.
	let tournamentAnnouncement = "";
	if (live?.status === "finished") {
		tournamentAnnouncement = hasFinalTournamentVerdict(live)
			? `Tournament ${live.name ?? `#${live.tournamentId}`} finished. ${live.done} of ${live.total} games completed.`
			: `Tournament ${live.name ?? `#${live.tournamentId}`} finished with partial results. ${live.done} of ${live.total} games completed.`;
	} else if (live?.status === "aborted") {
		tournamentAnnouncement = `Tournament ${live.name ?? `#${live.tournamentId}`} stopped after ${live.done} of ${live.total} games.${live.abortReason ? ` ${live.abortReason}` : ""}`;
	} else if (stopping) {
		tournamentAnnouncement =
			"Stopping tournament and preserving completed results.";
	} else if (preparing) {
		tournamentAnnouncement = `Preparing tournament bots. ${preparing.done} of ${preparing.total} ready.`;
	} else if (starting) {
		tournamentAnnouncement = "Starting tournament.";
	} else if (live?.status === "running") {
		const percent = live.total
			? Math.min(100, Math.floor((live.done / live.total) * 10) * 10)
			: 0;
		tournamentAnnouncement = `Tournament ${live.name ?? `#${live.tournamentId}`} running. ${percent}% complete.`;
	}

	// Completion is a handoff, not permanent app chrome. If the user is already
	// looking at the tournament, there is nothing to hand off; elsewhere the
	// notice gets a short window to invite them back to the result.
	useEffect(() => {
		if (!terminalNotice) return;
		if (page === "tournaments" && screen === "live" && viewing === null) {
			dismissTerminalNotice();
			return;
		}
		const timeout = window.setTimeout(dismissTerminalNotice, 8_000);
		return () => window.clearTimeout(timeout);
	}, [dismissTerminalNotice, page, screen, terminalNotice, viewing]);

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
				<output className="pyrat-sr-only" aria-live="polite" aria-atomic="true">
					{tournamentAnnouncement}
				</output>
				<Box
					h="100%"
					style={{ display: "flex", flexDirection: "column", minHeight: 0 }}
				>
					{/* A real flow lane keeps background tournament status out of page
					    controls. Hide it only while this same active detail is visible. */}
					{(page !== "tournaments" ||
						screen !== "live" ||
						viewing !== null) && (
						<LiveChip
							onOpenTournament={(destination) => {
								if (destination === "live") showLive();
								dismissTerminalNotice();
								handlePageNav("tournaments");
							}}
						/>
					)}
					<Box style={{ flex: 1, minHeight: 0, overflow: "auto" }}>
						{content}
					</Box>
				</Box>
			</AppShell.Main>
		</AppShell>
	);
}
