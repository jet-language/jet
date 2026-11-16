#!/usr/bin/env python3
"""Aggregate normalized audit measurements without external dependencies."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import statistics
import sys
from typing import Any

sys.dont_write_bytecode = True

from checkpoint import AuditError, atomic_write, canonical, inspect_run, load_json, load_run, require_inside, validate_result


def number(value: Any, field: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value < 0:
        raise AuditError(f"measurement {field} must be a non-negative number")
    return float(value)


def ratio(numerator: float, denominator: float) -> float | None:
    return numerator / denominator if denominator else None


def p90(values: list[float]) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * 0.9) - 1)]


def _partition(rows: list[dict[str, Any]], field: str) -> dict[str, list[dict[str, Any]]]:
    groups: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        groups.setdefault(str(row[field]), []).append(row)
    return groups


def aggregate(run_dir: Path) -> dict[str, Any]:
    run = load_run(run_dir)
    validation = inspect_run(run_dir, True)
    if validation["errors"]:
        raise AuditError("run validation failed before aggregation")
    groups: dict[tuple[str, str, str, str, str, str], list[dict[str, Any]]] = {}
    seen: set[tuple[tuple[str, str, str, str, str, str], str]] = set()
    skipped: list[str] = []

    for unit_path in sorted((run_dir / "units").glob("*.json")):
        unit = load_json(unit_path)
        if unit.get("state") != "done":
            continue
        source_ids = unit["input"].get("source_ids", [])
        if len(source_ids) != 1:
            raise AuditError(f"{unit['id']}: aggregation needs one canonical source_id")
        result = validate_result(load_json(run_dir / "results" / f"{unit['id']}.json"), unit["id"])
        project_id = source_ids[0]
        tokens = result["coverage"].get("lexical_tokens")
        token_count = number(tokens, "coverage.lexical_tokens") if isinstance(tokens, (int, float)) else None

        for index, measurement in enumerate(result["measurements"]):
            if not isinstance(measurement, dict):
                raise AuditError(f"{unit['id']} measurement {index} must be an object")
            required = ("operation_id", "feature_id", "level", "metric", "numerator", "denominator", "opportunity", "scope", "difficulty", "eligible")
            if any(key not in measurement for key in required):
                raise AuditError(f"{unit['id']} measurement {index} misses a required field")
            if not isinstance(measurement["eligible"], bool):
                raise AuditError(f"{unit['id']} measurement {index} eligible must be boolean")
            if measurement["numerator"] == "not-recorded" or measurement["denominator"] == "not-recorded":
                skipped.append(f"{unit['id']}:{index}:not-recorded")
                continue
            numerator = number(measurement["numerator"], "numerator")
            denominator = number(measurement["denominator"], "denominator")
            if numerator > denominator:
                raise AuditError(f"{unit['id']} measurement {index} numerator exceeds denominator")
            if measurement["eligible"] and denominator == 0:
                raise AuditError(f"{unit['id']} measurement {index} is eligible with zero denominator")
            key = (
                str(measurement["operation_id"]),
                str(measurement["feature_id"]),
                str(measurement["level"]),
                str(measurement["metric"]),
                str(measurement["scope"]),
                str(measurement["opportunity"]),
            )
            source_identity = unit["input"]["source_identity"]
            identity = (key, f"{source_identity}\0{unit['input']['language']}")
            if identity in seen:
                skipped.append(f"{unit['id']}:{index}:duplicate-source-identity")
                continue
            seen.add(identity)
            groups.setdefault(key, []).append(
                {
                    "project_id": project_id,
                    "source_identity": source_identity,
                    "language": unit["input"]["language"],
                    "domain": unit["input"]["domain"],
                    "stratum": unit["input"]["stratum"],
                    "difficulty": measurement["difficulty"],
                    "eligible": measurement["eligible"],
                    "numerator": numerator,
                    "denominator": denominator,
                    "lexical_tokens": token_count,
                    "ontology_ids": measurement.get("ontology_ids", []),
                    "surface": measurement.get("surface"),
                }
            )

    rankings = []
    for key, rows in groups.items():
        eligible = [row for row in rows if row["eligible"]]
        if not eligible:
            continue
        cells: dict[tuple[str, str, str], list[dict[str, Any]]] = {}
        for row in eligible:
            cells.setdefault((row["language"], row["domain"], row["stratum"]), []).append(row)
        cell_prevalence = [
            sum(row["numerator"] > 0 for row in cell_rows) / len(cell_rows)
            for cell_rows in cells.values()
        ]
        densities = [
            1000 * row["numerator"] / row["lexical_tokens"]
            for row in eligible
            if row["lexical_tokens"] and row["lexical_tokens"] > 0
        ]
        numerator = sum(row["numerator"] for row in eligible)
        denominator = sum(row["denominator"] for row in eligible)
        projects = _partition(eligible, "source_identity")
        positive_cells = sum(any(row["numerator"] > 0 for row in cell_rows) for cell_rows in cells.values())
        difficulties = sorted({str(row["difficulty"]) for row in eligible})
        ontology_ids = sorted({str(value) for row in eligible for value in row["ontology_ids"]})
        surfaces = sorted({str(row["surface"]) for row in eligible if row["surface"] is not None})
        rankings.append(
            {
                "operation_id": key[0],
                "feature_id": key[1],
                "level": key[2],
                "metric": key[3],
                "scope": key[4],
                "opportunity": key[5],
                "ontology_ids": ontology_ids,
                "surfaces": surfaces,
                "difficulties": difficulties,
                "eligible_projects": len(projects),
                "projects_with_use": sum(any(row["numerator"] > 0 for row in project_rows) for project_rows in projects.values()),
                "project_prevalence": ratio(
                    sum(any(row["numerator"] > 0 for row in project_rows) for project_rows in projects.values()),
                    len(projects),
                ),
                "balanced_project_prevalence": statistics.fmean(cell_prevalence),
                "equal_language_prevalence": statistics.fmean(
                    sum(row["numerator"] > 0 for row in language_rows) / len(language_rows)
                    for language_rows in _partition(eligible, "language").values()
                ),
                "equal_domain_prevalence": statistics.fmean(
                    sum(row["numerator"] > 0 for row in domain_rows) / len(domain_rows)
                    for domain_rows in _partition(eligible, "domain").values()
                ),
                "equal_stratum_prevalence": statistics.fmean(
                    sum(row["numerator"] > 0 for row in stratum_rows) / len(stratum_rows)
                    for stratum_rows in _partition(eligible, "stratum").values()
                ),
                "opportunity_share": ratio(numerator, denominator),
                "breadth": ratio(positive_cells, len(cells)),
                "total_numerator": numerator,
                "total_denominator": denominator,
                "median_density_per_1k_tokens": statistics.median(densities) if densities else None,
                "p90_density_per_1k_tokens": p90(densities),
                "eligible_cells": len(cells),
            }
        )

    rankings.sort(
        key=lambda row: (
            -row["balanced_project_prevalence"],
            -(row["opportunity_share"] or 0),
            row["feature_id"],
            row["metric"],
        )
    )
    return {
        "schema": 1,
        "run_id": run["run_id"],
        "plan_sha256": run.get("plan_sha256"),
        "result_set_sha256": validation["result_set_sha256"],
        "rankings": rankings,
        "skipped_measurements": skipped,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_dir")
    parser.add_argument("--output")
    args = parser.parse_args()
    try:
        result = aggregate(Path(args.run_dir).resolve())
        data = canonical(result)
        if args.output:
            run_dir = Path(args.run_dir).resolve()
            output = Path(args.output).resolve()
            require_inside(output, run_dir / "analysis", "aggregate output")
            atomic_write(output, data)
        else:
            sys.stdout.buffer.write(data)
        return 0
    except (AuditError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
