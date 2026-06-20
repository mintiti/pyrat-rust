import {
	ActionIcon,
	Box,
	Button,
	Checkbox,
	Group,
	Paper,
	ScrollArea,
	Stack,
	Text,
	TextInput,
	Title,
	Tooltip,
} from "@mantine/core";
import { IconStar, IconStarFilled } from "@tabler/icons-react";
import { useAtomValue } from "jotai";
import { useEffect, useMemo, useState } from "react";
import { commands } from "../../bindings";
import type { TournamentSummary } from "../../bindings/generated";
import { discoveredBotsAtom } from "../../stores/botConfigAtom";
import { T, shortId } from "./theme";

// Pinned conditions, mirrored from the backend (tournament_config.rs) for the
// plan-line narration. The backend is the source of truth; these only render
// the forecast.
const GAMES_PER_MATCHUP = 15;
const MAX_PARALLEL = 4;
const EST_SECONDS_PER_GAME = 6.5;

export default function LaunchView() {
	const bots = useAtomValue(discoveredBotsAtom);
	const [selected, setSelected] = useState<Set<string>>(new Set());
	const [target, setTarget] = useState<string | null>(null);
	const [name, setName] = useState("");
	const [store, setStore] = useState<TournamentSummary[]>([]);
	const [launching, setLaunching] = useState(false);
	const [error, setError] = useState<string | null>(null);

	// Default-select all discovered bots once they load.
	useEffect(() => {
		setSelected((cur) =>
			cur.size === 0 ? new Set(bots.map((b) => b.agent_id)) : cur,
		);
	}, [bots]);

	useEffect(() => {
		commands.listTournaments().then((r) => {
			if (r.status === "ok") setStore(r.data);
		});
	}, []);

	const selectedBots = useMemo(
		() => bots.filter((b) => selected.has(b.agent_id)),
		[bots, selected],
	);

	const plan = useMemo(() => {
		const n = selectedBots.length;
		const pairs = target ? n - 1 : (n * (n - 1)) / 2;
		const games = Math.max(0, pairs) * GAMES_PER_MATCHUP;
		const mins = Math.max(
			1,
			Math.round((games * EST_SECONDS_PER_GAME) / MAX_PARALLEL / 60),
		);
		const shape = target
			? `${shortId(target)} vs ${n - 1} (gauntlet)`
			: `all pairs of ${n} (round-robin)`;
		return `${shape} · ${GAMES_PER_MATCHUP} games each · ${games} games · tiny preset · 200 ms/move · ${MAX_PARALLEL} concurrent · ~${mins} min`;
	}, [selectedBots, target]);

	const toggle = (id: string) =>
		setSelected((cur) => {
			const next = new Set(cur);
			if (next.has(id)) {
				next.delete(id);
				if (target === id) setTarget(null);
			} else {
				next.add(id);
			}
			return next;
		});

	const star = (id: string) => {
		setTarget((cur) => (cur === id ? null : id));
		setSelected((cur) => new Set(cur).add(id));
	};

	const launch = async () => {
		setError(null);
		setLaunching(true);
		const picks = selectedBots.map((b) => ({
			agent_id: b.agent_id,
			run_command: b.run_command,
			working_dir: b.working_dir,
		}));
		const res = await commands.startTournament({
			bots: picks,
			target,
			name: name.trim() || null,
		});
		setLaunching(false);
		if (res.status === "error") setError(res.error);
		// On success the TournamentStartedEvent flips the store to the live view.
	};

	return (
		<Box p="lg" style={{ maxWidth: 980, margin: "0 auto" }}>
			<Group align="flex-start" grow gap="lg">
				<Paper withBorder p="md" radius="md">
					<Title order={4} mb="md">
						New tournament
					</Title>

					<Text size="xs" c="dimmed" mb={6}>
						Bots <span style={{ float: "right" }}>measure</span>
					</Text>
					<Stack gap={6}>
						{bots.map((b) => {
							const isTarget = target === b.agent_id;
							return (
								<Paper
									key={b.agent_id}
									withBorder
									radius="sm"
									p="xs"
									bg={T.panel2}
								>
									<Group justify="space-between" wrap="nowrap">
										<Checkbox
											checked={selected.has(b.agent_id)}
											onChange={() => toggle(b.agent_id)}
											label={
												<Group gap={6}>
													<Text size="sm">{b.name}</Text>
													<Text size="xs" c="dimmed">
														{b.language}
													</Text>
												</Group>
											}
										/>
										<Group gap="sm" wrap="nowrap">
											<Text
												size="xs"
												c="dimmed"
												style={{ maxWidth: 220 }}
												truncate
											>
												{b.working_dir}
											</Text>
											<Tooltip
												label="Measure this bot (gauntlet)"
												position="left"
											>
												<ActionIcon
													variant="subtle"
													color={isTarget ? "yellow" : "gray"}
													onClick={() => star(b.agent_id)}
												>
													{isTarget ? (
														<IconStarFilled size={16} />
													) : (
														<IconStar size={16} />
													)}
												</ActionIcon>
											</Tooltip>
										</Group>
									</Group>
								</Paper>
							);
						})}
						{bots.length === 0 && (
							<Text size="sm" c="dimmed">
								No bots discovered. Add scan paths on the Bots page.
							</Text>
						)}
					</Stack>

					<TextInput
						mt="md"
						label="Name"
						placeholder="ckpt-1200, after-mud-fix…"
						value={name}
						onChange={(e) => setName(e.currentTarget.value)}
					/>

					<Text size="sm" c="dimmed" mt="md" mb="sm">
						{plan}
					</Text>
					{error && (
						<Text size="sm" c="red" mb="sm">
							{error}
						</Text>
					)}
					<Button
						color="yellow"
						disabled={selectedBots.length < 2 || launching}
						loading={launching}
						onClick={launch}
					>
						Launch
					</Button>
				</Paper>

				<Paper
					withBorder
					p="md"
					radius="md"
					style={{ alignSelf: "flex-start" }}
				>
					<Text size="xs" tt="uppercase" c="dimmed" mb="sm" fw={700}>
						In this store
					</Text>
					<ScrollArea.Autosize mah={360}>
						<Stack gap={0}>
							{store.length === 0 && (
								<Text size="sm" c="dimmed">
									No tournaments yet.
								</Text>
							)}
							{store.map((t) => (
								<Group
									key={t.id}
									justify="space-between"
									py={8}
									style={{ borderBottom: `1px solid ${T.line}` }}
								>
									<Text size="sm">{t.name ?? `#${t.id}`}</Text>
									<Text size="xs" c="dimmed">
										{t.finished ? "finished" : `${t.done}/${t.total}`} ·{" "}
										{t.format}
									</Text>
								</Group>
							))}
						</Stack>
					</ScrollArea.Autosize>
				</Paper>
			</Group>
		</Box>
	);
}
