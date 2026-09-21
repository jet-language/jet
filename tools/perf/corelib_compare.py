#!/usr/bin/env python3
"""Measure matched Core workloads after compilation.

The benchmark has three executable arms:

* ``rust_native`` is a standalone optimized Rust implementation of the
  workload and is the Rust baseline.
* ``rust_core_aot`` is the same workload through Jet's Rust-backed Core API.
* ``jet_source_aot`` imports the checked-in Jet source module.

Compilation and source generation happen before samples.  Samples measure only
process execution, and every arm's stdout is checked against the same expected
result.  The source arm is evidence for a cutover decision; it is not silently
called a pass when it loses the strict Rust gate.

Run from the repository root:

    python3 tools/perf/corelib_compare.py --rounds 5000 --samples 5

Add ``--require-parity`` to fail unless every Jet arm is at most the configured
Rust ratio (default 1.05).
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import time
from statistics import median

ROOT = Path(__file__).resolve().parents[2]
JET_ENV = ROOT / "scripts/agent/jet-env"
SCRATCH_ROOT = Path.home() / ".cache" / "jet-luna" / "corelib-compare"
JET_WORKLOAD = r'''fn to_u8(n: Int) -> U8 { (U8{n & 255}) }

fn run() {
    payload := [U8]{}
    i := 0
    loop i < 257 {
        payload.push(to_u8(i % 251))
        i += 1
    }
    rounds :: {rounds}
    encoded := ""
    i = 0
    loop i < rounds {
        encoded = b64.encode(payload)
        i += 1
    }
    print(encoded.len())
}
'''

RUST_WORKLOAD = r'''const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { ALPHABET[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn main() {
    let payload: Vec<u8> = (0..257).map(|i| (i % 251) as u8).collect();
    let rounds: usize = {rounds};
    let mut encoded = String::new();
    for _ in 0..rounds {
        encoded = encode(&payload);
    }
    println!("{}", encoded.len());
}
'''



def run(command: list[str], *, cwd: Path = ROOT, timeout: int = 900) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


def build_jet(entry: Path, cwd: Path) -> tuple[Path | None, dict[str, object]]:
    completed = run([str(JET_ENV), "jet", "build", str(entry), "--release"], cwd=cwd)
    artifact = entry.parent / ".jet" / "build" / entry.stem
    return (
        artifact if completed.returncode == 0 and artifact.is_file() else None,
        {
            "exit": completed.returncode,
            "stdout": completed.stdout,
            "stderr": completed.stderr,
            "artifact": str(artifact),
        },
    )


def run_binary(binary: Path, expected: str) -> int:
    started = time.perf_counter_ns()
    completed = run([str(binary)])
    elapsed = time.perf_counter_ns() - started
    if completed.returncode != 0:
        raise SystemExit(
            f"{binary.name} failed with exit {completed.returncode}\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    if completed.stdout != expected:
        raise SystemExit(
            f"{binary.name} output mismatch: expected {expected!r}, got {completed.stdout!r}"
        )
    return elapsed


def ratio(value: int, baseline: int) -> float:
    return value / baseline if baseline else float("inf")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rounds", type=int, default=5000)
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--max-ratio", type=float, default=1.05)
    parser.add_argument("--require-parity", action="store_true")
    args = parser.parse_args()
    if args.rounds <= 0 or args.samples <= 0 or args.max_ratio <= 0:
        parser.error("--rounds, --samples, and --max-ratio must be positive")

    expected = "344\n"
    SCRATCH_ROOT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".corelib-compare-", dir=SCRATCH_ROOT) as raw_dir:
        scratch = Path(raw_dir)
        source_file = scratch / "source.jet"
        source_file.write_text(
            "use core.encoding.base64 as builtin_b64\n"
            "pub fn encode(data: [U8]) -> String { builtin_b64.encode(data) }\n",
            encoding="utf-8",
        )
        rust_entry = scratch / "rust_builtin.jet"
        rust_entry.write_text(
            f'''package {{
    name: "corelib_compare_rust"
    version: "0.1.0"
    edition: "2026"
    authority: {{ holds: {{ allow: [IO, Mem.Alloc] }} }}
    outputs: {{ app: .Executable{{ entry: run }} }}
}}
use core.encoding.base64 as b64
'''
            + JET_WORKLOAD.replace("{rounds}", str(args.rounds)),
            encoding="utf-8",
        )
        source_entry = scratch / "jet_source.jet"
        source_entry.write_text(
            f'''package {{
    name: "corelib_compare_source"
    version: "0.1.0"
    edition: "2026"
    authority: {{ holds: {{ allow: [IO, Mem.Alloc] }} }}
    outputs: {{ app: .Executable{{ entry: run }} }}
}}
use source as b64
'''
            + JET_WORKLOAD.replace("{rounds}", str(args.rounds)),
            encoding="utf-8",
        )
        native_source = scratch / "rust_native.rs"
        native_source.write_text(RUST_WORKLOAD.replace("{rounds}", str(args.rounds)), encoding="utf-8")

        rust_artifact, rust_build = build_jet(rust_entry, scratch)
        source_artifact, source_build = build_jet(source_entry, scratch)
        native_artifact = scratch / "rust_native"
        native_compile = run(
            [str(JET_ENV), "rustc", "-O", str(native_source), "-o", str(native_artifact)],
            cwd=ROOT,
        )
        builds = {
            "rust_native": {
                "exit": native_compile.returncode,
                "stdout": native_compile.stdout,
                "stderr": native_compile.stderr,
                "artifact": str(native_artifact),
            },
            "rust_core_aot": rust_build,
            "jet_source_aot": source_build,
        }
        artifacts = {
            "rust_native": native_artifact,
            "rust_core_aot": rust_artifact,
            "jet_source_aot": source_artifact,
        }
        if any(path is None or not path.is_file() for path in artifacts.values()):
            print(json.dumps({"builds": builds, "metric": "runtime_only_ns", "pass": False}, sort_keys=True))
            return 1

        # Warm-up validates outputs but is excluded from the reported samples.
        for path in artifacts.values():
            run_binary(path, expected)
        samples = {name: [] for name in artifacts}
        for _ in range(args.samples):
            for name, path in artifacts.items():
                samples[name].append(run_binary(path, expected))

    medians = {name: int(median(values)) for name, values in samples.items()}
    baseline = medians["rust_native"]
    ratios = {name: ratio(value, baseline) for name, value in medians.items()}
    jet_ratios = {name: ratios[name] for name in ("rust_core_aot", "jet_source_aot")}
    passed = all(value <= args.max_ratio for value in jet_ratios.values())
    report = {
        "workload": "base64 encode, 257-byte payload",
        "rounds": args.rounds,
        "samples": args.samples,
        "metric": "runtime_only_ns",
        "max_ratio": args.max_ratio,
        "builds": builds,
        "arms": {
            name: {"samples": values, "median_ns": medians[name], "ratio_to_rust_native": ratios[name]}
            for name, values in samples.items()
        },
        "jet_ratios_to_rust_native": jet_ratios,
        "output": expected.rstrip("\n"),
        "pass": passed,
    }
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if passed or not args.require_parity else 1


if __name__ == "__main__":
    raise SystemExit(main())
