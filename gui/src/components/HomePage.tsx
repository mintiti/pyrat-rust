import {
	Button,
	Card,
	Center,
	SimpleGrid,
	Stack,
	Text,
	ThemeIcon,
	Title,
} from "@mantine/core";
import {
	IconMicroscope,
	IconPlayerPlay,
	IconTrophy,
} from "@tabler/icons-react";
import type { GameView } from "../App";
import { useMatchStore } from "../stores/matchStore";

type Props = {
	onNavigate: (view: GameView) => void;
	onOpenTournaments: () => void;
};

export default function HomePage({ onNavigate, onOpenTournaments }: Props) {
	return (
		<Center mih="100%" py="lg">
			<SimpleGrid
				cols={{ base: 1, sm: 3 }}
				spacing={{ base: "sm", sm: "md" }}
				w="100%"
				maw={900}
				px="lg"
			>
				<Card shadow="sm" withBorder padding="lg">
					<Stack align="center" justify="space-between" h="100%" gap="sm">
						<ThemeIcon size={48} radius="md" variant="light">
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
							Play
						</Button>
					</Stack>
				</Card>
				<Card shadow="sm" withBorder padding="lg">
					<Stack align="center" justify="space-between" h="100%" gap="sm">
						<ThemeIcon size={48} radius="md" variant="light" color="violet">
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
							Analyze
						</Button>
					</Stack>
				</Card>
				<Card shadow="sm" withBorder padding="lg">
					<Stack align="center" justify="space-between" h="100%" gap="sm">
						<ThemeIcon size={48} radius="md" variant="light" color="yellow">
							<IconTrophy size={32} />
						</ThemeIcon>
						<Stack align="center" gap={4}>
							<Title order={4}>Tournament</Title>
							<Text size="sm" c="dimmed" ta="center">
								Rank a bot against a pool and watch the standings live
							</Text>
						</Stack>
						<Button
							variant="light"
							color="yellow"
							fullWidth
							onClick={onOpenTournaments}
						>
							Open tournaments
						</Button>
					</Stack>
				</Card>
			</SimpleGrid>
		</Center>
	);
}
