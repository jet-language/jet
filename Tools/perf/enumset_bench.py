#!/usr/bin/env python3
"""Dense enum-set representations on matched workloads (#3133, ENUM-F05).

Arms (all print identical stdout; parity is checked on every sample):

* ``rust_native``       u64 mask (the Rust peer)
* ``rust_btreeset``     ``BTreeSet<Case>`` (the Rust ordered-set peer)
* ``jet_set_enum``      ``Set<Case>``
* ``jet_bits``          ``Bits`` over dense case indexes
* ``jet_int_mask``      hand-written ``Int`` mask
* ``jet_packed_fields`` a record owning one ``Int`` word with named accessors

Every program runs the same phases for each case count (8, 32, 64) and reports
per-phase nanoseconds on stderr: construction, update, membership,
setops (union/intersection/difference), iteration (ordered, by dense case
index) and conversion (to and from the index list).  Dense masks come from a
fixed hash; sparse masks set at most two bits (below bit 63, so every mask stays a
non-negative Int).  The stdout checksum folds each mask modulo 1000003.  A bit position is always the
dense declaration index, never a wire discriminant.

Allocation count and peak live bytes come from ``Tools/perf/alloccount.c``
(LD_PRELOAD); peak RSS comes from ``wait4``.  A cell that cannot be measured is
recorded as ``unavailable`` with a reason, never as zero.

    python3 Tools/perf/enumset_bench.py --samples 5 --json ~/.cache/jet-luna/perf/enumset.json
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
JET_ENV = ROOT / "Tools/agent/jet-env"
DEFAULT_JET = Path.home() / ".cache/jet-luna/safe-jet.sh"
SCRATCH = Path.home() / ".cache/jet-luna/enumset-bench"
PHASES = ("construction", "update", "membership", "setops", "iteration", "conversion")
JET_ARMS = ("jet_set_enum", "jet_bits", "jet_int_mask", "jet_packed_fields")
RUST_ARMS = ("rust_native", "rust_btreeset")
PACKAGE = 'name: "enumset_bench"\nversion: "0.1.0"\nauthority: {\n    holds: {\n        allow: [IO, Mem.Alloc, Time]\n    }\n}\n'


def full_mask(k: int) -> str:
    return "(0 - 1)" if k == 64 else str((1 << k) - 1)


# --- Jet sources -----------------------------------------------------------

JET_COMMON = """use core.time as time

enum Case {{
{cases}
}}

@ALL :: [{all}]

fn dense(r: Int) -> Int {{
    x :: (r * 40503 + 12345) % 1000003
    return (x | (x << 20) | (x << 40)) & {full}
}}

fn sparse(r: Int) -> Int {{
    return (1 << (r % {kbits})) | (1 << ((r * 7) % {kbits}))
}}

fn mask_of(r: Int) -> Int {{
    if r % 2 == 0 -> return dense(r)
    return sparse(r)
}}
"""

JET_REPS = {
    "jet_set_enum": """
fn mk(mask: Int) -> Set<Case> {{
    out := Set.from([Case]{{}})
    i := 0
    loop i < {k} {{
        if (mask >> i) & 1 == 1 -> &out.add(@ALL[i])
        i += 1
    }}
    return out
}}

fn has(s: Set<Case>, i: Int) -> Bool {{ s.has(@ALL[i]) }}

fn set_bit(s: &Set<Case>, i: Int) {{ &s.add(@ALL[i]) }}

fn clear_bit(s: &Set<Case>, i: Int) {{ &s.remove(@ALL[i]) }}

fn uni(a: Set<Case>, b: Set<Case>) -> Set<Case> {{ a.union(b) }}

fn inter(a: Set<Case>, b: Set<Case>) -> Set<Case> {{ a.intersection(b) }}

fn diff(a: Set<Case>, b: Set<Case>) -> Set<Case> {{ a.difference(b) }}
""",
    "jet_bits": """
fn mk(mask: Int) -> Bits {{
    out := Bits.new()
    i := 0
    loop i < {k} {{
        if (mask >> i) & 1 == 1 -> &out.add(i)
        i += 1
    }}
    return out
}}

fn has(s: Bits, i: Int) -> Bool {{ s.has(i) }}

fn set_bit(s: &Bits, i: Int) {{ &s.add(i) }}

fn clear_bit(s: &Bits, i: Int) {{ &s.remove(i) }}

fn combine(a: Bits, b: Bits, op: Int) -> Bits {{
    out := Bits.new()
    i := 0
    loop i < {k} {{
        x :: a.has(i)
        y :: b.has(i)
        keep := x && !y
        if op == 0 -> keep = x || y
        if op == 1 -> keep = x && y
        if keep -> &out.add(i)
        i += 1
    }}
    return out
}}

fn uni(a: Bits, b: Bits) -> Bits {{ combine(a, b, 0) }}

fn inter(a: Bits, b: Bits) -> Bits {{ combine(a, b, 1) }}

fn diff(a: Bits, b: Bits) -> Bits {{ combine(a, b, 2) }}
""",
    "jet_int_mask": """
fn mk(mask: Int) -> Int {{ mask }}

fn has(s: Int, i: Int) -> Bool {{ (s >> i) & 1 == 1 }}

fn set_bit(s: &Int, i: Int) {{ s = s | (1 << i) }}

fn clear_bit(s: &Int, i: Int) {{ s = s - (s & (1 << i)) }}

fn uni(a: Int, b: Int) -> Int {{ a | b }}

fn inter(a: Int, b: Int) -> Int {{ a & b }}

fn diff(a: Int, b: Int) -> Int {{ a - (a & b) }}
""",
    "jet_packed_fields": """
struct Word {{
    bits: Int
}}

fn mk(mask: Int) -> Word {{ Word{{bits: mask}} }}

fn has(s: Word, i: Int) -> Bool {{ (s.bits >> i) & 1 == 1 }}

fn set_bit(s: &Word, i: Int) {{ s.bits = s.bits | (1 << i) }}

fn clear_bit(s: &Word, i: Int) {{ s.bits = s.bits - (s.bits & (1 << i)) }}

fn uni(a: Word, b: Word) -> Word {{ Word{{bits: a.bits | b.bits}} }}

fn inter(a: Word, b: Word) -> Word {{ Word{{bits: a.bits & b.bits}} }}

fn diff(a: Word, b: Word) -> Word {{ Word{{bits: a.bits - (a.bits & b.bits)}} }}
""",
}

JET_DRIVER = """
fn to_mask(s: {ty}) -> Int {{
    m := 0
    i := 0
    loop i < {k} {{
        if has(s, i) -> m = m | (1 << i)
        i += 1
    }}
    return m % 1000003
}}

fn items(s: {ty}) -> [Int] {{
    out := [Int]{{}}
    i := 0
    loop i < {k} {{
        if has(s, i) -> &out.push(i)
        i += 1
    }}
    return out
}}

fn from_items(xs: [Int]) -> {ty} {{
    m := 0
    loop x in xs -> m = m | (1 << x)
    return mk(m)
}}

fn run() {{
    rounds :: {rounds}
    check := 0

    w :: time.instant()
    r := 0
    loop r < rounds {{
        check = (check * 31 + to_mask(mk(mask_of(r)))) % 1000000007
        r += 1
    }}
    t_construction :: w.elapsed().in(.Nanoseconds)

    w2 :: time.instant()
    s := mk(0)
    r = 0
    loop r < rounds {{
        set_bit(&s, r % {kbits})
        if r % 3 == 0 -> clear_bit(&s, (r * 5) % {kbits})
        r += 1
    }}
    check = (check * 31 + to_mask(s)) % 1000000007
    t_update :: w2.elapsed().in(.Nanoseconds)

    w3 :: time.instant()
    probe :: mk(dense(7))
    hits := 0
    r = 0
    loop r < rounds {{
        if has(probe, r % {k}) -> hits += 1
        r += 1
    }}
    check = (check * 31 + hits) % 1000000007
    t_membership :: w3.elapsed().in(.Nanoseconds)

    w4 :: time.instant()
    r = 0
    loop r < rounds {{
        a :: mk(mask_of(r))
        b :: mk(mask_of(r + 1))
        check = (check * 31 + to_mask(uni(a, b)) + to_mask(inter(a, b)) * 3 + to_mask(diff(a, b)) * 5) % 1000000007
        r += 1
    }}
    t_setops :: w4.elapsed().in(.Nanoseconds)

    w5 :: time.instant()
    total := 0
    r = 0
    loop r < rounds /% 8 {{
        loop x in items(probe) -> total += x
        r += 1
    }}
    check = (check * 31 + total) % 1000000007
    t_iteration :: w5.elapsed().in(.Nanoseconds)

    w6 :: time.instant()
    r = 0
    loop r < rounds /% 8 {{
        check = (check * 31 + to_mask(from_items(items(mk(mask_of(r)))))) % 1000000007
        r += 1
    }}
    t_conversion :: w6.elapsed().in(.Nanoseconds)

    print("k={k} order={{items(mk(dense(3)))}}")
    print("check={{check}}")
    eprint("construction={{t_construction}} update={{t_update}} membership={{t_membership}} setops={{t_setops}} iteration={{t_iteration}} conversion={{t_conversion}}")
}}
"""

JET_TYPES = {"jet_set_enum": "Set<Case>", "jet_bits": "Bits", "jet_int_mask": "Int", "jet_packed_fields": "Word"}


def jet_source(arm: str, k: int, rounds: int) -> str:
    common = JET_COMMON.format(
        cases="\n".join(f"    C{i}" for i in range(k)),
        all=", ".join(f"Case.C{i}" for i in range(k)),
        full=full_mask(k),
        k=k,
        kbits=min(k, 63),
    )
    return common + JET_REPS[arm].format(k=k) + JET_DRIVER.format(ty=JET_TYPES[arm], k=k, kbits=min(k, 63), rounds=rounds)


# --- Rust sources ----------------------------------------------------------

RUST_SOURCE = r"""use std::collections::BTreeSet;
use std::hint::black_box;
use std::time::Instant;

const K: u32 = {k};
const FULL: u64 = {full};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Case(u8);

fn dense(r: i64) -> u64 {{
    let x = ((r * 40503 + 12345) % 1000003) as u64;
    (x | (x << 20) | (x << 40)) & FULL
}}
fn sparse(r: i64) -> u64 {{ (1u64 << (r % {kbits})) | (1u64 << ((r * 7) % {kbits})) }}
fn mask_of(r: i64) -> u64 {{ if r % 2 == 0 {{ dense(r) }} else {{ sparse(r) }} }}

trait Rep: Sized {{
    fn mk(mask: u64) -> Self;
    fn has(&self, i: u32) -> bool;
    fn set_bit(&mut self, i: u32);
    fn clear_bit(&mut self, i: u32);
    fn uni(&self, b: &Self) -> Self;
    fn inter(&self, b: &Self) -> Self;
    fn diff(&self, b: &Self) -> Self;
}}

impl Rep for u64 {{
    fn mk(mask: u64) -> Self {{ mask }}
    fn has(&self, i: u32) -> bool {{ (*self >> i) & 1 == 1 }}
    fn set_bit(&mut self, i: u32) {{ *self |= 1 << i }}
    fn clear_bit(&mut self, i: u32) {{ *self &= !(1 << i) }}
    fn uni(&self, b: &Self) -> Self {{ self | b }}
    fn inter(&self, b: &Self) -> Self {{ self & b }}
    fn diff(&self, b: &Self) -> Self {{ self & !b }}
}}

impl Rep for BTreeSet<Case> {{
    fn mk(mask: u64) -> Self {{ (0..K).filter(|i| (mask >> i) & 1 == 1).map(|i| Case(i as u8)).collect() }}
    fn has(&self, i: u32) -> bool {{ self.contains(&Case(i as u8)) }}
    fn set_bit(&mut self, i: u32) {{ self.insert(Case(i as u8)); }}
    fn clear_bit(&mut self, i: u32) {{ self.remove(&Case(i as u8)); }}
    fn uni(&self, b: &Self) -> Self {{ self.union(b).copied().collect() }}
    fn inter(&self, b: &Self) -> Self {{ self.intersection(b).copied().collect() }}
    fn diff(&self, b: &Self) -> Self {{ self.difference(b).copied().collect() }}
}}

fn to_mask<R: Rep>(s: &R) -> i64 {{
    let mut m: u64 = 0;
    for i in 0..K {{ if s.has(i) {{ m |= 1 << i; }} }}
    (m % 1000003) as i64
}}
fn items<R: Rep>(s: &R) -> Vec<i64> {{ (0..K).filter(|&i| s.has(i)).map(|i| i as i64).collect() }}
fn from_items<R: Rep>(xs: &[i64]) -> R {{ R::mk(xs.iter().fold(0u64, |m, &x| m | (1 << x))) }}

fn run<R: Rep>() {{
    let rounds: i64 = {rounds};
    let mut check: i64 = 0;
    let w = Instant::now();
    for r in 0..rounds {{ check = (check * 31 + to_mask(&R::mk(mask_of(black_box(r))))) % 1000000007; }}
    let t_construction = w.elapsed().as_nanos();
    let w = Instant::now();
    let mut s = R::mk(0);
    for r in 0..rounds {{
        s.set_bit((r % {kbits}) as u32);
        if r % 3 == 0 {{ s.clear_bit(((r * 5) % {kbits}) as u32); }}
    }}
    check = (check * 31 + to_mask(&s)) % 1000000007;
    let t_update = w.elapsed().as_nanos();
    let w = Instant::now();
    let probe = R::mk(dense(7));
    let mut hits = 0i64;
    for r in 0..rounds {{ if probe.has((black_box(r) % K as i64) as u32) {{ hits += 1; }} }}
    check = (check * 31 + hits) % 1000000007;
    let t_membership = w.elapsed().as_nanos();
    let w = Instant::now();
    for r in 0..rounds {{
        let a = R::mk(mask_of(r));
        let b = R::mk(mask_of(r + 1));
        check = (check * 31 + to_mask(&a.uni(&b)) + to_mask(&a.inter(&b)) * 3 + to_mask(&a.diff(&b)) * 5) % 1000000007;
    }}
    let t_setops = w.elapsed().as_nanos();
    let w = Instant::now();
    let mut total = 0i64;
    for _ in 0..rounds / 8 {{ for x in items(black_box(&probe)) {{ total += x; }} }}
    check = (check * 31 + total) % 1000000007;
    let t_iteration = w.elapsed().as_nanos();
    let w = Instant::now();
    for r in 0..rounds / 8 {{ check = (check * 31 + to_mask(&from_items::<R>(&items(&R::mk(mask_of(r)))))) % 1000000007; }}
    let t_conversion = w.elapsed().as_nanos();
    let order: Vec<String> = items(&R::mk(dense(3))).iter().map(|i| i.to_string()).collect();
    println!("k={{K}} order=[{{}}]", order.join(", "));
    println!("check={{check}}");
    eprintln!("construction={{t_construction}} update={{t_update}} membership={{t_membership}} setops={{t_setops}} iteration={{t_iteration}} conversion={{t_conversion}}");
}}

fn main() {{ run::<{rep}>(); }}
"""


def rust_source(arm: str, k: int, rounds: int) -> str:
    rep = "u64" if arm == "rust_native" else "BTreeSet<Case>"
    full = "u64::MAX" if k == 64 else str((1 << k) - 1)
    return RUST_SOURCE.format(k=k, kbits=min(k, 63), full=full, rounds=rounds, rep=rep)


# --- measurement -----------------------------------------------------------

def sh(cmd: list[str], cwd: Path, env: dict[str, str] | None = None, timeout: int = 1800) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout, check=False)


def build_alloccount(work: Path) -> Path | None:
    lib = work / "liballoccount.so"
    r = sh([str(JET_ENV), "cc", "-O2", "-shared", "-fPIC", "-o", str(lib), str(ROOT / "Tools/perf/alloccount.c")], ROOT)
    return lib if r.returncode == 0 and lib.is_file() else None


def build(arm: str, k: int, rounds: int, work: Path, jet: str) -> dict[str, object]:
    d = work / f"k{k}"
    d.mkdir(parents=True, exist_ok=True)
    stem = f"{arm}_k{k}"
    if arm in RUST_ARMS:
        src = d / f"{stem}.rs"
        src.write_text(rust_source(arm, k, rounds))
        binary = d / stem
        started = time.perf_counter_ns()
        r = sh([str(JET_ENV), "rustc", "-C", "opt-level=3", "-C", "codegen-units=1", str(src), "-o", str(binary)], ROOT)
    else:
        (d / "package.jet").write_text(PACKAGE)
        src = d / f"{stem}.jet"
        src.write_text(jet_source(arm, k, rounds))
        binary = d / ".jet/build" / stem
        binary.unlink(missing_ok=True)
        started = time.perf_counter_ns()
        r = sh([jet, "build", "--release", src.name], d)
    elapsed = time.perf_counter_ns() - started
    ok = r.returncode == 0 and binary.is_file()
    return {"binary": str(binary) if ok else None, "build_ns": elapsed, "exit": r.returncode,
            "error_tail": None if ok else (r.stderr + r.stdout)[-1500:]}


def run_once(binary: str, env: dict[str, str] | None = None) -> tuple[int, str, str, int]:
    started = time.perf_counter_ns()
    proc = subprocess.Popen([binary], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    out, err = proc.communicate(timeout=900)
    wall = time.perf_counter_ns() - started
    return proc.returncode, out.decode(), err.decode(), wall


def run_rss(binary: str) -> int | None:
    started = os.fork()
    if started == 0:
        devnull = os.open(os.devnull, os.O_WRONLY)
        os.dup2(devnull, 1)
        os.dup2(devnull, 2)
        os.execv(binary, [binary])
    _, status, usage = os.wait4(started, 0)
    return usage.ru_maxrss if os.WIFEXITED(status) and os.WEXITSTATUS(status) == 0 else None


def parse_phases(stderr: str) -> dict[str, int] | None:
    for line in stderr.splitlines()[::-1]:
        fields = dict(part.split("=", 1) for part in line.split() if "=" in part)
        if all(p in fields for p in PHASES):
            return {p: int(fields[p]) for p in PHASES}
    return None


def measure(binary: str, samples: int, lib: Path | None, work: Path) -> dict[str, object]:
    runs = []
    for _ in range(samples + 1):
        code, out, err, wall = run_once(binary)
        if code != 0:
            return {"status": "unavailable", "reason": f"exit {code}", "stderr_tail": err[-800:]}
        phases = parse_phases(err)
        if phases is None:
            return {"status": "unavailable", "reason": "phase timings missing from stderr", "stderr_tail": err[-800:]}
        runs.append((out, wall, phases))
    runs = runs[1:]  # warm-up excluded
    outputs = {out for out, _, _ in runs}
    cell: dict[str, object] = {
        "status": "measured",
        "stdout": runs[0][0],
        "stable_stdout": len(outputs) == 1,
        "wall_median_ns": int(statistics.median(w for _, w, _ in runs)),
        "phase_median_ns": {p: int(statistics.median(ph[p] for _, _, ph in runs)) for p in PHASES},
        "peak_rss_kib": run_rss(binary) or "unavailable",
        "binary_bytes": os.path.getsize(binary),
    }
    if lib is None:
        cell["allocations"] = {"status": "unavailable", "reason": "allocation counter did not build"}
    else:
        count = work / "alloc.count"
        count.unlink(missing_ok=True)
        env = dict(os.environ, LD_PRELOAD=str(lib), ALLOCCOUNT_OUT=str(count))
        code, _, _, _ = run_once(binary, env)
        if code == 0 and count.is_file():
            fields = {k: int(v) for k, v in (kv.split("=") for kv in count.read_text().split())}
            cell["allocations"] = {"status": "measured", **fields} if fields.get("allocations") else {
                "status": "unavailable", "reason": "binary bypasses the preloadable libc allocator"}
        else:
            cell["allocations"] = {"status": "unavailable", "reason": f"counted run exit {code}"}
    return cell


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--rounds", type=int, default=200000)
    parser.add_argument("--sizes", default="8,32,64")
    parser.add_argument("--json", type=Path, required=True)
    parser.add_argument("--jet", default=str(DEFAULT_JET))
    parser.add_argument("--max-ratio", type=float, default=1.05)
    args = parser.parse_args()
    sizes = [int(s) for s in args.sizes.split(",")]
    if args.samples < 1 or args.rounds < 8 or any(s < 1 or s > 64 for s in sizes):
        parser.error("--samples >= 1, --rounds >= 8, sizes in 1..64")
    work = SCRATCH
    work.mkdir(parents=True, exist_ok=True)
    lib = build_alloccount(work)
    head = sh(["git", "rev-parse", "HEAD"], ROOT).stdout.strip()
    version = sh([args.jet, "--version"], ROOT)
    report: dict[str, object] = {
        "card": 3133, "finding": "ENUM-F05", "commit": head,
        "dirty_tree": bool(sh(["git", "status", "--porcelain"], ROOT).stdout.strip()),
        "jet": args.jet, "jet_version": (version.stdout or version.stderr).strip()[:200],
        "rustc": sh([str(JET_ENV), "rustc", "--version"], ROOT).stdout.strip(),
        "machine": {"platform": platform.platform(), "cpu": platform.processor() or platform.machine(), "cpus": os.cpu_count()},
        "samples": args.samples, "rounds": args.rounds, "phases": list(PHASES),
        "parity_band_vs_rust": args.max_ratio, "cells": [],
    }
    for k in sizes:
        cells: dict[str, dict[str, object]] = {}
        for arm in RUST_ARMS + JET_ARMS:
            built = build(arm, k, args.rounds, work, args.jet)
            cell: dict[str, object] = {"k": k, "arm": arm, "build_ns": built["build_ns"]}
            if built["binary"] is None:
                cell.update(status="unavailable", reason="build failed", error_tail=built["error_tail"])
            else:
                cell.update(measure(str(built["binary"]), args.samples, lib, work))
            cells[arm] = cell
            print(f"k={k} {arm}: {cell['status']}", flush=True)
        reference = cells["rust_native"]
        for arm, cell in cells.items():
            if cell["status"] != "measured" or reference["status"] != "measured":
                cell["parity"] = "unknown"
                continue
            cell["parity"] = "match" if cell["stdout"] == reference["stdout"] else "MISMATCH"
            cell["ratio_to_rust_native"] = {
                p: round(cell["phase_median_ns"][p] / max(1, reference["phase_median_ns"][p]), 3) for p in PHASES}
            cell["losing_phases"] = [p for p, v in cell["ratio_to_rust_native"].items() if v > args.max_ratio]
            report["cells"].append(cell)
        report["cells"].extend(c for c in cells.values() if c not in report["cells"])
    args.json.parent.mkdir(parents=True, exist_ok=True)
    args.json.write_text(json.dumps(report, indent=2))
    print(args.json)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
