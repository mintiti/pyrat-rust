//! Shared visual tokens for the tournament module, matching the maquette
//! (dark, cheese accent). Kept local so the tournament surface has one place
//! for its palette until a project-wide design language is named.

export const T = {
	cheese: "#f0b429",
	cheeseDim: "#8a6a1d",
	win: "#6fcf73",
	loss: "#e36c6c",
	draw: "#9aa0ac",
	muted: "#8a8f9b",
	line: "var(--mantine-color-dark-4)",
	panel: "var(--mantine-color-dark-6)",
	panel2: "var(--mantine-color-dark-5)",
	rat: "rgba(132, 94, 247, 0.9)", // player1 (violet)
	python: "rgba(32, 201, 151, 0.9)", // player2 (teal)
} as const;

/** Canonical, orientation-free key for a matchup pair. */
export function pairKey(a: string, b: string): string {
	return a <= b ? `${a}|${b}` : `${b}|${a}`;
}

/** Strip a namespace from an agent id for compact display ("pyrat/greedy" → "greedy"). */
export function shortId(id: string): string {
	return id.split("/").pop() ?? id;
}
