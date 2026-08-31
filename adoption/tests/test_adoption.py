#!/usr/bin/env python3
"""Focused stdlib tests for the adoption-pack validator."""

from __future__ import annotations

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

PACK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PACK))

from validate import validate_air_gap, validate_bundle, validate_calendar, validate_links, validate_pack  # noqa: E402


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def add_artifact(bundle: Path, path: str, content: bytes) -> dict[str, object]:
    target = bundle / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(content)
    return {
        "id": path.replace("/", "-").replace(".", "-"),
        "kind": "",
        "path": path,
        "sha256": hashlib.sha256(content).hexdigest(),
        "bytes": len(content),
    }


def make_bundle(bundle: Path) -> None:
    version = "1.0.0"
    commit = "a" * 40
    lock_sha = "b" * 64
    binary_content = b"adoption-bundle-fixture\n"
    binary = add_artifact(bundle, "bin/adoption-app", binary_content)
    binary["kind"] = "binary"

    spdx = (
        "SPDXVersion: SPDX-2.3\n"
        "DataLicense: CC0-1.0\n"
        "SPDXID: SPDXRef-DOCUMENT\n"
        f"DocumentNamespace: https://jet-lang.dev/spdx/adoption-app-{version}-sha256-{lock_sha}\n"
        f"DocumentName: adoption-app-{version}\n\n"
        "PackageName: adoption-app\n"
        "SPDXID: SPDXRef-root\n"
        f"PackageVersion: {version}\n"
        "PackageChecksum: SHA256: " + hashlib.sha256(binary_content).hexdigest() + "\n"
        "PackageDownloadLocation: NOASSERTION\n"
    ).encode()
    sbom = add_artifact(bundle, "sbom.spdx", spdx)
    sbom["kind"] = "sbom-spdx"
    sbom["package_name"] = "adoption-app"
    sbom["subject"] = binary["id"]

    provenance_value = {
        "schema": "jet.adoption.provenance/v1",
        "statement": {
            "_type": "https://in-toto.io/Statement/v1",
            "subject": [{"name": "bin/adoption-app", "digest": {"sha256": binary["sha256"]}}],
            "predicateType": "https://slsa.dev/provenance/v1",
            "predicate": {
                "buildDefinition": {
                    "buildType": "https://jet-lang.dev/build/v1",
                    "externalParameters": {"source": {"commit": commit}},
                    "resolvedDependencies": [{"uri": "lock", "digest": {"sha256": lock_sha}}],
                },
                "runDetails": {
                    "builder": {"id": "fixture-builder"},
                    "metadata": {"invocationId": "fixture-invocation"},
                },
            },
        },
        "signature": {
            "algorithm": "ed25519",
            "key_id": "fixture-key",
            "value": "fixture-signature",
            "detached_artifact_sha256": hashlib.sha256(b"fixture-signature\n").hexdigest(),
        },
        "verification": {
            "status": "verified",
            "verifier": "fixture-only",
            "evidence": "fixture verification record",
        },
    }
    provenance_content = (json.dumps(provenance_value, indent=2, sort_keys=True) + "\n").encode()
    provenance = add_artifact(bundle, "provenance.json", provenance_content)
    provenance["kind"] = "provenance"
    provenance["subject"] = binary["id"]
    signature = add_artifact(bundle, "provenance.sig", b"fixture-signature\n")
    signature["kind"] = "signature"
    signature["subject"] = provenance["id"]

    licenses_value = {
        "schema": "jet.adoption.licenses/v1",
        "release_version": version,
        "complete": True,
        "entries": [
            {
                "name": "adoption-app",
                "version": version,
                "license_expression": "MIT",
                "source": "fixture source record",
                "notice": "fixture notice record",
            }
        ],
    }
    licenses_content = (json.dumps(licenses_value, indent=2, sort_keys=True) + "\n").encode()
    licenses = add_artifact(bundle, "licenses.json", licenses_content)
    licenses["kind"] = "licenses"

    security = add_artifact(
        bundle,
        "security-policy.md",
        b"## Reporting\n## Response\n## Bundle handling\n",
    )
    security["kind"] = "security-policy"

    support_value = {
        "schema": "jet.adoption.support-policy/v1",
        "release_version": version,
        "decision": "D-ADOPT-LTS1",
        "status": "preview-no-lts-claim",
        "calendar_ref": "adoption/release/calendar.json",
        "lts": None,
    }
    support_content = (json.dumps(support_value, indent=2, sort_keys=True) + "\n").encode()
    support = add_artifact(bundle, "support-policy.json", support_content)
    support["kind"] = "support-policy"

    reproducibility_value = {
        "schema": "jet.adoption.reproducibility/v1",
        "release_version": version,
        "lock_sha256": lock_sha,
        "status": "verified",
        "independent_rebuild": True,
        "checks": [{"argv": ["jet", "build", "--locked", "run.jet"], "result": "verified"}],
    }
    reproducibility_content = (json.dumps(reproducibility_value, indent=2, sort_keys=True) + "\n").encode()
    reproducibility = add_artifact(bundle, "reproducibility.json", reproducibility_content)
    reproducibility["kind"] = "reproducibility"

    fixture = PACK / "fixtures" / "air-gap" / "fixture.json"
    airgap_content = fixture.read_bytes()
    airgap = add_artifact(bundle, "air-gap/fixture.json", airgap_content)
    airgap["kind"] = "air-gap-bundle"
    for relative in ["releases/1.0.0/jet", "releases/1.1.0/jet"]:
        target = bundle / "air-gap" / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((fixture.parent / relative).read_bytes())

    manifest = {
        "schema": "jet.adoption.artifact-manifest/v1",
        "version": "1.0.0",
        "release": {
            "name": "adoption-app",
            "version": version,
            "commit": commit,
            "platform": "fixture-linux-x86_64",
            "lock_sha256": lock_sha,
        },
        "artifacts": [binary, sbom, provenance, signature, licenses, security, support, reproducibility, airgap],
        "generated_by": "adoption validator test fixture",
    }
    write_json(bundle / "artifact-manifest.json", manifest)


class AdoptionPackTests(unittest.TestCase):
    def test_checked_in_pack_is_consistent(self) -> None:
        self.assertEqual(validate_pack(PACK), [])

    def test_air_gap_fixture_covers_the_state_machine(self) -> None:
        self.assertEqual(validate_air_gap(PACK / "fixtures" / "air-gap" / "fixture.json"), [])

    def test_calendar_binds_each_pending_schedule_token_to_its_field(self) -> None:
        with tempfile.TemporaryDirectory(prefix="jet-adoption-calendar-") as directory:
            calendar = json.loads((PACK / "release" / "calendar.json").read_text(encoding="utf-8"))
            calendar["first_lts"]["start"] = calendar["first_lts"]["active_until"]
            path = Path(directory) / "calendar.json"
            write_json(path, calendar)
            self.assertTrue(any("exact owner token" in error for error in validate_calendar(path)))

    def test_calendar_records_ratified_lts_policy(self) -> None:
        calendar = json.loads((PACK / "release" / "calendar.json").read_text(encoding="utf-8"))
        self.assertEqual(calendar["status"], "ratified-awaiting-schedule")
        self.assertEqual(calendar["policy"]["cadence"], "annual")
        self.assertEqual(calendar["policy"]["active_months"], 12)
        self.assertEqual(calendar["policy"]["maintenance_months"], 24)
        self.assertEqual(calendar["policy"]["total_months"], 36)
        self.assertEqual(calendar["policy"]["maximum_overlapping_lines"], 3)

    def test_local_links_are_checked(self) -> None:
        with tempfile.TemporaryDirectory(prefix="jet-adoption-links-") as directory:
            root = Path(directory)
            (root / "ok.md").write_text("[target](target.md)\n", encoding="utf-8")
            (root / "target.md").write_text("# Target\n", encoding="utf-8")
            self.assertEqual(validate_links(root), [])
            (root / "ok.md").write_text("[missing](missing.md)\n", encoding="utf-8")
            self.assertTrue(validate_links(root))

    def test_bundle_binds_sbom_provenance_licenses_and_digests(self) -> None:
        with tempfile.TemporaryDirectory(prefix="jet-adoption-bundle-") as directory:
            bundle = Path(directory)
            make_bundle(bundle)
            self.assertEqual(validate_bundle(bundle), [])

            binary = bundle / "bin/adoption-app"
            binary.write_bytes(b"tampered\n")
            self.assertTrue(any("binary digest mismatch" in error for error in validate_bundle(bundle)))

    def test_bundle_binds_provenance_to_lock_digest(self) -> None:
        with tempfile.TemporaryDirectory(prefix="jet-adoption-provenance-") as directory:
            bundle = Path(directory)
            make_bundle(bundle)
            provenance_path = bundle / "provenance.json"
            provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
            provenance["statement"]["predicate"]["buildDefinition"]["resolvedDependencies"][0]["digest"]["sha256"] = "c" * 64
            write_json(provenance_path, provenance)
            self.assertTrue(any("lock digest" in error for error in validate_bundle(bundle)))

    def test_pending_support_policy_cannot_be_published(self) -> None:
        with tempfile.TemporaryDirectory(prefix="jet-adoption-publish-") as directory:
            bundle = Path(directory)
            make_bundle(bundle)
            errors = validate_bundle(bundle, publishable=True)
            self.assertTrue(any("cannot be published" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
