import {
	ActionIcon,
	Badge,
	Box,
	Button,
	Center,
	Code,
	Group,
	Paper,
	ScrollArea,
	SimpleGrid,
	Stack,
	Text,
	ThemeIcon,
	Title,
	Tooltip,
} from "@mantine/core";
import {
	IconFolderPlus,
	IconRefresh,
	IconRobot,
	IconTrash,
} from "@tabler/icons-react";
import { open } from "@tauri-apps/plugin-dialog";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { useState } from "react";
import type { DiscoveredBot } from "../bindings/generated";
import {
	asyncScanPathsAtom,
	discoveredBotsAtom,
	refreshBotsAtom,
	scanPathsAtom,
} from "../stores/botConfigAtom";

export default function BotsPage() {
	const scanPaths = useAtomValue(scanPathsAtom);
	const [, setScanPaths] = useAtom(asyncScanPathsAtom);
	const discovered = useAtomValue(discoveredBotsAtom);
	const refresh = useSetAtom(refreshBotsAtom);
	const [selectedId, setSelectedId] = useState<string | null>(null);

	const selectedBot = discovered.find((b) => b.agent_id === selectedId) ?? null;

	const handleAddFolder = async () => {
		const result = await open({ directory: true, multiple: false });
		if (typeof result === "string") {
			const next = [...scanPaths, result];
			await setScanPaths(next);
			await refresh();
		}
	};

	const handleRemovePath = async (path: string) => {
		const next = scanPaths.filter((p) => p !== path);
		await setScanPaths(next);
		await refresh();
	};

	const handleRefresh = () => {
		refresh();
	};

	return (
		<Stack h="100%" px="lg" pb="lg">
			{/* Header */}
			<Group
				justify="space-between"
				py="sm"
				style={{ borderBottom: "1px solid var(--mantine-color-dark-4)" }}
			>
				<Title order={3}>Bots</Title>
				<Tooltip label="Re-scan all paths">
					<ActionIcon variant="subtle" onClick={handleRefresh}>
						<IconRefresh size={18} />
					</ActionIcon>
				</Tooltip>
			</Group>

			{/* Scan paths */}
			<Stack gap="xs">
				<Text size="sm" fw={500}>
					Scan Paths
				</Text>
				{scanPaths.length === 0 && (
					<Text size="sm" c="dimmed">
						No scan paths yet. Add a folder to discover bots.
					</Text>
				)}
				{scanPaths.map((path) => (
					<Group key={path} gap="xs" wrap="nowrap">
						<Code
							style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}
						>
							{path}
						</Code>
						<ActionIcon
							variant="subtle"
							color="red"
							size="sm"
							onClick={() => handleRemovePath(path)}
						>
							<IconTrash size={14} />
						</ActionIcon>
					</Group>
				))}
				<Button
					variant="light"
					size="xs"
					leftSection={<IconFolderPlus size={14} />}
					onClick={handleAddFolder}
					style={{ alignSelf: "start" }}
				>
					Add Folder
				</Button>
			</Stack>

			{/* Bot grid + detail panel */}
			<Group grow flex={1} style={{ overflow: "hidden" }} align="start">
				<ScrollArea h="100%" offsetScrollbars>
					{scanPaths.length > 0 && discovered.length === 0 ? (
						<Center py="xl">
							<Text c="dimmed">
								No bots found. Each bot needs a bot.toml file.
							</Text>
						</Center>
					) : (
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

							{/* Discovered bot cards */}
							{discovered.map((bot) => (
								<BotCard
									key={bot.agent_id}
									bot={bot}
									selected={selectedId === bot.agent_id}
									onClick={() => setSelectedId(bot.agent_id)}
								/>
							))}
						</SimpleGrid>
					)}
				</ScrollArea>

				{/* Right panel: detail or empty state */}
				{selectedBot ? (
					<BotDetail bot={selectedBot} />
				) : (
					<Center h="100%">
						<Stack align="center" gap="sm">
							<ThemeIcon size={80} radius="100%" variant="light" color="gray">
								<IconRobot size={40} />
							</ThemeIcon>
							<Text c="dimmed" fw={500} size="lg">
								Select a bot to view details
							</Text>
						</Stack>
					</Center>
				)}
			</Group>
		</Stack>
	);
}

function BotCard({
	bot,
	selected,
	onClick,
}: {
	bot: DiscoveredBot;
	selected: boolean;
	onClick: () => void;
}) {
	return (
		<Box
			p="md"
			component="button"
			type="button"
			onClick={onClick}
			style={{
				cursor: "pointer",
				border: selected
					? "2px solid var(--mantine-color-yellow-filled)"
					: "1px solid var(--mantine-color-dark-4)",
				borderRadius: "var(--mantine-radius-md)",
				background: "transparent",
				color: "inherit",
				textAlign: "left",
				width: "100%",
				boxShadow: selected ? "var(--mantine-shadow-sm)" : undefined,
			}}
		>
			<Group gap="xs" mb={4}>
				<Text fw="bold" lineClamp={1} style={{ flex: 1 }}>
					{bot.name}
				</Text>
				{bot.language && (
					<Badge size="xs" variant="light">
						{bot.language}
					</Badge>
				)}
			</Group>
			<Text size="xs" c="dimmed" lineClamp={2}>
				{bot.description || "(no description)"}
			</Text>
		</Box>
	);
}

function BotDetail({ bot }: { bot: DiscoveredBot }) {
	return (
		<Paper withBorder p="md" h="100%">
			<ScrollArea h="100%" offsetScrollbars>
				<Stack gap="sm">
					<Title order={4}>{bot.name}</Title>

					<DetailRow label="Agent ID" value={bot.agent_id} />
					<DetailRow label="Language" value={bot.language} />
					<DetailRow label="Developer" value={bot.developer} />
					<DetailRow label="Description" value={bot.description} />
					<DetailRow label="Run Command">
						<Code>{bot.run_command}</Code>
					</DetailRow>
					<DetailRow label="Working Directory">
						<Code>{bot.working_dir}</Code>
					</DetailRow>

					{bot.tags.length > 0 && (
						<div>
							<Text size="xs" c="dimmed" mb={4}>
								Tags
							</Text>
							<Group gap={4}>
								{bot.tags.map((tag) => (
									<Badge key={tag} size="xs" variant="outline">
										{tag}
									</Badge>
								))}
							</Group>
						</div>
					)}
				</Stack>
			</ScrollArea>
		</Paper>
	);
}

function DetailRow({
	label,
	value,
	children,
}: {
	label: string;
	value?: string;
	children?: React.ReactNode;
}) {
	const content = children ?? <Text size="sm">{value || "\u2014"}</Text>;
	return (
		<div>
			<Text size="xs" c="dimmed">
				{label}
			</Text>
			{content}
		</div>
	);
}
