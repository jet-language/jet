#!/usr/bin/env python3
"""Bounded Wave A enum-codec benchmark harness.

The harness describes one fixed ``Command`` workload and, when an existing
canonical codec command is explicitly supplied, measures that command.  It
never implements a codec itself.  In particular, the JSON-looking strings in
``WORKLOAD`` are fixture wire values; ``json.dumps`` is used only for this
report's machine-readable output.

The default invocation is a static, no-runtime report::

    python3 tools/perf/enum_codec_bench.py

A runtime invocation requires an existing adapter supplied by the caller.  The
adapter is an executable command (``--command`` or
``JET_ENUM_CODEC_BENCH_COMMAND``), not a serializer added by this harness.  If
its command line has no ``{operation}``, ``{form}``, ``{case}``, ``{input}``,
``{expected}``, ``{output}``, ``{metrics}``, or ``{rounds}`` placeholders, the
harness appends those options::

    python3 tools/perf/enum_codec_bench.py --run \\
        --command 'path/to/existing-enum-codec-adapter'

The adapter protocol is deliberately small and explicit:

* ``encode`` reads the source expression at ``--input`` and writes the exact
  wire bytes to ``--output``;
* ``decode`` reads wire bytes at ``--input`` and may use the expected source
  expression at ``--expected``; exit zero means accepted and non-zero means
  rejected;
* both operations may write a JSON object to ``--metrics`` with non-negative
  integer ``codec_ns`` and/or ``allocation_count`` fields.  Missing fields are
  reported unavailable, never estimated by this script.

Every sample uses the same payload, rounds, limits, and scratch policy.  The
report separates process-wall timing from optional adapter-reported codec
timing, retains unavailable cells, and does not compare a peer implementation.
"""

from __future__ import annotations

import argparse
from collections import Counter
import json
import math
import os
from pathlib import Path
import platform
import signal
import shlex
import statistics
import subprocess
import sys
import tempfile
import time
from typing import Any, Callable

try:
    import resource
except ImportError:  # pragma: no cover - resource is Unix-only.
    resource = None  # type: ignore[assignment]


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(__file__).resolve()
SCRATCH_DEFAULT = Path.home() / ".cache" / "jet-luna" / "enum-codec-bench"
PROTOCOL_VERSION = 1
FORMS = ("external", "internal", "untagged", "adjacent")
CASES = ("payload", "evolution", "unknown", "malformed")
OPERATIONS = ("encode", "decode")
UNAVAILABLE = "canonical enum codec command was not provided; static mode does not execute Core"
ADJACENT_UNAVAILABLE = (
    "adjacent tag/content codec entry point is unavailable by contract; "
    "this cell is never substituted with an external serializer"
)


# These are fixed wire witnesses, not a Python implementation of any codec.
# The same semantic value is used for the known payload in every form.
WORKLOAD_CASES: dict[str, dict[str, str]] = {
    "payload": {
        "source_expression": 'Command.Write("hello")',
        "semantic_value": 'Command.Write("hello")',
        "decode_expectation": "accept",
        "purpose": "equal known payload across all supported enum forms",
    },
    "evolution": {
        "source_expression": 'Command.Archive("hello")',
        "semantic_value": 'future Command.Archive("hello")',
        "decode_expectation": "reject",
        "purpose": "future variant/evolution input kept separate from known payload",
    },
    "unknown": {
        "source_expression": 'Command.Future("hello")',
        "semantic_value": 'unknown Command.Future("hello")',
        "decode_expectation": "reject",
        "purpose": "unknown variant input; no unknown-retention claim is made",
    },
    "malformed": {
        "source_expression": 'Command.Write(7)',
        "semantic_value": "known Command.Write with malformed payload shape",
        "decode_expectation": "reject",
        "purpose": "known variant with the wrong payload type",
    },
}

WIRE_PAYLOADS: dict[str, dict[str, str]] = {
    "external": {
        "payload": '{"Write":"hello"}',
        "evolution": '{"Archive":"hello"}',
        "unknown": '{"Future":"hello"}',
        "malformed": '{"Write":7}',
    },
    "internal": {
        "payload": '{"type":"Write","value":"hello"}',
        "evolution": '{"type":"Archive","value":"hello"}',
        "unknown": '{"type":"Future","value":"hello"}',
        "malformed": '{"type":"Write","value":7}',
    },
    "untagged": {
        "payload": '"hello"',
        "evolution": '{"Archive":"hello"}',
        "unknown": '{"Future":"hello"}',
        "malformed": "7",
    },
    "adjacent": {
        "payload": '{"kind":"Write","body":"hello"}',
        "evolution": '{"kind":"Archive","body":"hello"}',
        "unknown": '{"kind":"Future","body":"hello"}',
        "malformed": '{"kind":"Write","body":7}',
    },
}

FORM_INFO: dict[str, dict[str, Any]] = {
    "external": {
        "description": "externally tagged canonical enum representation",
        "entrypoint": "core.encoding.json.to_string/decode<T>",
        "marker": "none",
        "runtime_supported": True,
    },
    "internal": {
        "description": "internally tagged canonical enum representation",
        "entrypoint": "core.encoding.json.to_string/decode<T>",
        "marker": '#Discriminant("type")',
        "runtime_supported": True,
    },
    "untagged": {
        "description": "untagged canonical enum representation",
        "entrypoint": "core.encoding.json.to_string/decode<T>",
        "marker": "#Untagged",
        "runtime_supported": True,
    },
    "adjacent": {
        "description": "adjacently tagged tag/content representation",
        "entrypoint": "adjacent marker entry point",
        "marker": "unavailable",
        "runtime_supported": False,
    },
}


class ProtocolError(Exception):
    """The supplied adapter did not satisfy the explicit harness protocol."""


def positive_int(name: str, maximum: int) -> Callable[[str], int]:
    def parse(value: str) -> int:
        try:
            parsed = int(value)
        except ValueError as exc:
            raise argparse.ArgumentTypeError(f"{name} must be an integer") from exc
        if parsed <= 0 or parsed > maximum:
            raise argparse.ArgumentTypeError(
                f"{name} must be between 1 and {maximum}"
            )
        return parsed

    return parse


def positive_float(name: str, maximum: float) -> Callable[[str], float]:
    def parse(value: str) -> float:
        try:
            parsed = float(value)
        except ValueError as exc:
            raise argparse.ArgumentTypeError(f"{name} must be a number") from exc
        if not math.isfinite(parsed) or parsed <= 0 or parsed > maximum:
            raise argparse.ArgumentTypeError(
                f"{name} must be greater than zero and at most {maximum:g}"
            )
        return parsed

    return parse


def _same_or_child(path: Path, parent: Path) -> bool:
    return path == parent or parent in path.parents


def safe_scratch_root(raw: str) -> Path:
    """Resolve and validate a disk-oriented scratch root before creating it."""
    candidate = Path(raw).expanduser()
    if not candidate.is_absolute():
        candidate = Path.cwd() / candidate
    resolved = candidate.resolve()
    tmp_root = Path("/tmp").resolve()
    target_root = (ROOT / "target").resolve()
    if resolved == Path("/"):
        raise ValueError("scratch root must not be the filesystem root")
    if _same_or_child(resolved, tmp_root):
        raise ValueError("scratch root must not be /tmp or a child of /tmp")
    if _same_or_child(resolved, target_root):
        raise ValueError("scratch root must not be the repository target directory")
    resolved.mkdir(parents=True, exist_ok=True)
    if not os.access(resolved, os.W_OK):
        raise ValueError(f"scratch root is not writable: {resolved}")
    return resolved


def validate_workload() -> dict[str, Any]:
    """Return static checks without parsing or re-encoding fixture wire values."""
    checks: list[dict[str, str]] = []
    if tuple(FORM_INFO) != FORMS:
        raise ProtocolError("form inventory is not deterministic")
    if tuple(WORKLOAD_CASES) != CASES:
        raise ProtocolError("case inventory is not deterministic")
    for form in FORMS:
        if tuple(WIRE_PAYLOADS[form]) != CASES:
            raise ProtocolError(f"wire fixture inventory is incomplete for {form}")
        for case in CASES:
            wire = WIRE_PAYLOADS[form][case]
            if not wire or "\n" in wire:
                raise ProtocolError(f"wire fixture is empty or multiline: {form}/{case}")
            wire.encode("utf-8")
    semantic = WORKLOAD_CASES["payload"]["semantic_value"]
    if semantic != 'Command.Write("hello")':
        raise ProtocolError("known payload semantic value changed")
    checks.extend(
        [
            {"name": "form_inventory", "status": "ok"},
            {"name": "case_inventory", "status": "ok"},
            {"name": "equal_known_payload", "status": "ok"},
            {"name": "wire_fixtures_are_bounded", "status": "ok"},
            {"name": "adjacent_is_explicitly_unavailable", "status": "ok"},
        ]
    )
    return {"status": "ok", "checks": checks}


def workload_description() -> dict[str, Any]:
    return {
        "id": "enum-codec-wave-a-v1",
        "card": 3151,
        "protocol_version": PROTOCOL_VERSION,
        "enum": "Command",
        "variants": [
            {"name": "Read", "payload": "unit"},
            {"name": "Write", "payload": "String"},
        ],
        "forms": [
            {
                "name": form,
                **FORM_INFO[form],
                "wire_payloads": WIRE_PAYLOADS[form],
            }
            for form in FORMS
        ],
        "cases": [
            {"name": name, **details, "wire_is_fixture_only": True}
            for name, details in WORKLOAD_CASES.items()
        ],
        "equal_payload": {
            "case": "payload",
            "semantic_value": WORKLOAD_CASES["payload"]["semantic_value"],
            "wire_payload_by_form": {
                form: WIRE_PAYLOADS[form]["payload"]
                for form in FORMS
            },
        },
        "no_peer_comparison": True,
    }


def _env_value(cli_value: str | None, name: str) -> str:
    if cli_value:
        return cli_value
    return os.environ.get(name, "unavailable") or "unavailable"


def metadata(args: argparse.Namespace, command_text: str | None) -> dict[str, Any]:
    compiler = _env_value(args.compiler, "JET_ENUM_CODEC_COMPILER")
    compiler_version = _env_value(args.compiler_version, "JET_ENUM_CODEC_COMPILER_VERSION")
    toolchain = _env_value(args.toolchain, "JET_ENUM_CODEC_TOOLCHAIN")
    library = _env_value(args.library, "JET_ENUM_CODEC_LIBRARY")
    library_version = _env_value(args.library_version, "JET_ENUM_CODEC_LIBRARY_VERSION")
    entrypoint = os.environ.get(
        "JET_ENUM_CODEC_ENTRYPOINT", "core.encoding.json.to_string/decode<T>"
    )
    optimized_env = os.environ.get("JET_ENUM_CODEC_OPTIMIZED")
    optimized_verified: bool | None
    if optimized_env is None:
        optimized_verified = None
    else:
        optimized_verified = optimized_env.strip().lower() in {"1", "true", "yes", "on"}
    return {
        "compiler": {
            "name": compiler,
            "version": compiler_version,
            "source": "environment-or-cli" if compiler != "unavailable" else "unavailable",
        },
        "toolchain": {
            "name": toolchain,
            "python": platform.python_version(),
            "runner": sys.executable,
        },
        "library": {
            "name": library,
            "version": library_version,
            "entrypoint": entrypoint,
            "status": "provided-command-not-verified"
            if command_text
            else "canonical-entrypoint-not-provided",
        },
        "optimized": {
            "requested_profile": args.profile,
            "requested": args.profile == "release",
            "verified": optimized_verified,
            "settings": {
                "profile": args.profile,
                "debug_assertions": False if args.profile == "release" else None,
                "lto": os.environ.get("JET_ENUM_CODEC_LTO", "unavailable"),
            },
        },
        "command": {
            "configured": bool(command_text),
            "template": command_text or "unavailable",
        },
        "repository_commit": os.environ.get(
            "JET_ENUM_CODEC_COMMIT", "unavailable"
        ),
    }


def _format_command(
    command_text: str,
    *,
    operation: str,
    form: str,
    case: str,
    input_path: Path,
    expected_path: Path,
    output_path: Path,
    metrics_path: Path,
    rounds: int,
) -> list[str]:
    try:
        parts = shlex.split(command_text)
    except ValueError as exc:
        raise ProtocolError(f"invalid command quoting: {exc}") from exc
    if not parts:
        raise ProtocolError("configured command is empty")
    values = {
        "operation": operation,
        "form": form,
        "case": case,
        "input": str(input_path),
        "expected": str(expected_path),
        "output": str(output_path),
        "metrics": str(metrics_path),
        "rounds": str(rounds),
    }
    placeholders = set(values)
    formatted: list[str] = []
    used_placeholder = False
    for part in parts:
        if "{" in part or "}" in part:
            try:
                formatted_part = part.format(**values)
            except (KeyError, IndexError, ValueError) as exc:
                raise ProtocolError(f"unknown command placeholder in {part!r}") from exc
            used_placeholder = used_placeholder or formatted_part != part or "{" in part
            formatted.append(formatted_part)
        else:
            formatted.append(part)
    if not used_placeholder:
        formatted.extend(
            [
                "--operation",
                operation,
                "--form",
                form,
                "--case",
                case,
                "--input",
                str(input_path),
                "--expected",
                str(expected_path),
                "--output",
                str(output_path),
                "--metrics",
                str(metrics_path),
                "--rounds",
                str(rounds),
            ]
        )
    return formatted


def _limit_child(cpu_seconds: int, memory_limit_mib: int, max_output_bytes: int) -> None:
    if resource is None:
        return
    try:
        cpu_limit = getattr(resource, "RLIMIT_CPU", None)
        if cpu_limit is not None:
            resource.setrlimit(cpu_limit, (cpu_seconds, cpu_seconds + 1))
        address_limit = getattr(resource, "RLIMIT_AS", None)
        if address_limit is not None:
            memory_bytes = memory_limit_mib * 1024 * 1024
            resource.setrlimit(address_limit, (memory_bytes, memory_bytes))
        file_limit = getattr(resource, "RLIMIT_FSIZE", None)
        if file_limit is not None:
            resource.setrlimit(file_limit, (max_output_bytes, max_output_bytes))
    except (OSError, ValueError):
        # A platform may expose a limit but reject changing it.  The wall-time
        # guard still applies, and the report retains the requested limits.
        return


def _kill_process_group(process: subprocess.Popen[bytes]) -> None:
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            return
        try:
            process.wait(timeout=1)
            return
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                return
    try:
        process.kill()
    except ProcessLookupError:
        return
    process.wait()


def _read_bounded(path: Path, maximum: int) -> bytes:
    if not path.exists():
        return b""
    with path.open("rb") as stream:
        value = stream.read(maximum + 1)
    if len(value) > maximum:
        raise ProtocolError(f"adapter output exceeded {maximum} bytes: {path.name}")
    return value


def _load_metrics(path: Path, maximum: int) -> dict[str, Any]:
    if not path.exists() or path.stat().st_size == 0:
        return {}
    raw = _read_bounded(path, maximum)
    try:
        value = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ProtocolError(f"metrics file is not JSON: {path.name}") from exc
    if not isinstance(value, dict):
        raise ProtocolError("metrics JSON must be an object")
    for field in ("codec_ns", "allocation_count"):
        if field in value and (
            not isinstance(value[field], int) or isinstance(value[field], bool) or value[field] < 0
        ):
            raise ProtocolError(f"metrics field {field} must be a non-negative integer")
    return value


def _sample_command(
    command_text: str,
    *,
    form: str,
    case: str,
    operation: str,
    input_path: Path,
    expected_path: Path,
    expected_wire: bytes,
    expected_decode: str,
    sample_index: int,
    scratch: Path,
    args: argparse.Namespace,
    environment: dict[str, str],
) -> dict[str, Any]:
    stem = f"{form}-{case}-{operation}-{sample_index}"
    output_path = scratch / f"{stem}.output"
    metrics_path = scratch / f"{stem}.metrics.json"
    stdout_path = scratch / f"{stem}.stdout"
    stderr_path = scratch / f"{stem}.stderr"
    for stale in (output_path, metrics_path, stdout_path, stderr_path):
        stale.unlink(missing_ok=True)
    command = _format_command(
        command_text,
        operation=operation,
        form=form,
        case=case,
        input_path=input_path,
        expected_path=expected_path,
        output_path=output_path,
        metrics_path=metrics_path,
        rounds=args.rounds,
    )
    child_env = dict(environment)
    child_env.update(
        {
            "JET_ENUM_CODEC_BENCH_PROTOCOL": str(PROTOCOL_VERSION),
            "JET_ENUM_CODEC_BENCH_PROFILE": args.profile,
        }
    )
    started = time.perf_counter_ns()
    process: subprocess.Popen[bytes] | None = None
    try:
        with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
            popen_kwargs: dict[str, Any] = {
                "cwd": ROOT,
                "env": child_env,
                "stdout": stdout,
                "stderr": stderr,
            }
            if os.name == "posix":
                popen_kwargs["start_new_session"] = True
                popen_kwargs["preexec_fn"] = lambda: _limit_child(
                    max(1, math.ceil(args.timeout_seconds)),
                    args.memory_limit_mib,
                    args.max_output_bytes,
                )
            process = subprocess.Popen(command, **popen_kwargs)
            try:
                returncode = process.wait(timeout=args.timeout_seconds)
                timed_out = False
            except subprocess.TimeoutExpired:
                _kill_process_group(process)
                returncode = process.returncode if process.returncode is not None else -signal.SIGTERM
                timed_out = True
    except OSError as exc:
        elapsed = time.perf_counter_ns() - started
        return {
            "status": "error",
            "reason": f"could not start canonical codec command: {exc}",
            "returncode": None,
            "wall_ns": elapsed,
            "codec_ns": None,
            "allocation_count": None,
            "encoded_bytes": len(expected_wire),
            "stderr": "",
        }
    elapsed = time.perf_counter_ns() - started
    try:
        stdout_bytes = _read_bounded(stdout_path, args.max_output_bytes)
        stderr_bytes = _read_bounded(stderr_path, args.max_output_bytes)
        metrics = _load_metrics(metrics_path, args.max_output_bytes)
    except ProtocolError as exc:
        return {
            "status": "error",
            "reason": str(exc),
            "returncode": returncode,
            "wall_ns": elapsed,
            "codec_ns": None,
            "allocation_count": None,
            "encoded_bytes": len(expected_wire),
            "stderr": "",
        }
    stderr_text = stderr_bytes.decode("utf-8", errors="replace")[:4096]
    if timed_out:
        return {
            "status": "timeout",
            "reason": f"adapter exceeded {args.timeout_seconds:g}s timeout",
            "returncode": returncode,
            "wall_ns": elapsed,
            "codec_ns": None,
            "allocation_count": None,
            "encoded_bytes": len(expected_wire),
            "stderr": stderr_text,
        }
    if operation == "encode":
        try:
            actual_wire = _read_bounded(output_path, args.max_output_bytes)
        except ProtocolError as exc:
            return {
                "status": "error",
                "reason": str(exc),
                "returncode": returncode,
                "wall_ns": elapsed,
                "codec_ns": None,
                "allocation_count": None,
                "encoded_bytes": len(expected_wire),
                "stderr": stderr_text,
            }
        if returncode != 0:
            return {
                "status": "error",
                "reason": f"encode command exited {returncode}",
                "returncode": returncode,
                "wall_ns": elapsed,
                "codec_ns": metrics.get("codec_ns"),
                "allocation_count": metrics.get("allocation_count"),
                "encoded_bytes": len(actual_wire),
                "stderr": stderr_text,
            }
        if actual_wire != expected_wire:
            return {
                "status": "error",
                "reason": "canonical encode output differs from the fixed wire fixture",
                "returncode": returncode,
                "wall_ns": elapsed,
                "codec_ns": metrics.get("codec_ns"),
                "allocation_count": metrics.get("allocation_count"),
                "encoded_bytes": len(actual_wire),
                "stderr": stderr_text,
            }
        return {
            "status": "measured",
            "reason": "canonical encode output matched the fixed wire fixture",
            "returncode": returncode,
            "wall_ns": elapsed,
            "codec_ns": metrics.get("codec_ns"),
            "allocation_count": metrics.get("allocation_count"),
            "encoded_bytes": len(actual_wire),
            "stderr": stderr_text,
            "stdout_bytes": len(stdout_bytes),
        }
    if expected_decode == "accept":
        status = "measured" if returncode == 0 else "error"
        reason = "canonical decode accepted the fixed wire fixture" if returncode == 0 else f"decode command exited {returncode} for an accepting fixture"
        outcome = "accepted" if returncode == 0 else "failed"
    else:
        status = "measured" if returncode != 0 else "error"
        reason = "canonical decode rejected the expected-invalid fixture" if returncode != 0 else "decode command accepted a fixture expected to be rejected"
        outcome = "rejected" if returncode != 0 else "accepted"
    return {
        "status": status,
        "reason": reason,
        "returncode": returncode,
        "wall_ns": elapsed,
        "codec_ns": metrics.get("codec_ns"),
        "allocation_count": metrics.get("allocation_count"),
        "encoded_bytes": len(expected_wire),
        "decode_outcome": outcome,
        "stderr": stderr_text,
        "stdout_bytes": len(stdout_bytes),
    }


def _base_cell(form: str, case: str, operation: str) -> dict[str, Any]:
    wire = WIRE_PAYLOADS[form][case]
    return {
        "id": f"{form}/{case}/{operation}",
        "form": form,
        "case": case,
        "operation": operation,
        "source_expression": WORKLOAD_CASES[case]["source_expression"],
        "wire_payload": wire,
        "wire_payload_bytes": len(wire.encode("utf-8")),
        "decode_expectation": WORKLOAD_CASES[case]["decode_expectation"],
        "status": "unavailable",
        "reason": UNAVAILABLE,
        "sample_results": [],
        "process_wall_median_ns": None,
        "codec_median_ns": None,
        "encode_median_ns": None,
        "decode_median_ns": None,
        "encode_time_ns": None,
        "decode_time_ns": None,
        "allocation_count": None,
        "allocation_median": None,
        "encoded_bytes": len(wire.encode("utf-8")),
        "encoded_bytes_status": "fixture",
        "allocation_status": "unavailable",
        "timing_status": "unavailable",
        "field_status": {
            "encoded_bytes": "fixture",
            "allocation_count": "unavailable",
            "encode_median_ns": "not_applicable" if operation == "decode" else "unavailable",
            "decode_median_ns": "not_applicable" if operation == "encode" else "unavailable",
        },
    }


def _run_cell(
    base: dict[str, Any],
    *,
    command_text: str,
    scratch: Path,
    fixture_paths: dict[tuple[str, str], tuple[Path, Path]],
    args: argparse.Namespace,
    environment: dict[str, str],
) -> dict[str, Any]:
    form = base["form"]
    case = base["case"]
    operation = base["operation"]
    source_path, wire_path = fixture_paths[(form, case)]
    input_path = source_path if operation == "encode" else wire_path
    expected_path = wire_path if operation == "encode" else source_path
    expected_wire = WIRE_PAYLOADS[form][case].encode("utf-8")
    samples = [
        _sample_command(
            command_text,
            form=form,
            case=case,
            operation=operation,
            input_path=input_path,
            expected_path=expected_path,
            expected_wire=expected_wire,
            expected_decode=WORKLOAD_CASES[case]["decode_expectation"],
            sample_index=index,
            scratch=scratch,
            args=args,
            environment=environment,
        )
        for index in range(args.samples)
    ]
    base["sample_results"] = samples
    statuses = [sample["status"] for sample in samples]
    if any(status == "timeout" for status in statuses):
        base["status"] = "timeout"
        base["reason"] = "at least one sample exceeded the timeout"
    elif any(status == "error" for status in statuses):
        base["status"] = "error"
        base["reason"] = "at least one sample failed the adapter protocol or fixture check"
    elif all(status == "measured" for status in statuses):
        base["status"] = "measured"
        base["reason"] = "all samples completed with the expected codec outcome"
        wall = [int(sample["wall_ns"]) for sample in samples]
        base["process_wall_median_ns"] = int(statistics.median(wall))
        codec = [sample["codec_ns"] for sample in samples]
        allocations = [sample["allocation_count"] for sample in samples]
        if all(isinstance(value, int) for value in codec):
            base["codec_median_ns"] = int(statistics.median(codec))
            base["timing_status"] = "adapter_codec_and_process_wall"
        else:
            base["timing_status"] = "process_wall_only;adapter_codec_ns_unavailable"
        if all(isinstance(value, int) for value in allocations):
            base["allocation_median"] = int(statistics.median(allocations))
            base["allocation_count"] = base["allocation_median"]
            base["allocation_status"] = "measured"
        else:
            base["allocation_status"] = "adapter_allocation_count_unavailable"
        base["encoded_bytes_status"] = "verified" if operation == "encode" else "fixture"
    else:
        base["status"] = "error"
        base["reason"] = "no successful samples"
    if operation == "encode":
        base["encode_median_ns"] = base["codec_median_ns"]
        base["decode_median_ns"] = None
    else:
        base["encode_median_ns"] = None
        base["decode_median_ns"] = base["codec_median_ns"]
    base["encode_time_ns"] = base["encode_median_ns"]
    base["decode_time_ns"] = base["decode_median_ns"]
    base["field_status"]["encode_median_ns"] = (
        "measured" if base["encode_median_ns"] is not None else base["field_status"]["encode_median_ns"]
    )
    base["field_status"]["decode_median_ns"] = (
        "measured" if base["decode_median_ns"] is not None else base["field_status"]["decode_median_ns"]
    )
    return base


def make_cells(
    *,
    command_text: str | None,
    run: bool,
    args: argparse.Namespace,
    scratch: Path | None,
) -> list[dict[str, Any]]:
    cells: list[dict[str, Any]] = []
    fixture_paths: dict[tuple[str, str], tuple[Path, Path]] = {}
    environment = os.environ.copy()
    if run and command_text and scratch is not None:
        for form in FORMS:
            for case in CASES:
                stem = f"{form}-{case}"
                source_path = scratch / f"{stem}.source"
                wire_path = scratch / f"{stem}.wire"
                source_path.write_text(
                    WORKLOAD_CASES[case]["source_expression"] + "\n", encoding="utf-8"
                )
                wire_path.write_bytes(WIRE_PAYLOADS[form][case].encode("utf-8"))
                fixture_paths[(form, case)] = (source_path, wire_path)
    for form in FORMS:
        for case in CASES:
            for operation in OPERATIONS:
                cell = _base_cell(form, case, operation)
                if form == "adjacent":
                    cell["status"] = "unavailable"
                    cell["reason"] = ADJACENT_UNAVAILABLE
                elif operation == "encode" and case != "payload":
                    cell["status"] = "unsupported"
                    if case == "malformed":
                        cell["reason"] = (
                            "malformed wrong-shape witness is decode-only; no valid "
                            "Command constructor can encode it"
                        )
                    else:
                        cell["reason"] = (
                            "no source constructor exists in the closed Command workload "
                            f"for the {case} witness; decode-only rejection cell"
                        )
                elif not command_text:
                    cell["status"] = "unavailable"
                    cell["reason"] = (
                        "runtime requested without an existing canonical codec command; "
                        "no command was executed"
                    )
                elif not run:
                    cell["status"] = "unavailable"
                    cell["reason"] = (
                        "static/dry mode selected; provide --run and an existing canonical "
                        "codec command to measure this cell"
                    )
                else:
                    cell = _run_cell(
                        cell,
                        command_text=command_text,
                        scratch=scratch,
                        fixture_paths=fixture_paths,
                        args=args,
                        environment=environment,
                    )
                cells.append(cell)
    return cells


def summarize(cells: list[dict[str, Any]]) -> dict[str, Any]:
    counts = Counter(cell["status"] for cell in cells)
    unavailable = [
        {
            "id": cell["id"],
            "status": cell["status"],
            "reason": cell["reason"],
        }
        for cell in cells
        if cell["status"] in {"unavailable", "unsupported"}
    ]
    measured = counts.get("measured", 0)
    if counts.get("error") or counts.get("timeout"):
        status = "error"
    elif measured and len(counts) == 1:
        status = "measured"
    elif measured:
        status = "partial"
    else:
        status = "unavailable"
    return {
        "status": status,
        "cell_counts": dict(sorted(counts.items())),
        "measured_cells": measured,
        "unavailable_cells": unavailable,
        "measurement_cells_remain_unavailable": bool(unavailable),
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    execution = parser.add_mutually_exclusive_group()
    execution.add_argument(
        "--run",
        action="store_true",
        help="run an explicitly supplied existing canonical codec adapter",
    )
    execution.add_argument(
        "--dry-run",
        action="store_true",
        help="emit static workload/unavailable cells without running a command (default)",
    )
    parser.add_argument(
        "--command",
        default=None,
        help="existing adapter command; also reads JET_ENUM_CODEC_BENCH_COMMAND",
    )
    parser.add_argument(
        "--samples",
        type=positive_int("--samples", 99),
        default=5,
        help="number of fresh samples per executable cell (default: 5)",
    )
    parser.add_argument(
        "--rounds",
        type=positive_int("--rounds", 1_000_000),
        default=1,
        help="equal workload repetitions requested from the adapter (default: 1)",
    )
    parser.add_argument(
        "--timeout-seconds",
        "--timeout",
        dest="timeout_seconds",
        type=positive_float("--timeout-seconds", 3600.0),
        default=30.0,
        help="wall timeout per sample (default: 30 seconds)",
    )
    parser.add_argument(
        "--memory-limit-mib",
        type=positive_int("--memory-limit-mib", 65_536),
        default=1024,
        help="best-effort child address-space limit on Unix (default: 1024)",
    )
    parser.add_argument(
        "--max-output-bytes",
        type=positive_int("--max-output-bytes", 16 * 1024 * 1024),
        default=1 * 1024 * 1024,
        help="maximum adapter/output/metrics file size (default: 1048576)",
    )
    parser.add_argument(
        "--scratch-root",
        default=os.environ.get("JET_ENUM_CODEC_BENCH_SCRATCH_ROOT", str(SCRATCH_DEFAULT)),
        help="disk-oriented scratch parent; /tmp and repository target are rejected",
    )
    parser.add_argument(
        "--profile",
        choices=("release", "debug"),
        default="release",
        help="requested optimized profile metadata (does not verify the adapter)",
    )
    parser.add_argument("--compiler", help="compiler name metadata override")
    parser.add_argument("--compiler-version", help="compiler version metadata override")
    parser.add_argument("--toolchain", help="toolchain metadata override")
    parser.add_argument("--library", help="codec library metadata override")
    parser.add_argument("--library-version", help="codec library version metadata override")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        static_validation = validate_workload()
    except ProtocolError as exc:
        parser.error(f"workload validation failed: {exc}")
    command_text = args.command
    if command_text is None:
        command_text = os.environ.get("JET_ENUM_CODEC_BENCH_COMMAND")
    if args.run and command_text is not None and not command_text.strip():
        parser.error("--command must not be empty when --run is used")
    # Static mode is intentionally the default even when an environment command
    # exists; only --run authorizes process execution.
    do_run = bool(args.run and command_text)
    scratch_path: Path | None = None
    if do_run:
        try:
            scratch_path = safe_scratch_root(args.scratch_root)
        except ValueError as exc:
            parser.error(str(exc))
    try:
        if do_run:
            with tempfile.TemporaryDirectory(prefix=".enum-codec-bench-", dir=scratch_path) as raw:
                scratch = Path(raw)
                cells = make_cells(
                    command_text=command_text,
                    run=True,
                    args=args,
                    scratch=scratch,
                )
        else:
            cells = make_cells(
                command_text=command_text,
                run=args.run,
                args=args,
                scratch=None,
            )
    except ProtocolError as exc:
        parser.error(str(exc))
    report = {
        "benchmark": "enum-codec-wave-a-v1",
        "card": 3151,
        "mode": "run" if args.run else "dry-run",
        "protocol_version": PROTOCOL_VERSION,
        "harness": str(SCRIPT.relative_to(ROOT)),
        "static_validation": static_validation,
        "workload": workload_description(),
        "metadata": metadata(args, command_text),
        "limits": {
            "samples": args.samples,
            "rounds": args.rounds,
            "timeout_seconds": args.timeout_seconds,
            "memory_limit_mib": args.memory_limit_mib,
            "max_output_bytes": args.max_output_bytes,
            "scratch_root": str(scratch_path)
            if do_run
            else str(Path(args.scratch_root).expanduser()),
        },
        "cells": cells,
        "summary": summarize(cells),
        "peer_comparison": {
            "status": "not_run",
            "reason": "peer comparison is outside card #3151 harness scope",
            "performance_win_claim": False,
            "arms": [],
        },
        "receipt": {
            "status": "not_available",
            "text": None,
            "reason": "no canonical runtime receipt is claimed by this harness",
        },
    }
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
