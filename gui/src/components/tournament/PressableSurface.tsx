import {
	type ButtonHTMLAttributes,
	type CSSProperties,
	forwardRef,
} from "react";

type PressableSurfaceProps = ButtonHTMLAttributes<HTMLButtonElement> & {
	/** Optional inset accent, used when the surface carries result/state colour. */
	accent?: string;
	/** Bare keeps the shared button semantics and focus treatment without a card shell. */
	variant?: "surface" | "bare";
	/** Rows move toward their destination; cards lift. */
	motion?: "row" | "card" | "none";
};

type PressableStyle = CSSProperties & {
	"--pyrat-pressable-accent"?: string;
};

/**
 * The shared interaction contract for card- and row-shaped navigation.
 *
 * Keeping this as a real button makes Enter, Space, focus, and disabled state
 * native behavior instead of a second interaction path we have to emulate.
 */
const PressableSurface = forwardRef<HTMLButtonElement, PressableSurfaceProps>(
	function PressableSurface(
		{
			accent,
			children,
			className,
			motion = "row",
			style,
			type = "button",
			variant = "surface",
			...props
		},
		ref,
	) {
		const classes = ["pyrat-pressable-surface", className]
			.filter(Boolean)
			.join(" ");
		const surfaceStyle: PressableStyle = {
			...style,
			...(accent ? { "--pyrat-pressable-accent": accent } : {}),
		};

		return (
			<button
				{...props}
				ref={ref}
				type={type}
				className={classes}
				data-motion={motion}
				data-variant={variant}
				style={surfaceStyle}
			>
				{children}
			</button>
		);
	},
);

export default PressableSurface;
