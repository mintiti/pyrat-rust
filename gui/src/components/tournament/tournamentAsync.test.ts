import { describe, expect, it } from "vitest";
import {
	createLatestRequestGuard,
	createTournamentReconcileQueue,
} from "./tournamentAsync";

function deferred() {
	let resolve: () => void = () => undefined;
	const promise = new Promise<void>((done) => {
		resolve = () => done();
	});
	return { promise, resolve };
}

describe("tournament async guards", () => {
	it("invalidates a historical snapshot for newer navigation and unmount", () => {
		const guard = createLatestRequestGuard();
		const firstHistoryRead = guard.begin();
		const newerHistoryRead = guard.begin();

		expect(guard.isCurrent(firstHistoryRead)).toBe(false);
		expect(guard.isCurrent(newerHistoryRead)).toBe(true);

		// `View live` and the LaunchView cleanup both use this invalidation path.
		guard.invalidate();
		expect(guard.isCurrent(newerHistoryRead)).toBe(false);
	});

	it("keeps a new tournament repair behind an old in-flight snapshot", async () => {
		const oldSnapshot = deferred();
		const calls: number[] = [];
		const queue = createTournamentReconcileQueue(
			async (tournamentId) => {
				calls.push(tournamentId);
				if (tournamentId === 1) await oldSnapshot.promise;
			},
			() => true,
		);

		queue.request(1);
		queue.request(2);
		expect(calls).toEqual([1]);

		oldSnapshot.resolve();
		await queue.waitForIdle();
		expect(calls).toEqual([1, 2]);
	});
});
