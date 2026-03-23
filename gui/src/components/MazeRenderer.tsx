import { Center, Loader } from "@mantine/core";
import { useElementSize } from "@mantine/hooks";
import { useEffect, useMemo, useState } from "react";
import type { PlayerSide } from "../bindings/generated";
import type { AssetMap } from "../renderer/assets";
import { loadAssets } from "../renderer/assets";
import {
	buildDrawInstructions,
	computeStaticGeometry,
} from "../renderer/instructions";
import { type LayoutMetrics, computeLayout } from "../renderer/layout";
import { buildPvOverlay, buildWallSet } from "../renderer/pvArrows";
import { generateTileMap } from "../renderer/tileMap";
import type { DisplayState } from "../stores/matchStore";
import {
	useCurrentBotInfo,
	useIsAtTip,
	useMatchStore,
} from "../stores/matchStore";
import DragOverlay from "./DragOverlay";
import MazeCanvas from "./MazeCanvas";
import PvOverlay from "./PvOverlay";

type Props = {
	gameState: DisplayState;
	layout?: LayoutMetrics | null;
	showCellIndices?: boolean;
	hideScoreStrip?: boolean;
};

export default function MazeRenderer({
	gameState,
	layout: externalLayout,
	showCellIndices,
	hideScoreStrip,
}: Props) {
	const [assets, setAssets] = useState<AssetMap | null>(null);
	const { ref, width, height } = useElementSize();
	const botInfo = useCurrentBotInfo();
	const showP1Arrows = useMatchStore((s) => s.showPlayer1Arrows);
	const showP2Arrows = useMatchStore((s) => s.showPlayer2Arrows);
	const mode = useMatchStore((s) => s.mode);
	const matchPhase = useMatchStore((s) => s.matchPhase);
	const isAtTip = useIsAtTip();
	const showDragOverlay =
		mode === "step" && isAtTip && matchPhase === "playing";

	useEffect(() => {
		loadAssets().then(setAssets);
	}, []);

	// Use external layout when provided (MazeColumn), otherwise compute internally
	const internalLayout = useMemo(() => {
		if (externalLayout !== undefined) return null;
		if (width === 0 || height === 0) return null;
		return computeLayout(width, height, gameState.width, gameState.height);
	}, [externalLayout, width, height, gameState.width, gameState.height]);

	const layout = externalLayout !== undefined ? externalLayout : internalLayout;

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
			{ showCellIndices, hideScoreStrip },
		);
	}, [
		gameState,
		layout,
		assets,
		tileMap,
		staticGeo,
		showCellIndices,
		hideScoreStrip,
	]);

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
			gameState.player1Destination,
			gameState.player2Destination,
			wallSet,
			gameState.width,
			gameState.height,
			layout,
			{ visibleSenders },
		);
	}, [botInfo, layout, gameState, wallSet, visibleSenders]);

	return (
		<div
			ref={externalLayout !== undefined ? undefined : ref}
			style={{ width: "100%", height: "100%", minHeight: 200 }}
		>
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
					{showDragOverlay && assets && (
						<DragOverlay
							layout={layout}
							wallSet={wallSet}
							gameState={gameState}
							assetMap={assets}
						/>
					)}
				</div>
			)}
		</div>
	);
}
