#!/usr/bin/env python3

from __future__ import annotations

import json
from pathlib import Path
import shutil
import subprocess
import sys
import unittest
import uuid


SCRIPT = Path(__file__).with_name("checkpoint.py")
AGGREGATE = Path(__file__).with_name("aggregate.py")
REPO = SCRIPT.resolve().parents[4]
RUN_BASE = REPO / ".tmp" / "surface-frequency-audit"
REPORT_BASE = REPO / "docs" / "audits"


class CheckpointTest(unittest.TestCase):
    def run_cli(self, *args: object) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *(str(arg) for arg in args)],
            check=True,
            text=True,
            capture_output=True,
        )

    def test_resume_complete_install_and_clean(self) -> None:
        token = uuid.uuid4().hex
        run = RUN_BASE / f"test-{token}"
        report = REPORT_BASE / f"surface-frequency-audit-2099-01-01-test-{token}.md"
        config = REPO / ".agents" / "skills" / "surface-frequency-audit" / "references" / "method.md"
        expected = {
            "operation_id": "error-propagation",
            "feature_id": "error-propagation",
            "level": "operation",
            "metric": "usage",
            "scope": "primary",
            "opportunity": "eligible propagation sites",
            "ontology_ids": ["C21"],
            "surface": "error propagation",
            "difficulty": "general",
        }
        expected_key = [expected[key] for key in ("operation_id", "feature_id", "level", "metric", "scope", "opportunity")]
        try:
            self.run_cli("init", run, "--report", report, "--config", config)

            units = [
                {
                    "id": "python-a",
                    "source_ids": ["repo:a"],
                    "source_identity": "tree:a",
                    "catalog_id": "python-test",
                    "source": {
                        "url": "https://example.test/a",
                        "pin": "abcdef1",
                        "language_version": "test",
                        "license": "MIT",
                        "inclusion_status": "included",
                        "sampling_frame_id": "frame-a",
                        "retrieved_at": "2099-01-01T00:00:00Z",
                        "parser": {"name": "ast", "version": "1"},
                    },
                    "language": "Python",
                    "domain": "CLI",
                    "stratum": "small",
                    "payload": {},
                },
                {
                    "id": "rust-b",
                    "source_ids": ["repo:b"],
                    "source_identity": "tree:b",
                    "catalog_id": "rust-test",
                    "source": {
                        "url": "https://example.test/b",
                        "pin": "abcdef2",
                        "language_version": "test",
                        "license": "MIT",
                        "inclusion_status": "included",
                        "sampling_frame_id": "frame-b",
                        "retrieved_at": "2099-01-01T00:00:00Z",
                        "parser": {"name": "parser", "version": "1"},
                    },
                    "language": "Rust",
                    "domain": "systems",
                    "stratum": "production",
                    "payload": {},
                },
            ]
            catalog = {
                "schema": 1,
                "catalogs": [
                    {
                        "id": catalog_id,
                        "language": language,
                        "version": "test",
                        "official_sources": [f"https://example.test/{language}/spec"],
                        "official_sections_total": 1,
                        "official_sections_mapped": 1,
                        "unmatched_sections": [],
                        "built_by": "builder",
                        "reviewed_by": "reviewer",
                        "reviewed_at": "2099-01-01T00:00:00Z",
                        "official_sections": [
                            {
                                "id": "section-1",
                                "url": f"https://example.test/{language}/spec#section-1",
                                "status": "mapped",
                                "measurement_keys": [expected_key],
                                "reason": None,
                            }
                        ],
                        "measurements": [expected],
                    }
                    for catalog_id, language in (("python-test", "Python"), ("rust-test", "Rust"))
                ],
            }
            units_path = run / "inbox" / "units.json"
            catalog_path = run / "inbox" / "catalog.json"
            units_path.write_text(json.dumps(units))
            catalog_path.write_text(json.dumps(catalog))
            self.run_cli("plan", run, units_path, "--catalog", catalog_path)

            unit_path = run / "units" / "python-a.json"
            original_unit = json.loads(unit_path.read_text())
            changed_unit = {**original_unit, "input": {**original_unit["input"], "language": "Changed"}}
            unit_path.write_text(json.dumps(changed_unit))
            rejected = subprocess.run(
                [sys.executable, str(SCRIPT), "validate", str(run)],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(rejected.returncode, 2)
            unit_path.write_text(json.dumps(original_unit))

            claimed = json.loads(self.run_cli("next", run, "--owner", "agent-a").stdout)
            self.assertEqual(claimed["id"], "python-a")
            partial_path = run / "inbox" / "python-a.partial.json"
            partial_path.write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "unit_id": "python-a",
                        "cursor": "src/main.py:20",
                        "completed_inputs": ["src/first.py"],
                        "measurements": [],
                        "warnings": [],
                    }
                )
            )
            self.run_cli(
                "checkpoint",
                run,
                "python-a",
                "--owner",
                "agent-a",
                "--cursor",
                "src/main.py:20",
                "--note",
                "first file done",
                "--partial",
                partial_path,
            )

            result = {
                "schema": 1,
                "unit_id": "python-a",
                "source_ids": ["repo:a"],
                "tool": {"name": "ast", "version": "1"},
                "coverage": {"files_seen": 2, "files_parsed": 1, "files_skipped": 1, "lexical_tokens": 100},
                "measurements": [
                    {
                        **expected,
                        "ontology_ids": ["C21"],
                        "surface": "error propagation",
                        "difficulty": "general",
                        "eligible": True,
                        "numerator": 0,
                        "denominator": 1,
                        "source_sites": [],
                    }
                ],
                "citations": ["https://example.test/a"],
                "warnings": ["one skipped file"],
            }
            result_path = run / "inbox" / "python-a.result.json"
            result_path.write_text(
                json.dumps({**result, "measurements": [], "citations": []})
            )
            rejected = subprocess.run(
                [sys.executable, str(SCRIPT), "complete", str(run), "python-a", "--owner", "agent-a", "--result", str(result_path)],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(rejected.returncode, 2)
            result_path.write_text(json.dumps(result))
            self.run_cli("complete", run, "python-a", "--owner", "agent-a", "--result", result_path)

            claimed = json.loads(self.run_cli("next", run, "--owner", "agent-b").stdout)
            self.assertEqual(claimed["id"], "rust-b")
            self.run_cli(
                "block",
                run,
                "rust-b",
                "--owner",
                "agent-b",
                "--reason",
                "parser unavailable",
                "--unavailable",
            )
            self.run_cli("validate", run, "--require-complete")
            subprocess.run(
                [sys.executable, str(AGGREGATE), str(run), "--output", str(run / "analysis" / "aggregate.json")],
                check=True,
                text=True,
                capture_output=True,
            )

            draft = run / "report.tmp.md"
            headings = (
                "# Surface frequency audit — 2099-01-01\n\n"
                "## Executive summary\n\nResult.\n\n"
                "### Decision view\n\n| Rank | Result |\n| --- | --- |\n| 1 | Keep |\n\n"
                "## What people do most\n\nMeasured.\n\n"
                "## Which surfaces they use\n\nMeasured.\n\n"
                "## Beginner adoption path\n\nMeasured.\n\n"
                "## Expert production path\n\nMeasured.\n\n"
                "## Jet recommendations\n\nMeasured.\n\n"
                "## Keep\n\nMeasured.\n\n"
                "## Watchlist\n\nMeasured.\n\n"
                "## What changes the ranking\n\nMeasured.\n\n"
                "## Coverage and limits\n\nrust-b is unavailable.\n\n"
                "## Methods and provenance\n\nPinned.\n\n"
                "## Sources\n\nhttps://example.test/a\n\n"
            )
            draft.write_text(headings + ("Evidence. " * 240))
            self.run_cli("install", run, draft)
            self.assertEqual(report.read_text(), draft.read_text())
            self.run_cli("clean", run)
            self.assertFalse(run.exists())
        finally:
            if run.exists():
                shutil.rmtree(run)
            report.unlink(missing_ok=True)

    def test_rejects_paths_outside_audit_roots(self) -> None:
        token = uuid.uuid4().hex
        unsafe_run = REPO / ".tmp" / f"unsafe-{token}"
        report = REPORT_BASE / f"surface-frequency-audit-2099-01-01-test-{token}.md"
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "init", str(unsafe_run), "--report", str(report)],
            check=False,
            text=True,
            capture_output=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertFalse(unsafe_run.exists())


if __name__ == "__main__":
    unittest.main()
