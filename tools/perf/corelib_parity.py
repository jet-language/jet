#!/usr/bin/env python3
"""Build a registry/source/conformance matrix for Jet Core.

The denominator is the canonical ``Prelude/Core.jet`` registry.  Generated Rust
views are checked before the matrix is emitted, and every registry member is
classified as one of: a source declaration, a Rust-dispatched builtin, or an
explicitly documented compiler-owned source exemption.

The conformance corpus is intentionally reported separately: effectful and
argument-rich APIs need a real witness, so an uncovered row is evidence rather
than an invented smoke test.  ``--require-conformance`` turns that evidence
into a strict gate when a release scope has supplied every witness.  ``--aot``
adds a deterministic output comparison through ``jet run`` and release AOT;
``--require-packages`` and ``--require-aot`` make the corresponding checks
blocking.

Run from the repository root:

    python3 tools/perf/corelib_parity.py
    python3 tools/perf/corelib_parity.py --conformance --aot
    python3 tools/perf/corelib_parity.py --packages
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CORE_REGISTRY = ROOT / "crates/jet-codegen/src/Prelude/Core.jet"
CORE_SOURCE = ROOT / "Core"
JET_ENV = ROOT / "scripts/agent/jet-env"
LEDGER_CHECK = ROOT / "scripts/agent/check-core-surface-ledger.mjs"
CONFORMANCE = ROOT / "scripts/agent/core-conformance.mjs"
SCRATCH_ROOT = Path.home() / ".cache" / "jet-luna" / "corelib-parity"


# These are compiler-owned surfaces with no source-level declaration shape.
# Ptr is the ratified raw-pointer type. The other three names are reserved
# words in function declaration position; the Rust Core dispatcher remains the
# canonical implementation for them.
SOURCE_EXEMPTIONS = {
    "core.mem.Ptr": "builtin raw-pointer type",
    "core.data.stream.take": "reserved builtin stream operation",
    "core.math.combinatorics.take": "reserved builtin combinatorics operation",
    "core.reactive.effect": "reserved builtin reactive operation",
}


def run(
    command: list[str],
    *,
    timeout: int = 900,
    cwd: Path = ROOT,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


def core_modules() -> dict[str, set[str]]:
    text = CORE_REGISTRY.read_text(encoding="utf-8")
    modules: dict[str, set[str]] = {}
    pattern = re.compile(r"(?m)^module\s+([A-Za-z0-9_.]+)\s+exports\s+\{([^}]*)\}")
    for match in pattern.finditer(text):
        modules[match.group(1)] = {
            item.strip() for item in match.group(2).split(",") if item.strip()
        }
    if not modules:
        raise RuntimeError("Core.jet contains no module declarations")
    return modules


def dispatcher_keys() -> set[str]:
    text = CORE_REGISTRY.read_text(encoding="utf-8")
    rows = re.findall(
        r"(?m)^dispatcher_row\s+plain\s+([A-Za-z0-9_.]+)\s+([A-Za-z_][A-Za-z0-9_]*)",
        text,
    )
    return {f"{module}.{member}" for module, member in rows}


def source_modules() -> dict[str, set[str]]:
    result: dict[str, set[str]] = {}
    for path in sorted(CORE_SOURCE.rglob("*.jet")):
        if not path.is_file():
            continue
        relative = list(path.relative_to(CORE_SOURCE).with_suffix("").parts)
        if len(relative) >= 2 and relative[-1] == relative[-2]:
            relative = relative[:-1]
        module = ".".join(relative)
        if module == "":
            continue
        if module != "app" and not module.startswith("core."):
            module = f"core.{module}"
        text = path.read_text(encoding="utf-8", errors="replace")
        names = set(re.findall(r"\bpub\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)", text))
        names.update(
            re.findall(r"\bpub\s+(?:struct|enum)\s+([A-Za-z_][A-Za-z0-9_]*)", text)
        )
        names.update(re.findall(r"(?m)^@([A-Za-z_][A-Za-z0-9_]*)\s*::", text))
        result.setdefault(module, set()).update(names)
    return result


def source_matrix() -> dict[str, object]:
    registry = core_modules()
    source = source_modules()
    dispatched = dispatcher_keys()
    source_only: list[str] = []
    builtin_only: list[dict[str, str]] = []
    missing: list[str] = []
    for module, members in registry.items():
        for member in sorted(members):
            key = f"{module}.{member}"
            if member in source.get(module, set()):
                continue
            if key in SOURCE_EXEMPTIONS:
                builtin_only.append({"key": key, "reason": SOURCE_EXEMPTIONS[key]})
                continue
            if key in dispatched:
                builtin_only.append({"key": key, "reason": "Rust Core dispatcher"})
                continue
            missing.append(key)
    for module, names in source.items():
        for name in sorted(names - registry.get(module, set())):
            source_only.append(f"{module}.{name}")
    return {
        "registry_modules": len(registry),
        "registry_members": sum(len(items) for items in registry.values()),
        "source_modules": len(source),
        "source_covered_members": sum(
            1
            for module, members in registry.items()
            for member in members
            if member in source.get(module, set())
        ),
        "builtin_only_members": sorted(builtin_only, key=lambda item: item["key"]),
        "missing_members": sorted(missing),
        "source_only_members": source_only,
    }


def conformance_matrix() -> dict[str, object]:
    completed = run([str(JET_ENV), "node", str(CONFORMANCE), "--check"])
    output = (completed.stdout + completed.stderr).strip()
    first = output.splitlines()[0] if output else ""
    match = re.search(
        r"denominator:\s+(\d+) public function\(s\);\s+(\d+) program\(s\);\s+(\d+) carve-out\(s\);\s+(\d+) uncovered row\(s\)",
        first,
    )
    result: dict[str, object] = {
        "exit": completed.returncode,
        "summary": first,
        "uncovered": None,
    }
    if match:
        result.update(
            {
                "public_functions": int(match.group(1)),
                "programs": int(match.group(2)),
                "carve_outs": int(match.group(3)),
                "uncovered": int(match.group(4)),
            }
        )
    return result


def package_matrix() -> dict[str, object]:
    roots = [
        path
        for path in sorted(CORE_SOURCE.rglob("*.jet"))
        if path.is_file()
        and path.read_text(encoding="utf-8", errors="replace").lstrip().startswith("package")
    ]
    packages: list[dict[str, object]] = []
    for path in roots:
        completed = run([str(JET_ENV), "jet", "check", str(path), "--json"])
        try:
            payload = json.loads(completed.stdout)
        except json.JSONDecodeError:
            payload = {"reports": []}
        reports = [
            report
            for report in payload.get("reports", [])
            if report.get("severity") == "error"
        ]
        packages.append(
            {
                "path": str(path.relative_to(ROOT)),
                "exit": completed.returncode,
                "errors": len(reports),
                "codes": dict(Counter(report.get("code") for report in reports)),
            }
        )
    return {
        "package_roots": len(packages),
        "failed_roots": sum(item["exit"] != 0 for item in packages),
        "errors": sum(item["errors"] for item in packages),
        "packages": packages,
    }


def aot_run_matrix() -> dict[str, object]:
    """Compare one pure Core call through ``jet run`` and release AOT.

    Keep this probe deliberately small and deterministic.  It proves the
    cross-tier output contract without turning the parity inventory into a
    benchmark or depending on effectful host state.
    """

    stem = f"core_parity_aot_{os.getpid()}"
    SCRATCH_ROOT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".core-parity-aot-", dir=SCRATCH_ROOT) as raw:
        scratch = Path(raw)
        entry = scratch / f"{stem}.jet"
        entry.write_text(
            """use core.encoding.base64 as b64

fn run() {
    print(b64.encode([U8]{102, 111, 111}))
}
""",
            encoding="utf-8",
        )
        jet_run = run([str(JET_ENV), "jet", "run", str(entry)], cwd=scratch)
        build = run(
            [str(JET_ENV), "jet", "build", str(entry), "--release"],
            cwd=scratch,
        )
        artifacts = [
            path
            for path in scratch.rglob("*")
            if path.is_file() and path.name == stem and os.access(path, os.X_OK)
        ]
        artifact = artifacts[0] if len(artifacts) == 1 else None
        aot = run([str(artifact)]) if build.returncode == 0 and artifact else None
        expected = "Zm9v\n"
        result: dict[str, object] = {
            "fixture": str(entry.relative_to(ROOT)) if entry.is_relative_to(ROOT) else str(entry),
            "expected_stdout": expected,
            "jet_run": {
                "exit": jet_run.returncode,
                "stdout": jet_run.stdout,
                "stderr": jet_run.stderr,
            },
            "aot_build": {
                "exit": build.returncode,
                "stdout": build.stdout,
                "stderr": build.stderr,
            },
            "aot": None,
            "equal": False,
        }
        if aot is not None:
            result["aot"] = {
                "exit": aot.returncode,
                "stdout": aot.stdout,
                "stderr": aot.stderr,
            }
            result["equal"] = (
                jet_run.returncode == 0
                and aot.returncode == 0
                and jet_run.stdout == expected
                and aot.stdout == expected
            )
    return result


def aot_failed(result: dict[str, object]) -> bool:
    return not bool(result.get("equal"))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--conformance", action="store_true", help="run the Core witness audit")
    parser.add_argument("--packages", action="store_true", help="check every Core package root")
    parser.add_argument("--aot", action="store_true", help="compare a deterministic Jet run and release AOT probe")
    parser.add_argument(
        "--require-aot",
        action="store_true",
        help="fail when the deterministic AOT/run probe is not equal",
    )
    parser.add_argument(
        "--require-packages",
        action="store_true",
        help="fail when any source package root has semantic errors",
    )
    parser.add_argument(
        "--require-conformance",
        action="store_true",
        help="fail when the witness audit reports uncovered rows",
    )
    parser.add_argument("--json", action="store_true", help="emit one JSON object")
    args = parser.parse_args()

    ledger = run([str(JET_ENV), "node", str(LEDGER_CHECK), "--check"])
    if ledger.returncode != 0:
        sys.stderr.write(ledger.stdout + ledger.stderr)
        return ledger.returncode

    result: dict[str, object] = {"source": source_matrix()}
    if args.conformance or args.require_conformance:
        result["conformance"] = conformance_matrix()
    if args.packages or args.require_packages:
        result["packages"] = package_matrix()
    if args.aot or args.require_aot:
        result["aot"] = aot_run_matrix()

    source_result = result["source"]
    failures = list(source_result["missing_members"])
    if failures:
        sys.stderr.write("unimplemented Core registry members:\n")
        sys.stderr.write("\n".join(f"  {item}" for item in failures) + "\n")
        return 1
    if args.require_packages:
        packages = result["packages"]
        if packages["failed_roots"] or packages["errors"]:
            sys.stderr.write(
                f"Core source packages remain invalid: "
                f"{packages['failed_roots']} failed roots, {packages['errors']} errors\n"
            )
            return 1
    if args.require_aot and aot_failed(result["aot"]):
        sys.stderr.write("Core AOT/run probe did not produce equal output\n")
        return 1
    if args.require_conformance:
        uncovered = result["conformance"].get("uncovered")
        if uncovered is None or uncovered != 0:
            sys.stderr.write(f"Core conformance remains uncovered: {uncovered}\n")
            return 1

    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
