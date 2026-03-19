import { Center, Text } from "@mantine/core";
import { useElementSize } from "@mantine/hooks";
import { useMemo } from "react";
import type { MazeState } from "../bindings/generated";
import { computeLayout } from "../renderer/layout";
import EventTimeline, { TIMELINE_HEIGHT } from "./EventTimeline";
import MazeRenderer from "./MazeRenderer";

type Props = {
	connecting: boolean;
	displayState: MazeState | null;
	previewError: string | null;
	hasMatch: boolean;
};

export default function MazeColumn({
	connecting,
	displayState,
	previewError,
	hasMatch,
}: Props) {
	const { ref, width, height } = useElementSize();

	const layout = useMemo(() => {
		if (width === 0 || height === 0) return null;
		const mazeH = hasMatch ? height - TIMELINE_HEIGHT : height;
		if (!displayState || mazeH <= 0) return null;
		return computeLayout(width, mazeH, displayState.width, displayState.height);
	}, [width, height, displayState, hasMatch]);

	return (
		<div
			ref={ref}
			style={{
				flex: 1,
				minWidth: 0,
				display: "flex",
				flexDirection: "column",
				overflow: "hidden",
			}}
		>
			<div style={{ flex: 1, minHeight: 0 }}>
				{connecting ? (
					<Center h="100%">
						<Text c="dimmed" size="sm">
							Connecting bots...
						</Text>
					</Center>
				) : displayState ? (
					<MazeRenderer gameState={displayState} layout={layout} />
				) : previewError ? (
					<Center h="100%">
						<Text c="red" size="sm">
							{previewError}
						</Text>
					</Center>
				) : (
					<Center h="100%">
						<Text c="dimmed" size="sm">
							Generating preview...
						</Text>
					</Center>
				)}
			</div>
			{hasMatch && <EventTimeline layout={layout} />}
		</div>
	);
}
