import { Button, Group, Modal, Stack, Text } from "@mantine/core";

type Props = {
	title: string;
	description: string;
	opened: boolean;
	onClose: () => void;
	onConfirm: () => void;
	confirmLabel?: string;
};

export default function ConfirmModal({
	title,
	description,
	opened,
	onClose,
	onConfirm,
	confirmLabel,
}: Props) {
	return (
		<Modal withCloseButton={false} opened={opened} onClose={onClose}>
			<Stack>
				<div>
					<Text fz="lg" fw="bold" mb={10}>
						{title}
					</Text>
					<Text>{description}</Text>
				</div>
				<Group justify="right">
					<Button variant="default" onClick={onClose}>
						Cancel
					</Button>
					<Button color="red" onClick={onConfirm}>
						{confirmLabel ?? "Confirm"}
					</Button>
				</Group>
			</Stack>
		</Modal>
	);
}
