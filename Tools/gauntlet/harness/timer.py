#!/usr/bin/env python3
"""Run one command and emit machine-local timing data as JSON.

The original of gauntlet tool entry timer; its Jet twin is Timer.jet beside it.

Each command runs through launch.c beside this file, so its times and peak
memory are its own, not the timer's: the launcher's clock brackets just the
command (exec to reap, and to its first stdout byte, which it relays), and
its peak is each process's exact VmHWM, which getrusage here can't give
(Linux carries this python's high-water mark into its children's
ru_maxrss). JET_GAUNTLET_LAUNCHER names a launcher already built from
launch.c; without it the timer builds one with `cc`.
"""

import json
import os
import subprocess
import sys
import tempfile


TIMER_SCHEMA = "jet.gauntlet.timer.v2"
LAUNCHER_SOURCE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "launch.c")
# The launcher reads every time.
CLOCK = {
    "source": "launch.c",
    "implementation": "clock_gettime(CLOCK_MONOTONIC)",
    "resolution_ns": 1,
    "monotonic": True,
    "adjustable": False,
}


class Refusal(Exception):
    """The timer can't time anything: printed as `{"error": …}`, exit 64."""


def seconds(value_ns):
    return None if value_ns is None else value_ns / 1_000_000_000


def times(wall_ns, first_ns, exit_code):
    return {
        "wall_time_ns": wall_ns,
        "wall_seconds": seconds(wall_ns),
        "exit_code": exit_code,
        "time_to_first_stdout_ns": first_ns,
        "time_to_first_stdout_seconds": seconds(first_ns),
    }


def emit_sample(wall_ns, first_ns, peak_kb, exit_code, steps=None, error=None):
    sample = {"schema": TIMER_SCHEMA, "clock": CLOCK, **times(wall_ns, first_ns, exit_code), "peak_rss_kb": peak_kb}
    if steps is not None:
        sample["steps"] = steps
    if error is not None:
        sample["error"] = error
    print(json.dumps(sample, separators=(",", ":")))


def launcher(scratch):
    built = os.environ.get("JET_GAUNTLET_LAUNCHER")
    if built:
        return built
    path = os.path.join(scratch, "launch")
    result = subprocess.run(["cc", "-O2", "-o", path, LAUNCHER_SOURCE])
    if result.returncode != 0:
        raise Refusal(f"cannot build the launcher from {LAUNCHER_SOURCE}: cc exited {result.returncode}")
    return path


def run_child(launch, report, command):
    """Runs `command` through the launcher with its stdout drained through a
    pipe, stdin and stderr inherited. Returns its times, its peak KiB and its
    exec errno (0 when it ran)."""
    child = subprocess.Popen([launch, report, *command], stdout=subprocess.PIPE, stderr=None)
    while child.stdout.read(65536):
        pass
    launcher_code = child.wait()
    with open(report) as lines:
        fields = {line.split()[0]: line.split()[1:] for line in lines if line.strip()}
    if len(fields.get("end", [])) != 7:
        raise Refusal(f"the launcher exited {launcher_code} without its report")
    code, wall_ns, first_ns, peak_kb, _, _, exec_errno = (int(field) for field in fields["end"])
    return times(wall_ns, None if first_ns < 0 else first_ns, code), peak_kb, exec_errno


def run_commands(commands, sequence):
    """Times `commands` one after another as one sample: its wall time is the
    sum of theirs, its first stdout byte the first one's (after the commands
    before it), its peak the largest and its exit code the first nonzero."""
    with tempfile.TemporaryDirectory(prefix="jet-timer-") as scratch:
        launch = launcher(scratch)
        report = os.path.join(scratch, "report")
        wall_ns = 0
        first_ns = None
        peak_kb = 0
        exit_code = 0
        steps = []
        for command in commands:
            step, step_peak_kb, exec_errno = run_child(launch, report, command)
            if first_ns is None and step["time_to_first_stdout_ns"] is not None:
                first_ns = wall_ns + step["time_to_first_stdout_ns"]
            wall_ns += step["wall_time_ns"]
            peak_kb = max(peak_kb, step_peak_kb)
            if exec_errno != 0:
                error = f"cannot run `{command[0]}`: exec failed (errno {exec_errno})"
                emit_sample(wall_ns, first_ns, peak_kb, 127, steps if sequence else None, error)
                return
            steps.append(step)
            if step["exit_code"] != 0 and exit_code == 0:
                exit_code = step["exit_code"]
        emit_sample(wall_ns, first_ns, peak_kb, exit_code, steps if sequence else None)


def commands_of(argv):
    if "--sequence-json" in argv:
        index = argv.index("--sequence-json")
        if index + 1 >= len(argv):
            raise Refusal("sequence-json requires a value")
        try:
            commands = json.loads(argv[index + 1])
        except json.JSONDecodeError as error:
            raise Refusal(f"invalid sequence JSON: {error}")
        if not isinstance(commands, list) or not commands or any(not isinstance(command, list) or not command for command in commands):
            raise Refusal("sequence must be a non-empty list of commands")
        return commands, True
    try:
        separator = argv.index("--")
    except ValueError:
        raise Refusal("timer requires -- before command")
    command = argv[separator + 1 :]
    if not command:
        raise Refusal("timer requires a command")
    return [command], False


def main() -> int:
    try:
        commands, sequence = commands_of(sys.argv)
        run_commands(commands, sequence)
    except Refusal as refusal:
        print(json.dumps({"error": str(refusal)}))
        return 64
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
