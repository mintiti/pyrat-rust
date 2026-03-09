import { Center, Loader } from "@mantine/core";
import { useElementSize } from "@mantine/hooks";
import { useEffect, useMemo, useState } from "react";
import type { MazeState, PlayerSide } from "../bindings/generated";
import type { AssetMap } from "../renderer/assets";
import { loadAssets } from "../renderer/assets";
import {
	buildDrawInstructions,
	computeStaticGeometry,
} from "../renderer/instructions";
import { computeLayout } from "../renderer/layout";
import { buildPvOverlay, buildWallSet } from "../renderer/pvArrows";
import { generateTileMap } from "../renderer/tileMap";
import { useCurrentBotInfo, useMatchStore } from "../stores/matchStore";
import MazeCanvas from "./MazeCanvas";
import PvOverlay from "./PvOverlay";

type Props = {
	gameState: MazeState;
	showCellIndices?: boolean;
};

export default function MazeRenderer({ gameState, showCellIndices }: Props) {
	const [assets, setAssets] = useState<AssetMap | null>(null);
	const { ref, width, height } = useElementSize();
	const botInfo = useCurrentBotInfo();
	const showP1Arrows = useMatchStore((s) => s.showPlayer1Arrows);
	const showP2Arrows = useMatchStore((s) => s.showPlayer2Arrows);

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

	// Static geometry (walls + corners) — only changes when walls or layout change
	const staticGeo = useMemo(() => {
		if (!layout) return null;
		return computeStaticGeometry(gameState.walls, layout);
	}, [gameState.walls, layout]);

	const instructions = useMemo(() => {
		if (!assets || !layout || !staticGeo) return null;
		return buildDrawInstructions(
			gameState,
			layout,
			assets,
			tileMap,
			staticGeo,
			{ showCellIndices },
		);
	}, [gameState, layout, assets, tileMap, staticGeo, showCellIndices]);

	const wallSet = useMemo(
		() => buildWallSet(gameState.walls),
		[gameState.walls],
	);

	const visibleSenders = useMemo(() => {
		if (showP1Arrows && showP2Arrows) return undefined;
		const set = new Set<PlayerSide>();
		if (showP1Arrows) set.add("Player1");
		if (showP2Arrows) set.add("Player2");
		return set;
	}, [showP1Arrows, showP2Arrows]);

	const pvOverlayData = useMemo(() => {
		if (!botInfo || !layout) return null;
		if (visibleSenders?.size === 0) return null;
		return buildPvOverlay(
			botInfo,
			gameState.player1.position,
			gameState.player2.position,
			wallSet,
			gameState.width,
			gameState.height,
			layout,
			{ visibleSenders },
		);
	}, [botInfo, layout, gameState, wallSet, visibleSenders]);

	return (
		<div ref={ref} style={{ width: "100%", height: "100%", minHeight: 200 }}>
			{!assets || !instructions || !layout ? (
				<Center h="100%">
					<Loader />
				</Center>
			) : (
				<div style={{ position: "relative" }}>
					<MazeCanvas
						instructions={instructions}
						width={layout.canvasWidth}
						height={layout.canvasHeight}
					/>
					{pvOverlayData && (
						<PvOverlay
							overlay={pvOverlayData}
							width={layout.canvasWidth}
							height={layout.canvasHeight}
						/>
					)}
				</div>
			)}
		</div>
	);
}
