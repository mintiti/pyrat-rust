import {
	Divider,
	Drawer,
	NumberInput,
	SegmentedControl,
	Slider,
	Stack,
	Switch,
	Text,
	TextInput,
} from "@mantine/core";
import { useAtom, useAtomValue } from "jotai";
import { useEffect, useState } from "react";
import type { MatchConfigParams } from "../bindings/generated";
import {
	PRESET_VALUES,
	type PresetName,
	asyncMatchConfigAtom,
	matchConfigAtom,
	validate,
} from "../stores/matchConfigAtom";
import SettingRow from "./common/SettingRow";

type Props = {
	opened: boolean;
	onClose: () => void;
};

const SIZE_PRESETS: { value: PresetName; label: string }[] = [
	{ value: "tiny", label: "Tiny" },
	{ value: "small", label: "Small" },
	{ value: "medium", label: "Medium" },
	{ value: "large", label: "Large" },
	{ value: "huge", label: "Huge" },
];

const MAZE_PRESETS: { value: string; label: string }[] = [
	{ value: "classic", label: "Classic" },
	{ value: "open", label: "Open" },
];

/** Derive which size preset matches, or null if custom. */
function detectSizePreset(d: MatchConfigParams): PresetName | null {
	for (const [name, vals] of Object.entries(PRESET_VALUES)) {
		if (name === "open") continue;
		if (
			d.width === vals.width &&
			d.height === vals.height &&
			d.max_turns === vals.max_turns &&
			d.cheese_count === vals.cheese_count
		)
			return name as PresetName;
	}
	return null;
}

/** Derive maze type from density values. */
function detectMazeType(d: MatchConfigParams): "classic" | "open" | null {
	if (d.wall_density === 0 && d.mud_density === 0) return "open";
	if (d.wall_density === 0.7 && d.mud_density === 0.1 && d.mud_range === 3)
		return "classic";
	return null;
}

export default function MatchConfigDrawer({ opened, onClose }: Props) {
	const committed = useAtomValue(matchConfigAtom);
	const [, setConfig] = useAtom(asyncMatchConfigAtom);
	const [draft, setDraft] = useState<MatchConfigParams>(committed);

	// Reset draft when drawer opens
	useEffect(() => {
		if (opened) setDraft(committed);
	}, [opened, committed]);

	const errors = validate(draft);
	const isCustom = draft.preset === "custom";

	const update = (patch: Partial<MatchConfigParams>) =>
		setDraft((prev) => ({ ...prev, ...patch }));

	const handleSizeChange = (value: string) => {
		const name = value as PresetName;
		if (name in PRESET_VALUES) {
			const vals = PRESET_VALUES[name as keyof typeof PRESET_VALUES];
			// Keep current maze settings, apply size values
			update({
				preset: name,
				width: vals.width,
				height: vals.height,
				max_turns: vals.max_turns,
				cheese_count: vals.cheese_count,
			});
		}
	};

	const handleMazeChange = (value: string) => {
		if (value === "open") {
			const sizePreset = detectSizePreset(draft);
			if (sizePreset && sizePreset !== "open") {
				// Keep size, switch to open maze
				update({
					preset: "open",
					wall_density: 0,
					mud_density: 0,
					mud_range: 2,
				});
			} else {
				update({
					preset: "open",
					...PRESET_VALUES.open,
				});
			}
		} else {
			// Classic maze
			const sizePreset = detectSizePreset(draft);
			if (sizePreset) {
				update({
					preset: sizePreset,
					wall_density: 0.7,
					mud_density: 0.1,
					mud_range: 3,
				});
			} else {
				update({
					wall_density: 0.7,
					mud_density: 0.1,
					mud_range: 3,
				});
			}
		}
	};

	const handleCustomToggle = (checked: boolean) => {
		if (checked) {
			update({ preset: "custom" });
		} else {
			// Snap back to nearest preset
			const size = detectSizePreset(draft);
			const maze = detectMazeType(draft);
			if (size && maze === "open") {
				update({
					preset: "open",
					...PRESET_VALUES.open,
					width: draft.width,
					height: draft.height,
					max_turns: draft.max_turns,
					cheese_count: draft.cheese_count,
				});
			} else if (size) {
				update({ preset: size });
			} else {
				update({ preset: "medium", ...PRESET_VALUES.medium });
			}
		}
	};

	const handleClose = () => {
		if (Object.keys(validate(draft)).length === 0) {
			setConfig(draft);
		}
		onClose();
	};

	const sizePreset = isCustom ? null : detectSizePreset(draft);
	const mazeType = isCustom ? null : detectMazeType(draft);

	return (
		<Drawer
			opened={opened}
			onClose={handleClose}
			title="Match Configuration"
			position="right"
			size={400}
		>
			<Stack gap="md">
				{/* Preset selectors */}
				<div>
					<Text size="sm" fw={500} mb={4}>
						Size
					</Text>
					<SegmentedControl
						fullWidth
						size="xs"
						data={SIZE_PRESETS}
						value={sizePreset ?? "medium"}
						onChange={handleSizeChange}
						disabled={isCustom}
					/>
				</div>

				<div>
					<Text size="sm" fw={500} mb={4}>
						Maze
					</Text>
					<SegmentedControl
						fullWidth
						size="xs"
						data={MAZE_PRESETS}
						value={mazeType ?? "classic"}
						onChange={handleMazeChange}
						disabled={isCustom}
					/>
				</div>

				<Switch
					label="Customize"
					checked={isCustom}
					onChange={(e) => handleCustomToggle(e.currentTarget.checked)}
				/>

				{/* Custom fields */}
				{isCustom && (
					<>
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
							<SegmentedControl
								size="xs"
								data={[
									{ value: "corners", label: "Corners" },
									{ value: "random", label: "Random" },
								]}
								value={draft.player_start}
								onChange={(v) => update({ player_start: v })}
							/>
						</SettingRow>
					</>
				)}

				{/* Seed — always visible */}
				<Divider label="Seed" labelPosition="left" />
				<SettingRow label="Seed" description="Empty = random each game">
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
						style={{ width: 120 }}
					/>
				</SettingRow>

				{/* Summary for preset mode */}
				{!isCustom && (
					<Text size="xs" c="dimmed">
						{draft.width}×{draft.height}, {draft.cheese_count} cheese,{" "}
						{draft.max_turns} turns
					</Text>
				)}
			</Stack>
		</Drawer>
	);
}
