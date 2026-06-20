import {
	ActionIcon,
	Box,
	Button,
	Checkbox,
	Collapse,
	Group,
	NumberInput,
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
import type {
	GameFactoryConfig,
	TournamentSummary,
} from "../../bindings/generated";
import { discoveredBotsAtom } from "../../stores/botConfigAtom";
import { validate } from "../../stores/gameConfig";
import { useTournamentStore } from "../../stores/tournamentStore";
import GameFactoryForm from "./GameFactoryForm";
import { T, shortId } from "./theme";

// Rough wall-time estimate for the plan line. Not load-bearing — the planner
// drives real timing; this just sets expectations.
const EST_SECONDS_PER_GAME = 6.5;

interface Methodology {
	mazes_per_matchup: number;
	move_timeout_ms: number;
	preprocessing_timeout_ms: number;
	max_parallel: number;
}

export default function LaunchView() {
	const bots = useAtomValue(discoveredBotsAtom);
	const live = useTournamentStore((s) => s.live);
	const showLive = useTournamentStore((s) => s.showLive);

	const [selected, setSelected] = useState<Set<string>>(new Set());
	const [target, setTarget] = useState<string | null>(null);
	const [name, setName] = useState("");
	const [store, setStore] = useState<TournamentSummary[]>([]);
	const [launching, setLaunching] = useState(false);
	const [error, setError] = useState<string | null>(null);

	// Conditions, pre-filled from the backend defaults (ladder recipe). Null
	// until they load — the Rust constants are the single source of truth, so
	// there's no stale mirror to drift.
	const [factory, setFactory] = useState<GameFactoryConfig | null>(null);
	const [method, setMethod] = useState<Methodology | null>(null);
	const [seedInput, setSeedInput] = useState("");
	const [advanced, setAdvanced] = useState(false);

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
		commands.getTournamentLaunchDefaults().then((r) => {
			if (r.status === "ok") {
				setFactory(r.data.factory);
				setMethod({
					mazes_per_matchup: r.data.mazes_per_matchup,
					move_timeout_ms: r.data.move_timeout_ms,
					preprocessing_timeout_ms: r.data.preprocessing_timeout_ms,
					max_parallel: r.data.max_parallel,
				});
			} else {
				setError(r.error);
			}
		});
	}, []);

	const selectedBots = useMemo(
		() => bots.filter((b) => selected.has(b.agent_id)),
		[bots, selected],
	);

	const factoryErrors = useMemo(
		() => (factory ? validate(factory) : {}),
		[factory],
	);
	const hasFactoryError = Object.keys(factoryErrors).length > 0;

	const plan = useMemo(() => {
		if (!factory || !method) return "";
		const n = selectedBots.length;
		const pairs = target ? n - 1 : (n * (n - 1)) / 2;
		const games = Math.max(0, pairs) * method.mazes_per_matchup * 2;
		const mins = Math.max(
			1,
			Math.round(
				(games * EST_SECONDS_PER_GAME) / Math.max(1, method.max_parallel) / 60,
			),
		);
		const shape = target
			? `${shortId(target)} vs ${n - 1} (gauntlet)`
			: `all pairs of ${n} (round-robin)`;
		return `${shape} · ${factory.width}×${factory.height} · ${method.mazes_per_matchup * 2} games/matchup · ${games} games · ${method.move_timeout_ms} ms/move · ${method.max_parallel} concurrent · ~${mins} min`;
	}, [selectedBots, target, factory, method]);

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
		if (!factory || !method) return;
		setError(null);
		setLaunching(true);
		const picks = selectedBots.map((b) => ({
			agent_id: b.agent_id,
			run_command: b.run_command,
			working_dir: b.working_dir,
		}));
		const trimmedSeed = seedInput.trim();
		const res = await commands.startTournament({
			bots: picks,
			target,
			name: name.trim() || null,
			factory,
			mazes_per_matchup: method.mazes_per_matchup,
			move_timeout_ms: method.move_timeout_ms,
			preprocessing_timeout_ms: method.preprocessing_timeout_ms,
			max_parallel: method.max_parallel,
			tournament_seed: trimmedSeed === "" ? null : Number(trimmedSeed),
		});
		setLaunching(false);
		if (res.status === "error") setError(res.error);
		// On success the TournamentStartedEvent flips the store to the live view.
	};

	const tournamentRunning = live?.status === "running";

	return (
		<Box p="lg" style={{ maxWidth: 980, margin: "0 auto" }}>
			{tournamentRunning && live && (
				<Paper withBorder p="sm" radius="md" mb="md" bg={T.panel2}>
					<Group justify="space-between">
						<Text size="sm">
							<b>{live.name ?? `#${live.tournamentId}`}</b> is running ·{" "}
							{live.done}/{live.total} games
						</Text>
						<Group gap="xs">
							<Button size="compact-sm" variant="light" onClick={showLive}>
								View live
							</Button>
							<Button
								size="compact-sm"
								variant="subtle"
								color="red"
								onClick={() => commands.stopTournament()}
							>
								Stop
							</Button>
						</Group>
					</Group>
				</Paper>
			)}

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

					<Text size="xs" tt="uppercase" c="dimmed" mt="lg" mb="sm" fw={700}>
						Conditions
					</Text>
					{factory && method ? (
						<Stack gap="sm">
							<SettingRowInline
								label="Mazes / matchup"
								hint="×2 games (paired seats)"
							>
								<NumberInput
									w={90}
									min={1}
									value={method.mazes_per_matchup}
									onChange={(v) =>
										setMethod({ ...method, mazes_per_matchup: Number(v) || 1 })
									}
								/>
							</SettingRowInline>
							<SettingRowInline label="Move budget" hint="ms / move">
								<NumberInput
									w={110}
									min={1}
									step={50}
									value={method.move_timeout_ms}
									onChange={(v) =>
										setMethod({ ...method, move_timeout_ms: Number(v) || 1 })
									}
								/>
							</SettingRowInline>

							<GameFactoryForm
								value={factory}
								onChange={setFactory}
								errors={factoryErrors}
							/>

							<Button
								variant="subtle"
								size="compact-xs"
								onClick={() => setAdvanced((a) => !a)}
								style={{ alignSelf: "flex-start" }}
							>
								{advanced ? "Hide methodology" : "Advanced methodology"}
							</Button>
							<Collapse in={advanced}>
								<Stack gap="sm">
									<SettingRowInline label="Concurrency" hint="parallel matches">
										<NumberInput
											w={90}
											min={1}
											value={method.max_parallel}
											onChange={(v) =>
												setMethod({ ...method, max_parallel: Number(v) || 1 })
											}
										/>
									</SettingRowInline>
									<SettingRowInline label="Preprocessing budget" hint="ms">
										<NumberInput
											w={110}
											min={1}
											step={100}
											value={method.preprocessing_timeout_ms}
											onChange={(v) =>
												setMethod({
													...method,
													preprocessing_timeout_ms: Number(v) || 1,
												})
											}
										/>
									</SettingRowInline>
									<SettingRowInline label="Seed" hint="blank = random">
										<NumberInput
											w={160}
											min={0}
											max={Number.MAX_SAFE_INTEGER}
											allowDecimal={false}
											value={seedInput === "" ? "" : Number(seedInput)}
											onChange={(v) => setSeedInput(v === "" ? "" : String(v))}
										/>
									</SettingRowInline>
								</Stack>
							</Collapse>
						</Stack>
					) : (
						<Text size="sm" c="dimmed">
							Loading defaults…
						</Text>
					)}

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
						disabled={
							selectedBots.length < 2 ||
							launching ||
							!factory ||
							!method ||
							hasFactoryError
						}
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

/** A compact label/control row for the methodology knobs (lighter than the
 * shared `SettingRow`, which is sized for the factory form). */
function SettingRowInline({
	label,
	hint,
	children,
}: {
	label: string;
	hint?: string;
	children: React.ReactNode;
}) {
	return (
		<Group justify="space-between" wrap="nowrap" gap="lg">
			<Stack gap={0}>
				<Text size="sm" fw={500}>
					{label}
				</Text>
				{hint && (
					<Text size="xs" c="dimmed">
						{hint}
					</Text>
				)}
			</Stack>
			<div style={{ flexShrink: 0 }}>{children}</div>
		</Group>
	);
}
