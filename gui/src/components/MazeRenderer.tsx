import { Center, Loader } from "@mantine/core";
import { useElementSize } from "@mantine/hooks";
import { useEffect, useMemo, useState } from "react";
import type { AssetMap } from "../renderer/assets";
import { loadAssets } from "../renderer/assets";
import { buildDrawInstructions } from "../renderer/instructions";
import { computeLayout } from "../renderer/layout";
import { generateTileMap } from "../renderer/tileMap";
import type { MazeState } from "../types/game";
import MazeCanvas from "./MazeCanvas";

type Props = {
	gameState: MazeState;
	showCellIndices?: boolean;
};

export default function MazeRenderer({ gameState, showCellIndices }: Props) {
	const [assets, setAssets] = useState<AssetMap | null>(null);
	const { ref, width, height } = useElementSize();

	useEffect(() => {
		loadAssets().then(setAssets);
	}, []);

	const layout = useMemo(() => {
		if (width === 0 || height === 0) return null;
		return computeLayout(width, height, gameState.width, gameState.height);
	}, [width, height, gameState.width, gameState.height]);

	const tileMap = useMemo(
		() => generateTileMap(gameState.width, gameState.height, 42),
		[gameState.width, gameState.height],
	);

	const instructions = useMemo(() => {
		if (!assets || !layout) return null;
		return buildDrawInstructions(gameState, layout, assets, tileMap, {
			showCellIndices,
		});
	}, [gameState, layout, assets, tileMap, showCellIndices]);

	return (
		<div ref={ref} style={{ width: "100%", height: "100%", minHeight: 200 }}>
			{!assets || !instructions || !layout ? (
				<Center h="100%">
					<Loader />
				</Center>
			) : (
				<MazeCanvas
					instructions={instructions}
					width={layout.canvasWidth}
					height={layout.canvasHeight}
				/>
			)}
		</div>
	);
}
