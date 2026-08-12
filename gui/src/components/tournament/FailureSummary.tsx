import { Box, Group, Paper, Stack, Text } from "@mantine/core";
import { IconAlertTriangle, IconChevronRight } from "@tabler/icons-react";
import type { TournamentLive } from "../../stores/tournamentStore";
import { useTournamentStore } from "../../stores/tournamentStore";
import PressableSurface from "./PressableSurface";
import { T, shortId } from "./theme";

/** Overview severity stays visible while exact evidence lives one rung down at
 * the affected matchup and leg. Healthy tournaments render nothing. */
export default function FailureSummary({ live }: { live: TournamentLive }) {
	const navigate = useTournamentStore((state) => state.navigate);
	const detailedFailures = Object.values(live.failuresByPair).reduce(
		(total, failures) => total + failures.length,
		0,
	);
	const attemptCount = Math.max(live.failure, detailedFailures);
	if (attemptCount === 0 && live.exhausted === 0) return null;

	const pairs = Object.values(live.failuresByPair)
		.filter((failures) => failures.length > 0)
		.map((failures) => {
			const first = failures[0];
			if (!first) return null;
			const implicated = [
				...new Set(
					failures
						.map((failure) => failure.failingPlayerId)
						.filter((id): id is string => id !== null),
				),
			];
			return {
				a: first.player1Id,
				b: first.player2Id,
				attempts: failures.length,
				exhausted: new Set(
					failures
						.filter((failure) => failure.exhausted)
						.map((failure) => failure.repetitionIndex),
				).size,
				implicated,
			};
		})
		.filter((pair): pair is NonNullable<typeof pair> => pair !== null);
	const severe = live.exhausted > 0 || live.lifecycle === "failed";

	return (
		<Paper
			component="output"
			withBorder
			p="sm"
			radius="md"
			mt="md"
			bg={T.panel2}
			style={{ borderColor: severe ? T.loss : T.cheeseDim }}
		>
			<Group gap="xs" wrap="nowrap" align="flex-start">
				<IconAlertTriangle
					size={16}
					color={severe ? T.loss : T.cheese}
					style={{ marginTop: 2, flex: "0 0 auto" }}
				/>
				<Box style={{ flex: 1, minWidth: 0 }}>
					<Text size="sm" fw={700}>
						{attemptCount} failed {attemptCount === 1 ? "attempt" : "attempts"}
						{live.exhausted > 0
							? ` · ${live.exhausted} exhausted schedule ${live.exhausted === 1 ? "slot" : "slots"}`
							: " · retries recovered so far"}
					</Text>
					<Text size="xs" c="dimmed" mt={2}>
						Open an affected matchup to see the bot, seat, attempt, exact
						reason, and copyable identifiers.
					</Text>
					{pairs.length === 0 ? (
						<Text size="xs" c="dimmed" mt="xs">
							Failure details are refreshing from durable state…
						</Text>
					) : (
						<Stack gap={5} mt="xs">
							{pairs.map((pair) => (
								<PressableSurface
									key={`${pair.a}|${pair.b}`}
									motion="row"
									onClick={() =>
										navigate({ kind: "matchup", a: pair.a, b: pair.b })
									}
									aria-label={`Inspect failures for ${shortId(pair.a, live.players)} versus ${shortId(pair.b, live.players)}`}
								>
									<Group gap="xs" wrap="nowrap" px="xs" py={6}>
										<Text size="xs" style={{ flex: 1 }}>
											{shortId(pair.a, live.players)} vs{" "}
											{shortId(pair.b, live.players)} · {pair.attempts} failed
											{pair.exhausted > 0
												? ` · ${pair.exhausted} exhausted`
												: ""}
											{pair.implicated.length > 0
												? ` · affected: ${pair.implicated.map((id) => shortId(id, live.players)).join(", ")}`
												: " · infrastructure/unattributed"}
										</Text>
										<IconChevronRight
											size={14}
											color={T.muted}
											className="pyrat-pressable-surface__chevron"
										/>
									</Group>
								</PressableSurface>
							))}
						</Stack>
					)}
				</Box>
			</Group>
		</Paper>
	);
}
