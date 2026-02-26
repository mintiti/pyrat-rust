import { useEffect, useRef } from "react";
import type {
	DrawInstructions,
	SpriteInstruction,
} from "../renderer/instructions";

type Props = {
	instructions: DrawInstructions;
	width: number;
	height: number;
};

function drawSprite(ctx: CanvasRenderingContext2D, s: SpriteInstruction) {
	const needsTransform = s.rotation || s.flipX || s.flipY;

	if (needsTransform) {
		ctx.save();
		const cx = s.dx + s.dw / 2;
		const cy = s.dy + s.dh / 2;
		ctx.translate(cx, cy);

		if (s.rotation) {
			ctx.rotate((s.rotation * Math.PI) / 180);
		}
		ctx.scale(s.flipX ? -1 : 1, s.flipY ? -1 : 1);
		ctx.drawImage(s.image, -s.dw / 2, -s.dh / 2, s.dw, s.dh);
		ctx.restore();
	} else {
		ctx.drawImage(s.image, s.dx, s.dy, s.dw, s.dh);
	}
}

export default function MazeCanvas({ instructions, width, height }: Props) {
	const canvasRef = useRef<HTMLCanvasElement>(null);

	useEffect(() => {
		const canvas = canvasRef.current;
		if (!canvas) return;

		const ctx = canvas.getContext("2d");
		if (!ctx) return;

		const dpr = window.devicePixelRatio || 1;
		canvas.width = width * dpr;
		canvas.height = height * dpr;
		canvas.style.width = `${width}px`;
		canvas.style.height = `${height}px`;
		ctx.scale(dpr, dpr);

		// Background
		ctx.fillStyle = instructions.background;
		ctx.fillRect(0, 0, width, height);

		// Sprites
		for (const sprite of instructions.sprites) {
			drawSprite(ctx, sprite);
		}

		// Text
		for (const t of instructions.texts) {
			ctx.font = `${t.fontSize}px monospace`;
			ctx.fillStyle = t.color;
			ctx.textAlign = t.align ?? "left";
			ctx.fillText(t.text, t.x, t.y);
		}
	}, [instructions, width, height]);

	return <canvas ref={canvasRef} />;
}
