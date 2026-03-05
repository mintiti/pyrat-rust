export type TileAssignment = {
	tileIndex: number;
	rotation: 0 | 1 | 2 | 3;
	flipX: boolean;
	flipY: boolean;
};

/** Mulberry32 PRNG — deterministic from a seed. */
function mulberry32(seed: number): () => number {
	let s = seed | 0;
	return () => {
		s = (s + 0x6d2b79f5) | 0;
		let t = Math.imul(s ^ (s >>> 15), 1 | s);
		t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
}

export function generateTileMap(
	width: number,
	height: number,
	seed = 42,
): TileAssignment[][] {
	const rng = mulberry32(seed);
	const map: TileAssignment[][] = [];

	for (let y = 0; y < height; y++) {
		const row: TileAssignment[] = [];
		for (let x = 0; x < width; x++) {
			row.push({
				tileIndex: Math.floor(rng() * 10),
				rotation: (Math.floor(rng() * 4) & 3) as 0 | 1 | 2 | 3,
				flipX: rng() > 0.5,
				flipY: rng() > 0.5,
			});
		}
		map.push(row);
	}

	return map;
}
