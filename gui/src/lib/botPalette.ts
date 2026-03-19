import type { PlayerSide } from "../bindings/generated";

export type ColorEntry = {
	mantine: string; // for Mantine components (Badge, border vars)
	saturated: string; // rgba for canvas arrows (best line)
	pale: string; // rgba for canvas arrows (alt lines)
};

export const SLOT_PALETTE: Record<PlayerSide, ColorEntry> = {
	Player1: {
		mantine: "violet",
		saturated: "rgba(132, 94, 247, 0.85)",
		pale: "rgba(132, 94, 247, 0.35)",
	},
	Player2: {
		mantine: "teal",
		saturated: "rgba(32, 201, 151, 0.85)",
		pale: "rgba(32, 201, 151, 0.35)",
	},
};

export const PLAYER_LABEL: Record<PlayerSide, string> = {
	Player1: "Rat",
	Player2: "Python",
};
