#!/usr/bin/env python3
"""Run one command and emit machine-local timing data as JSON."""

import json
import resource
import subprocess
import sys
import time


TIMER_SCHEMA = "jet.gauntlet.timer.v2"
MONOTONIC_INFO = time.get_clock_info("monotonic")
CLOCK = {
    "source": "time.monotonic_ns",
    "implementation": MONOTONIC_INFO.implementation,
    "resolution_ns": max(1, round(MONOTONIC_INFO.resolution * 1_000_000_000)),
    "monotonic": MONOTONIC_INFO.monotonic,
    "adjustable": MONOTONIC_INFO.adjustable,
}


def elapsed_ns(started_ns, ended_ns):
    return max(0, ended_ns - started_ns)


def seconds(value_ns):
    return None if value_ns is None else value_ns / 1_000_000_000


def emit_sample(started_ns, first_stdout_ns, exit_code, steps=None, error=None):
    ended_ns = time.monotonic_ns()
    wall_ns = elapsed_ns(started_ns, ended_ns)
    first_ns = None if first_stdout_ns is None else elapsed_ns(started_ns, first_stdout_ns)
    sample = {
        "schema": TIMER_SCHEMA,
        "clock": CLOCK,
        "wall_time_ns": wall_ns,
        "wall_seconds": seconds(wall_ns),
        "peak_rss_kb": resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss,
        "exit_code": exit_code,
        "time_to_first_stdout_ns": first_ns,
        "time_to_first_stdout_seconds": seconds(first_ns),
    }
    if steps is not None:
        sample["steps"] = steps
    if error is not None:
        sample["error"] = error
    print(json.dumps(sample, separators=(",", ":")))


def run_child(command):
    child_started_ns = time.monotonic_ns()
    child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=None)
    first = child.stdout.read(1)
    first_stdout_ns = time.monotonic_ns() if first else None
    while child.stdout.read(65536):
        pass
    child_code = child.wait()
    child_ended_ns = time.monotonic_ns()
    child_wall_ns = elapsed_ns(child_started_ns, child_ended_ns)
    child_first_ns = None if first_stdout_ns is None else elapsed_ns(child_started_ns, first_stdout_ns)
    return child_code, {
        "wall_time_ns": child_wall_ns,
        "wall_seconds": seconds(child_wall_ns),
        "exit_code": child_code,
        "time_to_first_stdout_ns": child_first_ns,
        "time_to_first_stdout_seconds": seconds(child_first_ns),
    }, first_stdout_ns


def run_sequence(encoded: str) -> int:
    try:
        commands = json.loads(encoded)
    except json.JSONDecodeError as error:
        print(json.dumps({"error": f"invalid sequence JSON: {error}"}))
        return 64
    if not isinstance(commands, list) or not commands or any(not isinstance(command, list) or not command for command in commands):
        print(json.dumps({"error": "sequence must be a non-empty list of commands"}))
        return 64

    started_ns = time.monotonic_ns()
    first_stdout_ns = None
    exit_code = 0
    steps = []
    for command in commands:
        try:
            child_code, step, child_first_stdout_ns = run_child(command)
        except OSError as error:
            emit_sample(started_ns, first_stdout_ns, 127, steps, str(error))
            return 0
        if child_first_stdout_ns is not None and first_stdout_ns is None:
            first_stdout_ns = child_first_stdout_ns
        steps.append(step)
        if child_code != 0 and exit_code == 0:
            exit_code = child_code
    emit_sample(started_ns, first_stdout_ns, exit_code, steps)
    return 0


def main() -> int:
    if "--sequence-json" in sys.argv:
        index = sys.argv.index("--sequence-json")
        if index + 1 >= len(sys.argv):
            print(json.dumps({"error": "sequence-json requires a value"}))
            return 64
        return run_sequence(sys.argv[index + 1])

    try:
        separator = sys.argv.index("--")
    except ValueError:
        print(json.dumps({"error": "timer requires -- before command"}))
        return 64

    command = sys.argv[separator + 1 :]
    if not command:
        print(json.dumps({"error": "timer requires a command"}))
        return 64

    started_ns = time.monotonic_ns()
    try:
        child_code, _, first_stdout_ns = run_child(command)
        emit_sample(started_ns, first_stdout_ns, child_code)
    except OSError as error:
        emit_sample(started_ns, None, 127, error=str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
