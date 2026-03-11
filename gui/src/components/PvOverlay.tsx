import { useEffect, useRef } from "react";
import {
	ARROWHEAD_SIZE_FACTOR,
	type PvOverlayData,
	TARGET_STROKE_WIDTH,
} from "../renderer/pvArrows";

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
			ctx.lineWidth = TARGET_STROKE_WIDTH;
			ctx.stroke();
		}

		for (const arrow of overlay.arrows) {
			const { points } = arrow;
			ctx.strokeStyle = arrow.color;
			ctx.lineWidth = arrow.thickness;
			ctx.lineCap = "round";
			ctx.lineJoin = "round";

			ctx.beginPath();
			ctx.moveTo(points[0].x, points[0].y);
			for (let i = 1; i < points.length; i++) {
				ctx.lineTo(points[i].x, points[i].y);
			}
			ctx.stroke();

			// Arrowhead at the tip
			const tip = points[points.length - 1];
			const prev = points[points.length - 2];
			const angle = Math.atan2(tip.y - prev.y, tip.x - prev.x);
			const headSize = arrow.thickness * ARROWHEAD_SIZE_FACTOR;
			ctx.fillStyle = arrow.color;
			drawArrowhead(ctx, tip.x, tip.y, angle, headSize);
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
