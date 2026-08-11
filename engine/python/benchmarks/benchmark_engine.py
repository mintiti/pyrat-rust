"""Dependency-free benchmarks for the public Python engine paths.

Build the extension in release mode before trusting the timings:

    uv run --directory engine maturin develop --release

Run the complete medium scenario:

    uv run --directory engine python python/benchmarks/benchmark_engine.py

The harness uses only the Python standard library plus the installed engine. It
keeps fixture construction, resets for stateful samples, and validation outside
the measured intervals. Timings are evidence, not test thresholds.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import socket
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from functools import partial
from pathlib import Path
from typing import (
    Any,
    Callable,
    Dict,
    Sequence,
    Tuple,
)

import pyrat_engine
import pyrat_engine._core as engine_core
from pyrat_engine import GameBuilder, GameConfig
from pyrat_engine.env import PyRatEnv

Point = Tuple[int, int]
Edge = Tuple[Point, Point]
WallInput = Tuple[Point, Point]
MudInput = Tuple[Point, Point, int]
ActionPair = Tuple[int, int]
EnvironmentAction = Dict[str, int]

CASE_NAMES = (
    "create/random",
    "create/fixed",
    "create/reused-maze",
    "setup/paired-maze",
    "reset/random",
    "reset/fixed",
    "reset/reused-maze",
    "observation/pair",
    "step/pyrat",
    "step/env",
)

SCENARIO_NAME = "medium"
FIXED_SEED = 0x50595241545F4245
RANDOM_SEED_BASE = 0xC0DEC0DE12345678
ACTION_SEED = 0xA17C9E3779B97F4A
MASK_U64 = (1 << 64) - 1

RANDOM_OPERATIONS_PER_BATCH = 4
FIXED_OPERATIONS_PER_BATCH = 16
REUSED_OPERATIONS_PER_BATCH = 16
OBSERVATION_PAIRS_PER_BATCH = 256
MAX_TRACE_TURNS = 128
MAX_ADAPTIVE_ITERATIONS = 100_000
MUD_PERIOD = 10
MUD_OFFSET = 4

NANOSECONDS_PER_MICROSECOND = 1_000.0
NANOSECONDS_PER_MILLISECOND = 1_000_000.0
NANOSECONDS_PER_SECOND = 1_000_000_000.0
OPERATIONS_PER_KILO = 1_000.0
OPERATIONS_PER_MEGA = 1_000_000.0
OPERATIONS_PER_GIGA = 1_000_000_000.0


@dataclass(frozen=True)
class MeasuredBatch:
    """One operation-only timed block."""

    elapsed_ns: int
    operations: int


@dataclass(frozen=True)
class BenchmarkCase:
    """A filterable benchmark case."""

    name: str
    measure_batch: Callable[[], MeasuredBatch]


@dataclass(frozen=True)
class CreateContext:
    config: Any
    seeds: tuple[int, ...]
    width: int
    height: int


@dataclass(frozen=True)
class ResetContext:
    game: Any
    seeds: tuple[int, ...]
    width: int
    height: int


@dataclass(frozen=True)
class MazeCreateContext:
    config: Any
    maze: Any
    seeds: tuple[int, ...]
    width: int
    height: int


@dataclass(frozen=True)
class MazeResetContext:
    game: Any
    maze: Any
    seeds: tuple[int, ...]
    width: int
    height: int


@dataclass(frozen=True)
class ObservationContext:
    game: Any
    tokens: tuple[int, ...]
    width: int
    height: int


@dataclass(frozen=True)
class StepContext:
    game: Any
    seed: int
    actions: tuple[ActionPair, ...]


@dataclass(frozen=True)
class EnvironmentStepContext:
    env: PyRatEnv
    seed: int
    actions: tuple[EnvironmentAction, ...]


@dataclass(frozen=True)
class CaseResult:
    """Repeated samples for one benchmark case."""

    name: str
    operations_per_batch: int
    iterations_per_sample: int
    discarded_batches: int
    samples: tuple[MeasuredBatch, ...]

    def ns_per_operation(self) -> list[float]:
        return [sample.elapsed_ns / sample.operations for sample in self.samples]

    def summary(self) -> dict[str, float]:
        values = self.ns_per_operation()
        median_ns = statistics.median(values)
        return {
            "median_ns_per_operation": median_ns,
            "min_ns_per_operation": min(values),
            "max_ns_per_operation": max(values),
            "median_operations_per_second": (NANOSECONDS_PER_SECOND / median_ns),
        }

    def as_json(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "operations_per_batch": self.operations_per_batch,
            "iterations_per_sample": self.iterations_per_sample,
            "discarded_batches": self.discarded_batches,
            "summary": self.summary(),
            "samples": [
                {
                    "elapsed_ns": sample.elapsed_ns,
                    "operations": sample.operations,
                    "ns_per_operation": (sample.elapsed_ns / sample.operations),
                }
                for sample in self.samples
            ],
        }


def _canonical_edge(first: Point, second: Point) -> Edge:
    return (first, second) if first < second else (second, first)


def _fixed_layout(
    width: int,
    height: int,
) -> tuple[tuple[WallInput, ...], tuple[MudInput, ...]]:
    """Build a stable connected snake maze with classic-like wall and mud load."""
    all_edges: set[Edge] = set()
    passages: set[Edge] = set()

    for y in range(height):
        for x in range(width - 1):
            edge = _canonical_edge((x, y), (x + 1, y))
            all_edges.add(edge)
            passages.add(edge)

    for y in range(height - 1):
        for x in range(width):
            all_edges.add(_canonical_edge((x, y), (x, y + 1)))

        connector_x = width - 1 if y % 2 == 0 else 0
        passages.add(
            _canonical_edge(
                (connector_x, y),
                (connector_x, y + 1),
            )
        )

    walls = tuple(sorted(all_edges.difference(passages)))
    mud: list[MudInput] = []
    for index, (first, second) in enumerate(sorted(passages)):
        if index % MUD_PERIOD == MUD_OFFSET:
            mud.append((first, second, 2 + (index // MUD_PERIOD) % 2))

    return walls, tuple(mud)


def _fixed_cheese(width: int, height: int, count: int) -> tuple[Point, ...]:
    """Keep cheese central so the prepared corner traces collect none."""
    center_x = width // 2
    center_y = height // 2
    player_starts = {(0, 0), (width - 1, height - 1)}
    candidates = [
        (x, y)
        for y in range(height)
        for x in range(width)
        if (x, y) not in player_starts
    ]
    candidates.sort(
        key=lambda point: (
            abs(point[0] - center_x) + abs(point[1] - center_y),
            point[1],
            point[0],
        )
    )

    if len(candidates) < count:
        raise RuntimeError("Fixed benchmark scenario does not have enough cheese cells")
    return tuple(candidates[:count])


def _splitmix64(value: int) -> int:
    value = (value + 0x9E3779B97F4A7C15) & MASK_U64
    value = ((value ^ (value >> 30)) * 0xBF58476D1CE4E5B9) & MASK_U64
    value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & MASK_U64
    return value ^ (value >> 31)


def _prepared_seeds(base: int, count: int) -> tuple[int, ...]:
    return tuple(_splitmix64(base + index) for index in range(count))


def _prepared_actions(seed: int, turns: int) -> tuple[ActionPair, ...]:
    state = seed
    actions: list[ActionPair] = []
    for _ in range(turns):
        state = _splitmix64(state)
        player_one = state % 5
        state = _splitmix64(state)
        player_two = state % 5
        actions.append((player_one, player_two))
    return tuple(actions)


def _validate_dimensions(game: Any, width: int, height: int) -> None:
    if game.width != width or game.height != height:
        raise RuntimeError(
            "Benchmark operation returned a game with unexpected dimensions"
        )
    # Materialize a value after timing so each produced object is observably used.
    _ = game.state_hash


def _measure_create(context: CreateContext) -> MeasuredBatch:
    config = context.config
    seeds = context.seeds
    last_game = None

    started_ns = time.perf_counter_ns()
    for seed in seeds:
        last_game = config.create(seed=seed)
    elapsed_ns = time.perf_counter_ns() - started_ns

    if last_game is None:
        raise RuntimeError("Create benchmark did not execute an operation")
    _validate_dimensions(last_game, context.width, context.height)
    return MeasuredBatch(elapsed_ns, len(seeds))


def _measure_reset(context: ResetContext) -> MeasuredBatch:
    game = context.game
    seeds = context.seeds

    started_ns = time.perf_counter_ns()
    for seed in seeds:
        game.reset(seed=seed)
    elapsed_ns = time.perf_counter_ns() - started_ns

    _validate_dimensions(game, context.width, context.height)
    return MeasuredBatch(elapsed_ns, len(seeds))


def _measure_create_with_maze(context: MazeCreateContext) -> MeasuredBatch:
    config = context.config
    maze = context.maze
    seeds = context.seeds
    last_game = None

    started_ns = time.perf_counter_ns()
    for seed in seeds:
        last_game = config.create_with_maze(maze, seed=seed)
    elapsed_ns = time.perf_counter_ns() - started_ns

    if last_game is None:
        raise RuntimeError("Reused-maze create benchmark did not execute an operation")
    _validate_dimensions(last_game, context.width, context.height)
    return MeasuredBatch(elapsed_ns, len(seeds))


def _measure_paired_maze_setup(context: MazeCreateContext) -> MeasuredBatch:
    config = context.config
    maze = context.maze
    seeds = context.seeds
    last_pair = None

    started_ns = time.perf_counter_ns()
    for seed in seeds:
        last_pair = (
            config.create_with_maze(maze, seed=seed),
            config.create_with_maze(maze, seed=seed),
        )
    elapsed_ns = time.perf_counter_ns() - started_ns

    if last_pair is None:
        raise RuntimeError("Paired-maze setup benchmark did not execute an operation")
    first, second = last_pair
    _validate_dimensions(first, context.width, context.height)
    _validate_dimensions(second, context.width, context.height)
    if first.state_hash != second.state_hash:
        raise RuntimeError("Same-seed paired setup produced different games")
    return MeasuredBatch(elapsed_ns, len(seeds))


def _measure_reset_with_maze(context: MazeResetContext) -> MeasuredBatch:
    game = context.game
    maze = context.maze
    seeds = context.seeds

    started_ns = time.perf_counter_ns()
    for seed in seeds:
        game.reset_with_maze(maze, seed=seed)
    elapsed_ns = time.perf_counter_ns() - started_ns

    _validate_dimensions(game, context.width, context.height)
    return MeasuredBatch(elapsed_ns, len(seeds))


def _measure_observation_pair(context: ObservationContext) -> MeasuredBatch:
    game = context.game
    tokens = context.tokens
    player_one = None
    player_two = None

    started_ns = time.perf_counter_ns()
    for _ in tokens:
        player_one, player_two = game.get_observations()
    elapsed_ns = time.perf_counter_ns() - started_ns

    if player_one is None or player_two is None:
        raise RuntimeError("Observation benchmark did not execute an operation")
    expected_cheese_shape = (context.width, context.height)
    expected_movement_shape = (context.width, context.height, 4)
    if player_one.cheese_matrix.shape != expected_cheese_shape:
        raise RuntimeError("Player-one cheese observation has an unexpected shape")
    if player_two.movement_matrix.shape != expected_movement_shape:
        raise RuntimeError("Player-two movement observation has an unexpected shape")
    return MeasuredBatch(elapsed_ns, len(tokens))


def _measure_step(context: StepContext) -> MeasuredBatch:
    game = context.game
    actions = context.actions

    # Reset is setup for the trace, so it stays outside the measured interval.
    game.reset(seed=context.seed)
    last_result = None
    started_ns = time.perf_counter_ns()
    for player_one, player_two in actions:
        last_result = game.step(player_one, player_two)
    elapsed_ns = time.perf_counter_ns() - started_ns

    if last_result is None:
        raise RuntimeError("PyRat.step benchmark did not execute an operation")
    if last_result[0]:
        raise RuntimeError("Prepared PyRat.step trace crossed game_over")
    if game.turn != len(actions):
        raise RuntimeError(
            "Prepared PyRat.step trace advanced an unexpected turn count"
        )
    if game.player1_score != 0.0 or game.player2_score != 0.0:
        raise RuntimeError("Prepared PyRat.step trace collected benchmark cheese")
    return MeasuredBatch(elapsed_ns, len(actions))


def _measure_environment_step(
    context: EnvironmentStepContext,
) -> MeasuredBatch:
    env = context.env
    actions = context.actions

    # Environment reset and its paired observations are separate benchmark work.
    env.reset(seed=context.seed)
    last_result = None
    started_ns = time.perf_counter_ns()
    for action in actions:
        last_result = env.step(action)
    elapsed_ns = time.perf_counter_ns() - started_ns

    if last_result is None:
        raise RuntimeError("PyRatEnv.step benchmark did not execute an operation")
    terminations = last_result[2]
    if any(terminations.values()):
        raise RuntimeError("Prepared PyRatEnv.step trace crossed termination")
    if env.game.turn != len(actions):
        raise RuntimeError(
            "Prepared PyRatEnv.step trace advanced an unexpected turn count"
        )
    if env.game.player1_score != 0.0 or env.game.player2_score != 0.0:
        raise RuntimeError("Prepared PyRatEnv.step trace collected benchmark cheese")
    return MeasuredBatch(elapsed_ns, len(actions))


def _build_cases() -> list[BenchmarkCase]:
    random_config = GameConfig.preset(SCENARIO_NAME)
    width = random_config.width
    height = random_config.height
    max_turns = random_config.max_turns

    random_seeds = _prepared_seeds(
        RANDOM_SEED_BASE,
        RANDOM_OPERATIONS_PER_BATCH,
    )
    random_game = random_config.create(seed=random_seeds[0])
    cheese_count = len(random_game.cheese_positions())

    walls, mud = _fixed_layout(width, height)
    cheese = _fixed_cheese(width, height, cheese_count)
    fixed_config = (
        GameBuilder(width, height)
        .with_max_turns(max_turns)
        .with_custom_maze(walls=list(walls), mud=list(mud))
        .with_corner_positions()
        .with_custom_cheese(list(cheese))
        .build()
    )

    fixed_seeds = _prepared_seeds(
        FIXED_SEED,
        FIXED_OPERATIONS_PER_BATCH,
    )
    reused_seeds = _prepared_seeds(
        RANDOM_SEED_BASE ^ FIXED_SEED,
        REUSED_OPERATIONS_PER_BATCH,
    )
    reused_maze = random_config.generate_maze(seed=FIXED_SEED)
    trace_turns = min(MAX_TRACE_TURNS, max_turns // 2)
    action_pairs = _prepared_actions(ACTION_SEED, trace_turns)
    environment_actions = tuple(
        {
            "player_1": player_one,
            "player_2": player_two,
        }
        for player_one, player_two in action_pairs
    )

    create_random = CreateContext(
        random_config,
        random_seeds,
        width,
        height,
    )
    create_fixed = CreateContext(
        fixed_config,
        fixed_seeds,
        width,
        height,
    )
    create_reused_maze = MazeCreateContext(
        random_config,
        reused_maze,
        reused_seeds,
        width,
        height,
    )
    reset_random = ResetContext(
        random_game,
        random_seeds,
        width,
        height,
    )
    reset_fixed = ResetContext(
        fixed_config.create(seed=FIXED_SEED),
        fixed_seeds,
        width,
        height,
    )
    reset_reused_maze = MazeResetContext(
        random_config.create_with_maze(reused_maze, seed=reused_seeds[0]),
        reused_maze,
        reused_seeds,
        width,
        height,
    )
    observation_pair = ObservationContext(
        fixed_config.create(seed=FIXED_SEED),
        tuple(range(OBSERVATION_PAIRS_PER_BATCH)),
        width,
        height,
    )
    step = StepContext(
        fixed_config.create(seed=FIXED_SEED),
        FIXED_SEED,
        action_pairs,
    )
    environment_step = EnvironmentStepContext(
        PyRatEnv(fixed_config, seed=FIXED_SEED),
        FIXED_SEED,
        environment_actions,
    )

    return [
        BenchmarkCase(
            "create/random",
            partial(_measure_create, create_random),
        ),
        BenchmarkCase(
            "create/fixed",
            partial(_measure_create, create_fixed),
        ),
        BenchmarkCase(
            "create/reused-maze",
            partial(_measure_create_with_maze, create_reused_maze),
        ),
        BenchmarkCase(
            "setup/paired-maze",
            partial(_measure_paired_maze_setup, create_reused_maze),
        ),
        BenchmarkCase(
            "reset/random",
            partial(_measure_reset, reset_random),
        ),
        BenchmarkCase(
            "reset/fixed",
            partial(_measure_reset, reset_fixed),
        ),
        BenchmarkCase(
            "reset/reused-maze",
            partial(_measure_reset_with_maze, reset_reused_maze),
        ),
        BenchmarkCase(
            "observation/pair",
            partial(_measure_observation_pair, observation_pair),
        ),
        BenchmarkCase(
            "step/pyrat",
            partial(_measure_step, step),
        ),
        BenchmarkCase(
            "step/env",
            partial(_measure_environment_step, environment_step),
        ),
    ]


def _record_sample(case: BenchmarkCase, iterations: int) -> MeasuredBatch:
    elapsed_ns = 0
    operations = 0
    for _ in range(iterations):
        batch = case.measure_batch()
        elapsed_ns += batch.elapsed_ns
        operations += batch.operations
    return MeasuredBatch(elapsed_ns, operations)


def _run_case(
    case: BenchmarkCase,
    warmup_batches: int,
    sample_count: int,
    target_sample_ns: int,
    smoke: bool,
) -> CaseResult:
    discarded: list[MeasuredBatch] = []
    for _ in range(warmup_batches):
        discarded.append(case.measure_batch())

    first_batch = discarded[-1]
    if first_batch.operations <= 0 or first_batch.elapsed_ns <= 0:
        raise RuntimeError(f"Benchmark case {case.name} returned an empty timed batch")

    if smoke:
        iterations = 1
    else:
        typical_batch_ns = statistics.median(batch.elapsed_ns for batch in discarded)
        iterations = max(
            1,
            math.ceil(target_sample_ns / typical_batch_ns),
        )
        iterations = min(iterations, MAX_ADAPTIVE_ITERATIONS)

    samples = tuple(_record_sample(case, iterations) for _ in range(sample_count))
    return CaseResult(
        name=case.name,
        operations_per_batch=first_batch.operations,
        iterations_per_sample=iterations,
        discarded_batches=len(discarded),
        samples=samples,
    )


def _run_git(repo_root: Path, arguments: Sequence[str]) -> str | None:
    try:
        completed = subprocess.run(
            ["git", *arguments],
            cwd=str(repo_root),
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            check=False,
        )
    except OSError:
        return None
    if completed.returncode != 0:
        return None
    return completed.stdout.strip()


def _metadata(
    repo_root: Path,
    args: argparse.Namespace,
    warmup_batches: int,
    sample_count: int,
    target_sample_ns: int,
) -> dict[str, Any]:
    clock = time.get_clock_info("perf_counter")
    git_status = _run_git(repo_root, ["status", "--porcelain"])
    return {
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "scenario": SCENARIO_NAME,
        "mode": "smoke" if args.smoke else "benchmark",
        "timing_scope": (
            "operation-only timed blocks; fixture setup, stateful resets, "
            "and validation are excluded"
        ),
        "configuration": {
            "warmup_batches": warmup_batches,
            "sample_count": sample_count,
            "target_sample_ns": target_sample_ns,
            "selected_cases": args.case or list(CASE_NAMES),
        },
        "git": {
            "commit": _run_git(repo_root, ["rev-parse", "HEAD"]),
            "dirty": None if git_status is None else bool(git_status),
        },
        "engine": {
            "version": pyrat_engine.__version__,
            "extension_path": getattr(engine_core, "__file__", None),
            "release_build_required": True,
        },
        "python": {
            "implementation": platform.python_implementation(),
            "version": platform.python_version(),
            "executable": sys.executable,
        },
        "host": {
            "hostname": socket.gethostname(),
            "platform": platform.platform(),
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "processor": platform.processor(),
            "cpu_count": os.cpu_count(),
        },
        "clock": {
            "name": "perf_counter",
            "implementation": clock.implementation,
            "monotonic": clock.monotonic,
            "adjustable": clock.adjustable,
            "resolution_seconds": clock.resolution,
        },
        "command": [sys.executable, *sys.argv],
    }


def _build_report(
    results: Sequence[CaseResult],
    metadata: dict[str, Any],
) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "metadata": metadata,
        "cases": [result.as_json() for result in results],
    }


def _format_duration(ns_per_operation: float) -> str:
    if ns_per_operation >= NANOSECONDS_PER_MILLISECOND:
        return f"{ns_per_operation / NANOSECONDS_PER_MILLISECOND:.3f} ms"
    if ns_per_operation >= NANOSECONDS_PER_MICROSECOND:
        return f"{ns_per_operation / NANOSECONDS_PER_MICROSECOND:.3f} us"
    return f"{ns_per_operation:.1f} ns"


def _format_rate(operations_per_second: float) -> str:
    if operations_per_second >= OPERATIONS_PER_GIGA:
        return f"{operations_per_second / OPERATIONS_PER_GIGA:.2f}G/s"
    if operations_per_second >= OPERATIONS_PER_MEGA:
        return f"{operations_per_second / OPERATIONS_PER_MEGA:.2f}M/s"
    if operations_per_second >= OPERATIONS_PER_KILO:
        return f"{operations_per_second / OPERATIONS_PER_KILO:.2f}K/s"
    return f"{operations_per_second:.2f}/s"


def _print_human(
    results: Sequence[CaseResult],
    metadata: dict[str, Any],
) -> None:
    git = metadata["git"]
    python = metadata["python"]
    host = metadata["host"]
    print("PyRat Python boundary benchmark")
    print(
        "Release extension required; build with "
        "`uv run --directory engine maturin develop --release`."
    )
    print(
        f"commit={git['commit']} dirty={git['dirty']}  "
        f"python={python['implementation']} {python['version']}  "
        f"host={host['machine']} {host['system']}"
    )
    print()
    print(
        f"{'case':<22} {'median':>12} {'ops/s':>12} "
        f"{'min':>12} {'max':>12} {'samples':>8} {'ops/sample':>12}"
    )
    print("-" * 96)
    for result in results:
        summary = result.summary()
        operations_per_sample = result.samples[0].operations
        print(
            f"{result.name:<22} "
            f"{_format_duration(summary['median_ns_per_operation']):>12} "
            f"{_format_rate(summary['median_operations_per_second']):>12} "
            f"{_format_duration(summary['min_ns_per_operation']):>12} "
            f"{_format_duration(summary['max_ns_per_operation']):>12} "
            f"{len(result.samples):>8} "
            f"{operations_per_sample:>12}"
        )


def _write_json(path_text: str, report: dict[str, Any]) -> None:
    path = Path(path_text)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as output:
        json.dump(report, output, indent=2, sort_keys=True)
        output.write("\n")


def _positive_int(value: str) -> int:
    try:
        parsed = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("expected an integer") from error
    if parsed <= 0:
        raise argparse.ArgumentTypeError("expected a positive integer")
    return parsed


def _positive_float(value: str) -> float:
    try:
        parsed = float(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("expected a number") from error
    if parsed <= 0.0:
        raise argparse.ArgumentTypeError("expected a positive number")
    return parsed


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Benchmark public PyRat Python calls without adding a timing "
            "dependency or a performance threshold."
        )
    )
    parser.add_argument(
        "--case",
        action="append",
        choices=CASE_NAMES,
        help="run only this case; repeat to select multiple cases",
    )
    parser.add_argument(
        "--warmup",
        type=_positive_int,
        default=3,
        help="discarded warmup batches per case (default: 3)",
    )
    parser.add_argument(
        "--samples",
        type=_positive_int,
        default=15,
        help="recorded samples per case (default: 15)",
    )
    parser.add_argument(
        "--sample-time-ms",
        type=_positive_float,
        default=50.0,
        help="adaptive operation-only time per sample (default: 50)",
    )
    parser.add_argument(
        "--json",
        metavar="PATH",
        help="write JSON alongside human output; use '-' for JSON-only stdout",
    )
    parser.add_argument(
        "--smoke",
        action="store_true",
        help="run one warmup and one single-batch sample with no speed assertion",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    cases = _build_cases()
    selected_names = set(args.case or CASE_NAMES)
    selected_cases = [case for case in cases if case.name in selected_names]

    warmup_batches = 1 if args.smoke else args.warmup
    sample_count = 1 if args.smoke else args.samples
    target_sample_ns = int(args.sample_time_ms * NANOSECONDS_PER_MILLISECOND)
    results = [
        _run_case(
            case,
            warmup_batches,
            sample_count,
            target_sample_ns,
            args.smoke,
        )
        for case in selected_cases
    ]

    repo_root = Path(__file__).resolve().parents[3]
    metadata = _metadata(
        repo_root,
        args,
        warmup_batches,
        sample_count,
        target_sample_ns,
    )
    report = _build_report(results, metadata)

    if args.json == "-":
        print(json.dumps(report, indent=2, sort_keys=True))
        return 0

    _print_human(results, metadata)
    if args.json:
        _write_json(args.json, report)
        print()
        print(f"JSON report: {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
