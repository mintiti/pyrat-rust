import { useEffect, useRef } from "react";
import type { PvOverlayData } from "../renderer/pvArrows";

type Props = {
	overlay: PvOverlayData;
	width: number;
	height: number;
};

function drawArrowhead(
	ctx: CanvasRenderingContext2D,
	toX: number,
	toY: number,
	angle: number,
	size: number,
) {
	ctx.save();
	ctx.translate(toX, toY);
	ctx.rotate(angle);
	ctx.beginPath();
	ctx.moveTo(0, 0);
	ctx.lineTo(-size, -size / 2);
	ctx.lineTo(-size, size / 2);
	ctx.closePath();
	ctx.fill();
	ctx.restore();
}

export default function PvOverlay({ overlay, width, height }: Props) {
	const canvasRef = useRef<HTMLCanvasElement>(null);
	const dimsRef = useRef({ width: 0, height: 0, dpr: 0 });

	// Resize backing buffer
	useEffect(() => {
		const canvas = canvasRef.current;
		if (!canvas) return;

		const dpr = window.devicePixelRatio || 1;
		if (
			dimsRef.current.width !== width ||
			dimsRef.current.height !== height ||
			dimsRef.current.dpr !== dpr
		) {
			canvas.width = width * dpr;
			canvas.height = height * dpr;
			canvas.style.width = `${width}px`;
			canvas.style.height = `${height}px`;
			dimsRef.current = { width, height, dpr };
		}
	}, [width, height]);

	// Draw
	useEffect(() => {
		const canvas = canvasRef.current;
		if (!canvas) return;

		const ctx = canvas.getContext("2d");
		if (!ctx) return;

		const dpr = window.devicePixelRatio || 1;
		ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
		ctx.clearRect(0, 0, width, height);

		// Target markers
		for (const t of overlay.targets) {
			ctx.beginPath();
			ctx.arc(t.cx, t.cy, t.radius, 0, Math.PI * 2);
			ctx.strokeStyle = t.color;
			ctx.lineWidth = 3;
			ctx.stroke();
		}

		// Sort: alternatives first, best lines last (on top)
		const sorted = [...overlay.arrows].sort((a, b) => b.multipv - a.multipv);

		for (const arrow of sorted) {
			ctx.strokeStyle = arrow.color;
			ctx.lineWidth = arrow.thickness;
			ctx.lineCap = "round";
			ctx.lineJoin = "round";

			ctx.beginPath();
			ctx.moveTo(arrow.segments[0].fromX, arrow.segments[0].fromY);
			for (const seg of arrow.segments) {
				ctx.lineTo(seg.toX, seg.toY);
			}
			ctx.stroke();

			// Arrowhead at the end of the last segment
			const last = arrow.segments[arrow.segments.length - 1];
			const angle = Math.atan2(last.toY - last.fromY, last.toX - last.fromX);
			const headSize = arrow.thickness * 2.5;
			ctx.fillStyle = arrow.color;
			drawArrowhead(ctx, last.toX, last.toY, angle, headSize);
		}
	}, [overlay, width, height]);

	return (
		<canvas
			ref={canvasRef}
			style={{
				position: "absolute",
				top: 0,
				left: 0,
				pointerEvents: "none",
			}}
		/>
	);
}
