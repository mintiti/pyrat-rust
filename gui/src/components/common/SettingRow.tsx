import { Group, Stack, Text } from "@mantine/core";
import { type ReactElement, cloneElement, useId } from "react";

type Props = {
	label: string;
	description?: string;
	controlTarget?: "root" | "slider-thumb";
	children: ReactElement<{
		"aria-labelledby"?: string;
		"aria-describedby"?: string;
		attributes?: Record<string, Record<string, unknown> | undefined>;
	}>;
};

export default function SettingRow({
	label,
	description,
	controlTarget = "root",
	children,
}: Props) {
	const labelId = useId();
	const descriptionId = description ? `${labelId}-description` : undefined;
	const accessibility = {
		"aria-labelledby": [children.props["aria-labelledby"], labelId]
			.filter(Boolean)
			.join(" "),
		...(descriptionId
			? {
					"aria-describedby": [
						children.props["aria-describedby"],
						descriptionId,
					]
						.filter(Boolean)
						.join(" "),
				}
			: {}),
	};
	const control = cloneElement(
		children,
		controlTarget === "slider-thumb"
			? {
					attributes: {
						...children.props.attributes,
						thumb: {
							...children.props.attributes?.thumb,
							...accessibility,
						},
					},
				}
			: accessibility,
	);

	return (
		<Group justify="space-between" wrap="nowrap" gap="lg">
			<Stack gap={0} style={{ flexShrink: 1, minWidth: 0 }}>
				<Text id={labelId} size="sm" fw={500}>
					{label}
				</Text>
				{description && (
					<Text id={descriptionId} size="xs" c="dimmed">
						{description}
					</Text>
				)}
			</Stack>
			<div style={{ flexShrink: 0 }}>{control}</div>
		</Group>
	);
}
