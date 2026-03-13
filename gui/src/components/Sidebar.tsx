import { AppShell, Stack, Tooltip, UnstyledButton } from "@mantine/core";
import { IconCpu, IconGridDots } from "@tabler/icons-react";
import type { Icon } from "@tabler/icons-react";

export type Page = "game" | "bots";

type NavbarLinkProps = {
	icon: Icon;
	label: string;
	active: boolean;
	onClick: () => void;
};

function NavbarLink({ icon: Icon, label, active, onClick }: NavbarLinkProps) {
	return (
		<Tooltip label={label} position="right">
			<UnstyledButton
				onClick={onClick}
				style={{
					width: "3rem",
					height: "3rem",
					display: "flex",
					alignItems: "center",
					justifyContent: "center",
					borderLeft: `3px solid ${active ? "var(--mantine-primary-color-filled)" : "transparent"}`,
					borderRight: "3px solid transparent",
					color: active
						? "var(--mantine-color-white)"
						: "var(--mantine-color-dark-0)",
				}}
			>
				<Icon size="1.5rem" stroke={1.5} />
			</UnstyledButton>
		</Tooltip>
	);
}

const links: { icon: Icon; label: string; page: Page }[] = [
	{ icon: IconGridDots, label: "Game", page: "game" },
	{ icon: IconCpu, label: "Bots", page: "bots" },
];

type Props = {
	active: Page;
	onNavigate: (page: Page) => void;
};

export default function Sidebar({ active, onNavigate }: Props) {
	return (
		<>
			<AppShell.Section grow>
				<Stack justify="center" gap={0}>
					{links.map((link) => (
						<NavbarLink
							key={link.page}
							icon={link.icon}
							label={link.label}
							active={active === link.page}
							onClick={() => onNavigate(link.page)}
						/>
					))}
				</Stack>
			</AppShell.Section>
		</>
	);
}
