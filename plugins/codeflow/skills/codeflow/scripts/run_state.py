#!/usr/bin/env python3
"""Validate Codeflow DAGs and maintain an atomic, resumable run ledger."""

from __future__ import annotations

import argparse
import contextlib
import functools
import hashlib
import json
import os
import re
import sys
import tempfile
import uuid
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
from typing import Any


SCHEMA_VERSION = 1
NODE_ID = re.compile(r"^[a-z0-9][a-z0-9_-]{0,63}$")
RUN_ID = NODE_ID
KINDS = {"investigate", "design", "implement", "test", "review", "verify", "synthesize", "gate"}
MODES = {"read", "write", "verify"}
TERMINAL = {"passed", "failed", "blocked"}
REQUIRED_NODE_FIELDS = {
    "id",
    "kind",
    "mode",
    "objective",
    "depends_on",
    "paths",
    "acceptance",
    "forbidden",
    "fresh_context",
}


class CodeflowError(ValueError):
    pass


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise CodeflowError(f"cannot read JSON {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise CodeflowError(f"expected JSON object: {path}")
    return value


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def digest_json(value: Any) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def atomic_write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    data = json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    fd, temp_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp_name, path)
    finally:
        try:
            os.unlink(temp_name)
        except FileNotFoundError:
            pass


@contextlib.contextmanager
def run_lock(run_dir: Path):
    """Serialize ledger transactions across coordinator processes."""
    lock_path = run_dir / ".run.lock"
    try:
        handle = lock_path.open("a+b")
    except OSError as exc:
        raise CodeflowError(f"cannot open run lock {lock_path}: {exc}") from exc
    try:
        if os.name == "nt":
            import msvcrt

            if lock_path.stat().st_size == 0:
                handle.write(b"0")
                handle.flush()
            handle.seek(0)
            msvcrt.locking(handle.fileno(), msvcrt.LK_LOCK, 1)
        else:
            import fcntl

            fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
        yield
    finally:
        if os.name == "nt":
            import msvcrt

            handle.seek(0)
            msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            import fcntl

            fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
        handle.close()


def locked_command(function):
    @functools.wraps(function)
    def wrapper(args: argparse.Namespace):
        with run_lock(Path(args.run_dir)):
            return function(args)

    return wrapper


def require_string_list(value: Any, field: str, *, nonempty: bool = False) -> list[str]:
    if not isinstance(value, list) or any(not isinstance(item, str) or not item.strip() for item in value):
        raise CodeflowError(f"{field} must be a list of nonempty strings")
    if nonempty and not value:
        raise CodeflowError(f"{field} must not be empty")
    return value


def normalize_rel_path(value: str, field: str) -> str:
    if any(ord(character) < 32 for character in value):
        raise CodeflowError(f"{field} contains a control character")
    if "\\" in value:
        raise CodeflowError(f"{field} must use forward slashes: {value}")
    path = PurePosixPath(value)
    if path.is_absolute() or ".." in path.parts:
        raise CodeflowError(f"{field} must stay inside the workspace: {value}")
    normalized = str(path)
    if normalized in {"", "/"}:
        raise CodeflowError(f"{field} contains an empty path")
    return normalized


def path_contains(scope: str, candidate: str) -> bool:
    if scope == ".":
        return True
    return candidate == scope or candidate.startswith(scope.rstrip("/") + "/")


def paths_overlap(left: str, right: str) -> bool:
    return path_contains(left, right) or path_contains(right, left)


def validate_workflow(workflow: dict[str, Any]) -> dict[str, Any]:
    allowed_top = {"schema_version", "goal", "workspace", "limits", "acceptance", "nodes"}
    unknown_top = set(workflow) - allowed_top
    if unknown_top:
        raise CodeflowError(f"unknown workflow fields: {', '.join(sorted(unknown_top))}")
    if workflow.get("schema_version") != SCHEMA_VERSION:
        raise CodeflowError(f"schema_version must be {SCHEMA_VERSION}")
    if not isinstance(workflow.get("goal"), str) or not workflow["goal"].strip():
        raise CodeflowError("goal must be a nonempty string")
    workspace = workflow.get("workspace")
    if not isinstance(workspace, str) or not Path(workspace).is_absolute():
        raise CodeflowError("workspace must be an absolute path")
    if not Path(workspace).is_dir():
        raise CodeflowError(f"workspace does not exist: {workspace}")
    require_string_list(workflow.get("acceptance"), "acceptance", nonempty=True)

    limits = workflow.get("limits")
    if not isinstance(limits, dict):
        raise CodeflowError("limits must be an object")
    required_limits = {"max_parallel", "max_attempts", "max_cycles"}
    if set(limits) != required_limits:
        raise CodeflowError("limits must contain exactly max_parallel, max_attempts, and max_cycles")
    for key in sorted(required_limits):
        if not isinstance(limits[key], int) or isinstance(limits[key], bool) or limits[key] < 1:
            raise CodeflowError(f"limits.{key} must be a positive integer")

    nodes = workflow.get("nodes")
    if not isinstance(nodes, list) or not nodes:
        raise CodeflowError("nodes must be a nonempty list")
    by_id: dict[str, dict[str, Any]] = {}
    allowed_node = REQUIRED_NODE_FIELDS | {"write_scope", "max_attempts"}
    for index, node in enumerate(nodes):
        if not isinstance(node, dict):
            raise CodeflowError(f"nodes[{index}] must be an object")
        missing = REQUIRED_NODE_FIELDS - set(node)
        unknown = set(node) - allowed_node
        if missing:
            raise CodeflowError(f"nodes[{index}] missing fields: {', '.join(sorted(missing))}")
        if unknown:
            raise CodeflowError(f"nodes[{index}] unknown fields: {', '.join(sorted(unknown))}")
        node_id = node["id"]
        if not isinstance(node_id, str) or not NODE_ID.fullmatch(node_id):
            raise CodeflowError(f"invalid node id: {node_id!r}")
        if node_id in by_id:
            raise CodeflowError(f"duplicate node id: {node_id}")
        if node["kind"] not in KINDS:
            raise CodeflowError(f"{node_id}.kind must be one of {', '.join(sorted(KINDS))}")
        if node["mode"] not in MODES:
            raise CodeflowError(f"{node_id}.mode must be one of {', '.join(sorted(MODES))}")
        if not isinstance(node["objective"], str) or not node["objective"].strip():
            raise CodeflowError(f"{node_id}.objective must be a nonempty string")
        require_string_list(node["depends_on"], f"{node_id}.depends_on")
        paths = [normalize_rel_path(item, f"{node_id}.paths") for item in require_string_list(node["paths"], f"{node_id}.paths", nonempty=True)]
        for path in paths:
            resolve_artifact(Path(workspace), path, f"{node_id}.paths")
        node["paths"] = paths
        require_string_list(node["acceptance"], f"{node_id}.acceptance", nonempty=True)
        require_string_list(node["forbidden"], f"{node_id}.forbidden", nonempty=True)
        if not isinstance(node["fresh_context"], bool):
            raise CodeflowError(f"{node_id}.fresh_context must be boolean")
        if node["mode"] == "write":
            scopes = [normalize_rel_path(item, f"{node_id}.write_scope") for item in require_string_list(node.get("write_scope"), f"{node_id}.write_scope", nonempty=True)]
            if any(scope == "." or path_contains(".codeflow", scope) for scope in scopes):
                raise CodeflowError(f"{node_id}.write_scope cannot include the Codeflow ledger or entire workspace")
            for scope in scopes:
                resolve_artifact(Path(workspace), scope, f"{node_id}.write_scope")
            if any(not any(path_contains(path, scope) for path in paths) for scope in scopes):
                raise CodeflowError(f"{node_id}.write_scope must be contained by paths")
            node["write_scope"] = scopes
        elif "write_scope" in node:
            raise CodeflowError(f"{node_id}.write_scope is only valid for write mode")
        if "max_attempts" in node:
            attempts = node["max_attempts"]
            if not isinstance(attempts, int) or isinstance(attempts, bool) or not 1 <= attempts <= limits["max_attempts"]:
                raise CodeflowError(f"{node_id}.max_attempts must be between 1 and limits.max_attempts")
        if node["kind"] in {"review", "verify"} and node["mode"] != "verify":
            raise CodeflowError(f"{node_id} review/verify nodes must use verify mode")
        by_id[node_id] = node

    for node_id, node in by_id.items():
        for dep in node["depends_on"]:
            if dep not in by_id:
                raise CodeflowError(f"{node_id} depends on unknown node: {dep}")
            if dep == node_id:
                raise CodeflowError(f"{node_id} cannot depend on itself")

    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(node_id: str) -> None:
        if node_id in visiting:
            raise CodeflowError(f"workflow contains a cycle at {node_id}")
        if node_id in visited:
            return
        visiting.add(node_id)
        for dep in by_id[node_id]["depends_on"]:
            visit(dep)
        visiting.remove(node_id)
        visited.add(node_id)

    for node_id in by_id:
        visit(node_id)

    ancestors: dict[str, set[str]] = {}

    def node_ancestors(node_id: str) -> set[str]:
        if node_id not in ancestors:
            result: set[str] = set()
            for dep in by_id[node_id]["depends_on"]:
                result.add(dep)
                result.update(node_ancestors(dep))
            ancestors[node_id] = result
        return ancestors[node_id]

    for node_id in by_id:
        node_ancestors(node_id)

    writers = [node for node in nodes if node["mode"] == "write"]
    for index, left in enumerate(writers):
        for right in writers[index + 1 :]:
            ordered = left["id"] in ancestors[right["id"]] or right["id"] in ancestors[left["id"]]
            overlap = any(paths_overlap(a, b) for a in left["write_scope"] for b in right["write_scope"])
            if overlap and not ordered:
                raise CodeflowError(f"unordered write scopes overlap: {left['id']} and {right['id']}")

    reviews = [node for node in nodes if node["kind"] == "review" and node["mode"] == "verify"]
    if writers and not any(node["fresh_context"] for node in reviews):
        raise CodeflowError("write workflows need a fresh Sol review node")
    for writer in writers:
        if not any(writer["id"] in ancestors[review["id"]] for review in reviews):
            raise CodeflowError(f"write node {writer['id']} needs a downstream Sol review node")

    return workflow


def load_run(run_dir: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    state = read_json(run_dir / "run.json")
    if state.get("schema_version") != SCHEMA_VERSION:
        raise CodeflowError("unsupported run schema")
    if "workflow" in state:
        workflow = validate_workflow(state["workflow"])
    else:
        workflow = validate_workflow(read_json(run_dir / "workflow.json"))
        state["workflow"] = workflow
    if state.get("workflow_sha256") != digest_json(workflow):
        raise CodeflowError("run state contains a mismatched workflow")
    export = run_dir / "workflow.json"
    try:
        export_matches = digest_json(read_json(export)) == state["workflow_sha256"]
    except CodeflowError:
        export_matches = False
    if not export_matches:
        atomic_write_json(export, workflow)
    expected = {node["id"] for node in workflow["nodes"]}
    if set(state.get("nodes", {})) != expected:
        raise CodeflowError("run state does not match workflow nodes")
    state.setdefault("cycles", 0)
    for item in state["nodes"].values():
        item.setdefault("acceptance", [])
    return workflow, state


def add_event(state: dict[str, Any], event: str, node_id: str | None = None, detail: str | None = None) -> None:
    item = {"at": utc_now(), "event": event}
    if node_id is not None:
        item["node"] = node_id
    if detail:
        item["detail"] = detail
    state.setdefault("events", []).append(item)
    state["updated_at"] = item["at"]


def descendants(workflow: dict[str, Any], roots: set[str]) -> set[str]:
    result = set(roots)
    changed = True
    while changed:
        changed = False
        for node in workflow["nodes"]:
            if node["id"] not in result and any(dep in result for dep in node["depends_on"]):
                result.add(node["id"])
                changed = True
    return result


def refresh_ready(workflow: dict[str, Any], state: dict[str, Any]) -> None:
    if state["status"] != "active":
        return
    for node in workflow["nodes"]:
        item = state["nodes"][node["id"]]
        if item["status"] not in {"pending", "ready"}:
            continue
        deps_passed = all(state["nodes"][dep]["status"] == "passed" for dep in node["depends_on"])
        item["status"] = "ready" if deps_passed else "pending"
    if all(item["status"] == "passed" for item in state["nodes"].values()) and not uncovered_acceptance(workflow, state):
        state["status"] = "complete"
        add_event(state, "run-complete")


def covered_acceptance(workflow: dict[str, Any], state: dict[str, Any]) -> set[str]:
    covered: set[str] = set()
    for item in state["nodes"].values():
        if item["status"] == "passed":
            covered.update(item.get("acceptance", []))
    return covered


def uncovered_acceptance(workflow: dict[str, Any], state: dict[str, Any]) -> list[str]:
    covered = covered_acceptance(workflow, state)
    return [criterion for criterion in workflow["acceptance"] if criterion not in covered]


def node_by_id(workflow: dict[str, Any], node_id: str) -> dict[str, Any]:
    for node in workflow["nodes"]:
        if node["id"] == node_id:
            return node
    raise CodeflowError(f"unknown node: {node_id}")


def dependency_ancestors(workflow: dict[str, Any], node_id: str) -> set[str]:
    result: set[str] = set()
    pending = list(node_by_id(workflow, node_id)["depends_on"])
    while pending:
        dependency = pending.pop()
        if dependency in result:
            continue
        result.add(dependency)
        pending.extend(node_by_id(workflow, dependency)["depends_on"])
    return result


def effective_attempts(workflow: dict[str, Any], node: dict[str, Any]) -> int:
    return node.get("max_attempts", workflow["limits"]["max_attempts"])


def resolve_artifact(workspace: Path, value: str, field: str) -> tuple[str, Path]:
    rel = normalize_rel_path(value, field)
    resolved_workspace = workspace.resolve()
    resolved = (resolved_workspace / rel).resolve()
    if resolved != resolved_workspace and resolved_workspace not in resolved.parents:
        raise CodeflowError(f"{field} escapes workspace: {value}")
    return rel, resolved


def digest_path(path: Path) -> str:
    if not path.exists():
        raise CodeflowError(f"artifact does not exist: {path}")
    digest = hashlib.sha256()
    if path.is_file():
        digest.update(b"file\0")
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
        return digest.hexdigest()
    if not path.is_dir():
        raise CodeflowError(f"unsupported artifact type: {path}")
    digest.update(b"dir\0")
    children = sorted(path.rglob("*"))
    if any(child.is_symlink() for child in children):
        raise CodeflowError(f"artifact directory contains a symlink: {path}")
    for child in (item for item in children if item.is_file()):
        digest.update(str(child.relative_to(path)).encode("utf-8") + b"\0")
        with child.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    return digest.hexdigest()


def snapshot_workspace(workspace: Path) -> dict[str, list[int | str]]:
    snapshot: dict[str, list[int | str]] = {}
    root = workspace.resolve()
    for current, directories, files in os.walk(root, followlinks=False):
        directories[:] = [name for name in directories if name not in {".git", ".codeflow"}]
        current_path = Path(current)
        for name in files:
            path = current_path / name
            relative = path.relative_to(root).as_posix()
            stat = path.lstat()
            if path.is_symlink():
                snapshot[relative] = ["symlink", os.readlink(path), stat.st_mtime_ns]
            else:
                snapshot[relative] = ["file", stat.st_size, stat.st_mtime_ns, stat.st_mode]
    return snapshot


def changed_since(before: dict[str, Any], after: dict[str, Any]) -> set[str]:
    return {path for path in set(before) | set(after) if before.get(path) != after.get(path)}


def validate_report(
    report: dict[str, Any], node: dict[str, Any], workflow: dict[str, Any]
) -> tuple[dict[str, Any], dict[str, str]]:
    report.setdefault("acceptance", [])
    required = {"status", "summary", "acceptance", "evidence", "artifacts", "checks", "changed_paths", "findings", "risks", "next"}
    if set(report) != required:
        missing = required - set(report)
        unknown = set(report) - required
        parts = []
        if missing:
            parts.append(f"missing: {', '.join(sorted(missing))}")
        if unknown:
            parts.append(f"unknown: {', '.join(sorted(unknown))}")
        raise CodeflowError("invalid report fields (" + "; ".join(parts) + ")")
    if report["status"] not in TERMINAL:
        raise CodeflowError("report.status must be passed, failed, or blocked")
    if not isinstance(report["summary"], str) or not report["summary"].strip():
        raise CodeflowError("report.summary must be a nonempty string")
    evidence = require_string_list(report["evidence"], "report.evidence")
    if report["status"] == "passed" and not evidence:
        raise CodeflowError("passed report requires evidence")
    acceptance = require_string_list(report["acceptance"], "report.acceptance")
    unknown_acceptance = set(acceptance) - set(workflow["acceptance"])
    if unknown_acceptance:
        raise CodeflowError(f"report maps unknown acceptance criteria: {', '.join(sorted(unknown_acceptance))}")
    for field in ("findings", "risks", "next"):
        if not isinstance(report[field], list):
            raise CodeflowError(f"report.{field} must be a list")
    if not isinstance(report["checks"], list) or any(not isinstance(item, dict) for item in report["checks"]):
        raise CodeflowError("report.checks must be a list of objects")
    for check in report["checks"]:
        if set(check) != {"command", "status", "detail"}:
            raise CodeflowError("each report check needs command, status, and detail")
        if check["status"] not in {"passed", "failed", "not-run"}:
            raise CodeflowError("check status must be passed, failed, or not-run")
        if not isinstance(check["command"], str) or not check["command"].strip() or not isinstance(check["detail"], str):
            raise CodeflowError("check command must be nonempty and detail must be a string")
    if report["status"] == "passed" and any(check["status"] != "passed" for check in report["checks"]):
        raise CodeflowError("passed report cannot contain failed or not-run checks")
    if report["status"] == "passed" and node["mode"] == "verify" and not report["checks"]:
        raise CodeflowError("passed verify report requires at least one green check")

    artifacts = require_string_list(report["artifacts"], "report.artifacts")
    changed = require_string_list(report["changed_paths"], "report.changed_paths")
    normalized_artifacts: list[str] = []
    artifact_digests: dict[str, str] = {}
    if report["status"] == "passed":
        for value in artifacts:
            rel, resolved = resolve_artifact(Path(workflow["workspace"]), value, "report.artifacts")
            if not any(path_contains(scope, rel) for scope in node["paths"]):
                raise CodeflowError(f"artifact outside node paths: {rel}")
            normalized_artifacts.append(rel)
            artifact_digests[rel] = digest_path(resolved)
    else:
        normalized_artifacts = [normalize_rel_path(value, "report.artifacts") for value in artifacts]

    normalized_changed = [normalize_rel_path(value, "report.changed_paths") for value in changed]
    if node["mode"] != "write" and normalized_changed:
        raise CodeflowError("read and verify nodes cannot report changed paths")
    if node["mode"] == "write":
        for rel in normalized_changed:
            resolve_artifact(Path(workflow["workspace"]), rel, "report.changed_paths")
            if not any(path_contains(scope, rel) for scope in node["write_scope"]):
                raise CodeflowError(f"changed path outside write scope: {rel}")
    report["artifacts"] = normalized_artifacts
    report["changed_paths"] = normalized_changed
    return report, artifact_digests


def save_run(run_dir: Path, state: dict[str, Any]) -> None:
    atomic_write_json(run_dir / "run.json", state)


def cmd_validate(args: argparse.Namespace) -> dict[str, Any]:
    workflow = validate_workflow(read_json(Path(args.workflow)))
    return {"valid": True, "nodes": len(workflow["nodes"]), "workflow_sha256": digest_json(workflow)}


def cmd_init(args: argparse.Namespace) -> dict[str, Any]:
    workflow = validate_workflow(read_json(Path(args.workflow)))
    run_id = args.run_id or datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:6]
    if not RUN_ID.fullmatch(run_id):
        raise CodeflowError("run id must use lowercase letters, digits, underscores, or hyphens")
    run_dir = Path(args.root).resolve() / run_id
    if run_dir.exists():
        raise CodeflowError(f"run already exists: {run_dir}")
    run_dir.mkdir(parents=True)
    (run_dir / "results").mkdir()
    (run_dir / "incoming").mkdir()
    (run_dir / "snapshots").mkdir()
    normalized_workflow = json.loads(canonical_bytes(workflow).decode("utf-8"))
    atomic_write_json(run_dir / "workflow.json", normalized_workflow)
    now = utc_now()
    state = {
        "schema_version": SCHEMA_VERSION,
        "run_id": run_id,
        "status": "active",
        "created_at": now,
        "updated_at": now,
        "workflow_sha256": digest_json(normalized_workflow),
        "workflow": normalized_workflow,
        "cycles": 0,
        "nodes": {
            node["id"]: {
                "status": "pending",
                "attempts": 0,
                "worker": None,
                "started_at": None,
                "finished_at": None,
                "report": None,
                "report_sha256": None,
                "artifact_digests": {},
                "acceptance": [],
                "failure_digests": [],
                "snapshot": None,
            }
            for node in normalized_workflow["nodes"]
        },
        "events": [{"at": now, "event": "run-created"}],
    }
    refresh_ready(normalized_workflow, state)
    save_run(run_dir, state)
    return {"run_dir": str(run_dir), "run_id": run_id, "status": state["status"]}


@locked_command
def cmd_ready(args: argparse.Namespace) -> dict[str, Any]:
    run_dir = Path(args.run_dir)
    workflow, state = load_run(run_dir)
    refresh_ready(workflow, state)
    save_run(run_dir, state)
    ready = [node for node in workflow["nodes"] if state["nodes"][node["id"]]["status"] == "ready"]
    limit = workflow["limits"]["max_parallel"]
    return {"run_id": state["run_id"], "ready": [node["id"] for node in ready], "dispatch": ready[:limit]}


@locked_command
def cmd_start(args: argparse.Namespace) -> dict[str, Any]:
    run_dir = Path(args.run_dir)
    workflow, state = load_run(run_dir)
    if state["status"] != "active":
        raise CodeflowError(f"run is not active: {state['status']}")
    node = node_by_id(workflow, args.node_id)
    item = state["nodes"][args.node_id]
    refresh_ready(workflow, state)
    if item["status"] == "running" and item["worker"] == args.worker:
        return {"node": args.node_id, "status": "running", "attempt": item["attempts"], "idempotent": True}
    if item["status"] != "ready":
        raise CodeflowError(f"node {args.node_id} is not ready: {item['status']}")
    if item["attempts"] >= effective_attempts(workflow, node):
        raise CodeflowError(f"node {args.node_id} exhausted its attempt budget")
    if node["kind"] == "review":
        if args.worker != "sol-review":
            raise CodeflowError("review nodes must be assigned to the single sol-review worker")
        upstream_writers = [
            dependency
            for dependency in dependency_ancestors(workflow, args.node_id)
            if node_by_id(workflow, dependency)["mode"] == "write"
        ]
        if any(state["nodes"][writer]["worker"] == args.worker for writer in upstream_writers):
            raise CodeflowError("Sol review must use a different worker than implementation")
    if node["mode"] == "write":
        snapshot_rel = f"snapshots/{args.node_id}-attempt-{item['attempts'] + 1}.json"
        atomic_write_json(run_dir / snapshot_rel, snapshot_workspace(Path(workflow["workspace"])))
        item["snapshot"] = snapshot_rel
    item["status"] = "running"
    item["attempts"] += 1
    item["worker"] = args.worker
    item["started_at"] = utc_now()
    item["finished_at"] = None
    add_event(state, "node-started", args.node_id, f"attempt {item['attempts']} by {args.worker}")
    save_run(run_dir, state)
    return {"node": args.node_id, "status": "running", "attempt": item["attempts"]}


@locked_command
def cmd_finish(args: argparse.Namespace) -> dict[str, Any]:
    run_dir = Path(args.run_dir)
    workflow, state = load_run(run_dir)
    node = node_by_id(workflow, args.node_id)
    item = state["nodes"][args.node_id]
    raw_report = read_json(Path(args.report))
    report, artifact_digests = validate_report(raw_report, node, workflow)
    if node["mode"] == "write":
        if not item.get("snapshot"):
            raise CodeflowError(f"write node {args.node_id} has no start snapshot")
        before = read_json(run_dir / item["snapshot"])
        after = snapshot_workspace(Path(workflow["workspace"]))
        actual_changes = changed_since(before, after)
        outside_scope = sorted(
            path
            for path in actual_changes
            if not any(path_contains(scope, path) for scope in node["write_scope"])
        )
        if outside_scope:
            raise CodeflowError(f"actual changes outside write scope: {', '.join(outside_scope)}")
        unreported = sorted(
            path
            for path in actual_changes
            if not any(path_contains(reported, path) for reported in report["changed_paths"])
        )
        if unreported:
            raise CodeflowError(f"actual changes missing from report: {', '.join(unreported)}")
        unsupported = sorted(
            reported
            for reported in report["changed_paths"]
            if not any(path_contains(reported, path) for path in actual_changes)
        )
        if unsupported:
            raise CodeflowError(f"reported paths have no observed change: {', '.join(unsupported)}")
        if report["status"] == "passed" and not actual_changes:
            raise CodeflowError("passed write report requires an observed workspace change")
    report_sha = digest_json(report)
    if item["status"] in TERMINAL and item["report_sha256"] == report_sha:
        return {"node": args.node_id, "status": item["status"], "idempotent": True}
    if item["status"] != "running":
        raise CodeflowError(f"node {args.node_id} is not running: {item['status']}")
    result_rel = f"results/{args.node_id}-attempt-{item['attempts']}.json"
    result_path = run_dir / result_rel
    if result_path.exists():
        if digest_json(read_json(result_path)) != report_sha:
            raise CodeflowError(f"result already exists with different content: {result_path}")
    else:
        atomic_write_json(result_path, report)
    item["status"] = report["status"]
    item["finished_at"] = utc_now()
    item["report"] = result_rel
    item["report_sha256"] = report_sha
    item["artifact_digests"] = artifact_digests
    item["acceptance"] = report["acceptance"]
    if report["status"] == "failed":
        item["failure_digests"].append(report_sha)
    add_event(state, f"node-{report['status']}", args.node_id, report["summary"])
    refresh_ready(workflow, state)
    save_run(run_dir, state)
    return {"node": args.node_id, "status": item["status"], "run_status": state["status"]}


@locked_command
def cmd_retry(args: argparse.Namespace) -> dict[str, Any]:
    run_dir = Path(args.run_dir)
    workflow, state = load_run(run_dir)
    node = node_by_id(workflow, args.node_id)
    item = state["nodes"][args.node_id]
    allowed = {"failed"} | ({"blocked"} if args.allow_blocked else set())
    if item["status"] not in allowed:
        raise CodeflowError(f"node {args.node_id} cannot be retried from {item['status']}")
    if item["attempts"] >= effective_attempts(workflow, node):
        raise CodeflowError(f"node {args.node_id} exhausted its attempt budget")
    failures = item.get("failure_digests", [])
    if len(failures) >= 2 and failures[-1] == failures[-2]:
        raise CodeflowError(f"node {args.node_id} repeated the same failure without progress")
    if state["cycles"] >= workflow["limits"]["max_cycles"]:
        raise CodeflowError("run exhausted its repair-cycle budget")
    state["cycles"] += 1
    item["status"] = "pending"
    item["worker"] = None
    item["started_at"] = None
    item["finished_at"] = None
    item["report"] = None
    item["report_sha256"] = None
    item["artifact_digests"] = {}
    item["acceptance"] = []
    item["snapshot"] = None
    state["status"] = "active"
    add_event(state, "node-retried", args.node_id)
    refresh_ready(workflow, state)
    save_run(run_dir, state)
    return {"node": args.node_id, "status": item["status"], "attempts": item["attempts"]}


@locked_command
def cmd_resume(args: argparse.Namespace) -> dict[str, Any]:
    run_dir = Path(args.run_dir)
    workflow, state = load_run(run_dir)
    now = datetime.now(timezone.utc)
    invalid: set[str] = set()
    interrupted = False
    for node_id, item in state["nodes"].items():
        if item["status"] == "running":
            started = datetime.fromisoformat(item["started_at"])
            if (now - started).total_seconds() >= args.stale_after:
                item["status"] = "pending"
                item["worker"] = None
                item["started_at"] = None
                interrupted = True
                add_event(state, "node-interrupted", node_id, "stale running node returned to queue")
        if item["status"] == "passed":
            try:
                report = read_json(run_dir / item["report"])
                report_valid = digest_json(report) == item["report_sha256"]
            except (CodeflowError, TypeError):
                report_valid = False
            if not report_valid:
                invalid.add(node_id)
                continue
            for rel, expected in item.get("artifact_digests", {}).items():
                try:
                    _, path = resolve_artifact(Path(workflow["workspace"]), rel, "stored artifact")
                    current = digest_path(path)
                except CodeflowError:
                    current = None
                if current != expected:
                    invalid.add(node_id)
                    break
    if invalid:
        invalid = descendants(workflow, invalid)
        for node_id in invalid:
            item = state["nodes"][node_id]
            item["status"] = "pending"
            item["worker"] = None
            item["started_at"] = None
            item["finished_at"] = None
            item["report"] = None
            item["report_sha256"] = None
            item["artifact_digests"] = {}
            item["acceptance"] = []
            item["attempts"] = 0
            item["snapshot"] = None
            add_event(state, "node-invalidated", node_id, "artifact or upstream evidence changed")
    if state["status"] != "cancelled" and (invalid or interrupted or state["status"] != "complete"):
        state["status"] = "active"
    refresh_ready(workflow, state)
    save_run(run_dir, state)
    return {"run_id": state["run_id"], "status": state["status"], "invalidated": sorted(invalid)}


@locked_command
def cmd_sync(args: argparse.Namespace) -> dict[str, Any]:
    run_dir = Path(args.run_dir)
    old_workflow, state = load_run(run_dir)
    new_workflow = validate_workflow(read_json(Path(args.workflow)))
    if new_workflow["workspace"] != old_workflow["workspace"]:
        raise CodeflowError("sync cannot change workspace")
    for field in ("goal", "acceptance", "limits"):
        if new_workflow[field] != old_workflow[field]:
            raise CodeflowError(f"sync cannot change {field}")
    old_nodes = {node["id"]: node for node in old_workflow["nodes"]}
    new_nodes = {node["id"]: node for node in new_workflow["nodes"]}
    removed = set(old_nodes) - set(new_nodes)
    if removed:
        raise CodeflowError(f"sync cannot remove nodes: {', '.join(sorted(removed))}")
    for node_id, old_node in old_nodes.items():
        status = state["nodes"][node_id]["status"]
        if status in {"running", "passed", "failed", "blocked"} and old_node != new_nodes[node_id]:
            raise CodeflowError(f"sync cannot change {status} node: {node_id}")
    normalized = json.loads(canonical_bytes(new_workflow).decode("utf-8"))
    for node in normalized["nodes"]:
        if node["id"] not in state["nodes"]:
            state["nodes"][node["id"]] = {
                "status": "pending",
                "attempts": 0,
                "worker": None,
                "started_at": None,
                "finished_at": None,
                "report": None,
                "report_sha256": None,
                "artifact_digests": {},
                "acceptance": [],
                "failure_digests": [],
                "snapshot": None,
            }
        elif state["nodes"][node["id"]]["status"] == "ready":
            state["nodes"][node["id"]]["status"] = "pending"
    state["workflow"] = normalized
    state["workflow_sha256"] = digest_json(normalized)
    state["status"] = "active"
    add_event(state, "workflow-synced", detail=f"{len(new_nodes) - len(old_nodes)} nodes added")
    refresh_ready(normalized, state)
    save_run(run_dir, state)
    atomic_write_json(run_dir / "workflow.json", normalized)
    return {"run_id": state["run_id"], "nodes": len(new_nodes), "status": state["status"]}


@locked_command
def cmd_cancel(args: argparse.Namespace) -> dict[str, Any]:
    run_dir = Path(args.run_dir)
    _, state = load_run(run_dir)
    if state["status"] == "complete":
        raise CodeflowError("completed run cannot be cancelled")
    for node_id, item in state["nodes"].items():
        if item["status"] == "running":
            item["status"] = "blocked"
            item["finished_at"] = utc_now()
            add_event(state, "node-blocked", node_id, "run cancelled")
    state["status"] = "cancelled"
    add_event(state, "run-cancelled", detail=args.reason)
    save_run(run_dir, state)
    return {"run_id": state["run_id"], "status": "cancelled"}


def status_payload(workflow: dict[str, Any], state: dict[str, Any]) -> dict[str, Any]:
    counts: dict[str, int] = {}
    for item in state["nodes"].values():
        counts[item["status"]] = counts.get(item["status"], 0) + 1
    return {
        "run_id": state["run_id"],
        "goal": workflow["goal"],
        "status": state["status"],
        "updated_at": state["updated_at"],
        "cycles": state["cycles"],
        "counts": counts,
        "uncovered_acceptance": uncovered_acceptance(workflow, state),
        "ready": sorted(node_id for node_id, item in state["nodes"].items() if item["status"] == "ready"),
        "failed": sorted(node_id for node_id, item in state["nodes"].items() if item["status"] == "failed"),
        "blocked": sorted(node_id for node_id, item in state["nodes"].items() if item["status"] == "blocked"),
    }


@locked_command
def cmd_status(args: argparse.Namespace) -> dict[str, Any]:
    run_dir = Path(args.run_dir)
    workflow, state = load_run(run_dir)
    refresh_ready(workflow, state)
    save_run(run_dir, state)
    return status_payload(workflow, state)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    validate = sub.add_parser("validate", help="validate a workflow JSON document")
    validate.add_argument("workflow")
    validate.set_defaults(func=cmd_validate)
    init = sub.add_parser("init", help="create a new run")
    init.add_argument("workflow")
    init.add_argument("--root", default=".codeflow/runs")
    init.add_argument("--run-id")
    init.set_defaults(func=cmd_init)
    ready = sub.add_parser("ready", help="return currently dispatchable nodes")
    ready.add_argument("run_dir")
    ready.set_defaults(func=cmd_ready)
    start = sub.add_parser("start", help="mark a ready node running")
    start.add_argument("run_dir")
    start.add_argument("node_id")
    start.add_argument("--worker", required=True)
    start.set_defaults(func=cmd_start)
    finish = sub.add_parser("finish", help="record a structured worker report")
    finish.add_argument("run_dir")
    finish.add_argument("node_id")
    finish.add_argument("report")
    finish.set_defaults(func=cmd_finish)
    retry = sub.add_parser("retry", help="return a failed node to the queue")
    retry.add_argument("run_dir")
    retry.add_argument("node_id")
    retry.add_argument("--allow-blocked", action="store_true")
    retry.set_defaults(func=cmd_retry)
    resume = sub.add_parser("resume", help="recover stale work and revalidate artifacts")
    resume.add_argument("run_dir")
    resume.add_argument("--stale-after", type=int, default=1800)
    resume.set_defaults(func=cmd_resume)
    sync = sub.add_parser("sync", help="adapt a run with a validated workflow")
    sync.add_argument("run_dir")
    sync.add_argument("workflow")
    sync.set_defaults(func=cmd_sync)
    cancel = sub.add_parser("cancel", help="cancel a run without deleting evidence")
    cancel.add_argument("run_dir")
    cancel.add_argument("--reason", default="cancelled by user")
    cancel.set_defaults(func=cmd_cancel)
    status = sub.add_parser("status", help="summarize a run")
    status.add_argument("run_dir")
    status.set_defaults(func=cmd_status)
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    try:
        result = args.func(args)
    except CodeflowError as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, sort_keys=True), file=sys.stderr)
        return 2
    print(json.dumps({"ok": True, **result}, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
