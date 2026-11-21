#!/usr/bin/env python3
"""The probes of Docs/Audits/2026-11-19-debug-profile-best-in-existence.md
(revision 5), one capability axis at a time.

    python3 run.py JET REPO [--lldb PATH] [--gdb PATH] [--perf PATH] [AXIS ...]

JET is the compiler under review and REPO the checkout it was built from.
The probes run the Tests/Cases fixtures and stdin files of REPO as they
are, the programs beside this file, the 2026-11-18 review's probes and
Tools/Perf/DebugLatency. AXIS is a name from AXES at the end (default:
every one, in order); the axes d2, d3, d4, d6 and generic are Run.jet's,
beside this file. --lldb, --gdb and --perf name the peers a comparison
runs on the same image and host; without one, that comparison says it was
skipped.

Every jet command runs with PATH set to an empty directory, so nothing but
the given jet can be reached, in a temporary directory that holds copies of
the fixtures; peers run with the host's PATH. Machine paths are removed
from the output, and an output longer than 80 lines keeps its first 60 and
last 20. Each command's line gives its exit, its wall time, the CPU time it
and its children used and the host's load average when it started, so a
reader can judge the noise. A console session is driven one command at a
time, and each command's time is from sending it to the next `(Jet) `
prompt. Only the standard library is used.
"""
import ctypes
import glob
import json
import os
import re
import resource
import select
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time

if len(sys.argv) < 3:
    raise SystemExit(__doc__)
JET = os.path.abspath(sys.argv[1])
REPO = os.path.abspath(sys.argv[2])
PEERS = {}
CHOSEN = []
_rest = sys.argv[3:]
while _rest:
    word = _rest.pop(0)
    if word in ("--lldb", "--gdb", "--perf"):
        PEERS[word[2:]] = os.path.abspath(_rest.pop(0))
    else:
        CHOSEN.append(word)

HERE = os.path.dirname(os.path.abspath(__file__))
EARLIER = os.path.join(REPO, "Docs", "Audits", "BestDebuggerProfiler20261118", "Probes")
CASES = os.path.join(REPO, "Tests", "Cases")
SCRATCH = tempfile.mkdtemp(prefix="jet-best5-")
EMPTY = os.path.join(SCRATCH, "no-tools")
os.mkdir(EMPTY)
ENV = {key: value for key, value in os.environ.items() if key != "JET_OBSERVE" and not key.startswith("JET_DEVTOOLS_RELAY_")}
ENV.update(NO_COLOR="1", JET_TOOLCHAIN_ROOT=REPO, PATH=EMPTY)
PEER_ENV = dict(ENV, PATH=os.environ.get("PATH", ""))
OTHER_TOOLS = ["lldb", "gdb", "perf", "rr", "go", "pprof", "otelcol"]
PROMPT = b"(Jet) "


def scrub(text):
    pairs = [(JET, "jet"), (SCRATCH, "<work>"), (REPO, "<repo>")]
    pairs += [(path, name) for name, path in PEERS.items()]
    pairs.append((os.path.expanduser("~"), "~"))
    for path, name in pairs:
        text = text.replace(path, name)
    return re.sub(r"<work>/[A-Za-z0-9]+-\d+", "<work>", text)


def shown(argv):
    return " ".join(shlex.quote(scrub(word)) for word in argv)


def load():
    with open("/proc/loadavg") as handle:
        return " ".join(handle.read().split()[:3])


def emit(text):
    lines = scrub(text).rstrip("\n").split("\n")
    if lines == [""]:
        return
    if len(lines) > 80:
        lines = lines[:60] + ["... ({} lines not shown) ...".format(len(lines) - 80)] + lines[-20:]
    print("\n".join(lines))


def heading(text):
    print("== " + text)
    print()


def note(text):
    print("# " + text)
    print()


_count = [0]


def fresh(name):
    _count[0] += 1
    path = os.path.join(SCRATCH, "{}-{}".format(name, _count[0]))
    os.mkdir(path)
    return path


def fixture(suite, name):
    """A copy of Tests/Cases/SUITE/NAME.fixture."""
    _count[0] += 1
    path = os.path.join(SCRATCH, "{}-{}".format(name, _count[0]))
    shutil.copytree(os.path.join(CASES, suite, name + ".fixture"), path)
    return path


def probes(*names):
    """A directory holding copies of this review's probe programs NAMES."""
    path = fresh("probes")
    for name in names:
        shutil.copy(os.path.join(HERE, name), path)
    return path


def stdin_lines(suite, case):
    with open(os.path.join(CASES, suite, case + ".stdin")) as handle:
        return handle.read().rstrip("\n").split("\n")


def cpu_seconds():
    used = resource.getrusage(resource.RUSAGE_CHILDREN)
    return used.ru_utime + used.ru_stime


def spawn(argv, cwd, peer=False, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, preexec=None):
    return subprocess.Popen(argv, cwd=cwd, env=PEER_ENV if peer else ENV, stdin=stdin, stdout=stdout, stderr=stderr,
                            start_new_session=True, preexec_fn=preexec)


def stop(process):
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass  # it ended on its own
    process.wait()


class Result:
    def __init__(self, rc, out, err, wall, cpu):
        self.rc, self.out, self.err, self.wall, self.cpu = rc, out, err, wall, cpu
        self.text = out + err


def run(argv, cwd, stdin=None, limit=600, show=True, peer=False):
    """Runs ARGV to its end and prints it, its output and its costs."""
    started_load = load()
    before = cpu_seconds()
    started = time.monotonic()
    process = spawn(argv, cwd, peer=peer, stderr=subprocess.PIPE)
    try:
        out, err = process.communicate(input=stdin.encode() if stdin is not None else b"", timeout=limit)
        rc = process.returncode
    except subprocess.TimeoutExpired:
        stop(process)
        out, err = process.communicate()
        rc = "killed after {} s".format(limit)
    wall = time.monotonic() - started
    result = Result(rc, out.decode(errors="replace"), err.decode(errors="replace"), wall, cpu_seconds() - before)
    if show:
        typed = "  <<< " + repr(stdin) if stdin else ""
        print("$ " + shown(argv) + typed)
        emit(result.text)
        print("(exit {}; wall {:.0f} ms, cpu {:.0f} ms, load {})".format(rc, wall * 1000, result.cpu * 1000, started_load))
        print()
    return result


def console(argv, cwd, commands, limit=120, peer=False):
    """Drives a `jet debug` console one command at a time, timing each."""
    started_load = load()
    print("$ " + shown(argv) + "  (one command at a time)")
    started = time.monotonic()
    process = spawn(argv, cwd, peer=peer)
    fd = process.stdout.fileno()
    os.set_blocking(fd, False)
    buffer = bytearray()
    ended = [False]

    def until_prompt(since):
        deadline = time.monotonic() + limit
        seen = None
        while True:
            wait = 0.02 if seen is not None else max(0.0, deadline - time.monotonic())
            ready, _, _ = select.select([fd], [], [], wait)
            if ready:
                chunk = os.read(fd, 1 << 16)
                if not chunk:
                    ended[0] = True
                    return seen
                buffer.extend(chunk)
                if seen is None and PROMPT in buffer[since:]:
                    seen = time.monotonic()
            elif seen is not None or time.monotonic() >= deadline:
                return seen

    timings = []
    first = until_prompt(0)
    if first is not None:
        timings.append(("(start to the first prompt)", first - started))
    for command in commands:
        if ended[0]:
            timings.append((command, None))
            continue
        mark = len(buffer)
        sent = time.monotonic()
        try:
            process.stdin.write((command + "\n").encode())
            process.stdin.flush()
        except BrokenPipeError:
            ended[0] = True
            timings.append((command, None))
            continue
        answered = until_prompt(mark)
        timings.append((command, None if answered is None else answered - sent))
    try:
        process.stdin.close()
    except BrokenPipeError:
        pass  # the session already ended
    os.set_blocking(fd, True)
    try:
        rest, _ = process.communicate(timeout=limit)
        buffer.extend(rest or b"")
        rc = process.returncode
    except subprocess.TimeoutExpired:
        stop(process)
        rc = "killed after {} s".format(limit)
    wall = time.monotonic() - started
    text = buffer.decode(errors="replace")
    emit(text)
    print("(exit {}; wall {:.0f} ms, load {})".format(rc, wall * 1000, started_load))
    for command, seconds in timings:
        print("  {:>9}  {}".format("-" if seconds is None else "{:.1f} ms".format(seconds * 1000), command))
    print()
    return text, timings


class Dap:
    """A Debug Adapter Protocol client over a child's stdio."""

    def __init__(self, argv, cwd):
        print("$ " + shown(argv) + "  (DAP, one request at a time)")
        self.process = spawn(argv, cwd, stderr=subprocess.DEVNULL)
        self.fd = self.process.stdout.fileno()
        os.set_blocking(self.fd, False)
        self.buffer = bytearray()
        self.seq = 0
        self.values = {}

    def messages(self, wait):
        found = []
        ready, _, _ = select.select([self.fd], [], [], wait)
        if ready:
            chunk = os.read(self.fd, 1 << 16)
            self.buffer.extend(chunk)
        while True:
            head = self.buffer.find(b"\r\n\r\n")
            if head < 0:
                return found
            length = int(re.search(rb"Content-Length: *(\d+)", bytes(self.buffer[:head])).group(1))
            if len(self.buffer) < head + 4 + length:
                return found
            body = bytes(self.buffer[head + 4:head + 4 + length])
            del self.buffer[:head + 4 + length]
            found.append(json.loads(body))

    def request(self, command, arguments=None, until=None, limit=60, show_body=False):
        self.seq += 1
        seq = self.seq
        data = json.dumps({"seq": seq, "type": "request", "command": command, "arguments": arguments or {}}).encode()
        sent = time.monotonic()
        self.process.stdin.write(b"Content-Length: %d\r\n\r\n" % len(data) + data)
        self.process.stdin.flush()
        response, events, answered = None, [], None
        while time.monotonic() < sent + limit:
            for message in self.messages(0.05):
                if message.get("type") == "response" and message.get("request_seq") == seq:
                    response = message
                    if until is None:
                        answered = time.monotonic()
                elif message.get("type") == "event":
                    events.append(message)
                    if until is not None and answered is None and message.get("event") in until:
                        answered = time.monotonic()
                elif message.get("type") == "response":
                    events.append(message)
            if answered is not None and (response is not None or until is not None):
                break
        took = "-" if answered is None else "{:.1f} ms".format((answered - sent) * 1000)
        state = "no answer" if response is None else ("ok" if response.get("success") else "refused: " + str(response.get("message")))
        print("  {:>9}  {} {} -> {}".format(took, command, scrub(json.dumps(arguments or {}))[:120], state))
        for event in events:
            if event.get("type") == "response":
                print("             late response to {}: {}".format(event.get("command"), "ok" if event.get("success") else event.get("message")))
                continue
            body = event.get("body", {})
            if event.get("event") == "output":
                print("             output: " + scrub(body.get("output", "")).rstrip("\n").replace("\n", "\n                     "))
            elif event.get("event") in ("stopped", "continued", "terminated", "exited", "initialized", "thread"):
                print("             event {} {}".format(event.get("event"), scrub(json.dumps(body))[:200]))
        if show_body and response is not None:
            print("             body " + scrub(json.dumps(response.get("body", {})))[:700])
        return response

    def close(self):
        try:
            self.process.stdin.close()
        except BrokenPipeError:
            pass  # the adapter already ended
        try:
            self.process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            stop(self.process)
        print("(adapter exit {})".format(self.process.returncode))
        print()


def observed(argv, cwd, ready, limit=60):
    """Starts ARGV and waits for READY on its output. The pid to attach to is
    the started process's own, as a case's `{pid:NAME}` is."""
    print("$ " + shown(argv) + "  (in the background until {!r})".format(ready))
    process = spawn(argv, cwd)
    seen = b""
    deadline = time.monotonic() + limit
    os.set_blocking(process.stdout.fileno(), False)
    while ready.encode() not in seen and time.monotonic() < deadline and process.poll() is None:
        chosen, _, _ = select.select([process.stdout], [], [], 0.1)
        if chosen:
            seen += os.read(process.stdout.fileno(), 1 << 16)
    emit(seen.decode(errors="replace"))
    found = ready.encode() in seen
    print("({})".format("ready" if found else "not ready after {} s".format(limit)))
    print()
    return process, process.pid if found else None


def any_ptracer():
    # PR_SET_PTRACER_ANY, so a debugger may attach under Yama scope 1, as
    # the attach cases' authorizer does.
    ctypes.CDLL(None).prctl(0x59616d61, ctypes.c_ulong(0xFFFFFFFFFFFFFFFF), 0, 0, 0)


def trace(path):
    with open(path) as handle:
        return json.load(handle)["content"]


def perf_receipt(content):
    for section in content.get("receipt_sections", []):
        if section.get("name") == "perf":
            return section["value"]
    return {}


def took_ns(text):
    found = re.search(r"took (\d+) ns", text)
    return int(found.group(1)) if found else None


def dumps(work):
    return sorted(glob.glob(os.path.join(work, ".jet", "dumps", "*.jetdump")), key=os.path.getmtime)


# The axes, in the review's order.

def tools():
    heading("Nothing but jet: the tools a probe can reach")
    print("on the probes' PATH: " + ", ".join("{} {}".format(tool, "present" if shutil.which(tool, path=EMPTY) else "absent") for tool in OTHER_TOOLS))
    print("on the host's PATH: " + ", ".join("{} {}".format(tool, "present" if shutil.which(tool, path=PEER_ENV["PATH"]) else "absent") for tool in OTHER_TOOLS))
    print("peers given for comparison: " + (", ".join(sorted(PEERS)) or "none"))
    with open("/proc/cpuinfo") as handle:
        model = re.search(r"model name\s*:\s*(.*)", handle.read())
    with open("/proc/sys/kernel/perf_event_paranoid") as handle:
        paranoid = handle.read().strip()
    print("host: {}, {} CPUs, perf_event_paranoid {}".format(model.group(1) if model else "?", os.cpu_count(), paranoid))
    print()
    run([JET, "--version"], SCRATCH)


def d1():
    heading("D1, D4, D5, D6: Tools/Perf/DebugLatency, jet debug against lldb on one 120,004-line image")
    work = fresh("latency")
    shutil.copy(os.path.join(REPO, "Tools", "Perf", "DebugLatency", "Main.jet"), work)
    argv = [JET, "run", "Main.jet", "--", "--jet", JET, "--functions", "20000", "--runs", "5"]
    if "lldb" in PEERS:
        argv += ["--lldb", PEERS["lldb"]]
    run(argv, work, limit=3600)


def d5():
    heading("D5: watches")
    for name, case in (("WatchStopsAtTheChangingWrite", "watch_stops_at_the_changing_write"),
                       ("WatchFollowsAReallocatedList", "watch_follows_a_reallocated_list")):
        note(case)
        console([JET, "debug", "prog.jet"], fixture("Debug", name), stdin_lines("Debug", case))


def d7():
    heading("D7: hot patch")
    note("fix_swaps_function_in_native_process (the program writes its edit over prog.jet at round 1)")
    console([JET, "debug", "prog.jet", "--break", "prog.jet:15"], fixture("Debug", "FixSwapsFunctionInNativeProcess"),
            stdin_lines("Debug", "fix_swaps_function_in_native_process"))
    note("transactional_fix_checks_before_install")
    console([JET, "debug", "prog.jet", "--break", "prog.jet:26"], fixture("Debug", "TransactionalFixChecksBeforeInstall"),
            stdin_lines("Debug", "transactional_fix_checks_before_install"))


def d8():
    heading("D8: tasks, deadlocks")
    work = fixture("Debug", "DeadlockDumpHoldsEveryTask")
    run([JET, "run", "cycle.jet"], work, limit=60)
    run([JET, "run", "--observe", "cycle.jet"], work, limit=60)
    found = dumps(work)
    if found:
        console([JET, "debug", found[-1]], work, stdin_lines("Debug", "deadlock_dump_holds_every_task"))
    note("the 2026-11-18 review's two-task cycle, which hung then")
    work = fresh("deadlock")
    shutil.copy(os.path.join(EARLIER, "deadlock.jet"), work)
    run([JET, "run", "deadlock.jet"], work, limit=60)
    note("tasks_lists_every_task")
    console([JET, "debug", "prog.jet"], fixture("Debug", "TasksListsEveryTask"), stdin_lines("Debug", "tasks_lists_every_task"))


def d9():
    heading("D9: typed post-mortem dumps")
    work = fixture("Debug", "CrashDumpOpensTyped")
    run([JET, "run", "--observe", "crash.jet"], work, limit=60)
    found = dumps(work)
    if found:
        with open(found[-1], "rb") as handle:
            data = handle.read()
        print("dump: {} bytes, mode {:o}, holds the Secret's text b'hunter2': {}".format(
            len(data), os.stat(found[-1]).st_mode & 0o777, b"hunter2" in data))
        print()
        console([JET, "debug", found[-1]], work, stdin_lines("Debug", "crash_dump_opens_typed"))
    note("crash_dump_keeps_its_budget: a program holding 48 MB of lists")
    work = fixture("Debug", "CrashDumpKeepsItsBudget")
    run([JET, "run", "--observe", "big.jet"], work, limit=120)
    found = dumps(work)
    if found:
        print("dump: {} bytes".format(os.path.getsize(found[-1])))
        print()
        console([JET, "debug", found[-1]], work, stdin_lines("Debug", "crash_dump_keeps_its_budget"))


def d10():
    heading("D10: visualizers")
    note("debug_view_renders_without_running_the_target")
    console([JET, "debug", "prog.jet"], fixture("Debug", "DebugViewRendersWithoutRunningTheTarget"),
            stdin_lines("Debug", "debug_view_renders_without_running_the_target"))
    note("debug_view_package_ring_buffer")
    console([JET, "debug", "app/main.jet"], fixture("Debug", "DebugViewPackageRingBuffer"), stdin_lines("Debug", "debug_view_package_ring_buffer"))


def d11():
    heading("D11: surfaces: DAP launch, pause, restart, attach, reverse")
    stops = ("stopped", "terminated", "exited")
    work = fixture("Debug", "DapCLIPausesARunningTargetThroughProductionWire")
    main = os.path.join(work, "main.jet")
    for delay in (0.0, 0.1):
        note("pause {:.0f} ms after configurationDone".format(delay * 1000))
        dap = Dap([JET, "debug", "--dap", main], work)
        dap.request("initialize", {"adapterID": "jet", "pathFormat": "path"})
        dap.request("launch", {"program": main, "stopOnEntry": False, "args": [], "env": {}}, until=("initialized",))
        dap.request("configurationDone")
        time.sleep(delay)
        dap.request("pause", until=("stopped",))
        threads = dap.request("threads", show_body=True)
        if threads and threads.get("body", {}).get("threads"):
            dap.request("stackTrace", {"threadId": threads["body"]["threads"][0]["id"]}, show_body=True)
        dap.request("disconnect")
        dap.close()

    work = fixture("Debug", "DapCLIRestartsFromAStop")
    main = os.path.join(work, "main.jet")
    dap = Dap([JET, "debug", "--dap", main], work)
    dap.request("initialize", {"adapterID": "jet", "pathFormat": "path"})
    dap.request("launch", {"program": main, "stopOnEntry": False, "args": []}, until=("initialized",))
    dap.request("setBreakpoints", {"source": {"path": main}, "breakpoints": [{"line": 8}]})
    dap.request("configurationDone", until=stops)
    frames = dap.request("stackTrace", {"threadId": 1}, show_body=True)
    frame = frames["body"]["stackFrames"][0]["id"] if frames and frames.get("body", {}).get("stackFrames") else 0
    scopes = dap.request("scopes", {"frameId": frame}, show_body=True)
    scope = scopes["body"]["scopes"][0]["variablesReference"] if scopes and scopes.get("body", {}).get("scopes") else 0
    dap.request("continue", {"threadId": 1}, until=stops)
    dap.request("restart", until=stops)
    frames = dap.request("stackTrace", {"threadId": 1}, show_body=True)
    frame = frames["body"]["stackFrames"][0]["id"] if frames and frames.get("body", {}).get("stackFrames") else 0
    scopes = dap.request("scopes", {"frameId": frame}, show_body=True)
    scope = scopes["body"]["scopes"][0]["variablesReference"] if scopes and scopes.get("body", {}).get("scopes") else 0
    dap.request("variables", {"variablesReference": scope}, show_body=True)
    dap.request("evaluate", {"expression": "k", "context": "repl"}, show_body=True)
    dap.request("disconnect")
    dap.close()

    note("reverse over DAP, in a replay")
    work = fixture("Debug", "ReplayReverse")
    run([JET, "run", "--record=back", "prog.jet"], work)
    program = os.path.join(work, "prog.jet")
    dap = Dap([JET, "debug", "--dap", "--replay=back", program], work)
    dap.request("initialize", {"adapterID": "jet", "pathFormat": "path", "linesStartAt1": True, "columnsStartAt1": True}, show_body=True)
    dap.request("launch", {"program": program, "stopOnEntry": False, "args": []}, until=("initialized",))
    dap.request("setBreakpoints", {"source": {"path": program}, "breakpoints": [{"line": 21}]})
    dap.request("configurationDone", until=stops)
    dap.request("continue", {"threadId": 1}, until=stops)
    dap.request("reverseContinue", {"threadId": 1}, until=stops)
    dap.request("evaluate", {"expression": "k", "context": "repl"}, show_body=True)
    dap.request("stepBack", {"threadId": 1}, until=stops)
    dap.request("stackTrace", {"threadId": 1}, show_body=True)
    dap.request("setBreakpoints", {"source": {"path": program}, "breakpoints": []})
    dap.request("continue", {"threadId": 1}, until=("terminated", "exited"))
    dap.request("disconnect")
    dap.close()

    note("attach to a running -O0 image (it allows any tracer, as the attach cases' authorizer does)")
    work = fixture("Debug", "DebugAttachesToARunningProgram")
    run([JET, "build", "-O0", "prog.jet"], work)
    target = spawn([os.path.join(work, ".jet", "build", "prog")], work, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                   preexec=any_ptracer)
    time.sleep(0.5)
    text, _ = console([JET, "debug", "--attach", str(target.pid)], work, stdin_lines("Debug", "debug_attaches_to_a_running_program"))
    print("the target after the session: {}".format("running" if target.poll() is None else "ended {}".format(target.returncode)))
    print()
    stop(target)


def p1():
    heading("P1: sampling: four threads of fib for two seconds of CPU time, -O2")
    work = fixture("Perf", "EveryThreadKeepsWholeStacks")
    run([JET, "perf", "run", "-O2", "burn.jet", "--out", "burn.jettrace"], work)
    view = run([JET, "perf", "view", "burn.jettrace"], work, show=False)
    print("$ jet perf view burn.jettrace  (its flame graph and per-line rows)")
    emit(view.text[view.text.find("\nflamegraph"):].strip())
    print()
    content = trace(os.path.join(work, "burn.jettrace"))
    sampling = perf_receipt(content).get("capture", {}).get("sampling", {})
    profile = content.get("profile", {})
    print("sampling: " + json.dumps({key: sampling.get(key) for key in ("interval_us", "pc", "lenses")}))
    for key in ("reason", "overhead_status", "overhead_ns", "overhead_reason", "coverage"):
        print("{}: {}".format(key, profile.get(key)))
    print()
    if "perf" in PEERS:
        note("the peer: Linux perf on the same -O2 image at 8 kHz of task clock, frame-pointer stacks")
        run([JET, "build", "-O2", "burn.jet"], work)
        run([PEERS["perf"], "record", "-q", "-e", "task-clock:u", "-F", "8000", "-g", "-o", "perf.data", ".jet/build/burn"], work, peer=True)
        report = run([PEERS["perf"], "report", "-i", "perf.data", "--stdio", "--children", "--sort", "sym", "-n", "-g", "none"], work,
                     peer=True, show=False)
        print("$ perf report -i perf.data --stdio --children --sort sym -n -g none  (the header and the first 12 rows)")
        rows = [line for line in report.out.split("\n") if line.strip()]
        emit("\n".join([line for line in rows if line.startswith("#")][:12] + [line for line in rows if not line.startswith("#")][:12]))
        note("the peer's precise program counter on this host: PEBS-style cycles:upp and AMD IBS")
        run([PEERS["perf"], "record", "-q", "-e", "cycles:upp", "-o", "precise.data", ".jet/build/burn"], work, peer=True)
        run([PEERS["perf"], "record", "-q", "-e", "ibs_op//u", "-o", "ibs.data", ".jet/build/burn"], work, peer=True)
        print()
    note("precise_pc_names_its_source")
    work = fixture("Perf", "PrecisePcNamesItsSource")
    run([JET, "perf", "run", "main.jet", "-O2", "--out", "spin.jettrace"], work)
    pc = perf_receipt(trace(os.path.join(work, "spin.jettrace"))).get("capture", {}).get("sampling", {}).get("pc")
    print("pc: " + json.dumps(pc))
    print()


def p2():
    heading("P2: zones, frame marks, plots")
    work = fixture("Perf", "ZonesFramesPlotsShareTheTimeline")
    run([JET, "perf", "run", "-O2", "main.jet", "--out", "zones.jettrace"], work)
    run([JET, "perf", "view", "zones.jettrace"], work)
    note("zones.jet: two million calls, plain, with a zone off (jet run) and on (jet perf run), -O2, warm build")
    work = probes("zones.jet")
    run([JET, "build", "-O2", "zones.jet"], work)
    best = {}
    for _ in range(2):
        for label, argv in (("plain", [JET, "run", "-O2", "zones.jet", "--", "plain"]),
                            ("zone off", [JET, "run", "-O2", "zones.jet", "--", "zoned"]),
                            ("plain, sampled", [JET, "perf", "run", "-O2", "zones.jet", "--out", "plain.jettrace", "--", "plain"]),
                            ("zone on", [JET, "perf", "run", "-O2", "zones.jet", "--out", "zoned.jettrace", "--", "zoned"])):
            spent = took_ns(run(argv, work).text)
            if spent:
                best[label] = min(best.get(label, spent), spent)
    if len(best) == 4:
        print("per call (the faster of two runs): plain {:.2f} ns, zone off {:.2f} ns, plain sampled {:.2f} ns, zone on {:.2f} ns".format(
            *(best[label] / 2e6 for label in ("plain", "zone off", "plain, sampled", "zone on"))))
        print("a zone off costs {:.2f} ns; on, {:.2f} ns over the sampled plain run".format(
            (best["zone off"] - best["plain"]) / 2e6, (best["zone on"] - best["plain, sampled"]) / 2e6))
        print()
    view = run([JET, "perf", "view", "zoned.jettrace"], work, show=False)
    print("$ jet perf view zoned.jettrace  (its zone rows)")
    emit("\n".join(line for line in view.text.split("\n") if "zone" in line)[:4000])
    print()


def p3():
    heading("P3, P9: waits and wakers, off-CPU time, I/O, a live process's zones")
    for name in ("EveryWaitNamesItsWaker", "OffCPUTimeByStack", "IOOnTimelineWithoutInstrumentation"):
        note(name)
        work = fixture("Perf", name)
        run([JET, "perf", "run", "main.jet", "--out", "t.jettrace"], work)
        run([JET, "perf", "view", "t.jettrace"], work)
    note("attach_turns_zones_on: zones of a running program, half a second at a time")
    work = fixture("Perf", "AttachTurnsZonesOn")
    process, pid = observed([JET, "run", "--observe", "live.jet"], work, "READY")
    if pid:
        run([JET, "perf", "attach", str(pid), "--source", "live.jet", "--zones=500ms", "--out", "zones.jettrace"], work)
        run([JET, "perf", "view", "zones.jettrace"], work)
    stop(process)


def p4():
    heading("P4: typed heap snapshots, retention, diffs")
    work = fixture("Perf", "HeapSnapshotRetention")
    run([JET, "perf", "run", "-O2", "--lens=alloc", "chat.jet", "--out", "o2.jettrace"], work)
    run([JET, "perf", "view", "o2.jettrace"], work)
    run([JET, "perf", "compare", "o2.jettrace", "--heap", "start..after-chat"], work)
    run([JET, "perf", "view", "o2.jettrace", "--retained-by", "[Int]"], work)
    note("a heap snapshot of a running program")
    process, pid = observed([JET, "run", "--observe", "--checked-alloc", "held.jet"], work, "READY")
    if pid:
        run([JET, "perf", "attach", str(pid), "--lens=alloc", "--source", "held.jet", "--out", "attach.jettrace"], work)
        run([JET, "perf", "view", "attach.jettrace"], work)
    stop(process)


def p5():
    heading("P5: hardware counters: top-down and memory per line")
    work = fixture("PerfRun", "TopdownLevelsPerFunction")
    run([JET, "perf", "run", "--lens=cpu,topdown", "td.jet", "--out", "td.jettrace"], work)
    run([JET, "perf", "view", "td.jettrace"], work)
    if "perf" in PEERS:
        note("the peer: Linux perf's top-down on the same program")
        run([JET, "build", "td.jet"], work)
        run([PEERS["perf"], "stat", "--topdown", ".jet/build/td"], work, peer=True)
    work = fixture("PerfRun", "MemoryCountersPerLine")
    run([JET, "perf", "run", "--lens=cpu,memory", "walk.jet", "--out", "walk.jettrace"], work)
    run([JET, "perf", "view", "walk.jettrace"], work)


def p6():
    heading("P6: causal profiling, -O2 arm of causal_profile_predicts_speedup")
    work = fixture("Perf", "CausalProfilePredictsSpeedup")
    run([JET, "run", "-A", "-O2", "main.jet", "--", "measure", "before.txt"], work)
    run([JET, "perf", "run", "-A", "-O2", "main.jet", "--lens=causal", "--out", "causal.jettrace"], work)
    run([JET, "run", "-A", "-O2", "main.jet", "--", "measure", "after.txt"], work)
    for name in ("before.txt", "after.txt"):
        with open(os.path.join(work, name)) as handle:
            print("{}: {}".format(name, " ".join(handle.read().split())))
    print()
    run([JET, "run", "-A", "check.jet", "--", "o2", "causal.jettrace"], work)
    run([JET, "perf", "view", "causal.jettrace"], work)
    run([JET, "perf", "run", "-A", "-O2", "main.jet", "--lens=causal", "--out", "short.jettrace", "--", "60"], work)
    run([JET, "perf", "view", "short.jettrace"], work)


def p7():
    heading("P7: the compiler's cost facts beside measured samples")
    work = fixture("Perf", "PerfView")
    run([JET, "perf", "run", "main.jet", "--out", "spin.jettrace"], work)
    run([JET, "perf", "view", "spin.jettrace"], work)


def p8():
    heading("P8: diffs with significance; exports")
    work = fixture("Perf", "PerfView")
    for side, count in (("base", "50000000"), ("head", "100000000")):
        for index in range(1, 6):
            run([JET, "perf", "run", "main.jet", "--out", "{}/{}.jettrace".format(side, index), "--", count], work, show=index == 1)
    run([JET, "perf", "compare", "base", "head"], work)
    run([JET, "perf", "compare", "base/1.jettrace", "head/1.jettrace"], work)
    run([JET, "perf", "export", "base/1.jettrace", "--pprof"], work)
    run([JET, "perf", "view", "base/1.pb.gz"], work)
    run([JET, "perf", "export", "base/1.jettrace", "--otel", "--out", "trace.otlp.json"], work)
    run([JET, "perf", "view", "trace.otlp.json"], work)


def p10():
    heading("P10: attach to a running program: its flight ring and its live state")
    work = fixture("Perf", "AttachDrainsFlightRing")
    process, pid = observed([JET, "run", "--observe", "live.jet"], work, "READY")
    if pid:
        run([JET, "perf", "attach", str(pid), "--source", "live.jet", "--out", "ring.jettrace"], work)
        run([JET, "perf", "view", "ring.jettrace"], work)
        run([JET, "inspect", "live", "--attach", str(pid), "--once"], work)
    stop(process)


AXES = {"tools": tools, "d1": d1, "d5": d5, "d7": d7, "d8": d8, "d9": d9, "d10": d10, "d11": d11,
        "p1": p1, "p2": p2, "p3": p3, "p4": p4, "p5": p5, "p6": p6, "p7": p7, "p8": p8, "p10": p10}

try:
    for axis in CHOSEN or list(AXES):
        if axis not in AXES:
            raise SystemExit("no axis {}: the axes are {}".format(axis, ", ".join(AXES)))
        AXES[axis]()
        sys.stdout.flush()
finally:
    shutil.rmtree(SCRATCH, ignore_errors=True)
