import {
	Button,
	Code,
	CopyButton,
	Group,
	Paper,
	Stack,
	Text,
} from "@mantine/core";
import { IconAlertTriangle, IconCheck, IconCopy } from "@tabler/icons-react";
import type { TournamentProvenance } from "../../bindings/generated";
import type { MatchFailureRecord } from "../../stores/tournamentStore";
import { failureLabel } from "../../stores/tournamentStore";
import { T, shortId } from "./theme";

export default function FailureCard({
	tournamentId,
	failure,
	players,
	provenance,
}: {
	tournamentId: number;
	failure: MatchFailureRecord;
	players: string[];
	provenance: TournamentProvenance | null;
}) {
	const pythonId =
		failure.ratId === failure.player1Id ? failure.player2Id : failure.player1Id;
	const implicated = failure.failingPlayerId
		? shortId(failure.failingPlayerId, players)
		: "tournament infrastructure";
	const participant = provenance?.participants.find(
		(candidate) => candidate.player_id === failure.failingPlayerId,
	);
	const maxAttempts = provenance?.max_failures_per_pair ?? null;
	const attempt = failure.attemptIndex + 1;
	const evidence = [
		`Tournament: ${tournamentId}`,
		`Match: ${failure.matchId ?? "not recorded"}`,
		`Schedule slot: ${failure.repetitionIndex}`,
		`Attempt: ${failure.attemptIndex}`,
		`Pair: ${failure.player1Id} vs ${failure.player2Id}`,
		`Rat: ${failure.ratId}`,
		`Python: ${pythonId}`,
		`Failure kind: ${failure.kind}`,
		`Phase: ${failure.timeoutPhase ?? "not applicable"}`,
		`Implicated participant: ${failure.failingPlayerId ?? "unattributed"}`,
		`Exhausted: ${failure.exhausted}`,
		participant?.command ? `Command: ${participant.command}` : null,
		participant?.working_dir
			? `Working directory: ${participant.working_dir}`
			: null,
		`Reason: ${failure.reason}`,
	]
		.filter((line): line is string => line !== null)
		.join("\n");

	return (
		<Paper withBorder p="xs" radius="sm" style={{ borderColor: T.loss }}>
			<Stack gap={6}>
				<Group gap="xs" wrap="nowrap" align="flex-start">
					<IconAlertTriangle
						size={15}
						color={T.loss}
						style={{ marginTop: 2, flex: "0 0 auto" }}
					/>
					<div style={{ flex: 1, minWidth: 0 }}>
						<Text size="xs" fw={700}>
							{implicated} · {failureLabel(failure.kind, failure.timeoutPhase)}
						</Text>
						<Text size="xs" c="dimmed">
							{shortId(failure.ratId, players)} as Rat ·{" "}
							{shortId(pythonId, players)} as Python ·{" "}
							{failure.exhausted
								? `attempt ${attempt}${maxAttempts === null ? "" : `/${maxAttempts}`} exhausted this leg`
								: `attempt ${attempt} failed; the leg remained retryable`}
						</Text>
					</div>
				</Group>
				<Code
					block
					style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}
				>
					{failure.reason}
				</Code>
				<details>
					<summary style={{ cursor: "pointer", fontSize: 12, color: T.muted }}>
						Developer details
					</summary>
					<Stack gap={4} mt="xs">
						<Text size="xs" ff="monospace">
							tournament {tournamentId} · match {failure.matchId ?? "unknown"} ·
							slot {failure.repetitionIndex} · attempt {failure.attemptIndex}
						</Text>
						{participant?.command && (
							<Text
								size="xs"
								ff="monospace"
								style={{ overflowWrap: "anywhere" }}
							>
								{participant.command}
								{participant.working_dir
									? ` · cwd ${participant.working_dir}`
									: ""}
							</Text>
						)}
						<CopyButton value={evidence} timeout={1_500}>
							{({ copied, copy }) => (
								<Button
									size="compact-xs"
									variant="light"
									color={copied ? "green" : "gray"}
									leftSection={
										copied ? <IconCheck size={12} /> : <IconCopy size={12} />
									}
									onClick={copy}
									style={{ alignSelf: "flex-start" }}
								>
									{copied ? "Copied" : "Copy failure evidence"}
								</Button>
							)}
						</CopyButton>
					</Stack>
				</details>
			</Stack>
		</Paper>
	);
}
