import {
	Button,
	Card,
	Center,
	Group,
	Stack,
	Text,
	ThemeIcon,
	Title,
} from "@mantine/core";
import { IconMicroscope, IconPlayerPlay } from "@tabler/icons-react";
import type { GameView } from "../App";
import { useMatchStore } from "../stores/matchStore";

type Props = {
	onNavigate: (view: GameView) => void;
};

export default function HomePage({ onNavigate }: Props) {
	return (
		<Center h="100%">
			<Group gap="lg">
				<Card shadow="sm" withBorder padding="xl" w={260}>
					<Stack align="center" justify="space-between" h="100%" gap="md">
						<ThemeIcon size={56} radius="md" variant="light">
							<IconPlayerPlay size={32} />
						</ThemeIcon>
						<Stack align="center" gap={4}>
							<Title order={4}>Play a Game</Title>
							<Text size="sm" c="dimmed" ta="center">
								Set up a match between two bots and watch them play
							</Text>
						</Stack>
						<Button
							variant="light"
							fullWidth
							onClick={() => {
								useMatchStore.getState().setMode("auto");
								onNavigate("setup");
							}}
						>
							Start
						</Button>
					</Stack>
				</Card>
				<Card shadow="sm" withBorder padding="xl" w={260}>
					<Stack align="center" justify="space-between" h="100%" gap="md">
						<ThemeIcon size={56} radius="md" variant="light" color="violet">
							<IconMicroscope size={32} />
						</ThemeIcon>
						<Stack align="center" gap={4}>
							<Title order={4}>Analyze</Title>
							<Text size="sm" c="dimmed" ta="center">
								Step through a match turn by turn, watch bots think
							</Text>
						</Stack>
						<Button
							variant="light"
							color="violet"
							fullWidth
							onClick={() => {
								useMatchStore.getState().setMode("step");
								onNavigate("setup");
							}}
						>
							Start
						</Button>
					</Stack>
				</Card>
			</Group>
		</Center>
	);
}
