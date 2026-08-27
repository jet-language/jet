#!/usr/bin/env python3
"""Run one command and emit machine-local timing data as JSON."""

import json
import os
import resource
import subprocess
import sys
import time


def emit_sample(started, first_stdout, exit_code):
    print(json.dumps({
        "wall_seconds": time.monotonic() - started,
        "peak_rss_kb": resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss,
        "exit_code": exit_code,
        "time_to_first_stdout_seconds": first_stdout,
    }, separators=(",", ":")))


def run_sequence(encoded: str) -> int:
    try:
        commands = json.loads(encoded)
    except json.JSONDecodeError as error:
        print(json.dumps({"error": f"invalid sequence JSON: {error}"}))
        return 64
    if not isinstance(commands, list) or not commands or any(not isinstance(command, list) or not command for command in commands):
        print(json.dumps({"error": "sequence must be a non-empty list of commands"}))
        return 64

    started = time.monotonic()
    first_stdout = None
    exit_code = 0
    for command in commands:
        try:
            child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=None)
            first = child.stdout.read(1)
            if first and first_stdout is None:
                first_stdout = time.monotonic() - started
            while child.stdout.read(65536):
                pass
            child_code = child.wait()
        except OSError as error:
            print(json.dumps({
                "wall_seconds": time.monotonic() - started,
                "peak_rss_kb": resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss,
                "exit_code": 127,
                "time_to_first_stdout_seconds": first_stdout,
                "error": str(error),
            }))
            return 0
        if child_code != 0 and exit_code == 0:
            exit_code = child_code
    emit_sample(started, first_stdout, exit_code)
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

    started = time.monotonic()
    first_stdout = None
    exit_code = 127
    try:
        child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=None)
        first = child.stdout.read(1)
        if first:
            first_stdout = time.monotonic() - started
        while child.stdout.read(65536):
            pass
        exit_code = child.wait()
    except OSError as error:
        print(json.dumps({
            "wall_seconds": time.monotonic() - started,
            "peak_rss_kb": resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss,
            "exit_code": exit_code,
            "time_to_first_stdout_seconds": first_stdout,
            "error": str(error),
        }))
        return 0

    emit_sample(started, first_stdout, exit_code)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
