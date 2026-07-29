import { AppShell, Stack, Tooltip } from "@mantine/core";
import { IconCpu, IconGridDots, IconTrophy } from "@tabler/icons-react";
import type { Icon } from "@tabler/icons-react";
import PressableSurface from "./tournament/PressableSurface";

export type Page = "game" | "bots" | "tournaments";

type NavbarLinkProps = {
	icon: Icon;
	label: string;
	active: boolean;
	onClick: () => void;
};

function NavbarLink({ icon: Icon, label, active, onClick }: NavbarLinkProps) {
	return (
		<Tooltip label={label} position="right">
			<PressableSurface
				variant="bare"
				motion="none"
				className="pyrat-sidebar-button"
				aria-label={label}
				aria-current={active ? "page" : undefined}
				data-active={active || undefined}
				onClick={onClick}
			>
				<Icon size="1.5rem" stroke={1.5} aria-hidden="true" />
			</PressableSurface>
		</Tooltip>
	);
}

const links: { icon: Icon; label: string; page: Page }[] = [
	{ icon: IconGridDots, label: "Game", page: "game" },
	{ icon: IconCpu, label: "Bots", page: "bots" },
	{ icon: IconTrophy, label: "Tournaments", page: "tournaments" },
];

type Props = {
	active: Page;
	onNavigate: (page: Page) => void;
};

export default function Sidebar({ active, onNavigate }: Props) {
	return (
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
	);
}
