import {
	Card,
	Center,
	Group,
	Loader,
	MantineProvider,
	Stack,
	Text,
	Title,
	createTheme,
} from "@mantine/core";
import { useEffect, useState } from "react";
import { commands } from "./bindings";
import type { GameInfo } from "./bindings/generated";

const theme = createTheme({
	primaryColor: "yellow",
});

function GameCard({ info }: { info: GameInfo }) {
	return (
		<Card shadow="sm" padding="lg" radius="md" withBorder>
			<Stack gap="sm">
				<Title order={3}>Game Info</Title>
				<Group gap="xl">
					<div>
						<Text size="sm" c="dimmed">
							Board
						</Text>
						<Text fw={500}>
							{info.width} x {info.height}
						</Text>
					</div>
					<div>
						<Text size="sm" c="dimmed">
							Cheese
						</Text>
						<Text fw={500}>{info.total_cheese}</Text>
					</div>
					<div>
						<Text size="sm" c="dimmed">
							Max Turns
						</Text>
						<Text fw={500}>{info.max_turns}</Text>
					</div>
				</Group>
				<Group gap="xl">
					<div>
						<Text size="sm" c="dimmed">
							Rat (P1)
						</Text>
						<Text fw={500}>
							({info.player1_position[0]}, {info.player1_position[1]})
						</Text>
					</div>
					<div>
						<Text size="sm" c="dimmed">
							Python (P2)
						</Text>
						<Text fw={500}>
							({info.player2_position[0]}, {info.player2_position[1]})
						</Text>
					</div>
				</Group>
			</Stack>
		</Card>
	);
}

export default function App() {
	const [info, setInfo] = useState<GameInfo | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		commands
			.getGameInfo()
			.then((result) => {
				if (result.status === "ok") {
					setInfo(result.data);
				} else {
					setError(result.error);
				}
			})
			.catch((err: unknown) => setError(String(err)));
	}, []);

	return (
		<MantineProvider theme={theme} defaultColorScheme="dark">
			<Center h="100vh" p="md">
				{error ? (
					<Text c="red">{error}</Text>
				) : info ? (
					<GameCard info={info} />
				) : (
					<Loader />
				)}
			</Center>
		</MantineProvider>
	);
}
