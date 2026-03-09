import { Center, Loader } from "@mantine/core";
import { useElementSize } from "@mantine/hooks";
import { useEffect, useMemo, useState } from "react";
import type { MazeState } from "../bindings/generated";
import type { AssetMap } from "../renderer/assets";
import { loadAssets } from "../renderer/assets";
import {
	buildDrawInstructions,
	computeStaticGeometry,
} from "../renderer/instructions";
import { computeLayout } from "../renderer/layout";
import { buildPvOverlay } from "../renderer/pvArrows";
import { generateTileMap } from "../renderer/tileMap";
import type { BotInfoMap } from "../stores/matchStore";
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
	const rawBotInfo = useCurrentBotInfo();
	const showP1Arrows = useMatchStore((s) => s.showPlayer1Arrows);
	const showP2Arrows = useMatchStore((s) => s.showPlayer2Arrows);

	const botInfo = useMemo(() => {
		if (!rawBotInfo) return null;
		if (showP1Arrows && showP2Arrows) return rawBotInfo;
		const filtered: BotInfoMap = {};
		for (const [key, bucket] of Object.entries(rawBotInfo)) {
			const sender = key.split(":")[0];
			if (sender === "Player1" && !showP1Arrows) continue;
			if (sender === "Player2" && !showP2Arrows) continue;
			filtered[key] = bucket;
		}
		return Object.keys(filtered).length > 0 ? filtered : null;
	}, [rawBotInfo, showP1Arrows, showP2Arrows]);

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

	const pvOverlayData = useMemo(() => {
		if (!botInfo || !layout) return null;
		return buildPvOverlay(
			botInfo,
			gameState.player1.position,
			gameState.player2.position,
			gameState.walls,
			gameState.width,
			gameState.height,
			layout,
		);
	}, [botInfo, layout, gameState]);

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
