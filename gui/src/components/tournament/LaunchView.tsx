import {
	Box,
	Button,
	Checkbox,
	Collapse,
	Group,
	NumberInput,
	Paper,
	ScrollArea,
	SimpleGrid,
	Stack,
	Text,
	TextInput,
	Title,
} from "@mantine/core";
import { useMediaQuery } from "@mantine/hooks";
import {
	IconChevronRight,
	IconStar,
	IconStarFilled,
} from "@tabler/icons-react";
import { useAtomValue } from "jotai";
import {
	type ReactElement,
	cloneElement,
	useCallback,
	useEffect,
	useId,
	useMemo,
	useRef,
	useState,
} from "react";
import { commands } from "../../bindings";
import type {
	GameFactoryConfig,
	TournamentSummary,
} from "../../bindings/generated";
import { discoveredBotsAtom } from "../../stores/botConfigAtom";
import { validate } from "../../stores/gameConfig";
import { useTournamentStore } from "../../stores/tournamentStore";
import GameFactoryForm from "./GameFactoryForm";
import { PulseDot } from "./LiveView";
import PressableSurface from "./PressableSurface";
import { T, shortId } from "./theme";
import { createLatestRequestGuard } from "./tournamentAsync";

// Rough wall-time estimate for the plan line. Not load-bearing — the planner
// drives real timing; this just sets expectations.
const EST_SECONDS_PER_GAME = 6.5;

// Below this move budget, search-style bots routinely overrun. We keep the
// default ladder-comparable (200 ms) but flag a tight choice so a wall of
// timeouts reads as "I set this low", not "the tool is broken".
const TIGHT_MOVE_BUDGET_MS = 300;

interface Methodology {
	mazes_per_matchup: number;
	move_timeout_ms: number;
	preprocessing_timeout_ms: number;
	max_parallel: number;
}

export default function LaunchView() {
	const boundedColumns = useMediaQuery("(min-width: 48em)");
	const bots = useAtomValue(discoveredBotsAtom);
	const live = useTournamentStore((s) => s.live);
	const showLive = useTournamentStore((s) => s.showLive);
	const openSnapshot = useTournamentStore((s) => s.openSnapshot);
	const starting = useTournamentStore((s) => s.starting);
	const preparing = useTournamentStore((s) => s.preparing);
	const beginLaunch = useTournamentStore((s) => s.beginLaunch);
	const clearPreparing = useTournamentStore((s) => s.clearPreparing);

	const [selected, setSelected] = useState<Set<string>>(new Set());
	const [target, setTarget] = useState<string | null>(null);
	const [name, setName] = useState("");
	const [store, setStore] = useState<TournamentSummary[]>([]);
	const [recentExpanded, setRecentExpanded] = useState(false);
	const [storeLoading, setStoreLoading] = useState(true);
	const [storeError, setStoreError] = useState<string | null>(null);
	const [openingTournamentId, setOpeningTournamentId] = useState<number | null>(
		null,
	);
	const [defaultsLoading, setDefaultsLoading] = useState(true);
	const [defaultsError, setDefaultsError] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const openRequest = useRef(createLatestRequestGuard());

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

	const loadStore = useCallback(async () => {
		setStoreLoading(true);
		setStoreError(null);
		try {
			const result = await commands.listTournaments();
			if (result.status === "ok") setStore(result.data);
			else setStoreError(result.error);
		} catch (cause) {
			setStoreError(String(cause));
		} finally {
			setStoreLoading(false);
		}
	}, []);

	const loadDefaults = useCallback(async () => {
		setDefaultsLoading(true);
		setDefaultsError(null);
		try {
			const result = await commands.getTournamentLaunchDefaults();
			if (result.status === "ok") {
				setFactory(result.data.factory);
				setMethod({
					mazes_per_matchup: result.data.mazes_per_matchup,
					move_timeout_ms: result.data.move_timeout_ms,
					preprocessing_timeout_ms: result.data.preprocessing_timeout_ms,
					max_parallel: result.data.max_parallel,
				});
			} else {
				setDefaultsError(result.error);
			}
		} catch (cause) {
			setDefaultsError(String(cause));
		} finally {
			setDefaultsLoading(false);
		}
	}, []);

	const returnToLive = useCallback(() => {
		openRequest.current.invalidate();
		setOpeningTournamentId(null);
		showLive();
	}, [showLive]);

	const openTournament = async (tournament: TournamentSummary) => {
		if (live?.tournamentId === tournament.id) {
			returnToLive();
			return;
		}
		const request = openRequest.current.begin();
		setOpeningTournamentId(tournament.id);
		setStoreError(null);
		try {
			const result = await commands.getTournamentSnapshot(tournament.id);
			if (!openRequest.current.isCurrent(request)) return;
			if (result.status === "ok") openSnapshot(result.data);
			else setStoreError(result.error);
		} catch (cause) {
			if (openRequest.current.isCurrent(request)) setStoreError(String(cause));
		} finally {
			if (openRequest.current.isCurrent(request)) {
				setOpeningTournamentId(null);
			}
		}
	};

	useEffect(() => {
		void loadStore();
		void loadDefaults();
	}, [loadDefaults, loadStore]);

	useEffect(
		() => () => {
			// A slow historical read must not navigate the app after this launch
			// surface has been replaced by another page.
			openRequest.current.invalidate();
		},
		[],
	);

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
		if (n < 2) return "";
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

	const setMeasuredBot = (id: string) => {
		setTarget((cur) => (cur === id ? null : id));
		setSelected((cur) => new Set(cur).add(id));
	};

	const launch = async () => {
		if (!factory || !method) return;
		setError(null);
		beginLaunch();
		const picks = selectedBots.map((b) => ({
			agent_id: b.agent_id,
			run_command: b.run_command,
			working_dir: b.working_dir,
		}));
		const trimmedSeed = seedInput.trim();
		try {
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
			if (res.status === "ok") return;

			clearPreparing();
			// A user-initiated Stop during preparing surfaces as this; it's not
			// an error to alarm on.
			if (res.error !== "tournament start was cancelled") setError(res.error);
		} catch (cause) {
			clearPreparing();
			setError(String(cause));
		}
		// On success the TournamentStartedEvent flips the store to the live view
		// and clears `preparing`.
	};

	const tournamentRunning = live?.status === "running";

	const roster = (
		<Stack gap={6} pr={boundedColumns ? "xs" : 0}>
			{bots.map((bot) => {
				const isTarget = target === bot.agent_id;
				return (
					<Paper key={bot.agent_id} withBorder radius="sm" p="xs" bg={T.panel2}>
						<Group justify="space-between" wrap="nowrap" gap="xs">
							<Checkbox
								checked={selected.has(bot.agent_id)}
								onChange={() => toggle(bot.agent_id)}
								aria-label={`Include ${bot.name} in this tournament`}
								label={
									<Box style={{ minWidth: 0 }}>
										<Group gap={6} wrap="nowrap">
											<Text size="sm" truncate>
												{bot.name}
											</Text>
											<Text size="xs" c="dimmed" style={{ flexShrink: 0 }}>
												{bot.language}
											</Text>
										</Group>
										<Text size="xs" c="dimmed" truncate>
											{bot.working_dir}
										</Text>
									</Box>
								}
								styles={{
									root: { flex: 1, minWidth: 0 },
									body: { width: "100%" },
									labelWrapper: { flex: 1, minWidth: 0 },
									label: { display: "block", minWidth: 0 },
								}}
							/>
							<Button
								size="compact-xs"
								variant={isTarget ? "light" : "subtle"}
								color={isTarget ? "yellow" : "gray"}
								leftSection={
									isTarget ? (
										<IconStarFilled size={13} />
									) : (
										<IconStar size={13} />
									)
								}
								aria-pressed={isTarget}
								aria-label={`${isTarget ? "Stop measuring" : "Measure"} ${bot.name} against the selected field`}
								onClick={() => setMeasuredBot(bot.agent_id)}
								style={{ flexShrink: 0 }}
							>
								{isTarget ? "Measured" : "Measure"}
							</Button>
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
	);

	const conditions = (
		<Stack gap="sm" pr={boundedColumns ? "xs" : 0}>
			<TextInput
				label="Tournament name"
				placeholder="ckpt-1200, after-mud-fix…"
				value={name}
				onChange={(event) => setName(event.currentTarget.value)}
			/>

			{factory && method ? (
				<>
					<SettingRowInline
						label="Mazes / matchup"
						hint="×2 games (paired seats)"
					>
						<NumberInput
							w={90}
							min={1}
							value={method.mazes_per_matchup}
							onChange={(value) =>
								setMethod({
									...method,
									mazes_per_matchup: Number(value) || 1,
								})
							}
						/>
					</SettingRowInline>
					<SettingRowInline label="Move budget" hint="ms / move">
						<NumberInput
							w={110}
							min={1}
							step={50}
							value={method.move_timeout_ms}
							onChange={(value) =>
								setMethod({
									...method,
									move_timeout_ms: Number(value) || 1,
								})
							}
						/>
					</SettingRowInline>
					{method.move_timeout_ms < TIGHT_MOVE_BUDGET_MS && (
						<Text size="xs" c="dimmed" mt={-6}>
							Tight budget — search-style bots may exceed it; any failures show
							as bot health, not lost results.
						</Text>
					)}

					<GameFactoryForm
						value={factory}
						onChange={setFactory}
						errors={factoryErrors}
					/>

					<Button
						variant="subtle"
						size="compact-xs"
						onClick={() => setAdvanced((current) => !current)}
						aria-expanded={advanced}
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
									onChange={(value) =>
										setMethod({
											...method,
											max_parallel: Number(value) || 1,
										})
									}
								/>
							</SettingRowInline>
							<SettingRowInline label="Preprocessing budget" hint="ms">
								<NumberInput
									w={110}
									min={1}
									step={100}
									value={method.preprocessing_timeout_ms}
									onChange={(value) =>
										setMethod({
											...method,
											preprocessing_timeout_ms: Number(value) || 1,
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
									onChange={(value) =>
										setSeedInput(value === "" ? "" : String(value))
									}
								/>
							</SettingRowInline>
						</Stack>
					</Collapse>
				</>
			) : defaultsLoading ? (
				<Text size="sm" c="dimmed">
					Loading defaults…
				</Text>
			) : (
				<Paper withBorder p="sm" radius="sm" bg={T.panel2}>
					<Text size="sm" c="red" mb="xs" role="alert">
						Could not load tournament defaults
						{defaultsError ? `: ${defaultsError}` : "."}
					</Text>
					<Button size="compact-xs" variant="light" onClick={loadDefaults}>
						Retry
					</Button>
				</Paper>
			)}
		</Stack>
	);

	return (
		<Box
			p={{ base: "md", sm: "lg" }}
			h={boundedColumns ? "100%" : "auto"}
			style={{
				boxSizing: "border-box",
				maxWidth: 1060,
				margin: "0 auto",
				minHeight: 0,
				display: "flex",
				flexDirection: "column",
				overflow: boundedColumns ? "hidden" : "visible",
			}}
		>
			{tournamentRunning && live && (
				<Paper withBorder p="sm" radius="md" mb="md" bg={T.panel2}>
					<Group justify="space-between">
						<Text size="sm">
							<b>{live.name ?? `#${live.tournamentId}`}</b> is running ·{" "}
							{live.done}/{live.total} games
						</Text>
						<Group gap="xs">
							<Button size="compact-sm" variant="light" onClick={returnToLive}>
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

			<SimpleGrid
				cols={boundedColumns ? 2 : 1}
				spacing="md"
				style={{ flex: boundedColumns ? 1 : undefined, minHeight: 0 }}
			>
				<Paper
					withBorder
					p="md"
					radius="md"
					style={{
						display: "flex",
						flexDirection: "column",
						minHeight: 0,
						overflow: "hidden",
					}}
				>
					<Group justify="space-between">
						<Title order={4}>New tournament</Title>
						<Text size="xs" c="dimmed">
							{selectedBots.length}/{bots.length} bots
						</Text>
					</Group>
					<Text size="xs" c="dimmed" mb="sm">
						Select the field. Measure one bot for a gauntlet, or leave all
						unmeasured for a round-robin.
					</Text>
					{boundedColumns ? (
						<ScrollArea
							offsetScrollbars
							type="auto"
							style={{ flex: 1, minHeight: 120 }}
						>
							{roster}
						</ScrollArea>
					) : (
						<ScrollArea.Autosize mah={320} offsetScrollbars>
							{roster}
						</ScrollArea.Autosize>
					)}

					<Box
						pt="sm"
						mt="sm"
						style={{ borderTop: `1px solid ${T.line}`, flexShrink: 0 }}
					>
						<Button
							variant="subtle"
							size="compact-sm"
							fullWidth
							justify="space-between"
							onClick={() => setRecentExpanded((current) => !current)}
							aria-expanded={recentExpanded}
						>
							Recent tournaments ({store.length}) ·{" "}
							{recentExpanded ? "Hide" : "Show"}
						</Button>
						<Collapse in={recentExpanded}>
							<ScrollArea.Autosize mah={150} mt="xs" offsetScrollbars>
								{storeLoading ? (
									<Text size="sm" c="dimmed">
										Loading tournaments…
									</Text>
								) : storeError ? (
									<Group justify="space-between" gap="xs" wrap="nowrap">
										<Text size="xs" c="red" lineClamp={2} role="alert">
											Could not load: {storeError}
										</Text>
										<Button
											size="compact-xs"
											variant="light"
											onClick={loadStore}
										>
											Retry
										</Button>
									</Group>
								) : store.length === 0 ? (
									<Text size="sm" c="dimmed">
										No tournaments yet.
									</Text>
								) : (
									<Stack gap={6}>
										{store.map((tournament) => {
											const activeRow = live?.tournamentId === tournament.id;
											const running = activeRow
												? live.status === "running"
												: tournament.running;
											const done = activeRow ? live.done : tournament.done;
											const total = activeRow ? live.total : tournament.total;
											const complete = activeRow
												? live.status === "finished" && done >= total
												: tournament.finished && done >= total;
											const status = running
												? `running ${done}/${total}`
												: complete
													? "finished"
													: `partial ${done}/${total}`;
											return (
												<PressableSurface
													key={tournament.id}
													disabled={openingTournamentId === tournament.id}
													onClick={() => void openTournament(tournament)}
													aria-label={`Open ${tournament.name ?? `tournament ${tournament.id}`}, ${status}, ${done} of ${total} games`}
												>
													<Group gap="xs" wrap="nowrap" px="xs" py={7}>
														<Text size="sm" truncate>
															{tournament.name ?? `#${tournament.id}`}
														</Text>
														<Text
															size="xs"
															c="dimmed"
															style={{ flexShrink: 0 }}
														>
															{status} · {tournament.format}
														</Text>
														<IconChevronRight
															size={14}
															className="pyrat-pressable-surface__chevron"
														/>
													</Group>
												</PressableSurface>
											);
										})}
									</Stack>
								)}
							</ScrollArea.Autosize>
						</Collapse>
					</Box>
				</Paper>

				<Paper
					withBorder
					p="md"
					radius="md"
					style={{
						display: "flex",
						flexDirection: "column",
						minHeight: 0,
						overflow: "hidden",
					}}
				>
					<Title order={4} mb="sm">
						Conditions
					</Title>
					{boundedColumns ? (
						<ScrollArea
							offsetScrollbars
							type="auto"
							style={{ flex: 1, minHeight: 100 }}
						>
							{conditions}
						</ScrollArea>
					) : (
						<Box>{conditions}</Box>
					)}

					<Stack
						gap="xs"
						pt="sm"
						mt="sm"
						style={{ borderTop: `1px solid ${T.line}`, flexShrink: 0 }}
					>
						<Paper withBorder p="xs" radius="sm" bg={T.panel2}>
							<Text size="xs" tt="uppercase" c="dimmed" fw={700} mb={3}>
								Tournament plan
							</Text>
							<Text size="xs" c={plan ? undefined : "dimmed"} lineClamp={3}>
								{plan || "Select at least two bots to build the plan."}
							</Text>
						</Paper>
						{error && (
							<Box
								mah={72}
								tabIndex={0}
								style={{ overflowY: "auto" }}
								aria-label="Tournament launch error"
							>
								<Text size="sm" c="red" role="alert">
									{error}
								</Text>
							</Box>
						)}
						{starting && (
							<Group
								component="output"
								gap="xs"
								wrap="nowrap"
								aria-live="polite"
								aria-atomic="true"
							>
								<PulseDot />
								<Text size="xs" c="dimmed" lineClamp={2}>
									{preparing?.current
										? `Checking ${shortId(preparing.current)}… (${preparing.done}/${preparing.total})`
										: preparing
											? `Checking bot compatibility… (${preparing.done}/${preparing.total})`
											: "Starting tournament…"}
									{preparing ? " These checks do not count as games." : ""}
								</Text>
							</Group>
						)}
						<Group gap="sm" wrap="nowrap">
							<Button
								color="yellow"
								fullWidth
								disabled={
									selectedBots.length < 2 ||
									starting ||
									tournamentRunning ||
									!factory ||
									!method ||
									hasFactoryError
								}
								loading={starting}
								onClick={launch}
							>
								{starting
									? preparing
										? "Checking bots…"
										: "Starting…"
									: "Launch tournament"}
							</Button>
							{starting && (
								<Button
									variant="light"
									color="red"
									miw={70}
									onClick={() => void commands.stopTournament()}
								>
									Stop
								</Button>
							)}
						</Group>
					</Stack>
				</Paper>
			</SimpleGrid>
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
	children: ReactElement<{
		id?: string;
		"aria-describedby"?: string;
	}>;
}) {
	const generatedControlId = useId();
	const controlId = children.props.id ?? generatedControlId;
	const hintId = hint ? `${controlId}-hint` : undefined;
	const control = cloneElement(children, {
		id: controlId,
		...(hintId
			? {
					"aria-describedby": [children.props["aria-describedby"], hintId]
						.filter(Boolean)
						.join(" "),
				}
			: {}),
	});

	return (
		<Group justify="space-between" wrap="wrap" gap="lg">
			<Stack gap={0}>
				<Text component="label" htmlFor={controlId} size="sm" fw={500}>
					{label}
				</Text>
				{hint && (
					<Text id={hintId} size="xs" c="dimmed">
						{hint}
					</Text>
				)}
			</Stack>
			<div style={{ flexShrink: 0, maxWidth: "100%" }}>{control}</div>
		</Group>
	);
}
