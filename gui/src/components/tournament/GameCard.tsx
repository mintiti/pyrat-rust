import { Box, Group, Paper, Text } from "@mantine/core";
import { IconPlayerPlayFilled } from "@tabler/icons-react";
import { useState } from "react";
import { toDisplayState } from "../../stores/matchStore";
import type { FinishedGame } from "../../stores/tournamentStore";
import { resultFor } from "../../stores/tournamentStore";
import MazeRenderer from "../MazeRenderer";
import { T } from "./theme";
import { useGameReplay } from "./useGameReplay";

type Props = {
	tournamentId: number;
	game: FinishedGame;
	/** Whose perspective tints the card (gauntlet target). Defaults to canonical p1. */
	perspectiveId?: string;
	onOpen: () => void;
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
	const [hover, setHover] = useState(false);
	const replay = useGameReplay(tournamentId, game.matchId);

	const res = resultFor(game, perspectiveId ?? game.player1Id);
	const available = replay?.kind === "available";

	return (
		<Paper
			withBorder
			radius="sm"
			p={6}
			bg={T.panel2}
			style={{
				cursor: "pointer",
				borderColor: hover ? T.cheeseDim : undefined,
				boxShadow: `inset 3px 0 0 ${resColor(res)}`,
				transform: hover ? "translateY(-2px)" : undefined,
				transition: "border-color .15s, transform .15s",
			}}
			onMouseEnter={() => setHover(true)}
			onMouseLeave={() => setHover(false)}
			onClick={onOpen}
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
							{replay ? "unavailable" : "…"}
						</Text>
					</Box>
				)}
				{available && hover && (
					<Box
						pos="absolute"
						inset={0}
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
		</Paper>
	);
}
