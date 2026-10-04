#!/usr/bin/env python3
"""Hello-world compile benchmark (Docs/audits/compile-cost-hello-2026-10-02.md): wall time, peak RSS (max over the process tree,
via GNU time -v of the top process plus a /proc tree sampler), binary size
(as built and stripped), and run time, for Jet and peer toolchains.
Each toolchain: one cold build (its caches cleared where it has one) and one warm."""
import os, subprocess, time, json, shutil, threading

D = os.environ.get("BENCH_DIR", os.path.expanduser("~/.cache/jet-dev/scratch/bench-hello"))
TIME = "/nix/store/n0wrh3vjfwcqfyswwai0zcxvkpibq34v-time-1.10/bin/time"
GCC = "/nix/store/l5qkpzsr4gxvksh45b3nhxbkyr5cviar-gcc-wrapper-15.3.0/bin/gcc"
CLANG = "/nix/store/mw4gasdvwgscgpxpzihjgchfhs3hhqhn-clang-wrapper-21.1.8/bin/clang"
ZIG = "/nix/store/kbb1kfzlvqiwbcpw4sd2j3v63vqab3vg-zig-0.16.0/bin/zig"
GO = "/nix/store/i77g9dmcd399rmxk8688qfr4g2wzgk37-go-1.26.7/bin/go"
RUSTC = "/nix/store/cqlx62f919g8xf2f39bmykslpjdh9z0j-rustc-1.97.1/bin/rustc"
ODIN = "/nix/store/73b33yxdvy3fpv2x0l1l17sqa58amsxx-odin-dev-2026-05/bin/odin"
JET = os.environ.get("JET", os.path.expanduser("~/.cache/jet-dev/scratch/jet-release-night12/jet"))
STRIP = shutil.which("strip") or "/nix/store/l5qkpzsr4gxvksh45b3nhxbkyr5cviar-gcc-wrapper-15.3.0/bin/strip"

SRC = {
    "c": ('main.c', '#include <stdio.h>\nint main(void){puts("hello");return 0;}\n'),
    "zig": ('main.zig', 'const std = @import("std");\npub fn main() void { std.debug.print("hello\\n", .{}); }\n'),
    "rs": ('main.rs', 'fn main(){println!("hello");}\n'),
    "go": ('main.go', 'package main\nimport "fmt"\nfunc main(){fmt.Println("hello")}\n'),
    "odin": ('main.odin', 'package main\nimport "core:fmt"\nmain :: proc() { fmt.println("hello") }\n'),
    "jet": ('main.jet', 'fn run() {\n    print("hello")\n}\n'),
    "asm": ('hello.S', open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "hello.S")).read()),
}

def tree_peak(pid, stop, out):
    peak = 0
    while not stop.is_set():
        total = 0
        try:
            for p in os.listdir("/proc"):
                if not p.isdigit():
                    continue
                try:
                    with open(f"/proc/{p}/stat") as f:
                        raw = f.read()
                    fields = raw[raw.rfind(")") + 2:].split()
                    # include the process and all descendants by process group
                    if int(fields[2]) == pid:  # pgrp
                        with open(f"/proc/{p}/statm") as f:
                            total += int(f.read().split()[1]) * 4096
                except OSError:
                    pass
        except OSError:
            pass
        peak = max(peak, total)
        time.sleep(0.05)
    out.append(peak)

def run(name, cmd, cwd, env=None):
    t0 = time.time()
    p = subprocess.Popen(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
    stop = threading.Event(); out = []
    th = threading.Thread(target=tree_peak, args=(p.pid, stop, out)); th.start()
    log, _ = p.communicate()
    wall = time.time() - t0
    stop.set(); th.join()
    return {"rc": p.returncode, "wall_s": round(wall, 2), "peak_tree_mb": round(out[0] / 1e6, 1), "log": log.decode(errors="replace")[-600:]}

def size(path):
    if not os.path.exists(path):
        return None, None
    s = os.path.getsize(path)
    t = path + ".stripped"
    shutil.copy(path, t)
    subprocess.run([STRIP, t], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return s, os.path.getsize(t)

def runtime(path):
    if not path or not os.path.exists(path):
        return None
    best = 1e9
    for _ in range(5):
        t0 = time.time(); subprocess.run([path], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); best = min(best, time.time() - t0)
    return round(best * 1000, 2)

ONLY = os.environ.get("ONLY")
CASES = [
    ("Assembly gcc -nostdlib -static", "asm", lambda d: [GCC, "-nostdlib", "-static", "-o", "hello", "hello.S"], "hello", None),
    ("Assembly minimal ELF (-s -n, no build-id/relro)", "asm", lambda d: [GCC, "-nostdlib", "-static", "-s", "-Wl,-n,--build-id=none,-z,norelro", "-o", "hello", "hello.S"], "hello", None),
    ("C gcc -O2", "c", lambda d: [GCC, "-O2", "main.c", "-o", "hello"], "hello", None),
    ("C clang -O2", "c", lambda d: [CLANG, "-O2", "main.c", "-o", "hello"], "hello", None),
    ("Zig ReleaseFast", "zig", lambda d: [ZIG, "build-exe", "-O", "ReleaseFast", "main.zig", "--name", "hello", "--cache-dir", f"{d}/zc", "--global-cache-dir", f"{d}/zgc"], "hello", ["zc", "zgc"]),
    ("Zig ReleaseSmall", "zig", lambda d: [ZIG, "build-exe", "-O", "ReleaseSmall", "main.zig", "--name", "hello", "--cache-dir", f"{d}/zc", "--global-cache-dir", f"{d}/zgc"], "hello", ["zc", "zgc"]),
    ("Rust rustc -O", "rs", lambda d: [RUSTC, "-O", "main.rs", "-o", "hello"], "hello", None),
    ("Rust rustc debug", "rs", lambda d: [RUSTC, "main.rs", "-o", "hello"], "hello", None),
    ("Go go build", "go", lambda d: ["env", f"GOCACHE={d}/gocache", "GO111MODULE=off", GO, "build", "-o", "hello", "main.go"], "hello", ["gocache"]),
    ("Odin -o:speed", "odin", lambda d: [ODIN, "build", "main.odin", "-file", "-o:speed", "-out:hello"], "hello", None),
    ("Jet jet build (Rust reference, night12)", "jet", lambda d: [JET, "build", "main.jet", "--quiet"], None, [".jet"]),
    ("Jet jet build TRUE-COLD empty store (night12)", "jet", lambda d: ["env", f"JET_STORE_DIR={d}/store", JET, "build", "main.jet", "--quiet"], None, None),
    ("Jet jet build --release (night12)", "jet", lambda d: [JET, "build", "main.jet", "--release", "--quiet"], None, [".jet"]),
]

def find_jet_binary(d):
    best = None
    for root, _, files in os.walk(d):
        for f in files:
            p = os.path.join(root, f)
            if os.access(p, os.X_OK) and not f.endswith((".so", ".d", ".rlib", ".stripped")) and os.path.isfile(p) and "/deps/" not in p and "build-script" not in p:
                with open(p, "rb") as fh:
                    if fh.read(4) == b"\x7fELF":
                        if best is None or os.path.getmtime(p) > os.path.getmtime(best):
                            best = p
    return best

results = []
for label, lang, mk, out, caches in CASES:
    if ONLY and not any(o in label for o in ONLY.split(",")):
        continue
    d = os.path.join(D, label.replace(" ", "_").replace("/", "_").replace("(", "").replace(")", "").replace(",", ""))
    shutil.rmtree(d, ignore_errors=True); os.makedirs(d)
    fn, text = SRC[lang]
    open(os.path.join(d, fn), "w").write(text)
    row = {"toolchain": label}
    for phase in ("cold", "warm"):
        if phase == "warm" and out and os.path.exists(os.path.join(d, out)):
            os.remove(os.path.join(d, out))
        r = run(label, mk(d), d)
        row[phase] = {k: r[k] for k in ("rc", "wall_s", "peak_tree_mb")}
        if r["rc"] != 0:
            row[phase]["log"] = r["log"]
    binp = os.path.join(d, out) if out else find_jet_binary(d)
    row["binary"] = binp
    row["size_bytes"], row["stripped_bytes"] = size(binp) if binp else (None, None)
    row["run_ms"] = runtime(binp)
    results.append(row)
    print(json.dumps(row), flush=True)
json.dump(results, open(os.path.join(D, "results.json"), "w"), indent=1)
