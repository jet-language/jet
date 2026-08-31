#!/usr/bin/env python3
"""Stdlib-only checks for the Jet enterprise adoption pack.

Default mode checks the checked-in contract.  ``--bundle`` checks a concrete
release evidence directory.  ``--execute-playbook`` is the only mode that
starts external commands; it runs each declared command in a fresh copy.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, Iterable, Optional


PACK_SCHEMA_NAMES = {
    "playbook.schema.json",
    "artifact-manifest.schema.json",
    "provenance.schema.json",
    "license-inventory.schema.json",
    "support-policy.schema.json",
    "release-calendar.schema.json",
    "reproducibility.schema.json",
    "air-gap-fixture.schema.json",
    "case-study.schema.json",
}

PLAYBOOK_PHASES = {"prerequisites", "inventory", "pilot", "build-import", "test"}
OPERATOR_PHASES = {"rollout", "rollback", "ownership", "known-non-goals"}
PLAYBOOK_SECTIONS = PLAYBOOK_PHASES | OPERATOR_PHASES
BUNDLE_KINDS = {
    "binary",
    "sbom-spdx",
    "provenance",
    "signature",
    "licenses",
    "security-policy",
    "support-policy",
    "reproducibility",
    "air-gap-bundle",
}
OPTIONAL_BUNDLE_KINDS = {"sbom-cyclonedx"}
ALL_BUNDLE_KINDS = BUNDLE_KINDS | OPTIONAL_BUNDLE_KINDS
SHA256 = re.compile(r"^[0-9a-f]{64}$")
SEMVER = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
IDENT = re.compile(r"^[a-z0-9][a-z0-9-]*$")
RELEASE_ID = re.compile(r"^[a-z0-9][a-z0-9.-]*$")
OWNER_TOKEN_ANY = re.compile(r"\{owner:[^}]+\}")
SHELL_META = re.compile(r"[;&|<>`$\x00]")
SECRET_BYTES = [
    re.compile(rb"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----"),
    re.compile(rb"AKIA[0-9A-Z]{16}"),
    re.compile(
        rb"(?i)\b(?:password|token|secret|api[_-]?key)\b\s*[:=]\s*"
        rb"(?:[\"'][^\"'\r\n]{20,}[\"']|[A-Za-z0-9+/=_-]{20,})"
    ),
    re.compile(rb"(?i)https?://[^/\s:@]+:[^/\s@]+@"),
]

RATIFIED_POLICY = {
    "cadence": "annual",
    "active_months": 12,
    "maintenance_months": 24,
    "total_months": 36,
    "maximum_overlapping_lines": 3,
    "notice_rule": "publish calendar changes at least six months before effect; never shorten a live LTS line",
    "backport_rule": "active: security, critical compiler/runtime correctness, supported-host/toolchain breakage, and severe performance regressions; maintenance: security and critical data-loss, memory-safety, type-safety, or miscompilation fixes; no new language behavior",
}

OWNER_FIELD_TOKENS = {
    "first_lts.start": "{owner:D-ADOPT-LTS1.first_lts_start}",
    "first_lts.active_until": "{owner:D-ADOPT-LTS1.first_lts_active_until}",
    "first_lts.maintenance_until": "{owner:D-ADOPT-LTS1.first_lts_maintenance_until}",
    "first_lts.eol": "{owner:D-ADOPT-LTS1.first_lts_eol}",
    "first_lts.replacement": "{owner:D-ADOPT-LTS1.first_lts_replacement}",
    "support_matrix.editions": "{owner:D-ADOPT-LTS1.supported_editions}",
    "support_matrix.hosts": "{owner:D-ADOPT-LTS1.supported_hosts}",
}
OWNER_TOKENS = set(OWNER_FIELD_TOKENS.values())


def load_json(path: Path, errors: list[str]) -> Optional[dict[str, Any]]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        errors.append(f"{path}: invalid JSON: {exc}")
        return None
    if not isinstance(value, dict):
        errors.append(f"{path}: top level must be an object")
        return None
    return value


def strings(value: Any) -> Iterable[str]:
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for item in value.values():
            yield from strings(item)
    elif isinstance(value, list):
        for item in value:
            yield from strings(item)


def safe_path(root: Path, raw: Any, label: str, errors: list[str]) -> Optional[Path]:
    if not isinstance(raw, str) or not raw or "\x00" in raw or Path(raw).is_absolute():
        errors.append(f"{label}: path must be relative")
        return None
    relative = Path(raw)
    if ".." in relative.parts:
        errors.append(f"{label}: path escapes its root: {raw}")
        return None
    lexical = root / relative
    cursor = root
    for part in relative.parts:
        cursor /= part
        if cursor.is_symlink():
            errors.append(f"{label}: symlink path is not allowed: {raw}")
            return None
    candidate = lexical.resolve(strict=False)
    try:
        candidate.relative_to(root.resolve())
    except ValueError:
        errors.append(f"{label}: path escapes its root: {raw}")
        return None
    return lexical


def validate_schema_files(pack_root: Path) -> list[str]:
    errors: list[str] = []
    schema_root = pack_root / "schemas"
    actual = {path.name for path in schema_root.glob("*.schema.json")}
    for missing in sorted(PACK_SCHEMA_NAMES - actual):
        errors.append(f"{schema_root}: missing schema {missing}")
    for extra in sorted(actual - PACK_SCHEMA_NAMES):
        errors.append(f"{schema_root / extra}: unregistered schema")
    for name in sorted(PACK_SCHEMA_NAMES & actual):
        data = load_json(schema_root / name, errors)
        if data is None:
            continue
        for field in ("$schema", "$id", "type", "required"):
            if field not in data:
                errors.append(f"{schema_root / name}: schema lacks {field}")
        if data.get("type") != "object":
            errors.append(f"{schema_root / name}: top-level schema type must be object")
        if not isinstance(data.get("required"), list) or not data["required"]:
            errors.append(f"{schema_root / name}: required must be a non-empty list")
    return errors


def validate_calendar(path: Path) -> list[str]:
    errors: list[str] = []
    data = load_json(path, errors)
    if data is None:
        return errors
    if data.get("schema") != "jet.adoption.release-calendar/v1":
        errors.append(f"{path}: wrong schema")
    if data.get("status") != "ratified-awaiting-schedule":
        errors.append(f"{path}: calendar must remain ratified-awaiting-schedule")
    if data.get("decision") != "D-ADOPT-LTS1":
        errors.append(f"{path}: calendar must name D-ADOPT-LTS1")
    if data.get("policy") != RATIFIED_POLICY:
        errors.append(f"{path}: policy must match ratified D-ADOPT-LTS1 values")

    found: set[str] = set()
    for value in strings(data):
        for token in OWNER_TOKEN_ANY.findall(value):
            if token not in OWNER_TOKENS:
                errors.append(f"{path}: unknown owner token {token}")
            found.add(token)
        if re.fullmatch(r"\d{4}-\d{2}-\d{2}", value):
            errors.append(f"{path}: pending calendar contains an invented date {value}")
    missing = OWNER_TOKENS - found
    for token in sorted(missing):
        errors.append(f"{path}: missing owner token {token}")
    owner_fields = data.get("owner_fields")
    if (
        not isinstance(owner_fields, list)
        or any(not isinstance(item, str) for item in owner_fields)
        or len(owner_fields) != len(set(owner_fields))
        or set(owner_fields) != set(OWNER_FIELD_TOKENS)
    ):
        errors.append(f"{path}: owner_fields does not match the pending GA schedule surface")
    for field, token in OWNER_FIELD_TOKENS.items():
        current: Any = data
        for part in field.split("."):
            if not isinstance(current, dict):
                current = None
                break
            current = current.get(part)
        if current != token:
            errors.append(f"{path}: {field} must contain its exact owner token")
    return errors


def validate_case_studies(pack_root: Path) -> list[str]:
    errors: list[str] = []
    case_root = pack_root / "case-studies"
    for path in sorted(case_root.glob("*.json")):
        data = load_json(path, errors)
        if data is None:
            continue
        required = {"schema", "version", "capstone", "source", "jet", "measurements", "failures", "reproduction", "claims"}
        if data.get("schema") != "jet.adoption.case-study/v1":
            errors.append(f"{path}: wrong schema")
        missing = required - data.keys()
        if missing:
            errors.append(f"{path}: missing case-study fields: {', '.join(sorted(missing))}")
        if not data.get("measurements") or not data.get("failures") or not data.get("claims"):
            errors.append(f"{path}: case study needs measurements, failures, and bounded claims")
        reproduction = data.get("reproduction", {})
        if not isinstance(reproduction, dict) or not reproduction.get("machine") or not reproduction.get("receipt"):
            errors.append(f"{path}: case study needs clean-machine reproduction evidence")
    return errors


def validate_playbook(path: Path, pack_root: Path) -> list[str]:
    errors: list[str] = []
    data = load_json(path, errors)
    if data is None:
        return errors
    if data.get("schema") != "jet.adoption.playbook-checks/v1":
        errors.append(f"{path}: wrong schema")
    if not SEMVER.fullmatch(str(data.get("version", ""))):
        errors.append(f"{path}: version must be SemVer")
    if data.get("tier_neutral") is not True:
        errors.append(f"{path}: command registry must be tier-neutral")
    required_sections = data.get("required_sections")
    if not isinstance(required_sections, list) or any(not isinstance(item, str) for item in required_sections) or set(required_sections) != PLAYBOOK_SECTIONS:
        errors.append(f"{path}: required_sections must cover the complete playbook contract")

    project = safe_path(pack_root, data.get("representative_project"), f"{path}: representative_project", errors)
    if project is not None and not (project / "run.jet").is_file():
        errors.append(f"{path}: representative project lacks run.jet")

    ids: set[str] = set()
    command_phases: set[str] = set()
    commands = data.get("commands")
    if not isinstance(commands, list):
        errors.append(f"{path}: commands must be a list")
        commands = []
    for command in commands:
        if not isinstance(command, dict):
            errors.append(f"{path}: command entry must be an object")
            continue
        ident = command.get("id")
        if not isinstance(ident, str) or not IDENT.fullmatch(ident):
            errors.append(f"{path}: command id must be a lowercase identifier")
            ident_key = f"<invalid-command-{len(ids)}>"
        else:
            ident_key = ident
        if ident_key in ids:
            errors.append(f"{path}: duplicate command id {ident}")
        ids.add(ident_key)
        phase = command.get("phase")
        if isinstance(phase, str):
            command_phases.add(phase)
        if not isinstance(phase, str) or phase not in PLAYBOOK_PHASES:
            errors.append(f"{path}: command {ident} has invalid phase {phase}")
        argv = command.get("argv")
        if not isinstance(argv, list) or not argv or any(not isinstance(item, str) or not item for item in argv):
            errors.append(f"{path}: command {ident} must use a non-empty argv list")
        else:
            for item in argv:
                if SHELL_META.search(item):
                    errors.append(f"{path}: command {ident} contains shell metacharacters")
            if argv[0] != "jet":
                errors.append(f"{path}: baseline command {ident} must invoke jet")
        if command.get("cwd") != data.get("representative_project"):
            errors.append(f"{path}: command {ident} must use the representative project cwd")
        if type(command.get("expected_exit")) is not int:
            errors.append(f"{path}: command {ident} lacks an integer expected_exit")
        if command.get("requires_clean_copy") is not True:
            errors.append(f"{path}: command {ident} must require a clean copy")
        if command.get("network") != "denied":
            errors.append(f"{path}: command {ident} must require denied network")
        captures = command.get("captures")
        if not isinstance(captures, list) or any(not isinstance(item, str) or not item for item in captures) or not {"stdout", "stderr", "exit_code"}.issubset(captures):
            errors.append(f"{path}: command {ident} must capture stdout, stderr, and exit_code")
        elif project is not None:
            for capture in captures:
                if capture in {"stdout", "stderr", "exit_code"}:
                    continue
                safe_path(project, capture, f"{path}: command {ident} capture", errors)
        if any(key in command for key in ("tier", "language", "importer")):
            errors.append(f"{path}: command {ident} must not encode a language tier")

    if not {"prerequisites", "pilot", "build-import", "test"}.issubset(command_phases):
        errors.append(f"{path}: baseline commands must cover prerequisites, pilot, build-import, and test")

    operator_ids: set[str] = set()
    operator_phases: set[str] = set()
    operator_checks = data.get("operator_checks")
    if not isinstance(operator_checks, list):
        errors.append(f"{path}: operator_checks must be a list")
        operator_checks = []
    for check in operator_checks:
        if not isinstance(check, dict):
            errors.append(f"{path}: operator check must be an object")
            continue
        ident = check.get("id")
        if not isinstance(ident, str) or not IDENT.fullmatch(ident):
            errors.append(f"{path}: operator check id must be a lowercase identifier")
            ident_key = f"<invalid-operator-{len(operator_ids)}>"
        else:
            ident_key = ident
        if ident_key in ids or ident_key in operator_ids:
            errors.append(f"{path}: duplicate check id {ident}")
        operator_ids.add(ident_key)
        phase = check.get("phase")
        if isinstance(phase, str):
            operator_phases.add(phase)
        if not isinstance(phase, str) or phase not in OPERATOR_PHASES:
            errors.append(f"{path}: operator check {ident} has invalid phase {phase}")
        if not isinstance(check.get("action"), str) or not check["action"].strip():
            errors.append(f"{path}: operator check {ident} lacks an action")
        if not isinstance(check.get("evidence"), list) or not check["evidence"] or any(not isinstance(item, str) or not item.strip() for item in check["evidence"]):
            errors.append(f"{path}: operator check {ident} lacks evidence requirements")
    if operator_phases != OPERATOR_PHASES:
        errors.append(f"{path}: operator checks must cover rollout, rollback, ownership, and known-non-goals")
    return errors


def validate_links(pack_root: Path) -> list[str]:
    errors: list[str] = []
    link_pattern = re.compile(r"(?<!!)\[[^\]]+\]\(([^)\s]+)")
    for markdown in sorted(pack_root.rglob("*.md")):
        text = markdown.read_text(encoding="utf-8")
        for raw in link_pattern.findall(text):
            target = raw.strip("<>")
            if target.startswith(("https://", "http://", "mailto:")):
                continue
            file_part, _, anchor = target.partition("#")
            candidate = markdown if not file_part else (markdown.parent / file_part)
            candidate = candidate.resolve(strict=False)
            try:
                candidate.relative_to(pack_root.resolve().parent)
            except ValueError:
                errors.append(f"{markdown}: link escapes repository: {target}")
                continue
            if not candidate.exists():
                errors.append(f"{markdown}: broken local link {target}")
                continue
            if anchor and candidate.is_file():
                headings = {
                    re.sub(r"[^a-z0-9 -]", "", line[1:].lower()).strip().replace(" ", "-")
                    for line in candidate.read_text(encoding="utf-8").splitlines()
                    if line.startswith("#")
                }
                if anchor.lower() not in headings:
                    errors.append(f"{markdown}: broken anchor {target}")
    return errors


def secret_errors(root: Path) -> list[str]:
    errors: list[str] = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            errors.append(f"{path}: symlinks are not allowed in the adoption pack")
            continue
        if not path.is_file():
            continue
        try:
            data = path.read_bytes()
        except OSError as exc:
            errors.append(f"{path}: cannot read for secret check: {exc}")
            continue
        for pattern in SECRET_BYTES:
            if pattern.search(data):
                errors.append(f"{path}: secret-like material is not allowed in the adoption pack")
                break
    return errors


def validate_air_gap(path: Path) -> list[str]:
    errors: list[str] = []
    data = load_json(path, errors)
    if data is None:
        return errors
    if data.get("schema") != "jet.adoption.air-gap-fixture/v1":
        errors.append(f"{path}: wrong schema")
    if not SEMVER.fullmatch(str(data.get("fixture_version", ""))):
        errors.append(f"{path}: fixture_version must be SemVer")
    if not isinstance(data.get("purpose"), str) or not data["purpose"].strip():
        errors.append(f"{path}: fixture needs a purpose")
    if data.get("evidence_class") != "fixture-only":
        errors.append(f"{path}: air-gap data must remain fixture-only")
    network = data.get("network", {})
    if not isinstance(network, dict) or network.get("mode") != "denied" or network.get("attempted_connections") != 0:
        errors.append(f"{path}: fixture must model denied network with zero attempted connections")
    root = path.parent
    releases: dict[str, dict[str, Any]] = {}
    release_entries = data.get("releases")
    if not isinstance(release_entries, list):
        errors.append(f"{path}: releases must be a list")
        release_entries = []
    for release in release_entries:
        if not isinstance(release, dict):
            errors.append(f"{path}: release entry must be an object")
            continue
        ident = release.get("id")
        if not isinstance(ident, str) or not RELEASE_ID.fullmatch(ident):
            errors.append(f"{path}: release id must be a lowercase identifier")
            continue
        if ident in releases:
            errors.append(f"{path}: duplicate release {ident}")
        releases[ident] = release
        if not SEMVER.fullmatch(str(release.get("version", ""))):
            errors.append(f"{path}: release {ident} has an invalid version")
        if not isinstance(release.get("builder"), str) or not release["builder"]:
            errors.append(f"{path}: release {ident} lacks a builder")
        if not SHA256.fullmatch(str(release.get("sha256", ""))):
            errors.append(f"{path}: release {ident} has an invalid digest")
        if type(release.get("bytes")) is not int or release["bytes"] < 0:
            errors.append(f"{path}: release {ident} has an invalid byte count")
        if not isinstance(release.get("state"), str) or not release["state"]:
            errors.append(f"{path}: release {ident} lacks a state")
        object_path = safe_path(root, release.get("object"), f"{path}: release {ident}", errors)
        if object_path is None:
            continue
        if object_path.is_symlink() or not object_path.is_file():
            errors.append(f"{path}: release {ident} object is not a regular file")
            continue
        actual = object_path.read_bytes()
        if hashlib.sha256(actual).hexdigest() != release.get("sha256"):
            errors.append(f"{path}: release {ident} digest mismatch")
        if len(actual) != release.get("bytes"):
            errors.append(f"{path}: release {ident} byte count mismatch")
        if release.get("signature") != "fixture-only":
            errors.append(f"{path}: release {ident} must not claim a production signature")

    trust = data.get("trust", {})
    if not isinstance(trust, dict):
        errors.append(f"{path}: trust must be an object")
        trust = {}
    if trust.get("root_kind") != "fixture-public-key" or not isinstance(trust.get("key_id"), str) or not trust.get("key_id") or not isinstance(trust.get("public_key"), str) or not trust.get("public_key"):
        errors.append(f"{path}: fixture trust root must contain a public key identity")
    revoked_builders = trust.get("revoked_builders", [])
    if not isinstance(revoked_builders, list) or any(not isinstance(item, str) or not item for item in revoked_builders):
        errors.append(f"{path}: revoked_builders must be a list of non-empty strings")
        revoked_builders = []
    revoked = set(revoked_builders)
    declared_revocations: dict[str, dict[str, Any]] = {}
    revocation_entries = data.get("revocations")
    if not isinstance(revocation_entries, list):
        errors.append(f"{path}: revocations must be a list")
        revocation_entries = []
    for item in revocation_entries:
        if not isinstance(item, dict):
            errors.append(f"{path}: malformed builder revocation")
            continue
        if item.get("kind") != "builder" or not isinstance(item.get("id"), str) or not item.get("id") or not isinstance(item.get("reason"), str) or not item.get("reason") or type(item.get("sequence")) is not int or item["sequence"] < 1:
            errors.append(f"{path}: malformed builder revocation")
        elif item["id"] in declared_revocations:
            errors.append(f"{path}: duplicate builder revocation {item['id']}")
        else:
            declared_revocations[item["id"]] = item

    scenario_ids: set[str] = set()
    saw_allowed_update = False
    saw_denied_update = False
    saw_revoke = False
    scenario_entries = data.get("scenarios")
    if not isinstance(scenario_entries, list):
        errors.append(f"{path}: scenarios must be a list")
        scenario_entries = []
    for scenario in scenario_entries:
        if not isinstance(scenario, dict):
            errors.append(f"{path}: scenario entry must be an object")
            continue
        ident = scenario.get("id")
        if not isinstance(ident, str) or not IDENT.fullmatch(ident):
            errors.append(f"{path}: scenario id must be a lowercase identifier")
            continue
        if ident in scenario_ids:
            errors.append(f"{path}: duplicate scenario {ident}")
        scenario_ids.add(ident)
        operation = scenario.get("operation")
        target_id = scenario.get("to")
        target = releases.get(target_id) if isinstance(target_id, str) else None
        if target is None:
            errors.append(f"{path}: scenario {ident} targets unknown release")
            continue
        source_id = scenario.get("from")
        if source_id is not None and (not isinstance(source_id, str) or source_id not in releases):
            errors.append(f"{path}: scenario {ident} starts from unknown release")
        if scenario.get("network") != "denied":
            errors.append(f"{path}: scenario {ident} must deny network")
        if not isinstance(operation, str) or operation not in {"install", "update", "revoke"}:
            errors.append(f"{path}: scenario {ident} has an invalid operation")
        expected = scenario.get("expected")
        if not isinstance(expected, str) or expected not in {"allow", "deny"}:
            errors.append(f"{path}: scenario {ident} has an invalid expected result")
        builder = target.get("builder")
        builder_revoked = isinstance(builder, str) and builder in revoked
        if operation == "revoke":
            if not isinstance(builder, str) or builder not in declared_revocations:
                errors.append(f"{path}: scenario {ident} revokes an undeclared builder")
            elif isinstance(builder, str):
                revoked.add(builder)
        if expected == "allow" and builder_revoked:
            errors.append(f"{path}: scenario {ident} allows a revoked builder")
        if expected == "deny" and not builder_revoked and operation != "revoke":
            errors.append(f"{path}: scenario {ident} denies a non-revoked release without a reason")
        if expected == "deny" and "rebuild" not in str(scenario.get("reason", "")).lower():
            errors.append(f"{path}: denied scenario {ident} lacks rebuild recovery")
        saw_allowed_update |= operation == "update" and expected == "allow"
        saw_denied_update |= operation == "update" and expected == "deny"
        saw_revoke |= operation == "revoke" and expected == "deny"
    if not saw_allowed_update or not saw_denied_update or not saw_revoke:
        errors.append(f"{path}: fixture must cover allowed update, denied update, and revocation")
    return errors


def parse_spdx(text: str) -> tuple[dict[str, str], list[tuple[str, str]]]:
    fields: dict[str, str] = {}
    packages: list[dict[str, str]] = []
    current: Optional[dict[str, str]] = None
    for line in text.splitlines():
        key, separator, value = line.partition(": ")
        if not separator:
            continue
        if key == "PackageName":
            current = {"name": value, "version": ""}
            packages.append(current)
        elif key == "PackageVersion" and current is not None:
            current["version"] = value
        elif key in {"SPDXVersion", "DocumentName", "DocumentNamespace"}:
            fields[key] = value
        elif key == "PackageChecksum" and packages and current is packages[0]:
            algorithm, separator, checksum = value.partition(": ")
            fields["RootPackageChecksumAlgorithm"] = algorithm
            fields["RootPackageChecksum"] = checksum if separator else value
    return fields, [(item["name"], item["version"]) for item in packages]


def valid_license_expression(value: Any) -> bool:
    if not isinstance(value, str) or not value.strip() or value in {"NONE", "NOASSERTION", "UNKNOWN"}:
        return False
    expression = value.strip()
    if "NOASSERTION" in expression or "UNKNOWN" in expression:
        return False
    return bool(re.fullmatch(
        r"[A-Za-z0-9.+-]+(?:\s+(?:AND|OR|WITH)\s+[A-Za-z0-9.+-]+)*",
        expression,
    ))


def validate_bundle(bundle: Path, publishable: bool = False, calendar_path: Optional[Path] = None) -> list[str]:
    errors: list[str] = []
    manifest_path = bundle / "artifact-manifest.json"
    manifest = load_json(manifest_path, errors)
    if manifest is None:
        return errors
    if calendar_path is None:
        calendar_path = Path(__file__).resolve().parent / "release" / "calendar.json"
    calendar = load_json(calendar_path, errors)
    calendar_status = calendar.get("status") if calendar is not None else None
    if manifest.get("schema") != "jet.adoption.artifact-manifest/v1":
        errors.append(f"{manifest_path}: wrong schema")
    if not isinstance(manifest.get("generated_by"), str) or not manifest["generated_by"].strip():
        errors.append(f"{manifest_path}: generated_by must be non-empty")
    release = manifest.get("release", {})
    if not isinstance(release, dict):
        errors.append(f"{manifest_path}: release must be an object")
        release = {}
    version = release.get("version")
    commit = release.get("commit")
    lock_sha = release.get("lock_sha256")
    if manifest.get("version") != version:
        errors.append(f"{manifest_path}: manifest version disagrees with release.version")
    if not isinstance(release.get("name"), str) or not release["name"]:
        errors.append(f"{manifest_path}: release.name must be non-empty")
    if not isinstance(release.get("platform"), str) or not release["platform"]:
        errors.append(f"{manifest_path}: release.platform must be non-empty")
    if not SEMVER.fullmatch(str(version)):
        errors.append(f"{manifest_path}: release.version must be SemVer")
    if not COMMIT.fullmatch(str(commit)):
        errors.append(f"{manifest_path}: release.commit must be a 40-character lowercase commit")
    if not SHA256.fullmatch(str(lock_sha)):
        errors.append(f"{manifest_path}: release.lock_sha256 must be SHA-256")

    artifacts: dict[str, dict[str, Any]] = {}
    kinds: set[str] = set()
    kind_counts: dict[str, int] = {}
    artifact_entries = manifest.get("artifacts")
    if not isinstance(artifact_entries, list):
        errors.append(f"{manifest_path}: artifacts must be a list")
        artifact_entries = []
    for artifact in artifact_entries:
        if not isinstance(artifact, dict):
            errors.append(f"{manifest_path}: artifact entry must be an object")
            continue
        ident = artifact.get("id")
        kind = artifact.get("kind")
        if not isinstance(ident, str) or not IDENT.fullmatch(ident):
            errors.append(f"{manifest_path}: artifact id must be a lowercase identifier")
            continue
        if ident in artifacts:
            errors.append(f"{manifest_path}: duplicate artifact {ident}")
        artifacts[ident] = artifact
        if isinstance(kind, str):
            kinds.add(kind)
            kind_counts[kind] = kind_counts.get(kind, 0) + 1
            if kind_counts[kind] > 1:
                errors.append(f"{manifest_path}: duplicate artifact kind {kind}")
        if not isinstance(kind, str) or kind not in ALL_BUNDLE_KINDS:
            errors.append(f"{manifest_path}: unknown artifact kind {kind}")
        if not SHA256.fullmatch(str(artifact.get("sha256", ""))):
            errors.append(f"{manifest_path}: artifact {ident} has an invalid digest")
        if type(artifact.get("bytes")) is not int or artifact["bytes"] < 0:
            errors.append(f"{manifest_path}: artifact {ident} has an invalid byte count")
        file_path = safe_path(bundle, artifact.get("path"), f"{manifest_path}: artifact {ident}", errors)
        if file_path is None:
            continue
        if file_path.is_symlink() or not file_path.is_file():
            errors.append(f"{manifest_path}: artifact {ident} is not a regular file")
            continue
        content = file_path.read_bytes()
        if hashlib.sha256(content).hexdigest() != artifact.get("sha256"):
            label = "binary" if kind == "binary" else f"artifact {ident}"
            errors.append(f"{manifest_path}: {label} digest mismatch")
        if len(content) != artifact.get("bytes"):
            errors.append(f"{manifest_path}: artifact {ident} byte count mismatch")
    missing_kinds = BUNDLE_KINDS - kinds
    for kind in sorted(missing_kinds):
        errors.append(f"{manifest_path}: missing required artifact kind {kind}")

    def artifact_file(kind: str) -> Optional[Path]:
        artifact = next((item for item in artifacts.values() if item.get("kind") == kind), None)
        if artifact is None:
            return None
        return safe_path(bundle, artifact.get("path"), f"{manifest_path}: {kind}", errors)

    binary = next((item for item in artifacts.values() if item.get("kind") == "binary"), None)
    binary_hash = binary.get("sha256") if binary else None
    binary_id = binary.get("id") if binary else None
    sbom_path = artifact_file("sbom-spdx")
    package_name = None
    packages: list[tuple[str, str]] = []
    if sbom_path and sbom_path.is_file():
        try:
            fields, packages = parse_spdx(sbom_path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError) as exc:
            errors.append(f"{sbom_path}: cannot read SPDX document: {exc}")
            fields = {}
        sbom_artifact = next(item for item in artifacts.values() if item.get("kind") == "sbom-spdx")
        package_name = sbom_artifact.get("package_name")
        if sbom_artifact.get("subject") != binary_id:
            errors.append(f"{sbom_path}: SBOM subject does not name the binary artifact")
        if not package_name or not packages or packages[0][0] != package_name:
            errors.append(f"{sbom_path}: root package does not match artifact package_name")
        if package_name and release.get("name") != package_name:
            errors.append(f"{sbom_path}: root package disagrees with release.name")
        if fields.get("SPDXVersion") != "SPDX-2.3":
            errors.append(f"{sbom_path}: SPDX document must declare SPDX-2.3")
        if fields.get("DocumentName") != f"{package_name}-{version}":
            errors.append(f"{sbom_path}: DocumentName does not match release version")
        expected_namespace = f"https://jet-lang.dev/spdx/{package_name}-{version}-sha256-{lock_sha}"
        if fields.get("DocumentNamespace") != expected_namespace:
            errors.append(f"{sbom_path}: namespace is not bound to release.lock_sha256")
        if packages and packages[0][1] != version:
            errors.append(f"{sbom_path}: root PackageVersion does not match release version")
        root_checksum = fields.get("RootPackageChecksum")
        if root_checksum and root_checksum != "NOASSERTION":
            if fields.get("RootPackageChecksumAlgorithm") != "SHA256" or not SHA256.fullmatch(root_checksum):
                errors.append(f"{sbom_path}: root PackageChecksum must be a SHA256 value or NOASSERTION")

    cyclonedx_path = artifact_file("sbom-cyclonedx")
    if cyclonedx_path and cyclonedx_path.is_file():
        try:
            cdx = json.loads(cyclonedx_path.read_text(encoding="utf-8"))
            if not isinstance(cdx, dict):
                raise ValueError("top level must be an object")
            metadata = cdx.get("metadata", {})
            component = metadata.get("component", {}) if isinstance(metadata, dict) else {}
            if not isinstance(component, dict):
                component = {}
            if cdx.get("bomFormat") != "CycloneDX" or component.get("version") != version:
                errors.append(f"{cyclonedx_path}: CycloneDX root does not match release version")
            if cdx.get("serialNumber") != f"urn:uuid:jet-sha256-{lock_sha}":
                errors.append(f"{cyclonedx_path}: CycloneDX serial does not bind the release lock digest")
            if package_name and component.get("name") != package_name:
                errors.append(f"{cyclonedx_path}: CycloneDX root disagrees with SPDX root")
        except (OSError, UnicodeError, json.JSONDecodeError) as exc:
            errors.append(f"{cyclonedx_path}: invalid CycloneDX JSON: {exc}")

    provenance_path = artifact_file("provenance")
    if provenance_path and provenance_path.is_file():
        provenance = load_json(provenance_path, errors)
        if provenance is not None:
            provenance_artifact = next(item for item in artifacts.values() if item.get("kind") == "provenance")
            if provenance_artifact.get("subject") != binary_id:
                errors.append(f"{provenance_path}: provenance artifact does not name the binary artifact")
            statement = provenance.get("statement", {})
            if not isinstance(statement, dict):
                statement = {}
            subjects = statement.get("subject", [])
            if not isinstance(subjects, list):
                subjects = []
            subject = next(
                (item for item in subjects if isinstance(item, dict) and item.get("name") == binary.get("path")),
                None,
            ) if binary else None
            digest = subject.get("digest", {}) if isinstance(subject, dict) else {}
            if not isinstance(digest, dict) or digest.get("sha256") != binary_hash:
                errors.append(f"{provenance_path}: provenance subject does not bind the binary")
            if statement.get("_type") != "https://in-toto.io/Statement/v1":
                errors.append(f"{provenance_path}: missing in-toto Statement v1")
            if statement.get("predicateType") != "https://slsa.dev/provenance/v1":
                errors.append(f"{provenance_path}: missing SLSA provenance predicate")
            predicate = statement.get("predicate", {})
            if not isinstance(predicate, dict):
                predicate = {}
            build_definition = predicate.get("buildDefinition", {})
            if not isinstance(build_definition, dict):
                build_definition = {}
            run_details = predicate.get("runDetails", {})
            if not isinstance(run_details, dict):
                run_details = {}
            resolved_dependencies = build_definition.get("resolvedDependencies")
            if not build_definition.get("buildType") or not isinstance(resolved_dependencies, list) or not resolved_dependencies:
                errors.append(f"{provenance_path}: incomplete SLSA build definition")
            dependencies = resolved_dependencies if isinstance(resolved_dependencies, list) else []
            if not any(
                isinstance(item, dict)
                and item.get("uri") == "lock"
                and isinstance(item.get("digest"), dict)
                and item["digest"].get("sha256") == lock_sha
                for item in dependencies
            ):
                errors.append(f"{provenance_path}: provenance does not bind the release lock digest")
            builder = run_details.get("builder", {})
            metadata = run_details.get("metadata", {})
            if not isinstance(builder, dict) or not isinstance(metadata, dict) or not builder.get("id") or not metadata.get("invocationId"):
                errors.append(f"{provenance_path}: incomplete SLSA run details")
            external_parameters = build_definition.get("externalParameters", {})
            if not isinstance(external_parameters, dict):
                external_parameters = {}
            source = external_parameters.get("source", {})
            if not isinstance(source, dict):
                source = {}
            if source.get("commit") != commit:
                errors.append(f"{provenance_path}: source commit disagrees with release manifest")
            signature = provenance.get("signature", {})
            if not isinstance(signature, dict):
                signature = {}
            if (
                not signature.get("algorithm")
                or not signature.get("key_id")
                or not signature.get("value")
                or not SHA256.fullmatch(str(signature.get("detached_artifact_sha256", "")))
            ):
                errors.append(f"{provenance_path}: missing detached-signature evidence")
            verification = provenance.get("verification", {})
            if not isinstance(verification, dict):
                verification = {}
            if publishable and verification.get("status") != "verified":
                errors.append(f"{provenance_path}: publishable bundle needs verified provenance")

    signature_artifact = next((item for item in artifacts.values() if item.get("kind") == "signature"), None)
    provenance_artifact = next((item for item in artifacts.values() if item.get("kind") == "provenance"), None)
    if signature_artifact and provenance_artifact and signature_artifact.get("subject") != provenance_artifact.get("id"):
        errors.append(f"{manifest_path}: signature artifact does not name the provenance artifact")
    if provenance_path and provenance_path.is_file() and signature_artifact:
        provenance = load_json(provenance_path, [])
        signature = provenance.get("signature") if provenance else None
        detached = signature.get("detached_artifact_sha256") if isinstance(signature, dict) else None
        if detached != signature_artifact.get("sha256"):
            errors.append(f"{manifest_path}: signature artifact digest disagrees with provenance")

    license_path = artifact_file("licenses")
    if license_path and license_path.is_file():
        licenses = load_json(license_path, errors)
        if licenses is not None:
            if licenses.get("release_version") != version or licenses.get("complete") is not True:
                errors.append(f"{license_path}: license inventory is not complete for this release")
            license_entries = licenses.get("entries")
            if not isinstance(license_entries, list) or not license_entries:
                errors.append(f"{license_path}: license inventory needs entries")
                license_entries = []
            entries = {(item.get("name"), item.get("version")): item for item in license_entries if isinstance(item, dict)}
            for package in packages:
                entry = entries.get(package)
                if entry is None:
                    errors.append(f"{license_path}: missing license entry for {package[0]}#{package[1]}")
                elif not valid_license_expression(entry.get("license_expression")):
                    errors.append(f"{license_path}: unusable SPDX expression for {package[0]}#{package[1]}")
                elif not entry.get("source") or not entry.get("notice"):
                    errors.append(f"{license_path}: incomplete source/notice for {package[0]}#{package[1]}")

    security_path = artifact_file("security-policy")
    if security_path and security_path.is_file():
        try:
            security = security_path.read_text(encoding="utf-8")
            for heading in ("## Reporting", "## Response", "## Bundle handling"):
                if heading not in security:
                    errors.append(f"{security_path}: missing {heading}")
        except (OSError, UnicodeError) as exc:
            errors.append(f"{security_path}: cannot read security policy: {exc}")

    support_path = artifact_file("support-policy")
    if support_path and support_path.is_file():
        support = load_json(support_path, errors)
        if support is not None:
            if support.get("release_version") != version or support.get("decision") != "D-ADOPT-LTS1":
                errors.append(f"{support_path}: support policy is not bound to this release and decision")
            status = support.get("status")
            if status not in {"preview-no-lts-claim", "published"}:
                errors.append(f"{support_path}: support policy has an invalid status")
            if support.get("calendar_ref") != "adoption/release/calendar.json":
                errors.append(f"{support_path}: support policy must name the canonical calendar")
            lts = support.get("lts")
            if status == "preview-no-lts-claim" and lts is not None:
                errors.append(f"{support_path}: preview support policy must not contain LTS values")
            if status == "published":
                required_lts = {
                    "cadence",
                    "active_months",
                    "maintenance_months",
                    "total_months",
                    "maximum_overlapping_lines",
                    "start",
                    "active_until",
                    "maintenance_until",
                    "eol",
                    "replacement",
                    "supported_editions",
                    "supported_hosts",
                    "notice_rule",
                    "backport_rule",
                }
                if not isinstance(lts, dict) or not required_lts.issubset(lts):
                    errors.append(f"{support_path}: published support policy lacks complete LTS values")
                if calendar_status != "ratified":
                    errors.append(f"{support_path}: published support is forbidden until D-ADOPT-LTS1 is ratified")
            if publishable and status != "published":
                errors.append(f"{support_path}: pending support policy cannot be published")
            if publishable and calendar_status != "ratified":
                errors.append(f"{support_path}: publishable bundle requires a ratified D-ADOPT-LTS1 calendar")
            if (status == "published" or publishable) and any(OWNER_TOKEN_ANY.search(value) for value in strings(support)):
                errors.append(f"{support_path}: publishable support policy contains an owner token")

    reproducibility_path = artifact_file("reproducibility")
    if reproducibility_path and reproducibility_path.is_file():
        reproducibility = load_json(reproducibility_path, errors)
        if reproducibility is not None:
            if reproducibility.get("release_version") != version or reproducibility.get("lock_sha256") != lock_sha:
                errors.append(f"{reproducibility_path}: reproducibility receipt disagrees with release identity")
            if reproducibility.get("status") != "verified" or reproducibility.get("independent_rebuild") is not True:
                errors.append(f"{reproducibility_path}: reproducibility receipt is not independently verified")
            checks = reproducibility.get("checks")
            if not isinstance(checks, list) or not checks:
                errors.append(f"{reproducibility_path}: reproducibility receipt needs verified checks")
                checks = []
            for check in checks:
                argv = check.get("argv", []) if isinstance(check, dict) else []
                if not isinstance(check, dict) or check.get("result") != "verified":
                    errors.append(f"{reproducibility_path}: every reproducibility check must be verified")
                if not isinstance(argv, list) or not argv or any(not isinstance(item, str) or SHELL_META.search(item) for item in argv):
                    errors.append(f"{reproducibility_path}: reproducibility commands must be shell-free argv lists")

    airgap_path = artifact_file("air-gap-bundle")
    if airgap_path and airgap_path.is_file():
        airgap = load_json(airgap_path, errors)
        if airgap is not None and not isinstance(airgap.get("schema"), str):
            errors.append(f"{airgap_path}: air-gap evidence has the wrong schema")
        elif airgap is not None and not airgap["schema"].startswith("jet.adoption.air-gap-fixture/"):
            errors.append(f"{airgap_path}: air-gap evidence has the wrong schema")
        if airgap is not None:
            errors.extend(validate_air_gap(airgap_path))

    errors.extend(secret_errors(bundle))
    return errors


def validate_pack(pack_root: Path) -> list[str]:
    errors: list[str] = []
    errors.extend(validate_schema_files(pack_root))
    errors.extend(validate_calendar(pack_root / "release" / "calendar.json"))
    errors.extend(validate_playbook(pack_root / "playbook" / "command-checks.json", pack_root))
    errors.extend(validate_air_gap(pack_root / "fixtures" / "air-gap" / "fixture.json"))
    errors.extend(validate_case_studies(pack_root))
    errors.extend(validate_links(pack_root))
    errors.extend(secret_errors(pack_root))
    return errors


def execute_playbook(pack_root: Path, jet_binary: str, timeout: int) -> list[str]:
    errors: list[str] = []
    path = pack_root / "playbook" / "command-checks.json"
    data = load_json(path, errors)
    if data is None:
        return errors
    fixture = safe_path(pack_root, data.get("representative_project"), f"{path}: representative_project", errors)
    if fixture is None:
        return errors
    if timeout <= 0:
        return ["playbook executor: timeout must be positive"]
    found_binary = shutil.which(jet_binary)
    binary_path = Path(found_binary) if found_binary else Path(jet_binary)
    if not binary_path.is_absolute():
        binary_path = Path.cwd() / binary_path
    if not binary_path.is_file():
        return [f"playbook executor: cannot find jet binary {jet_binary!r}"]
    binary_path = binary_path.resolve()
    commands = data.get("commands")
    if not isinstance(commands, list):
        return [f"{path}: commands must be a list"]
    for command in commands:
        if not isinstance(command, dict) or not isinstance(command.get("argv"), list) or not command["argv"]:
            errors.append(f"{path}: executor skipped malformed command")
            continue
        with tempfile.TemporaryDirectory(prefix="jet-adoption-playbook-") as temporary:
            project = Path(temporary) / "project"
            try:
                shutil.copytree(fixture, project)
            except OSError as exc:
                errors.append(f"playbook command {command.get('id', '<unknown>')} could not copy fixture: {exc}")
                continue
            argv = list(command["argv"])
            argv[0] = str(binary_path)
            environment = {
                "PATH": os.environ.get("PATH", ""),
                "JETPACK_DENY_NETWORK": "1",
                "NO_COLOR": "1",
            }
            try:
                result = subprocess.run(
                    argv,
                    cwd=project,
                    env=environment,
                    capture_output=True,
                    text=True,
                    timeout=timeout,
                    check=False,
                )
            except (OSError, UnicodeError) as exc:
                errors.append(f"playbook command {command.get('id', '<unknown>')} failed to run: {exc}")
                continue
            except subprocess.TimeoutExpired:
                errors.append(f"playbook command {command.get('id', '<unknown>')} exceeded {timeout}s")
                continue
            if result.returncode != command["expected_exit"]:
                errors.append(
                    f"playbook command {command['id']} exited {result.returncode}, expected {command['expected_exit']}"
                )
            captures = command.get("captures", [])
            if isinstance(captures, list):
                for capture in captures:
                    if capture in {"stdout", "stderr", "exit_code"}:
                        continue
                    output = safe_path(project, capture, f"{path}: command {command.get('id')} capture", errors)
                    if output is None or output.is_symlink() or not output.is_file():
                        errors.append(f"playbook command {command.get('id', '<unknown>')} did not produce {capture}")
    return errors


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--publishable", action="store_true")
    parser.add_argument("--execute-playbook", action="store_true")
    parser.add_argument("--jet", default="jet", help="jet executable for --execute-playbook")
    parser.add_argument("--timeout", type=int, default=900)
    args = parser.parse_args(argv)

    errors = validate_pack(args.root)
    if args.bundle:
        errors.extend(validate_bundle(args.bundle, args.publishable, args.root / "release" / "calendar.json"))
    if args.execute_playbook and not errors:
        errors.extend(execute_playbook(args.root, args.jet, args.timeout))
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    print("adoption pack: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
