import { confirm } from "@tauri-apps/plugin-dialog";

/** Stop is consequential because v1 cannot resume unfinished schedule slots.
 * Keep the wording shared across every live stop affordance. */
export async function confirmTournamentStop(): Promise<boolean> {
	return confirm(
		"Completed games and failures will be kept, but unfinished schedule slots cannot be resumed. Stop this tournament?",
		{
			title: "Stop tournament",
			kind: "warning",
		},
	);
}

export const TOURNAMENT_CONTENTION_WARNING =
	"A tournament is running. This match shares local CPU with it and can distort time-budgeted results. Start the match anyway?";
