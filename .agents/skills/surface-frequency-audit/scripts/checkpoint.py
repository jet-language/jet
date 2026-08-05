#!/usr/bin/env python3
"""Atomic, resumable checkpoints for a surface-frequency audit run."""

from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from typing import Any, Iterator


SCHEMA = 1
UNIT_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
TERMINAL = {"done", "blocked", "unavailable"}
REPO = Path(__file__).resolve().parents[4]
RUN_BASE = (REPO / ".tmp" / "surface-frequency-audit").resolve()
REPORT_BASE = (REPO / "docs" / "audits").resolve()
REPORT_NAME = re.compile(r"^surface-frequency-audit-\d{4}-\d{2}-\d{2}(?:-[A-Za-z0-9._-]+)?\.md$")
REPORT_HEADINGS = (
    "# Surface frequency audit",
    "## Executive summary",
    "### Decision view",
    "## What people do most",
    "## Which surfaces they use",
    "## Beginner adoption path",
    "## Expert production path",
    "## Jet recommendations",
    "## Keep",
    "## Watchlist",
    "## What changes the ranking",
    "## Coverage and limits",
    "## Methods and provenance",
    "## Sources",
)


class AuditError(Exception):
    pass


def now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")


def parse_time(value: str | None) -> dt.datetime | None:
    return dt.datetime.fromisoformat(value) if value else None


def canonical(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n").encode()


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_digest(path: Path) -> str:
    return digest(path.read_bytes())


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise AuditError(f"cannot read JSON {path}: {error}") from error


def atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temp_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp_name, path)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(temp_name)


def write_json(path: Path, value: Any) -> None:
    atomic_write(path, canonical(value))


def paths(run_dir: Path) -> tuple[Path, Path, Path]:
    return run_dir / "run.json", run_dir / "units", run_dir / "results"


def require_run_dir(run_dir: Path) -> None:
    if run_dir.parent != RUN_BASE:
        raise AuditError(f"run directory must be a direct child of {RUN_BASE}")


def require_report_path(report_path: Path) -> None:
    if report_path.parent != REPORT_BASE or not REPORT_NAME.fullmatch(report_path.name):
        raise AuditError(f"report must be a dated surface-frequency audit under {REPORT_BASE}")


def require_inside(path: Path, parent: Path, label: str) -> None:
    if not path.is_relative_to(parent):
        raise AuditError(f"{label} must stay inside {parent}")


def load_run(run_dir: Path, check_config: bool = True) -> dict[str, Any]:
    require_run_dir(run_dir)
    run_path, _, _ = paths(run_dir)
    run = load_json(run_path)
    if run.get("schema") != SCHEMA:
        raise AuditError(f"unsupported run schema: {run.get('schema')}")
    if check_config:
        changed = []
        for item in run.get("configs", []):
            path = Path(item["path"])
            if not path.is_file() or file_digest(path) != item["sha256"]:
                changed.append(str(path))
        if changed:
            raise AuditError("pinned config changed or disappeared: " + ", ".join(changed))
    return run


@contextlib.contextmanager
def locked(path: Path) -> Iterator[None]:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+") as handle:
        fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
        yield


def unit_path(run_dir: Path, unit_id: str) -> Path:
    if not UNIT_ID.fullmatch(unit_id):
        raise AuditError(f"invalid unit id: {unit_id!r}")
    return run_dir / "units" / f"{unit_id}.json"


def load_unit(run_dir: Path, unit_id: str) -> dict[str, Any]:
    unit = load_json(unit_path(run_dir, unit_id))
    if unit.get("schema") != SCHEMA or unit.get("id") != unit_id:
        raise AuditError(f"invalid unit file for {unit_id}")
    return unit


def save_unit(run_dir: Path, unit: dict[str, Any]) -> None:
    write_json(unit_path(run_dir, unit["id"]), unit)


def event(unit: dict[str, Any], action: str, owner: str | None = None, note: str | None = None) -> None:
    row = {"at": now(), "action": action}
    if owner:
        row["owner"] = owner
    if note:
        row["note"] = note
    unit.setdefault("history", []).append(row)


def is_expired(unit: dict[str, Any]) -> bool:
    expiry = parse_time(unit.get("lease_expires_at"))
    return unit.get("state") == "claimed" and expiry is not None and expiry <= dt.datetime.now(dt.timezone.utc)


def claim_one(run_dir: Path, unit_id: str, owner: str, lease_hours: float) -> dict[str, Any] | None:
    lock_path = run_dir / "locks" / f"{unit_id}.lock"
    with locked(lock_path):
        unit = load_unit(run_dir, unit_id)
        state = unit["state"]
        if state == "claimed" and not is_expired(unit) and unit.get("owner") != owner:
            return None
        if state in TERMINAL:
            return None
        if state not in {"pending", "claimed"}:
            raise AuditError(f"invalid state for {unit_id}: {state}")
        action = "reclaimed" if is_expired(unit) else "claimed"
        unit["state"] = "claimed"
        unit["owner"] = owner
        unit["attempt"] = int(unit.get("attempt", 0)) + (action == "reclaimed" or state == "pending")
        unit["lease_expires_at"] = (
            dt.datetime.now(dt.timezone.utc) + dt.timedelta(hours=lease_hours)
        ).isoformat(timespec="seconds")
        event(unit, action, owner)
        save_unit(run_dir, unit)
        return unit


def validate_result(result: Any, unit_id: str) -> dict[str, Any]:
    if not isinstance(result, dict) or result.get("schema") != SCHEMA:
        raise AuditError("result must be a schema-1 JSON object")
    if result.get("unit_id") != unit_id:
        raise AuditError("result unit_id does not match the claimed unit")
    required = {"source_ids": list, "tool": dict, "coverage": dict, "measurements": list, "citations": list, "warnings": list}
    for key, kind in required.items():
        if not isinstance(result.get(key), kind):
            raise AuditError(f"result field {key!r} must be {kind.__name__}")
    if not result["source_ids"] or not result["measurements"] or not result["citations"]:
        raise AuditError("completed results need sources, measurements, and citations")
    for key in ("name", "version"):
        if not isinstance(result["tool"].get(key), str) or not result["tool"][key].strip():
            raise AuditError(f"tool.{key} must be a non-empty string")
    coverage = result["coverage"]
    for key in ("files_seen", "files_parsed", "files_skipped"):
        if not isinstance(coverage.get(key), int) or coverage[key] < 0:
            raise AuditError(f"coverage.{key} must be a non-negative integer")
    if coverage["files_seen"] != coverage["files_parsed"] + coverage["files_skipped"]:
        raise AuditError("coverage must account for every seen file")
    if not any(
        isinstance(coverage.get(key), (int, float))
        and not isinstance(coverage.get(key), bool)
        and coverage.get(key) >= 0
        for key in ("normalized_lines", "lexical_tokens")
    ):
        raise AuditError("coverage needs normalized_lines or lexical_tokens")
    if not all(isinstance(citation, str) and citation.strip() for citation in result["citations"]):
        raise AuditError("citations must be non-empty strings")
    identities = set()
    fields = ("operation_id", "feature_id", "level", "metric", "scope", "opportunity")
    for index, measurement in enumerate(result["measurements"]):
        if not isinstance(measurement, dict) or any(field not in measurement for field in fields):
            raise AuditError(f"measurement {index} misses its identity fields")
        identity = tuple(str(measurement[field]) for field in fields)
        if identity in identities:
            raise AuditError(f"duplicate measurement key: {identity}")
        identities.add(identity)
        if measurement["metric"] != "usage":
            raise AuditError("normalized measurement metric must be 'usage'")
        if measurement["level"] not in {"operation", "surface"}:
            raise AuditError("measurement level must be operation or surface")
        if not isinstance(measurement.get("ontology_ids"), list) or not measurement["ontology_ids"]:
            raise AuditError(f"measurement {index} needs ontology_ids")
        if measurement.get("difficulty") not in {"entry", "general", "expert", "unknown"}:
            raise AuditError(f"measurement {index} has invalid difficulty")
        if not isinstance(measurement.get("surface"), str) or not measurement["surface"].strip():
            raise AuditError(f"measurement {index} needs a surface label")
        if not isinstance(measurement.get("eligible"), bool):
            raise AuditError(f"measurement {index} eligible must be boolean")
        if not isinstance(measurement.get("source_sites"), list):
            raise AuditError(f"measurement {index} source_sites must be a list")
        numerator = measurement.get("numerator")
        denominator = measurement.get("denominator")
        if numerator == "not-recorded" or denominator == "not-recorded":
            if numerator != "not-recorded" or denominator != "not-recorded":
                raise AuditError("numerator and denominator must both be not-recorded")
            continue
        for name, value in (("numerator", numerator), ("denominator", denominator)):
            if isinstance(value, bool) or not isinstance(value, (int, float)) or value < 0:
                raise AuditError(f"measurement {index} {name} must be non-negative")
        if numerator > denominator:
            raise AuditError(f"measurement {index} numerator exceeds denominator")
        if measurement["eligible"] and denominator == 0:
            raise AuditError(f"measurement {index} eligible row has zero denominator")
        if numerator > 0 and not measurement["source_sites"]:
            raise AuditError(f"measurement {index} positive use needs a sampled source site")
    return result


def measurement_keys(items: list[dict[str, Any]]) -> set[tuple[str, str, str, str, str, str]]:
    fields = ("operation_id", "feature_id", "level", "metric", "scope", "opportunity")
    return {tuple(str(item[field]) for field in fields) for item in items}


def measurement_signatures(items: list[dict[str, Any]]) -> set[tuple[Any, ...]]:
    signatures = set()
    fields = ("operation_id", "feature_id", "level", "metric", "scope", "opportunity")
    for item in items:
        key = tuple(str(item[field]) for field in fields)
        signatures.add(
            (
                *key,
                tuple(sorted(str(value) for value in item.get("ontology_ids", []))),
                str(item.get("surface")),
                str(item.get("difficulty")),
            )
        )
    return signatures


def validate_catalog(raw: Any) -> dict[str, dict[str, Any]]:
    if not isinstance(raw, dict) or raw.get("schema") != SCHEMA or not isinstance(raw.get("catalogs"), list):
        raise AuditError("catalog file must contain a schema-1 catalogs list")
    catalogs: dict[str, dict[str, Any]] = {}
    for entry in raw["catalogs"]:
        required = ("id", "language", "version", "official_sources", "official_sections_total", "official_sections_mapped", "unmatched_sections", "built_by", "reviewed_by", "reviewed_at", "official_sections", "measurements")
        if not isinstance(entry, dict) or any(key not in entry for key in required):
            raise AuditError("catalog entry misses required provenance")
        catalog_id = entry["id"]
        if not isinstance(catalog_id, str) or not UNIT_ID.fullmatch(catalog_id) or catalog_id in catalogs:
            raise AuditError(f"invalid or duplicate catalog id: {catalog_id!r}")
        for key in ("language", "version", "built_by", "reviewed_by", "reviewed_at"):
            if not isinstance(entry[key], str) or not entry[key].strip():
                raise AuditError(f"catalog {catalog_id} needs {key}")
        if entry["built_by"] == entry["reviewed_by"]:
            raise AuditError(f"catalog {catalog_id} needs an independent reviewer")
        if not isinstance(entry["official_sources"], list) or not entry["official_sources"] or not all(
            isinstance(url, str) and url.startswith(("https://", "http://")) for url in entry["official_sources"]
        ):
            raise AuditError(f"catalog {catalog_id} needs official source URLs")
        total = entry["official_sections_total"]
        mapped = entry["official_sections_mapped"]
        unmatched = entry["unmatched_sections"]
        if not isinstance(total, int) or not isinstance(mapped, int) or total <= 0 or mapped <= 0:
            raise AuditError(f"catalog {catalog_id} has invalid section counts")
        if not isinstance(unmatched, list) or not all(isinstance(item, str) and item for item in unmatched):
            raise AuditError(f"catalog {catalog_id} has invalid unmatched sections")
        if mapped + len(unmatched) != total:
            raise AuditError(f"catalog {catalog_id} section counts do not reconcile")
        measurements = entry["measurements"]
        fields = ("operation_id", "feature_id", "level", "metric", "scope", "opportunity", "ontology_ids", "surface", "difficulty")
        if not isinstance(measurements, list) or not measurements or any(
            not isinstance(row, dict) or any(field not in row for field in fields) for row in measurements
        ):
            raise AuditError(f"catalog {catalog_id} has invalid measurements")
        if len(measurement_keys(measurements)) != len(measurements):
            raise AuditError(f"catalog {catalog_id} has duplicate measurements")
        for row in measurements:
            if row["metric"] != "usage" or row["level"] not in {"operation", "surface"}:
                raise AuditError(f"catalog {catalog_id} has invalid measurement kind")
            if not isinstance(row["ontology_ids"], list) or not row["ontology_ids"]:
                raise AuditError(f"catalog {catalog_id} measurement needs ontology_ids")
            if row["difficulty"] not in {"entry", "general", "expert", "unknown"}:
                raise AuditError(f"catalog {catalog_id} measurement has invalid difficulty")
            if not isinstance(row["surface"], str) or not row["surface"].strip():
                raise AuditError(f"catalog {catalog_id} measurement needs a surface label")
        operation_ids = {row["operation_id"] for row in measurements if row["level"] == "operation"}
        if any(row["operation_id"] not in operation_ids for row in measurements if row["level"] == "surface"):
            raise AuditError(f"catalog {catalog_id} has a surface without an operation row")
        sections = entry["official_sections"]
        if not isinstance(sections, list) or len(sections) != total:
            raise AuditError(f"catalog {catalog_id} needs its full official section inventory")
        section_ids = set()
        mapped_keys: set[tuple[str, str, str, str, str, str]] = set()
        unmatched_ids = set()
        mapped_count = 0
        valid_keys = measurement_keys(measurements)
        for section in sections:
            if not isinstance(section, dict) or any(key not in section for key in ("id", "url", "status", "measurement_keys", "reason")):
                raise AuditError(f"catalog {catalog_id} has an invalid official section")
            section_id = section["id"]
            if not isinstance(section_id, str) or not section_id or section_id in section_ids:
                raise AuditError(f"catalog {catalog_id} has a duplicate official section")
            section_ids.add(section_id)
            if not isinstance(section["url"], str) or not section["url"].startswith(("https://", "http://")):
                raise AuditError(f"catalog {catalog_id} section needs an official URL")
            keys = section["measurement_keys"]
            if section["status"] == "mapped":
                if not isinstance(keys, list) or not keys:
                    raise AuditError(f"catalog {catalog_id} mapped section needs measurement keys")
                converted = {tuple(str(value) for value in key) for key in keys if isinstance(key, list) and len(key) == 6}
                if len(converted) != len(keys) or not converted.issubset(valid_keys):
                    raise AuditError(f"catalog {catalog_id} section maps unknown measurement keys")
                mapped_keys.update(converted)
                mapped_count += 1
            elif section["status"] == "unmatched":
                if keys or not isinstance(section["reason"], str) or not section["reason"].strip():
                    raise AuditError(f"catalog {catalog_id} unmatched section needs only a reason")
                unmatched_ids.add(section_id)
            else:
                raise AuditError(f"catalog {catalog_id} section has invalid status")
        if mapped_count != mapped or unmatched_ids != set(unmatched) or mapped_keys != valid_keys:
            raise AuditError(f"catalog {catalog_id} section-to-measurement map does not reconcile")
        catalogs[catalog_id] = entry
    if not catalogs:
        raise AuditError("catalog file is empty")
    return catalogs


def validate_partial(partial: Any, unit: dict[str, Any]) -> None:
    if not isinstance(partial, dict) or partial.get("schema") != SCHEMA or partial.get("unit_id") != unit["id"]:
        raise AuditError("partial checkpoint must be schema 1 and name the unit_id")
    completed = partial.get("completed_inputs")
    if not isinstance(completed, list) or not all(isinstance(item, str) and item for item in completed) or len(set(completed)) != len(completed):
        raise AuditError("partial completed_inputs must be unique non-empty strings")
    if not isinstance(partial.get("warnings"), list) or not all(isinstance(item, str) for item in partial["warnings"]):
        raise AuditError("partial warnings must be strings")
    measurements = partial.get("measurements")
    if not isinstance(measurements, list):
        raise AuditError("partial measurements must be a list")
    if measurements:
        dummy = {
            "schema": SCHEMA,
            "unit_id": unit["id"],
            "source_ids": unit["input"]["source_ids"],
            "tool": unit["input"]["source"]["parser"],
            "coverage": {"files_seen": 0, "files_parsed": 0, "files_skipped": 0, "lexical_tokens": 0},
            "measurements": measurements,
            "citations": [unit["input"]["source"]["url"]],
            "warnings": partial["warnings"],
        }
        validate_result(dummy, unit["id"])
        if not measurement_signatures(measurements).issubset(measurement_signatures(unit["input"]["payload"]["expected_measurements"])):
            raise AuditError("partial measurements are outside the frozen catalog")


def cmd_init(args: argparse.Namespace) -> None:
    run_dir = Path(args.run_dir).resolve()
    require_run_dir(run_dir)
    if run_dir.exists():
        raise AuditError(f"run directory already exists: {run_dir}")
    report_path = Path(args.report).resolve()
    require_report_path(report_path)
    if report_path.exists():
        raise AuditError(f"report already exists: {report_path}")
    configs = []
    mandatory = [Path(__file__).resolve(), Path(__file__).with_name("aggregate.py").resolve()]
    for raw in [*args.config, *(str(path) for path in mandatory)]:
        path = Path(raw).resolve()
        if not path.is_file():
            raise AuditError(f"config does not exist: {path}")
        if not any(item["path"] == str(path) for item in configs):
            configs.append({"path": str(path), "sha256": file_digest(path)})
    (run_dir / "units").mkdir(parents=True)
    (run_dir / "results").mkdir()
    (run_dir / "locks").mkdir()
    (run_dir / "inbox").mkdir()
    (run_dir / "partials").mkdir()
    (run_dir / "analysis").mkdir()
    run = {
        "schema": SCHEMA,
        "run_id": run_dir.name,
        "created_at": now(),
        "updated_at": now(),
        "phase": "initialized",
        "revision": 0,
        "report_path": str(report_path),
        "configs": configs,
        "planned_units": 0,
    }
    write_json(run_dir / "run.json", run)
    print(json.dumps(run, indent=2))


def cmd_plan(args: argparse.Namespace) -> None:
    run_dir = Path(args.run_dir).resolve()
    load_run(run_dir)
    units_file = Path(args.units).resolve()
    require_inside(units_file, run_dir / "inbox", "units file")
    raw_units = load_json(units_file)
    catalog_file = Path(args.catalog).resolve()
    require_inside(catalog_file, run_dir / "inbox", "catalog file")
    catalog_raw = load_json(catalog_file)
    catalogs = validate_catalog(catalog_raw)
    if not isinstance(raw_units, list) or not raw_units:
        raise AuditError("units file must contain a non-empty JSON list")
    ids: set[str] = set()
    required = ("id", "source_ids", "source_identity", "catalog_id", "source", "language", "domain", "stratum", "payload")
    for item in raw_units:
        if not isinstance(item, dict) or any(key not in item for key in required):
            raise AuditError("each unit needs catalog, source, language, domain, stratum, and payload fields")
        unit_id = item["id"]
        if not isinstance(unit_id, str) or not UNIT_ID.fullmatch(unit_id) or unit_id in ids:
            raise AuditError(f"invalid or duplicate unit id: {unit_id!r}")
        if not isinstance(item["source_ids"], list) or len(item["source_ids"]) != 1:
            raise AuditError(f"unit {unit_id} needs one canonical source_id")
        if not isinstance(item["source_identity"], str) or not item["source_identity"].strip():
            raise AuditError(f"unit {unit_id} needs source_identity")
        catalog = catalogs.get(item["catalog_id"])
        if catalog is None or catalog["language"] != item["language"]:
            raise AuditError(f"unit {unit_id} has an unknown or mismatched catalog")
        source = item["source"]
        source_fields = ("url", "pin", "language_version", "license", "inclusion_status", "sampling_frame_id", "retrieved_at", "parser")
        if not isinstance(source, dict) or any(key not in source for key in source_fields):
            raise AuditError(f"unit {unit_id} misses source provenance")
        if not isinstance(source["url"], str) or not source["url"].startswith(("https://", "http://")):
            raise AuditError(f"unit {unit_id} has an invalid source URL")
        for key in ("pin", "language_version", "license", "sampling_frame_id", "retrieved_at"):
            if not isinstance(source[key], str) or not source[key].strip():
                raise AuditError(f"unit {unit_id} needs source.{key}")
        if source["language_version"] != catalog["version"] or source["inclusion_status"] not in {"included", "case-study"}:
            raise AuditError(f"unit {unit_id} has invalid version or inclusion status")
        if not isinstance(source["parser"], dict) or any(
            not isinstance(source["parser"].get(key), str) or not source["parser"][key].strip() for key in ("name", "version")
        ):
            raise AuditError(f"unit {unit_id} needs parser name and version")
        if not isinstance(item["payload"], dict):
            raise AuditError(f"unit {unit_id} payload must be an object")
        item["payload"] = {**item["payload"], "expected_measurements": catalog["measurements"]}
        ids.add(unit_id)
    with locked(run_dir / "locks" / "run.lock"):
        run = load_run(run_dir)
        if run["phase"] != "initialized" or any((run_dir / "units").iterdir()):
            raise AuditError("run is already planned")
        for item in raw_units:
            unit = {
                "schema": SCHEMA,
                "id": item["id"],
                "state": "pending",
                "owner": None,
                "lease_expires_at": None,
                "attempt": 0,
                "cursor": None,
                "note": None,
                "input": item,
                "input_sha256": digest(canonical(item)),
                "result_sha256": None,
                "history": [{"at": now(), "action": "planned"}],
            }
            save_unit(run_dir, unit)
        run["phase"] = "collecting"
        run["planned_units"] = len(raw_units)
        run["plan_sha256"] = digest(canonical(sorted(raw_units, key=lambda item: item["id"])))
        run["catalog_sha256"] = digest(canonical(catalog_raw))
        run["catalog_path"] = str(catalog_file)
        run["revision"] += 1
        run["updated_at"] = now()
        write_json(run_dir / "run.json", run)
    print(f"planned {len(raw_units)} units")


def cmd_claim(args: argparse.Namespace) -> None:
    run_dir = Path(args.run_dir).resolve()
    load_run(run_dir)
    unit = claim_one(run_dir, args.unit_id, args.owner, args.lease_hours)
    if unit is None:
        raise AuditError(f"unit is unavailable for claim: {args.unit_id}")
    print(json.dumps(unit, indent=2))


def cmd_next(args: argparse.Namespace) -> None:
    run_dir = Path(args.run_dir).resolve()
    load_run(run_dir)
    for path in sorted((run_dir / "units").glob("*.json")):
        unit = claim_one(run_dir, path.stem, args.owner, args.lease_hours)
        if unit is not None:
            print(json.dumps(unit, indent=2))
            return
    raise AuditError("no claimable unit remains")


def cmd_checkpoint(args: argparse.Namespace) -> None:
    run_dir = Path(args.run_dir).resolve()
    load_run(run_dir)
    with locked(run_dir / "locks" / f"{args.unit_id}.lock"):
        unit = load_unit(run_dir, args.unit_id)
        if unit.get("state") != "claimed" or unit.get("owner") != args.owner:
            raise AuditError("only the current owner can checkpoint this unit")
        unit["cursor"] = args.cursor
        unit["note"] = args.note
        partial_path = Path(args.partial).resolve()
        require_inside(partial_path, run_dir / "inbox", "partial checkpoint")
        partial = load_json(partial_path)
        validate_partial(partial, unit)
        if partial.get("cursor") != args.cursor:
            raise AuditError("partial checkpoint cursor does not match --cursor")
        partial_bytes = canonical(partial)
        atomic_write(run_dir / "partials" / f"{args.unit_id}.json", partial_bytes)
        unit["partial_sha256"] = digest(partial_bytes)
        unit["lease_expires_at"] = (
            dt.datetime.now(dt.timezone.utc) + dt.timedelta(hours=args.lease_hours)
        ).isoformat(timespec="seconds")
        event(unit, "checkpointed", args.owner, args.note)
        save_unit(run_dir, unit)
    print(f"checkpointed {args.unit_id}")


def cmd_complete(args: argparse.Namespace) -> None:
    run_dir = Path(args.run_dir).resolve()
    load_run(run_dir)
    result_path = Path(args.result).resolve()
    require_inside(result_path, run_dir / "inbox", "result")
    result = validate_result(load_json(result_path), args.unit_id)
    result_bytes = canonical(result)
    result_sha = digest(result_bytes)
    with locked(run_dir / "locks" / f"{args.unit_id}.lock"):
        unit = load_unit(run_dir, args.unit_id)
        if unit.get("state") == "done" and unit.get("result_sha256") == result_sha:
            print(f"already complete {args.unit_id}")
            return
        if unit.get("state") != "claimed" or unit.get("owner") != args.owner:
            raise AuditError("only the current owner can complete this unit")
        if result["source_ids"] != unit["input"]["source_ids"]:
            raise AuditError("result source_ids do not match the planned unit")
        if result["tool"] != unit["input"]["source"]["parser"]:
            raise AuditError("result tool does not match the frozen parser")
        expected = measurement_signatures(unit["input"]["payload"]["expected_measurements"])
        actual = measurement_signatures(result["measurements"])
        if actual != expected:
            missing = sorted(expected - actual)
            extra = sorted(actual - expected)
            raise AuditError(f"measurement catalog mismatch; missing={missing} extra={extra}")
        atomic_write(run_dir / "results" / f"{args.unit_id}.json", result_bytes)
        unit["state"] = "done"
        unit["result_sha256"] = result_sha
        unit["lease_expires_at"] = None
        event(unit, "completed", args.owner)
        save_unit(run_dir, unit)
    print(f"completed {args.unit_id} {result_sha}")


def cmd_block(args: argparse.Namespace) -> None:
    run_dir = Path(args.run_dir).resolve()
    load_run(run_dir)
    state = "unavailable" if args.unavailable else "blocked"
    with locked(run_dir / "locks" / f"{args.unit_id}.lock"):
        unit = load_unit(run_dir, args.unit_id)
        if unit.get("state") != "claimed" or unit.get("owner") != args.owner:
            raise AuditError("only the current owner can close this unit")
        unit["state"] = state
        unit["note"] = args.reason
        unit["lease_expires_at"] = None
        event(unit, state, args.owner, args.reason)
        save_unit(run_dir, unit)
    print(f"{state} {args.unit_id}")


def inspect_run(run_dir: Path, require_complete: bool) -> dict[str, Any]:
    run = load_run(run_dir)
    counts = {state: 0 for state in ("pending", "claimed", "done", "blocked", "unavailable")}
    errors: list[str] = []
    expired: list[str] = []
    work: list[dict[str, Any]] = []
    unit_paths = sorted((run_dir / "units").glob("*.json"))
    planned_inputs = []
    catalog_path = Path(run["catalog_path"]) if run.get("catalog_path") else None
    if catalog_path is None or not catalog_path.is_file():
        errors.append("frozen catalog is missing")
    else:
        try:
            catalog_raw = load_json(catalog_path)
            validate_catalog(catalog_raw)
            if digest(canonical(catalog_raw)) != run.get("catalog_sha256"):
                errors.append("frozen catalog digest mismatch")
        except AuditError as error:
            errors.append(f"frozen catalog: {error}")
    for path in unit_paths:
        unit = load_json(path)
        if digest(canonical(unit.get("input"))) != unit.get("input_sha256"):
            errors.append(f"{path.stem}: planned input digest mismatch")
        else:
            planned_inputs.append(unit["input"])
        state = unit.get("state")
        if state not in counts:
            errors.append(f"{path.stem}: invalid state {state!r}")
            continue
        counts[state] += 1
        if is_expired(unit):
            expired.append(path.stem)
        if state != "done":
            work.append(
                {
                    "id": path.stem,
                    "state": state,
                    "owner": unit.get("owner"),
                    "lease_expires_at": unit.get("lease_expires_at"),
                    "cursor": unit.get("cursor"),
                    "note": unit.get("note"),
                    "partial": str(run_dir / "partials" / f"{path.stem}.json") if unit.get("partial_sha256") else None,
                }
            )
        if state == "done":
            result_path = run_dir / "results" / f"{path.stem}.json"
            if not result_path.is_file():
                errors.append(f"{path.stem}: result missing")
                continue
            if file_digest(result_path) != unit.get("result_sha256"):
                errors.append(f"{path.stem}: result digest mismatch")
                continue
            try:
                result = validate_result(load_json(result_path), path.stem)
                if measurement_signatures(result["measurements"]) != measurement_signatures(unit["input"]["payload"]["expected_measurements"]):
                    errors.append(f"{path.stem}: result measurement catalog mismatch")
            except AuditError as error:
                errors.append(f"{path.stem}: {error}")
        partial_sha = unit.get("partial_sha256")
        if partial_sha:
            partial_path = run_dir / "partials" / f"{path.stem}.json"
            if not partial_path.is_file() or file_digest(partial_path) != partial_sha:
                errors.append(f"{path.stem}: partial checkpoint digest mismatch")
            else:
                try:
                    validate_partial(load_json(partial_path), unit)
                except AuditError as error:
                    errors.append(f"{path.stem}: {error}")
    if len(unit_paths) != run.get("planned_units", 0):
        errors.append("unit count does not match run plan")
    if len(planned_inputs) == len(unit_paths) and digest(canonical(sorted(planned_inputs, key=lambda item: item["id"]))) != run.get("plan_sha256"):
        errors.append("plan digest mismatch")
    if require_complete and (counts["pending"] or counts["claimed"]):
        errors.append("run has unfinished units")
    return {
        "run_id": run["run_id"],
        "phase": run["phase"],
        "report_path": run["report_path"],
        "counts": counts,
        "expired_claims": expired,
        "work": work,
        "errors": errors,
        "result_set_sha256": digest(canonical(sorted(
            (path.stem, load_json(path).get("result_sha256"))
            for path in unit_paths
            if load_json(path).get("state") == "done"
        ))),
        "resume_command": f"checkpoint.py next {run_dir} --owner AGENT_ID --lease-hours 4",
    }


def cmd_status(args: argparse.Namespace) -> None:
    summary = inspect_run(Path(args.run_dir).resolve(), False)
    print(json.dumps(summary, indent=2))
    if summary["errors"]:
        raise AuditError("run validation failed")


def cmd_validate(args: argparse.Namespace) -> None:
    summary = inspect_run(Path(args.run_dir).resolve(), args.require_complete)
    print(json.dumps(summary, indent=2))
    if summary["errors"]:
        raise AuditError("run validation failed")


def cmd_install(args: argparse.Namespace) -> None:
    run_dir = Path(args.run_dir).resolve()
    summary = inspect_run(run_dir, True)
    if summary["errors"]:
        raise AuditError("cannot install report while run validation fails")
    aggregate_path = run_dir / "analysis" / "aggregate.json"
    aggregate = load_json(aggregate_path)
    if aggregate.get("run_id") != summary["run_id"] or aggregate.get("result_set_sha256") != summary["result_set_sha256"]:
        raise AuditError("aggregate is missing or stale for the final result set")
    aggregate_script = Path(__file__).with_name("aggregate.py")
    rebuilt = subprocess.run(
        [sys.executable, str(aggregate_script), str(run_dir)],
        check=False,
        capture_output=True,
    )
    if rebuilt.returncode != 0 or aggregate_path.read_bytes() != rebuilt.stdout:
        raise AuditError("aggregate content does not match the pinned helper and final results")
    draft = Path(args.draft).resolve()
    if draft != run_dir / "report.tmp.md":
        raise AuditError(f"report draft must be {run_dir / 'report.tmp.md'}")
    text = draft.read_text()
    missing = [heading for heading in REPORT_HEADINGS if heading not in text]
    if missing:
        raise AuditError("report is missing headings: " + ", ".join(missing))
    if len(text) < 2000:
        raise AuditError("report is too short to satisfy the required evidence contract")
    for unit_path in sorted((run_dir / "units").glob("*.json")):
        unit = load_json(unit_path)
        if unit.get("state") in {"blocked", "unavailable"} and unit["id"] not in text:
            raise AuditError(f"report does not name terminal gap {unit['id']}")
    report_bytes = text.encode()
    with locked(run_dir / "locks" / "run.lock"):
        run = load_run(run_dir)
        report_path = Path(run["report_path"])
        require_report_path(report_path)
        if report_path.exists():
            raise AuditError(f"refusing to overwrite report: {report_path}")
        atomic_write(report_path, report_bytes)
        run["phase"] = "complete"
        run["report_sha256"] = digest(report_bytes)
        run["completed_at"] = now()
        run["updated_at"] = now()
        run["revision"] += 1
        write_json(run_dir / "run.json", run)
    print(f"installed {report_path} {run['report_sha256']}")


def cmd_clean(args: argparse.Namespace) -> None:
    run_dir = Path(args.run_dir).resolve()
    run = load_run(run_dir)
    if run["phase"] != "complete" or run_dir.parent != RUN_BASE:
        raise AuditError("refusing to clean an incomplete or unsafe run directory")
    report_path = Path(run["report_path"])
    if not report_path.is_file() or file_digest(report_path) != run.get("report_sha256"):
        raise AuditError("installed report is missing or changed")
    shutil.rmtree(run_dir)
    print(f"removed completed checkpoint {run_dir}")


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    commands = root.add_subparsers(dest="command", required=True)

    command = commands.add_parser("init")
    command.add_argument("run_dir")
    command.add_argument("--report", required=True)
    command.add_argument("--config", action="append", default=[])
    command.set_defaults(func=cmd_init)

    command = commands.add_parser("plan")
    command.add_argument("run_dir")
    command.add_argument("units")
    command.add_argument("--catalog", required=True)
    command.set_defaults(func=cmd_plan)

    for name, func in (("claim", cmd_claim), ("next", cmd_next)):
        command = commands.add_parser(name)
        command.add_argument("run_dir")
        if name == "claim":
            command.add_argument("unit_id")
        command.add_argument("--owner", required=True)
        command.add_argument("--lease-hours", type=float, default=4)
        command.set_defaults(func=func)

    command = commands.add_parser("checkpoint")
    command.add_argument("run_dir")
    command.add_argument("unit_id")
    command.add_argument("--owner", required=True)
    command.add_argument("--cursor", required=True)
    command.add_argument("--note", required=True)
    command.add_argument("--partial", required=True)
    command.add_argument("--lease-hours", type=float, default=4)
    command.set_defaults(func=cmd_checkpoint)

    command = commands.add_parser("complete")
    command.add_argument("run_dir")
    command.add_argument("unit_id")
    command.add_argument("--owner", required=True)
    command.add_argument("--result", required=True)
    command.set_defaults(func=cmd_complete)

    command = commands.add_parser("block")
    command.add_argument("run_dir")
    command.add_argument("unit_id")
    command.add_argument("--owner", required=True)
    command.add_argument("--reason", required=True)
    command.add_argument("--unavailable", action="store_true")
    command.set_defaults(func=cmd_block)

    command = commands.add_parser("status")
    command.add_argument("run_dir")
    command.set_defaults(func=cmd_status)

    command = commands.add_parser("validate")
    command.add_argument("run_dir")
    command.add_argument("--require-complete", action="store_true")
    command.set_defaults(func=cmd_validate)

    command = commands.add_parser("install")
    command.add_argument("run_dir")
    command.add_argument("draft")
    command.set_defaults(func=cmd_install)

    command = commands.add_parser("clean")
    command.add_argument("run_dir")
    command.set_defaults(func=cmd_clean)
    return root


def main() -> int:
    try:
        args = parser().parse_args()
        if hasattr(args, "lease_hours") and args.lease_hours <= 0:
            raise AuditError("lease hours must be positive")
        args.func(args)
        return 0
    except (AuditError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
