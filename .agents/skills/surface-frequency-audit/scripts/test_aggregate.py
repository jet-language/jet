#!/usr/bin/env python3

from __future__ import annotations

import json
from pathlib import Path
import shutil
import subprocess
import sys
import unittest
import uuid


SCRIPTS = Path(__file__).parent
CHECKPOINT = SCRIPTS / "checkpoint.py"
AGGREGATE = SCRIPTS / "aggregate.py"
REPO = CHECKPOINT.resolve().parents[4]
RUN_BASE = REPO / ".tmp" / "surface-frequency-audit"
REPORT_BASE = REPO / "docs" / "audits"


class AggregateTest(unittest.TestCase):
    def cli(self, script: Path, *args: object) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(script), *(str(arg) for arg in args)],
            check=True,
            text=True,
            capture_output=True,
        )

    def test_balanced_prevalence_opportunity_and_density(self) -> None:
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
            self.cli(CHECKPOINT, "init", run, "--report", report, "--config", config)

            specs = [
                ("python-a", "repo:a", "tree:a", "Python", "web", 2, 4, 1000),
                ("python-b", "repo:b", "tree:b", "Python", "web", 0, 3, 1000),
                ("rust-c", "repo:c", "tree:a", "Rust", "systems", 1, 2, 1000),
            ]
            units = [
                {
                    "id": unit_id,
                    "source_ids": [source_id],
                    "source_identity": source_identity,
                    "catalog_id": f"{language.lower()}-test",
                    "source": {
                        "url": f"https://example.test/{unit_id}",
                        "pin": f"pin-{unit_id}",
                        "language_version": "test",
                        "license": "MIT",
                        "inclusion_status": "included",
                        "sampling_frame_id": f"frame-{unit_id}",
                        "retrieved_at": "2099-01-01T00:00:00Z",
                        "parser": {"name": "parser", "version": "1"},
                    },
                    "language": language,
                    "domain": domain,
                    "stratum": "small",
                    "payload": {},
                }
                for unit_id, source_id, source_identity, language, domain, *_ in specs
            ]
            catalog = {
                "schema": 1,
                "catalogs": [
                    {
                        "id": f"{language.lower()}-test",
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
                    for language in ("Python", "Rust")
                ],
            }
            units_path = run / "inbox" / "units.json"
            catalog_path = run / "inbox" / "catalog.json"
            units_path.write_text(json.dumps(units))
            catalog_path.write_text(json.dumps(catalog))
            self.cli(CHECKPOINT, "plan", run, units_path, "--catalog", catalog_path)

            for unit_id, source_id, _source_identity, _language, _domain, numerator, denominator, tokens in specs:
                self.cli(CHECKPOINT, "claim", run, unit_id, "--owner", "agent")
                result = {
                    "schema": 1,
                    "unit_id": unit_id,
                    "source_ids": [source_id],
                    "tool": {"name": "parser", "version": "1"},
                    "coverage": {
                        "files_seen": 1,
                        "files_parsed": 1,
                        "files_skipped": 0,
                        "lexical_tokens": tokens,
                    },
                    "measurements": [
                        {
                            **expected,
                            "numerator": numerator,
                            "denominator": denominator,
                            "eligible": True,
                            "source_sites": ["src/main:1"] if numerator else [],
                        }
                    ],
                    "citations": [f"https://example.test/{unit_id}"],
                    "warnings": [],
                }
                result_path = run / "inbox" / f"{unit_id}.result.json"
                result_path.write_text(json.dumps(result))
                self.cli(CHECKPOINT, "complete", run, unit_id, "--owner", "agent", "--result", result_path)

            aggregate = json.loads(self.cli(AGGREGATE, run).stdout)
            row = aggregate["rankings"][0]
            self.assertAlmostEqual(row["project_prevalence"], 1 / 2)
            self.assertEqual(row["eligible_projects"], 2)
            self.assertAlmostEqual(row["balanced_project_prevalence"], 0.75)
            self.assertAlmostEqual(row["opportunity_share"], 1 / 3)
            self.assertEqual(row["breadth"], 1)
            self.assertEqual(row["median_density_per_1k_tokens"], 1)
            self.assertEqual(row["p90_density_per_1k_tokens"], 2)
        finally:
            if run.exists():
                shutil.rmtree(run)
            report.unlink(missing_ok=True)


if __name__ == "__main__":
    unittest.main()
