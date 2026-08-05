#!/usr/bin/env python3
"""Measure Stream creation, pull, memory, and early close against Python.

Run from the repository root after building the local Jet binary:

    python3 tools/perf/stream_bench.py

The output is JSON. Timings are host-specific; the workload and metric names
are fixed so repeated runs can be compared. Set
JET_STREAM_BENCH_COMMAND to override the Jet command, for example:

    JET_STREAM_BENCH_COMMAND='scripts/agent/jet-env target/debug/jet' \
        python3 tools/perf/stream_bench.py
"""

from __future__ import annotations

import json
import os
import resource
import shlex
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SIZES = (1, 1_000, 10_000)
CASES = ("creation", "pull", "early_close")
ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(__file__).resolve()


def expected_stdout(case: str, size: int) -> str:
    if case in ("creation", "early_close"):
        return "ok"
    if case == "pull":
        return str(size)
    raise ValueError(f"unknown case: {case}")


def peak_kib(value: int) -> int:
    """Normalize ru_maxrss to KiB on Linux and macOS."""
    return value // 1024 if sys.platform == "darwin" else value


def jet_source(case: str, size: int) -> str:
    if case == "creation":
        producer = """fn empty() => Stream<Int> {
}"""
        body = """loop value, empty() {
            break
        }"""
        expected = expected_stdout(case, size)
    elif case == "pull":
        producer = """fn one() => Stream<Int> {
    yield 1
}"""
        body = """loop value, one() {
            total += value
        }"""
        expected = expected_stdout(case, size)
    elif case == "early_close":
        producer = """fn many() => Stream<Int> {
    yield 1
    yield 2
}"""
        body = """loop value, many() {
            break
        }"""
        expected = expected_stdout(case, size)
    else:
        raise ValueError(f"unknown case: {case}")
    return f'''{producer}

fn run() {{
    total := 0
    i := 0
    loop i < {size} {{
        {body}
        i += 1
    }}
    print("{expected}")
}}
'''


def python_work(case: str, size: int) -> None:
    def empty():
        if False:
            yield 0

    def one():
        yield 1

    def many():
        yield 1
        yield 2

    if case == "creation":
        for _ in range(size):
            for _value in empty():
                break
    elif case == "pull":
        total = 0
        for _ in range(size):
            for value in one():
                total += value
        assert total == size
    elif case == "early_close":
        for _ in range(size):
            for _value in many():
                break
    else:
        raise ValueError(f"unknown case: {case}")


def python_worker(case: str, size: int) -> None:
    start = time.perf_counter_ns()
    python_work(case, size)
    elapsed = time.perf_counter_ns() - start
    print(
        json.dumps(
            {
                "wall_ns": elapsed,
                "peak_kib": peak_kib(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss),
            }
        )
    )


def jet_command() -> list[str]:
    configured = os.environ.get("JET_STREAM_BENCH_COMMAND")
    return shlex.split(configured) if configured else ["scripts/agent/jet-env", "target/debug/jet"]


def jet_worker(case: str, size: int, source: Path) -> None:
    start = time.perf_counter_ns()
    completed = subprocess.run(
        [*jet_command(), "run", str(source)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    elapsed = time.perf_counter_ns() - start
    if completed.returncode != 0:
        raise SystemExit(
            f"Jet benchmark failed for {case}/{size}:\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    actual = completed.stdout.strip()
    expected = expected_stdout(case, size)
    if actual != expected:
        raise SystemExit(
            f"Jet benchmark output failed for {case}/{size}: "
            f"expected {expected!r}, got {actual!r}"
        )
    print(
        json.dumps(
            {
                "wall_ns": elapsed,
                # `measure` starts one fresh worker for each case/size, so
                # RUSAGE_CHILDREN is the peak of this one Jet command rather
                # than a cumulative high-water mark from earlier rows.
                "peak_kib": peak_kib(resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss),
                "stdout": actual,
                "expected_stdout": expected,
            }
        )
    )


def run_worker(args: list[str]) -> bool:
    if args[:1] == ["--python-worker"]:
        python_worker(args[1], int(args[2]))
        return True
    if args[:1] == ["--jet-worker"]:
        jet_worker(args[1], int(args[2]), Path(args[3]))
        return True
    return False


def measure(worker: list[str]) -> dict[str, int | str]:
    completed = subprocess.run(
        [sys.executable, str(SCRIPT), *worker],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise SystemExit(completed.stderr or completed.stdout)
    return json.loads(completed.stdout)


def main() -> None:
    if run_worker(sys.argv[1:]):
        return
    results = []
    with tempfile.TemporaryDirectory(prefix="jet-stream-bench-") as temp:
        temp_dir = Path(temp)
        for size in SIZES:
            for case in CASES:
                source = temp_dir / f"{case}-{size}.jet"
                source.write_text(jet_source(case, size), encoding="utf-8")
                python = measure(["--python-worker", case, str(size)])
                jet = measure(["--jet-worker", case, str(size), str(source)])
                results.append({"case": case, "n": size, "python": python, "jet": jet})
    print(
        json.dumps(
            {"benchmark": "stream-pull-v1", "results": results},
            indent=2,
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
