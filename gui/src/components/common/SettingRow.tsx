import { Group, Stack, Text } from "@mantine/core";
import type { ReactNode } from "react";

type Props = {
	label: string;
	description?: string;
	children: ReactNode;
};

export default function SettingRow({ label, description, children }: Props) {
	return (
		<Group justify="space-between" wrap="nowrap" gap="lg">
			<Stack gap={0} style={{ flexShrink: 1, minWidth: 0 }}>
				<Text size="sm" fw={500}>
					{label}
				</Text>
				{description && (
					<Text size="xs" c="dimmed">
						{description}
					</Text>
				)}
			</Stack>
			<div style={{ flexShrink: 0 }}>{children}</div>
		</Group>
	);
}
