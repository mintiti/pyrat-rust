import {
	ActionIcon,
	Box,
	Button,
	Center,
	Divider,
	Group,
	Paper,
	ScrollArea,
	SimpleGrid,
	Stack,
	Text,
	TextInput,
	ThemeIcon,
	Title,
} from "@mantine/core";
import { IconArrowLeft, IconPlus, IconRobot } from "@tabler/icons-react";
import { useAtom, useAtomValue } from "jotai";
import { useEffect, useState } from "react";
import type { View } from "../App";
import {
	type BotConfig,
	asyncBotsAtom,
	botsAtom,
} from "../stores/botConfigAtom";

type Props = {
	onNavigate: (view: View) => void;
};

export default function BotsPage({ onNavigate }: Props) {
	const bots = useAtomValue(botsAtom);
	const [, setBots] = useAtom(asyncBotsAtom);
	const [selectedBotId, setSelectedBotId] = useState<string | null>(null);

	// Local draft for the selected bot — edits are cheap, only persisted on blur.
	const selectedBot = bots.find((b) => b.id === selectedBotId) ?? null;
	const [draft, setDraft] = useState<BotConfig | null>(null);

	// Sync draft when selection changes — keyed on ID, not object ref
	// biome-ignore lint/correctness/useExhaustiveDependencies: intentionally keyed on selectedBotId
	useEffect(() => {
		setDraft(selectedBot ? { ...selectedBot } : null);
	}, [selectedBotId]);

	const flushDraft = () => {
		if (!draft) return;
		setBots(bots.map((b) => (b.id === draft.id ? draft : b)));
	};

	const handleAdd = () => {
		const newBot: BotConfig = {
			id: crypto.randomUUID(),
			name: "New Bot",
			command: "",
			working_dir: null,
		};
		setBots([...bots, newBot]);
		setSelectedBotId(newBot.id);
	};

	const handleDelete = (id: string) => {
		setBots(bots.filter((b) => b.id !== id));
		if (selectedBotId === id) setSelectedBotId(null);
	};

	return (
		<Stack h="100vh" px="lg" pb="lg">
			<Group
				justify="space-between"
				py="sm"
				style={{ borderBottom: "1px solid var(--mantine-color-dark-4)" }}
			>
				<Group gap="sm">
					<ActionIcon variant="subtle" onClick={() => onNavigate("match")}>
						<IconArrowLeft size={18} />
					</ActionIcon>
					<Title order={3}>Bot Management</Title>
				</Group>
			</Group>

			<Group grow flex={1} style={{ overflow: "hidden" }} align="start">
				<ScrollArea h="100%" offsetScrollbars>
					<SimpleGrid cols={{ base: 1, md: 2 }} spacing="sm">
						{/* Built-in random bot card */}
						<Box
							p="md"
							style={{
								border: "1px solid var(--mantine-color-dark-4)",
								borderRadius: "var(--mantine-radius-md)",
								opacity: 0.5,
							}}
						>
							<Text fw="bold" lineClamp={1}>
								Random Bot
							</Text>
							<Text size="xs" c="dimmed">
								(built-in)
							</Text>
						</Box>

						{/* User bot cards */}
						{bots.map((bot) => (
							<Box
								key={bot.id}
								p="md"
								component="button"
								type="button"
								onClick={() => setSelectedBotId(bot.id)}
								style={{
									cursor: "pointer",
									border:
										selectedBotId === bot.id
											? "2px solid var(--mantine-color-yellow-filled)"
											: "1px solid var(--mantine-color-dark-4)",
									borderRadius: "var(--mantine-radius-md)",
									background: "transparent",
									color: "inherit",
									textAlign: "left",
									width: "100%",
									boxShadow:
										selectedBotId === bot.id
											? "var(--mantine-shadow-sm)"
											: undefined,
								}}
							>
								<Text fw="bold" lineClamp={1}>
									{bot.name}
								</Text>
								<Text size="xs" c="dimmed" lineClamp={1}>
									{bot.command || "(no command)"}
								</Text>
							</Box>
						))}

						{/* Add bot card */}
						<Box
							p="md"
							component="button"
							type="button"
							onClick={handleAdd}
							style={{
								cursor: "pointer",
								border: "1px dashed var(--mantine-color-dark-4)",
								borderRadius: "var(--mantine-radius-md)",
								background: "transparent",
								color: "inherit",
								width: "100%",
							}}
						>
							<Stack gap={0} justify="center" align="center" w="100%" h="100%">
								<Text mb={10}>Add New</Text>
								<IconPlus size="1.3rem" />
							</Stack>
						</Box>
					</SimpleGrid>
				</ScrollArea>

				{/* Right panel: details or empty state */}
				{draft ? (
					<Paper withBorder p="md" h="100%">
						<ScrollArea h="100%" offsetScrollbars>
							<Stack>
								<Divider variant="dashed" label="General Settings" />
								<TextInput
									label="Name"
									value={draft.name}
									onChange={(e) =>
										setDraft({ ...draft, name: e.currentTarget.value })
									}
									onBlur={flushDraft}
								/>
								<TextInput
									label="Command"
									description="Shell command to launch the bot"
									value={draft.command}
									onChange={(e) =>
										setDraft({ ...draft, command: e.currentTarget.value })
									}
									onBlur={flushDraft}
								/>
								<TextInput
									label="Working Directory"
									description="Optional. Defaults to current dir."
									value={draft.working_dir ?? ""}
									onChange={(e) =>
										setDraft({
											...draft,
											working_dir: e.currentTarget.value || null,
										})
									}
									onBlur={flushDraft}
								/>

								<Group justify="end">
									<Button color="red" onClick={() => handleDelete(draft.id)}>
										Remove
									</Button>
								</Group>
							</Stack>
						</ScrollArea>
					</Paper>
				) : (
					<Center h="100%">
						<Stack align="center" gap="sm">
							<ThemeIcon size={80} radius="100%" variant="light" color="gray">
								<IconRobot size={40} />
							</ThemeIcon>
							<Text c="dimmed" fw={500} size="lg">
								Select a bot to configure
							</Text>
						</Stack>
					</Center>
				)}
			</Group>
		</Stack>
	);
}
