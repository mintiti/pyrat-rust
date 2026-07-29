import { Box, Group, Text } from "@mantine/core";
import { IconPlayerPlayFilled } from "@tabler/icons-react";
import { toDisplayState } from "../../stores/matchStore";
import type { FinishedGame } from "../../stores/tournamentStore";
import { resultFor } from "../../stores/tournamentStore";
import MazeRenderer from "../MazeRenderer";
import PressableSurface from "./PressableSurface";
import { T } from "./theme";
import { useGameReplay } from "./useGameReplay";

type Props = {
	tournamentId: number;
	game: FinishedGame;
	/** Whose perspective tints the card (gauntlet target). Defaults to canonical p1. */
	perspectiveId?: string;
	onOpen: (() => void) | null;
};

const resColor = (r: "W" | "L" | "D") =>
	r === "W" ? T.win : r === "L" ? T.loss : T.draw;

/** A finished game as a card: final-position thumbnail, score, turns, ▶ on
 * hover. Lazy-loads the replay; a failed/missing match shows an indicator
 * instead of a board. */
export default function GameCard({
	tournamentId,
	game,
	perspectiveId,
	onOpen,
}: Props) {
	const replay = useGameReplay(tournamentId, game.matchId);

	const res = resultFor(game, perspectiveId ?? game.player1Id);
	const available = replay.kind === "available";
	const inspectable = game.matchId !== null && onOpen !== null;

	return (
		<PressableSurface
			className="pyrat-game-card"
			accent={resColor(res)}
			motion="card"
			aria-label={
				inspectable
					? `Open replay: ${game.player1Id} versus ${game.player2Id}, ${game.player1Score} to ${game.player2Score}`
					: `Saved result: ${game.player1Id} versus ${game.player2Id}, ${game.player1Score} to ${game.player2Score}; replay unavailable`
			}
			style={{ padding: 6, borderRadius: "var(--mantine-radius-sm)" }}
			disabled={!inspectable}
			onClick={onOpen ?? undefined}
		>
			<Box pos="relative">
				{available ? (
					<Box h={84}>
						<MazeRenderer
							gameState={toDisplayState(replay.final_state)}
							staticBoard
							hideScoreStrip
						/>
					</Box>
				) : (
					<Box
						h={78}
						style={{
							borderRadius: 4,
							background: "#16181e",
							display: "flex",
							alignItems: "center",
							justifyContent: "center",
						}}
					>
						<Text size="xs" c="dimmed">
							{replay.kind === "loading"
								? "loading…"
								: replay.kind === "error"
									? "couldn't load"
									: "unavailable"}
						</Text>
					</Box>
				)}
				{available && (
					<Box
						className="pyrat-game-card__overlay"
						pos="absolute"
						inset={0}
						aria-hidden="true"
						style={{
							display: "flex",
							alignItems: "center",
							justifyContent: "center",
							background: "rgba(0,0,0,.35)",
							borderRadius: 4,
						}}
					>
						<IconPlayerPlayFilled size={20} color="#fff" />
					</Box>
				)}
			</Box>
			<Group gap={8} mt={5} wrap="nowrap">
				<Text size="xs" fw={700} c={resColor(res)}>
					{res}
				</Text>
				<Text size="xs" ff="monospace">
					{game.player1Score}–{game.player2Score}
				</Text>
				{available && (
					<Text size="xs" c="dimmed" ff="monospace">
						{replay.turns}t
					</Text>
				)}
			</Group>
		</PressableSurface>
	);
}
