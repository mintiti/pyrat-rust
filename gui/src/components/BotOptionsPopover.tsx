import {
	ActionIcon,
	Group,
	Loader,
	NumberInput,
	Popover,
	Select,
	Stack,
	Switch,
	Text,
	TextInput,
	Tooltip,
} from "@mantine/core";
import { IconSettings } from "@tabler/icons-react";
import { useAtomValue, useSetAtom } from "jotai";
import { useEffect } from "react";
import type { BotOptionDef } from "../bindings/generated";
import {
	discoveredBotsAtom,
	resolveDiscoveredBot,
} from "../stores/botConfigAtom";
import { probeCacheAtom, triggerProbeAtom } from "../stores/botProbeAtom";
import { useMatchStore } from "../stores/matchStore";
import SettingRow from "./common/SettingRow";

type Props = {
	agentId: string;
	slot: "player1" | "player2";
};

export default function BotOptionsPopover({ agentId, slot }: Props) {
	const discovered = useAtomValue(discoveredBotsAtom);
	const probeCache = useAtomValue(probeCacheAtom);
	const triggerProbe = useSetAtom(triggerProbeAtom);

	const options =
		slot === "player1"
			? useMatchStore((s) => s.player1Options)
			: useMatchStore((s) => s.player2Options);
	const setOptions =
		slot === "player1"
			? useMatchStore.getState().setPlayer1Options
			: useMatchStore.getState().setPlayer2Options;

	// Fire probe on mount / agentId change
	useEffect(() => {
		const bot = resolveDiscoveredBot(agentId, discovered);
		if (!bot) return;
		triggerProbe({
			agentId: bot.agent_id,
			runCommand: bot.run_command,
			workingDir: bot.working_dir,
		});
	}, [agentId, discovered, triggerProbe]);

	const entry = probeCache[agentId];

	// Pre-fill defaults when probe succeeds and options are empty
	useEffect(() => {
		if (entry?.status !== "ok") return;
		const current =
			slot === "player1"
				? useMatchStore.getState().player1Options
				: useMatchStore.getState().player2Options;
		if (Object.keys(current).length > 0) return;

		const configurable = entry.data.options.filter(
			(d) => d.option_type !== "Button",
		);
		if (configurable.length === 0) return;

		const defaults: Record<string, string> = {};
		for (const def of configurable) {
			defaults[def.name] = def.default_value;
		}
		setOptions(defaults);
	}, [entry, slot, setOptions]);

	// Filter out Button-type options
	const defs =
		entry?.status === "ok"
			? entry.data.options.filter((d) => d.option_type !== "Button")
			: [];

	// Show inline status for probe states
	if (!entry) return null;

	if (entry.status === "loading") {
		return (
			<Group gap={4} wrap="nowrap">
				<Loader size={14} />
				<Text size="xs" c="dimmed">
					Probing...
				</Text>
			</Group>
		);
	}

	if (entry.status === "error") {
		return (
			<Tooltip label={entry.error} multiline maw={300}>
				<Text size="xs" c="red" style={{ cursor: "default" }}>
					Probe failed
				</Text>
			</Tooltip>
		);
	}

	if (defs.length === 0) return null;

	const updateOption = (name: string, value: string) => {
		const current =
			slot === "player1"
				? useMatchStore.getState().player1Options
				: useMatchStore.getState().player2Options;
		setOptions({ ...current, [name]: value });
	};

	return (
		<Popover width={280} position="bottom" withArrow shadow="md">
			<Popover.Target>
				<Tooltip label="Bot options">
					<ActionIcon variant="subtle" size="sm">
						<IconSettings size={14} />
					</ActionIcon>
				</Tooltip>
			</Popover.Target>
			<Popover.Dropdown>
				<Stack gap="sm">
					<Text size="xs" fw={600} c="dimmed">
						Bot Options
					</Text>
					{defs.map((def) => (
						<OptionControl
							key={def.name}
							def={def}
							value={options[def.name] ?? def.default_value}
							onChange={(v) => updateOption(def.name, v)}
						/>
					))}
				</Stack>
			</Popover.Dropdown>
		</Popover>
	);
}

function OptionControl({
	def,
	value,
	onChange,
}: {
	def: BotOptionDef;
	value: string;
	onChange: (v: string) => void;
}) {
	switch (def.option_type) {
		case "Check":
			return (
				<SettingRow label={def.name}>
					<Switch
						size="sm"
						checked={value === "true"}
						onChange={(e) =>
							onChange(e.currentTarget.checked ? "true" : "false")
						}
					/>
				</SettingRow>
			);
		case "Spin":
			return (
				<SettingRow label={def.name}>
					<NumberInput
						size="xs"
						min={def.min}
						max={def.max}
						value={Number(value)}
						onChange={(v) => typeof v === "number" && onChange(String(v))}
						style={{ width: 90 }}
					/>
				</SettingRow>
			);
		case "Combo":
			return (
				<SettingRow label={def.name}>
					<Select
						size="xs"
						data={def.choices}
						value={value}
						onChange={(v) => v && onChange(v)}
						allowDeselect={false}
						style={{ width: 120 }}
					/>
				</SettingRow>
			);
		case "String":
			return (
				<SettingRow label={def.name}>
					<TextInput
						size="xs"
						value={value}
						onChange={(e) => onChange(e.currentTarget.value)}
						style={{ width: 120 }}
					/>
				</SettingRow>
			);
		default:
			return null;
	}
}
