#!/usr/bin/env python3
"""The probes of Docs/Audits/2026-11-18-best-debugger-profiler.md, in order.

    python3 run.py <jet> <repo>            the probes, written to run.log
    python3 run.py <jet> <repo> latency    the first-stop bar on a 120,004-line
                                           image, written to latency.log

Every command runs with PATH set to an empty directory, so nothing but the
given `jet` can be reached: no lldb, gdb, Linux perf, go, pprof, otelcol or
rr (#6841 c34). Each runs at nice 19 on CPUs 8-15 with `NO_COLOR=1`, in a
temporary directory that holds copies of the probe programs. Machine paths
are removed from the output. Only the standard library is used.
"""
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

jet = os.path.abspath(sys.argv[1])
repo = os.path.abspath(sys.argv[2])
mode = sys.argv[3] if len(sys.argv) > 3 else "probes"
here = os.path.dirname(os.path.abspath(__file__))
scratch = tempfile.mkdtemp(prefix="jet-best-")
empty = os.path.join(scratch, "empty-path")
os.mkdir(empty)
env = {key: value for key, value in os.environ.items() if key != "JET_OBSERVE" and not key.startswith("JET_DEVTOOLS_RELAY_")}
env.update(NO_COLOR="1", JET_COMPILER_IDENTITY="jet@0.1.0#probe", JET_TOOLCHAIN_ROOT=repo, PATH=empty)
wrap = [shutil.which("taskset"), "-c", "8-15", shutil.which("nice"), "-n", "19", shutil.which("timeout")]
OTHER_TOOLS = ["lldb", "gdb", "perf", "go", "pprof", "otelcol", "rr"]


def scrub(text):
    for path, name in ((scratch, "<scratch>"), (repo, "<repo>"), (jet, "jet"), (os.path.expanduser("~"), "~")):
        text = text.replace(path, name)
    return text


def run(argv, cwd=scratch, stdin=None, limit=600, show=True):
    shown = " ".join("jet" if word == jet else scrub(word) for word in argv)
    started = time.monotonic()
    done = subprocess.run(wrap + [str(limit)] + argv, cwd=cwd, env=env, input=stdin, capture_output=True, text=True)
    seconds = time.monotonic() - started
    if show:
        print("$ " + shown + ("  <<< " + repr(stdin) if stdin else ""))
        text = scrub(done.stdout + done.stderr).rstrip("\n")
        if text:
            print(text)
        print("(exit {})".format(done.returncode))
        print()
    return done.returncode, done.stdout, done.stderr, seconds


def heading(text):
    print("== " + text)
    print()


def trace_of(out, err):
    found = re.search(r"Trace: (\S+\.jettrace)", out + err)
    return found.group(1) if found else None


def content(trace):
    _, out, _, _ = run([jet, "perf", "view", "--json", trace], show=False)
    return json.loads(out)["perf"]["trace"]["content"]


def sampling(name, n):
    rc, out, err, _ = run([jet, "perf", "run", "-O2", name, "--", str(n)])
    trace = trace_of(out, err)
    if rc != 0 or trace is None:
        return None
    profile = content(trace)["profile"]
    cpu_ms = profile["window_cpu_ns"] / 1e6
    outside = re.search(r"(\d+) of (\d+) samples are outside", profile["reason"])
    kept = int(outside.group(2)) if outside else sum(row["sample_count"] for row in profile["rows"])
    lost = int(outside.group(1)) if outside else 0
    print("process CPU {:.1f} ms, so {:.0f} samples at the stated 1 kHz; kept {} ({:.0f}%); outside {}'s functions: {} ({:.0f}% of kept)".format(
        cpu_ms, cpu_ms, kept, 100 * kept / cpu_ms, name, lost, 100 * lost / max(kept, 1)))
    print("reason: " + profile["reason"])
    print("overhead_status: {}; overhead_reason: {}; coverage: {}".format(profile["overhead_status"], profile["overhead_reason"], profile["coverage"]))
    for row in profile["rows"]:
        print("  {}#{} line {}: samples {}, inclusive {}".format(row["symbol"]["path"], row["symbol"]["name"], row["source"]["start_line"], row["sample_count"], row["inclusive_count"]))
    print()
    return trace


def descendants(pid):
    found = []
    for task in os.listdir("/proc/{}/task".format(pid)):
        try:
            with open("/proc/{}/task/{}/children".format(pid, task)) as handle:
                for child in handle.read().split():
                    found.append(int(child))
                    found.extend(descendants(int(child)))
        except OSError:
            pass
    return found


def observed(pid):
    try:
        with open("/proc/{}/environ".format(pid), "rb") as handle:
            return b"JET_OBSERVE=1" in handle.read().split(b"\0")
    except OSError:
        return False


def observed_hang():
    shown = "jet run --observe deadlock.jet"
    print("$ " + shown + "  (in the background)")
    process = subprocess.Popen(wrap + ["120", jet, "run", "--observe", "deadlock.jet"], cwd=scratch, env=env,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, start_new_session=True)
    target = None
    deadline = time.monotonic() + 90
    while target is None and time.monotonic() < deadline and process.poll() is None:
        time.sleep(0.5)
        for pid in descendants(process.pid):
            if observed(pid):
                target = pid
    if target is None:
        print("no process with JET_OBSERVE=1 appeared")
    else:
        time.sleep(2)
        try:
            name = os.path.basename(os.readlink("/proc/{}/exe".format(target)))
        except OSError:
            name = "?"
        print("(the observed process, `{}`, found; it has run 2 s)".format(name))
        print()
        run([jet, "inspect", "live", "--attach", str(target), "--once"])
    os.killpg(process.pid, signal.SIGKILL)
    process.wait()
    print()


def probes():
    heading("Tools: nothing but jet")
    print("on the probes' PATH: " + ", ".join("{} {}".format(tool, "present" if shutil.which(tool, path=empty) else "absent") for tool in OTHER_TOOLS))
    print("on the host's PATH: " + ", ".join("{} {}".format(tool, "present" if shutil.which(tool) else "absent") for tool in OTHER_TOOLS))
    print()
    for name in ("fib.jet", "fib4.jet", "deadlock.jet", "alloc.jet"):
        shutil.copy(os.path.join(here, name), scratch)

    heading("F1, D8: a two-task wait cycle")
    run([jet, "run", "deadlock.jet"], limit=20)
    observed_hang()

    heading("F2-F5, P1: the sampler on one busy thread and on four")
    one = sampling("fib.jet", 34)
    sampling("fib4.jet", 32)

    heading("P8: compare two captures of different work")
    run([jet, "perf", "run", "-O2", "fib.jet", "--", "30"], show=False)
    traces = sorted(os.listdir(os.path.join(scratch, ".jet", "perf")))
    small = [os.path.join(scratch, ".jet", "perf", t) for t in traces if os.path.join(scratch, ".jet", "perf", t) != one]
    if one and small:
        run([jet, "perf", "compare", small[-1], one])

    heading("c11: the HTML view needs nothing but a browser")
    if one:
        rc, out, _, _ = run([jet, "perf", "view", "--html", one], show=False)
        print("$ jet perf view --html <fib trace>")
        print("(exit {}) {} bytes; starts {!r}; external references: {}".format(
            rc, len(out), out[:15], sorted(set(re.findall(r"(?:src|href)=\"(https?://[^\"]+)", out)))))
        print()

    heading("P4: allocations")
    rc, out, err, _ = run([jet, "perf", "run", "alloc.jet", "--", "2000"])
    trace = trace_of(out, err)
    if trace:
        trace_content = content(trace)
        print("allocation rows: {}".format(len(trace_content["allocations"])))
        for row in trace_content["allocations"][:6]:
            print("  " + json.dumps(row, sort_keys=True))
        print()

    heading("P5: hardware counters")
    run([jet, "perf", "run", "--lens=memory", "fib.jet", "--", "30"])

    heading("F6-F8, D1, D4: jet debug's console")
    run([jet, "debug", "--break", "fib.jet:6", "fib.jet", "--", "20"], stdin="p n\np n + 1\nwatch n\nbt\nq\n")
    _, _, _, seconds = run([jet, "debug", "--break", "fib.jet:6", "fib.jet"], stdin="c\n" * 8 + "q\n", show=False)
    print("$ jet debug --break fib.jet:6 fib.jet  <<< 8 x 'c', then 'q'")
    print("wall {:.0f} ms, its warm O0 build included".format(seconds * 1000))
    print()
    run([jet, "debug", "--break", "fib.jet:6", "fib.jet"], stdin="help\nq\n")

    heading("F9, D3: an O2 image")
    run([jet, "debug", "-O2", "--break", "fib.jet:6", "fib.jet"], stdin="q\n")
    run([jet, "build", "-O2", "fib.jet"])
    run([jet, "debug", "--break", "fib.jet:6", ".jet/build/fib"], stdin="locals\nbt\nq\n")

    heading("F10, D2: record and replay")
    run([jet, "debug", "--record=r1", "fib.jet"], stdin="q\n")
    run([jet, "run", "--record=r1", "fib.jet", "--", "10"])


def latency():
    heading("D1: the first stop on a prebuilt 120,004-line image (R11's bar)")
    run([jet, "run", os.path.join(repo, "Tools/Perf/DebugLatency/Main.jet"), "--", "--jet", jet,
         "--functions", "20000", "--runs", "3", "--bar", "200"], cwd=repo, limit=1800)


try:
    latency() if mode == "latency" else probes()
finally:
    shutil.rmtree(scratch, ignore_errors=True)
