import {
	Button,
	Collapse,
	Group,
	NumberInput,
	SegmentedControl,
	Slider,
	Stack,
	Switch,
	Text,
} from "@mantine/core";
import { useState } from "react";
import type { GameFactoryConfig, PlayerStart } from "../../bindings/generated";
import {
	PRESET_VALUES,
	type PresetName,
	detectSizePreset,
} from "../../stores/gameConfig";
import SettingRow from "../common/SettingRow";

type Props = {
	value: GameFactoryConfig;
	onChange: (next: GameFactoryConfig) => void;
	errors: Record<string, string>;
};

const SIZE_PRESETS = ["tiny", "small", "medium", "large", "huge"] as const;

/** The game-instance distribution editor (board / maze / starts / cheese).
 * Presets up front; the per-field knobs behind "Advanced". Shares preset data
 * and validation with the Play config via `stores/gameConfig`. */
export default function GameFactoryForm({ value, onChange, errors }: Props) {
	const [advanced, setAdvanced] = useState(false);
	const set = (patch: Partial<GameFactoryConfig>) =>
		onChange({ ...value, ...patch });

	const applyPreset = (name: (typeof SIZE_PRESETS)[number]) => {
		const p = PRESET_VALUES[name];
		set({
			width: p.width,
			height: p.height,
			max_turns: p.max_turns,
			wall_density: p.wall_density,
			mud_density: p.mud_density,
			mud_range: p.mud_range,
			connected: p.connected,
			symmetric: p.symmetric,
			cheese_count: p.cheese_count,
			cheese_symmetric: p.cheese_symmetric,
			player_start: p.player_start as PlayerStart,
		});
	};

	const activePreset: PresetName | null = detectSizePreset(value);

	return (
		<Stack gap="sm">
			<SettingRow label="Board" description="size preset, or set fields below">
				<Group gap={4}>
					{SIZE_PRESETS.map((name) => (
						<Button
							key={name}
							size="compact-xs"
							variant={activePreset === name ? "filled" : "default"}
							color="yellow"
							onClick={() => applyPreset(name)}
						>
							{name}
						</Button>
					))}
				</Group>
			</SettingRow>

			<Button
				variant="subtle"
				size="compact-xs"
				onClick={() => setAdvanced((a) => !a)}
				style={{ alignSelf: "flex-start" }}
			>
				{advanced ? "Hide conditions" : "Advanced conditions"}
			</Button>

			<Collapse in={advanced}>
				<Stack gap="sm">
					<SettingRow label="Width">
						<NumberInput
							w={90}
							min={2}
							max={255}
							value={value.width}
							onChange={(v) => set({ width: Number(v) || 0 })}
							error={!!errors.width}
						/>
					</SettingRow>
					<SettingRow label="Height">
						<NumberInput
							w={90}
							min={2}
							max={255}
							value={value.height}
							onChange={(v) => set({ height: Number(v) || 0 })}
							error={!!errors.height}
						/>
					</SettingRow>
					<SettingRow label="Max turns">
						<NumberInput
							w={90}
							min={1}
							value={value.max_turns}
							onChange={(v) => set({ max_turns: Number(v) || 0 })}
							error={!!errors.max_turns}
						/>
					</SettingRow>
					<SettingRow label="Wall density">
						<Slider
							w={160}
							min={0}
							max={1}
							step={0.05}
							value={value.wall_density}
							onChange={(v) => set({ wall_density: v })}
						/>
					</SettingRow>
					<SettingRow label="Mud density">
						<Slider
							w={160}
							min={0}
							max={1}
							step={0.05}
							value={value.mud_density}
							onChange={(v) => set({ mud_density: v })}
						/>
					</SettingRow>
					<SettingRow label="Mud range" description="max mud cost">
						<NumberInput
							w={90}
							min={2}
							max={255}
							value={value.mud_range}
							onChange={(v) => set({ mud_range: Number(v) || 0 })}
							error={!!errors.mud_range}
						/>
					</SettingRow>
					<SettingRow label="Connected">
						<Switch
							checked={value.connected}
							onChange={(e) => set({ connected: e.currentTarget.checked })}
						/>
					</SettingRow>
					<SettingRow label="Symmetric maze">
						<Switch
							checked={value.symmetric}
							onChange={(e) => set({ symmetric: e.currentTarget.checked })}
						/>
					</SettingRow>
					<SettingRow
						label="Start positions"
						description="corners (fair) or random"
					>
						<SegmentedControl
							size="xs"
							data={[
								{ label: "Corners", value: "corners" },
								{ label: "Random", value: "random" },
							]}
							value={value.player_start}
							onChange={(v) => set({ player_start: v as PlayerStart })}
						/>
					</SettingRow>
					<SettingRow label="Cheese">
						<NumberInput
							w={90}
							min={1}
							value={value.cheese_count}
							onChange={(v) => set({ cheese_count: Number(v) || 0 })}
							error={!!errors.cheese_count}
						/>
					</SettingRow>
					<SettingRow label="Symmetric cheese">
						<Switch
							checked={value.cheese_symmetric}
							onChange={(e) =>
								set({ cheese_symmetric: e.currentTarget.checked })
							}
						/>
					</SettingRow>
					{errors.cheese_count && (
						<Text size="xs" c="red">
							{errors.cheese_count}
						</Text>
					)}
				</Stack>
			</Collapse>
		</Stack>
	);
}
