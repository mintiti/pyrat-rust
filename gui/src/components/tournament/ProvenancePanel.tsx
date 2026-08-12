import {
	Box,
	Code,
	Group,
	Paper,
	SimpleGrid,
	Stack,
	Text,
} from "@mantine/core";
import type { ReactNode } from "react";
import type { TournamentLive } from "../../stores/tournamentStore";
import { T, shortId } from "./theme";

const timingModeLabel = (mode: "wait" | "clock") =>
	mode === "wait" ? "wait for both bots" : "wall-clock deadlines";

/** The durable tournament recipe. Kept collapsed on the overview so the
 * verdict remains primary while every condition needed to interpret or rerun
 * the evidence is still inspectable in-place. */
export default function ProvenancePanel({ live }: { live: TournamentLive }) {
	const recipe = live.provenance;
	return (
		<Paper
			component="details"
			withBorder
			p="sm"
			radius="md"
			mt="lg"
			bg={T.panel2}
		>
			<Box component="summary" style={{ cursor: "pointer" }}>
				<Text span size="xs" tt="uppercase" c="dimmed" fw={700}>
					Conditions & methodology v
					{recipe?.interpretation.methodology_version ?? "?"}
				</Text>
			</Box>
			{recipe === null ? (
				<Text size="sm" c="dimmed" mt="sm">
					These conditions were not recorded for this older tournament. Current
					defaults are deliberately not substituted.
				</Text>
			) : (
				<Stack gap="md" mt="md">
					<SimpleGrid cols={{ base: 1, sm: 2 }} spacing="md">
						<RecipeGroup title="Game">
							<Fact label="Seed" value={String(recipe.tournament_seed)} mono />
							<Fact label="Config id" value={recipe.game_config_id} mono />
							<Fact
								label="Board"
								value={`${recipe.factory.width}×${recipe.factory.height} · ${recipe.factory.max_turns} turns · starts ${recipe.factory.player_start}`}
							/>
							<Fact
								label="Maze"
								value={`walls ${recipe.factory.wall_density} · mud ${recipe.factory.mud_density} (range ${recipe.factory.mud_range}) · ${recipe.factory.connected ? "connected" : "not forced connected"} · ${recipe.factory.symmetric ? "symmetric" : "asymmetric"}`}
							/>
							<Fact
								label="Cheese"
								value={`${recipe.factory.cheese_count} · ${recipe.factory.cheese_symmetric ? "symmetric" : "asymmetric"}`}
							/>
						</RecipeGroup>

						<RecipeGroup title="Schedule">
							<Fact
								label="Matchup"
								value={`${recipe.games_per_matchup} games${recipe.mazes_per_matchup === null ? "" : ` · ${recipe.mazes_per_matchup} shared mazes`}`}
							/>
							<Fact label="Seats" value={recipe.seat_policy} />
							<Fact
								label="Instances"
								value={recipe.interpretation.instance_policy}
							/>
							<Fact
								label="Retries"
								value={`${recipe.max_failures_per_pair} failed attempts per slot`}
							/>
							<Fact
								label="Concurrency"
								value={String(recipe.methodology.max_parallel)}
							/>
						</RecipeGroup>

						<RecipeGroup title="Timing">
							<Fact
								label="Mode"
								value={timingModeLabel(recipe.methodology.timing_mode)}
							/>
							<Fact
								label="Move / preprocess"
								value={`${recipe.methodology.move_timeout_ms} ms / ${recipe.methodology.preprocessing_timeout_ms} ms`}
							/>
							<Fact
								label="Startup / configure"
								value={`${recipe.methodology.startup_timeout_ms} ms / ${recipe.methodology.configure_timeout_ms} ms`}
							/>
							<Fact
								label="Network grace"
								value={`${recipe.methodology.network_grace_ms} ms`}
							/>
						</RecipeGroup>

						<RecipeGroup title="Rating interpretation">
							<Fact
								label="Anchor"
								value={`${shortId(recipe.interpretation.anchor_id, live.players)} = ${recipe.interpretation.anchor_elo}`}
							/>
							<Fact
								label="Estimator"
								value={`${recipe.interpretation.estimator} v${recipe.interpretation.estimator_version}`}
							/>
							<Fact
								label="Draw / prior"
								value={`${recipe.interpretation.draw_weight} / ${recipe.interpretation.prior_games} games`}
							/>
							<Fact
								label="Solver"
								value={`${recipe.interpretation.max_iterations} iterations · tolerance ${recipe.interpretation.tolerance}`}
							/>
							<Fact
								label="Readiness"
								value={`${recipe.interpretation.min_games_per_player} successful games per player`}
							/>
							<Fact
								label="Interval"
								value="95% normal interval for player − anchor"
							/>
						</RecipeGroup>
					</SimpleGrid>

					<Box>
						<Text size="xs" tt="uppercase" c="dimmed" fw={700} mb={6}>
							Participants & launch recipe
						</Text>
						<Stack gap={6}>
							{recipe.participants.map((participant) => (
								<Paper
									key={participant.player_id}
									withBorder
									p="xs"
									radius="sm"
								>
									<Group justify="space-between" align="baseline" gap="sm">
										<Text size="sm" fw={700}>
											{participant.display_name}
										</Text>
										<Code style={{ overflowWrap: "anywhere" }}>
											{participant.player_id}
										</Code>
									</Group>
									<Text size="xs" c="dimmed" mt={4}>
										agent <Code>{participant.agent_id}</Code>
										{participant.declared_version
											? ` · version ${participant.declared_version}`
											: " · version not declared"}
									</Text>
									<Text
										size="xs"
										mt={4}
										ff="monospace"
										style={{ overflowWrap: "anywhere" }}
									>
										{participant.command ?? "embedded participant"}
									</Text>
									{participant.working_dir && (
										<Text
											size="xs"
											c="dimmed"
											ff="monospace"
											style={{ overflowWrap: "anywhere" }}
										>
											from {participant.working_dir}
										</Text>
									)}
									<Text
										size="xs"
										c="dimmed"
										mt={4}
										style={{ overflowWrap: "anywhere" }}
									>
										options: {optionLabel(participant.options)}
										{participant.fingerprint
											? ` · ${participant.fingerprint.kind}: ${participant.fingerprint.value}`
											: " · no immutable fingerprint"}
									</Text>
								</Paper>
							))}
						</Stack>
					</Box>

					<Text size="xs" c="dimmed">
						{recipe.mutable_files_warning}
					</Text>
				</Stack>
			)}
		</Paper>
	);
}

function RecipeGroup({
	title,
	children,
}: { title: string; children: ReactNode }) {
	return (
		<Box>
			<Text size="xs" tt="uppercase" c="dimmed" fw={700} mb={6}>
				{title}
			</Text>
			<Stack gap={3}>{children}</Stack>
		</Box>
	);
}

function optionLabel(
	options:
		| { kind: "defaults" }
		| { kind: "explicit"; values: [string, string][] },
): string {
	return options.kind === "defaults"
		? "defaults"
		: options.values.map(([key, value]) => `${key}=${value}`).join(", ");
}

function Fact({
	label,
	value,
	mono = false,
}: { label: string; value: string; mono?: boolean }) {
	return (
		<Group gap="xs" wrap="nowrap" align="baseline">
			<Text size="xs" c="dimmed" w={88} style={{ flexShrink: 0 }}>
				{label}
			</Text>
			<Text size="xs" ff={mono ? "monospace" : undefined}>
				{value}
			</Text>
		</Group>
	);
}
