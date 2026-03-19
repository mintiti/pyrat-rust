import {
	ActionIcon,
	Button,
	Center,
	Divider,
	Group,
	NumberInput,
	ScrollArea,
	Select,
	Slider,
	Stack,
	Switch,
	Text,
	TextInput,
	Tooltip,
} from "@mantine/core";
import { IconCopy, IconDice, IconPlayerPlay } from "@tabler/icons-react";
import { useAtom, useAtomValue } from "jotai";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MatchConfigParams } from "../bindings/generated";
import { RANDOM_BOT_ID, botsAtom } from "../stores/botConfigAtom";
import {
	CLASSIC_MAZE,
	OPEN_MAZE,
	PRESET_VALUES,
	type PresetName,
	asyncMatchConfigAtom,
	detectMazeType,
	detectSizePreset,
	matchConfigAtom,
	validate,
} from "../stores/matchConfigAtom";
import {
	generatePreview,
	rerollPreview,
	useDisplayState,
	useMatchStore,
} from "../stores/matchStore";
import MazeRenderer from "./MazeRenderer";
import SettingRow from "./common/SettingRow";

type Props = {
	onStartMatch: () => void;
};

const SIZE_PRESETS: {
	name: Exclude<PresetName, "custom" | "open">;
	label: string;
}[] = [
	{ name: "tiny", label: "Tiny" },
	{ name: "small", label: "Small" },
	{ name: "medium", label: "Medium" },
	{ name: "large", label: "Large" },
	{ name: "huge", label: "Huge" },
];

const MAZE_PRESETS: { name: "classic" | "open"; label: string }[] = [
	{ name: "classic", label: "Classic" },
	{ name: "open", label: "Open" },
];

export default function SetupView({ onStartMatch }: Props) {
	const committed = useAtomValue(matchConfigAtom);
	const [, setConfig] = useAtom(asyncMatchConfigAtom);
	const bots = useAtomValue(botsAtom);

	const player1BotId = useMatchStore((s) => s.player1BotId);
	const player2BotId = useMatchStore((s) => s.player2BotId);
	const previewSeed = useMatchStore((s) => s.previewSeed);
	const previewError = useMatchStore((s) => s.previewError);
	const { setPlayer1BotId, setPlayer2BotId } = useMatchStore.getState();

	const displayState = useDisplayState();

	// Local draft — controls update this immediately for responsive UI
	const [draft, setDraft] = useState<MatchConfigParams>(committed);
	const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

	// Sync draft when committed changes externally (e.g. loaded from disk)
	const prevCommittedRef = useRef(committed);
	useEffect(() => {
		if (committed !== prevCommittedRef.current) {
			setDraft(committed);
			prevCommittedRef.current = committed;
		}
	}, [committed]);

	// Debounced write: draft → atom → preview regeneration
	useEffect(() => {
		if (debounceRef.current) clearTimeout(debounceRef.current);
		debounceRef.current = setTimeout(() => {
			setConfig(draft);
		}, 300);
		return () => {
			if (debounceRef.current) clearTimeout(debounceRef.current);
		};
	}, [draft, setConfig]);

	// Generate preview on committed config changes
	useEffect(() => {
		generatePreview(committed);
	}, [committed]);

	const errors = validate(draft);

	const update = useCallback(
		(patch: Partial<MatchConfigParams>) =>
			setDraft((prev) => ({ ...prev, ...patch })),
		[],
	);

	const botOptions = useMemo(
		() => [
			{ value: RANDOM_BOT_ID, label: "Random Bot" },
			...bots.map((b) => ({ value: b.id, label: b.name })),
		],
		[bots],
	);

	const activeSizePreset = detectSizePreset(draft);
	const activeMazeType = detectMazeType(draft);

	const handleSizeClick = (name: Exclude<PresetName, "custom" | "open">) => {
		const vals = PRESET_VALUES[name];
		update({
			preset: "custom",
			width: vals.width,
			height: vals.height,
			max_turns: vals.max_turns,
			cheese_count: vals.cheese_count,
		});
	};

	const handleMazeClick = (type: "classic" | "open") => {
		const maze = type === "open" ? OPEN_MAZE : CLASSIC_MAZE;
		update({
			preset: "custom",
			wall_density: maze.wall_density,
			mud_density: maze.mud_density,
			mud_range: maze.mud_range,
		});
	};

	const handleReroll = () => {
		// Flush draft immediately, then reroll
		setConfig(draft);
		rerollPreview(draft);
	};

	const handleCopySeed = () => {
		if (previewSeed != null) {
			navigator.clipboard.writeText(String(previewSeed));
		}
	};

	const handleStart = () => {
		// Flush draft immediately before navigating
		if (debounceRef.current) {
			clearTimeout(debounceRef.current);
			debounceRef.current = null;
		}
		setConfig(draft);
		onStartMatch();
	};

	const canStart =
		player1BotId != null &&
		player2BotId != null &&
		Object.keys(errors).length === 0;

	return (
		<Stack h="100%" gap={0}>
			{/* Bot bar */}
			<Group
				p="xs"
				justify="space-between"
				style={{
					borderBottom: "1px solid var(--mantine-color-dark-4)",
					flexShrink: 0,
				}}
			>
				<Group gap="sm">
					<Text size="sm" fw={600} c="blue">
						Rat
					</Text>
					<Select
						size="xs"
						placeholder="Select bot"
						data={botOptions}
						value={player1BotId}
						onChange={setPlayer1BotId}
						style={{ width: 180 }}
						allowDeselect={false}
					/>
					<Text size="sm" c="dimmed">
						vs
					</Text>
					<Text size="sm" fw={600} c="green">
						Python
					</Text>
					<Select
						size="xs"
						placeholder="Select bot"
						data={botOptions}
						value={player2BotId}
						onChange={setPlayer2BotId}
						style={{ width: 180 }}
						allowDeselect={false}
					/>
				</Group>
				<Button
					size="xs"
					leftSection={<IconPlayerPlay size={14} />}
					onClick={handleStart}
					disabled={!canStart}
				>
					Start
				</Button>
			</Group>

			{/* Two-panel layout */}
			<div style={{ flex: 1, display: "flex", overflow: "hidden" }}>
				{/* Left: maze preview */}
				<div style={{ flex: 1, minWidth: 0 }}>
					{displayState ? (
						<MazeRenderer gameState={displayState} hideScoreStrip />
					) : previewError ? (
						<Center h="100%">
							<Text c="red" size="sm">
								{previewError}
							</Text>
						</Center>
					) : (
						<Center h="100%">
							<Text c="dimmed" size="sm">
								Generating preview...
							</Text>
						</Center>
					)}
				</div>

				{/* Right: config panel */}
				<ScrollArea
					style={{
						width: 360,
						flexShrink: 0,
						borderLeft: "1px solid var(--mantine-color-dark-4)",
					}}
				>
					<Stack gap="md" p="md">
						{/* Size quick-fill */}
						<div>
							<Text size="sm" fw={500} mb={4}>
								Size
							</Text>
							<Group gap={4}>
								{SIZE_PRESETS.map((p) => (
									<Button
										key={p.name}
										size="compact-xs"
										variant={activeSizePreset === p.name ? "filled" : "light"}
										onClick={() => handleSizeClick(p.name)}
									>
										{p.label}
									</Button>
								))}
							</Group>
						</div>

						{/* Maze type quick-fill */}
						<div>
							<Text size="sm" fw={500} mb={4}>
								Maze Type
							</Text>
							<Group gap={4}>
								{MAZE_PRESETS.map((p) => (
									<Button
										key={p.name}
										size="compact-xs"
										variant={activeMazeType === p.name ? "filled" : "light"}
										onClick={() => handleMazeClick(p.name)}
									>
										{p.label}
									</Button>
								))}
							</Group>
						</div>

						<Divider label="Board" labelPosition="left" />

						<SettingRow label="Width">
							<NumberInput
								size="xs"
								min={2}
								max={255}
								value={draft.width}
								onChange={(v) => typeof v === "number" && update({ width: v })}
								error={errors.width}
								style={{ width: 90 }}
							/>
						</SettingRow>

						<SettingRow label="Height">
							<NumberInput
								size="xs"
								min={2}
								max={255}
								value={draft.height}
								onChange={(v) => typeof v === "number" && update({ height: v })}
								error={errors.height}
								style={{ width: 90 }}
							/>
						</SettingRow>

						<SettingRow label="Max Turns">
							<NumberInput
								size="xs"
								min={1}
								max={9999}
								value={draft.max_turns}
								onChange={(v) =>
									typeof v === "number" && update({ max_turns: v })
								}
								error={errors.max_turns}
								style={{ width: 90 }}
							/>
						</SettingRow>

						<Divider label="Maze" labelPosition="left" />

						<SettingRow label="Wall Density">
							<Slider
								min={0}
								max={1}
								step={0.05}
								value={draft.wall_density}
								onChange={(v) => update({ wall_density: v })}
								label={(v) => `${Math.round(v * 100)}%`}
								style={{ width: 140 }}
							/>
						</SettingRow>

						<SettingRow label="Mud Density">
							<Slider
								min={0}
								max={1}
								step={0.05}
								value={draft.mud_density}
								onChange={(v) => update({ mud_density: v })}
								label={(v) => `${Math.round(v * 100)}%`}
								style={{ width: 140 }}
							/>
						</SettingRow>

						<SettingRow label="Mud Range">
							<NumberInput
								size="xs"
								min={2}
								max={20}
								value={draft.mud_range}
								onChange={(v) =>
									typeof v === "number" && update({ mud_range: v })
								}
								error={errors.mud_range}
								style={{ width: 90 }}
								disabled={draft.mud_density === 0}
							/>
						</SettingRow>

						<SettingRow label="Connected">
							<Switch
								size="sm"
								checked={draft.connected}
								onChange={(e) => update({ connected: e.currentTarget.checked })}
							/>
						</SettingRow>

						<SettingRow label="Symmetric">
							<Switch
								size="sm"
								checked={draft.symmetric}
								onChange={(e) => update({ symmetric: e.currentTarget.checked })}
							/>
						</SettingRow>

						<Divider label="Cheese" labelPosition="left" />

						<SettingRow label="Count">
							<NumberInput
								size="xs"
								min={1}
								value={draft.cheese_count}
								onChange={(v) =>
									typeof v === "number" && update({ cheese_count: v })
								}
								error={errors.cheese_count}
								style={{ width: 90 }}
							/>
						</SettingRow>

						<SettingRow label="Symmetric">
							<Switch
								size="sm"
								checked={draft.cheese_symmetric}
								onChange={(e) =>
									update({ cheese_symmetric: e.currentTarget.checked })
								}
							/>
						</SettingRow>

						<Divider label="Players" labelPosition="left" />

						<SettingRow label="Start Positions">
							<Select
								size="xs"
								data={[
									{ value: "corners", label: "Corners" },
									{ value: "random", label: "Random" },
								]}
								value={draft.player_start}
								onChange={(v) => v && update({ player_start: v })}
								allowDeselect={false}
								style={{ width: 120 }}
							/>
						</SettingRow>

						<Divider label="Seed" labelPosition="left" />

						<SettingRow label="Seed" description="Empty = random each game">
							<Group gap={4} wrap="nowrap">
								<TextInput
									size="xs"
									placeholder="Random"
									value={draft.seed != null ? String(draft.seed) : ""}
									onChange={(e) => {
										const raw = e.currentTarget.value.trim();
										if (raw === "") {
											update({ seed: null });
										} else {
											const n = Number(raw);
											if (Number.isFinite(n) && n >= 0) {
												update({ seed: Math.floor(n) });
											}
										}
									}}
									style={{ width: 100 }}
								/>
								<Tooltip label="Copy current seed">
									<ActionIcon
										variant="subtle"
										size="sm"
										onClick={handleCopySeed}
										disabled={previewSeed == null}
									>
										<IconCopy size={14} />
									</ActionIcon>
								</Tooltip>
								<Tooltip label="Re-roll maze">
									<ActionIcon variant="subtle" size="sm" onClick={handleReroll}>
										<IconDice size={14} />
									</ActionIcon>
								</Tooltip>
							</Group>
						</SettingRow>

						{previewSeed != null && (
							<Text size="xs" c="dimmed">
								Current seed: {previewSeed}
							</Text>
						)}
					</Stack>
				</ScrollArea>
			</div>
		</Stack>
	);
}
