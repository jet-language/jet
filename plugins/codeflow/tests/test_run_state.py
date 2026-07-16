import argparse
import copy
import importlib.util
import json
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "skills" / "codeflow" / "scripts" / "run_state.py"
SPEC = importlib.util.spec_from_file_location("codeflow_run_state", SCRIPT)
run_state = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(run_state)


class CodeflowStateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.workspace = self.root / "workspace"
        self.workspace.mkdir()
        (self.workspace / "src").mkdir()
        (self.workspace / "tests").mkdir()
        self.workflow = {
            "schema_version": 1,
            "goal": "Implement and independently verify a feature",
            "workspace": str(self.workspace),
            "limits": {"max_parallel": 3, "max_attempts": 3, "max_cycles": 2},
            "acceptance": ["Implementation exists", "Independent review passes"],
            "nodes": [
                {
                    "id": "inspect",
                    "kind": "investigate",
                    "mode": "read",
                    "objective": "Inspect the feature owner",
                    "depends_on": [],
                    "paths": ["src", "tests"],
                    "acceptance": ["Return source evidence"],
                    "forbidden": ["edit files", "spawn subagents"],
                    "fresh_context": False,
                },
                {
                    "id": "implement",
                    "kind": "implement",
                    "mode": "write",
                    "objective": "Implement the feature",
                    "depends_on": ["inspect"],
                    "paths": ["src/feature.txt", "tests/test_feature.txt"],
                    "write_scope": ["src/feature.txt", "tests/test_feature.txt"],
                    "acceptance": ["Targeted proof passes"],
                    "forbidden": ["reset unrelated changes", "spawn subagents"],
                    "fresh_context": False,
                },
                {
                    "id": "review",
                    "kind": "review",
                    "mode": "verify",
                    "objective": "Try to disprove the implementation",
                    "depends_on": ["implement"],
                    "paths": ["src/feature.txt", "tests/test_feature.txt"],
                    "acceptance": ["Rerun proof and report findings"],
                    "forbidden": ["edit files", "use implementer reasoning", "spawn subagents"],
                    "fresh_context": True,
                },
            ],
        }

    def tearDown(self):
        self.temp.cleanup()

    def write_json(self, name, value):
        path = self.root / name
        path.write_text(json.dumps(value), encoding="utf-8")
        return path

    def init_run(self, workflow=None, run_id="test-run"):
        workflow_path = self.write_json("workflow-input.json", workflow or self.workflow)
        result = run_state.cmd_init(
            argparse.Namespace(workflow=str(workflow_path), root=str(self.root / "runs"), run_id=run_id)
        )
        return Path(result["run_dir"])

    def report(self, status="passed", *, artifacts=None, changed=None, summary="proof recorded"):
        return {
            "status": status,
            "summary": summary,
            "evidence": ["independent command passed"] if status == "passed" else ["failure reproduced"],
            "artifacts": artifacts or [],
            "checks": [{"command": "test command", "status": "passed" if status == "passed" else "failed", "detail": "observed"}],
            "changed_paths": changed or [],
            "findings": [],
            "risks": [],
            "next": [],
        }

    def start(self, run_dir, node, worker="worker"):
        return run_state.cmd_start(argparse.Namespace(run_dir=str(run_dir), node_id=node, worker=worker))

    def finish(self, run_dir, node, report, name=None):
        path = self.write_json(name or f"{node}-report.json", report)
        return run_state.cmd_finish(argparse.Namespace(run_dir=str(run_dir), node_id=node, report=str(path)))

    def pass_inspect(self, run_dir):
        self.start(run_dir, "inspect", "explorer")
        self.finish(run_dir, "inspect", self.report())

    def pass_implementation(self, run_dir):
        (self.workspace / "src" / "feature.txt").write_text("feature\n", encoding="utf-8")
        (self.workspace / "tests" / "test_feature.txt").write_text("proof\n", encoding="utf-8")
        self.start(run_dir, "implement", "writer")
        self.finish(
            run_dir,
            "implement",
            self.report(
                artifacts=["src/feature.txt", "tests/test_feature.txt"],
                changed=["src/feature.txt", "tests/test_feature.txt"],
            ),
        )

    def test_valid_workflow_and_full_lifecycle(self):
        validated = run_state.validate_workflow(copy.deepcopy(self.workflow))
        self.assertEqual(3, len(validated["nodes"]))
        run_dir = self.init_run()
        ready = run_state.cmd_ready(argparse.Namespace(run_dir=str(run_dir)))
        self.assertEqual(["inspect"], ready["ready"])
        self.pass_inspect(run_dir)
        self.assertEqual(["implement"], run_state.cmd_ready(argparse.Namespace(run_dir=str(run_dir)))["ready"])
        self.pass_implementation(run_dir)
        self.assertEqual(["review"], run_state.cmd_ready(argparse.Namespace(run_dir=str(run_dir)))["ready"])
        self.start(run_dir, "review", "fresh-reviewer")
        result = self.finish(run_dir, "review", self.report())
        self.assertEqual("complete", result["run_status"])
        status = run_state.cmd_status(argparse.Namespace(run_dir=str(run_dir)))
        self.assertEqual("complete", status["status"])
        self.assertEqual({"passed": 3}, status["counts"])

    def test_cycle_missing_dependency_and_overlap_are_rejected(self):
        cycle = copy.deepcopy(self.workflow)
        cycle["nodes"][0]["depends_on"] = ["review"]
        with self.assertRaisesRegex(run_state.CodeflowError, "cycle"):
            run_state.validate_workflow(cycle)

        missing = copy.deepcopy(self.workflow)
        missing["nodes"][0]["depends_on"] = ["absent"]
        with self.assertRaisesRegex(run_state.CodeflowError, "unknown node"):
            run_state.validate_workflow(missing)

        overlap = copy.deepcopy(self.workflow)
        second = copy.deepcopy(overlap["nodes"][1])
        second["id"] = "implement-two"
        second["depends_on"] = ["inspect"]
        overlap["nodes"].insert(2, second)
        overlap["nodes"][-1]["depends_on"] = ["implement", "implement-two"]
        with self.assertRaisesRegex(run_state.CodeflowError, "write scopes overlap"):
            run_state.validate_workflow(overlap)

    def test_write_requires_fresh_downstream_review(self):
        workflow = copy.deepcopy(self.workflow)
        workflow["nodes"][-1]["fresh_context"] = False
        with self.assertRaisesRegex(run_state.CodeflowError, "downstream fresh"):
            run_state.validate_workflow(workflow)

    def test_finish_is_idempotent_for_identical_report(self):
        run_dir = self.init_run()
        self.start(run_dir, "inspect")
        report = self.report()
        first = self.finish(run_dir, "inspect", report)
        second = self.finish(run_dir, "inspect", report)
        self.assertEqual("passed", first["status"])
        self.assertTrue(second["idempotent"])

    def test_resume_invalidates_changed_artifact_and_downstream(self):
        run_dir = self.init_run()
        self.pass_inspect(run_dir)
        self.pass_implementation(run_dir)
        self.start(run_dir, "review", "reviewer")
        self.finish(run_dir, "review", self.report())
        (self.workspace / "src" / "feature.txt").write_text("changed\n", encoding="utf-8")
        resumed = run_state.cmd_resume(argparse.Namespace(run_dir=str(run_dir), stale_after=0))
        self.assertEqual(["implement", "review"], resumed["invalidated"])
        ready = run_state.cmd_ready(argparse.Namespace(run_dir=str(run_dir)))
        self.assertEqual(["implement"], ready["ready"])

    def test_resume_recovers_stale_running_node(self):
        run_dir = self.init_run()
        self.start(run_dir, "inspect")
        state_path = run_dir / "run.json"
        state = json.loads(state_path.read_text(encoding="utf-8"))
        state["nodes"]["inspect"]["started_at"] = (datetime.now(timezone.utc) - timedelta(hours=2)).isoformat()
        run_state.atomic_write_json(state_path, state)
        run_state.cmd_resume(argparse.Namespace(run_dir=str(run_dir), stale_after=60))
        ready = run_state.cmd_ready(argparse.Namespace(run_dir=str(run_dir)))
        self.assertEqual(["inspect"], ready["ready"])

    def test_retry_budget_and_no_progress_guard(self):
        run_dir = self.init_run()
        failure = self.report(status="failed", summary="same failure")
        self.start(run_dir, "inspect")
        self.finish(run_dir, "inspect", failure, "failure-one.json")
        run_state.cmd_retry(
            argparse.Namespace(run_dir=str(run_dir), node_id="inspect", allow_blocked=False, force_no_progress=False)
        )
        self.start(run_dir, "inspect")
        self.finish(run_dir, "inspect", failure, "failure-two.json")
        with self.assertRaisesRegex(run_state.CodeflowError, "same failure"):
            run_state.cmd_retry(
                argparse.Namespace(run_dir=str(run_dir), node_id="inspect", allow_blocked=False, force_no_progress=False)
            )

    def test_report_path_traversal_and_scope_escape_are_rejected(self):
        run_dir = self.init_run()
        self.pass_inspect(run_dir)
        self.start(run_dir, "implement")
        traversal = self.report(artifacts=["../secret"], changed=["src/feature.txt"])
        with self.assertRaisesRegex(run_state.CodeflowError, "inside the workspace"):
            self.finish(run_dir, "implement", traversal, "traversal.json")
        outside_scope = self.report(changed=["src/other.txt"])
        with self.assertRaisesRegex(run_state.CodeflowError, "outside write scope"):
            self.finish(run_dir, "implement", outside_scope, "scope.json")

    def test_sync_adds_nodes_but_cannot_change_completed_contract(self):
        run_dir = self.init_run()
        self.pass_inspect(run_dir)
        adapted = copy.deepcopy(self.workflow)
        adapted["nodes"].insert(
            1,
            {
                "id": "inspect-tests",
                "kind": "investigate",
                "mode": "read",
                "objective": "Inspect test conventions",
                "depends_on": ["inspect"],
                "paths": ["tests"],
                "acceptance": ["Return test evidence"],
                "forbidden": ["edit files", "spawn subagents"],
                "fresh_context": True,
            },
        )
        adapted["nodes"][2]["depends_on"] = ["inspect", "inspect-tests"]
        adapted_path = self.write_json("adapted.json", adapted)
        result = run_state.cmd_sync(argparse.Namespace(run_dir=str(run_dir), workflow=str(adapted_path)))
        self.assertEqual(4, result["nodes"])
        self.assertEqual(["inspect-tests"], run_state.cmd_ready(argparse.Namespace(run_dir=str(run_dir)))["ready"])

        changed = copy.deepcopy(adapted)
        changed["nodes"][0]["objective"] = "Rewrite completed work"
        changed_path = self.write_json("changed.json", changed)
        with self.assertRaisesRegex(run_state.CodeflowError, "cannot change passed"):
            run_state.cmd_sync(argparse.Namespace(run_dir=str(run_dir), workflow=str(changed_path)))

        changed_goal = copy.deepcopy(adapted)
        changed_goal["goal"] = "Silent scope expansion"
        changed_goal_path = self.write_json("changed-goal.json", changed_goal)
        with self.assertRaisesRegex(run_state.CodeflowError, "cannot change goal"):
            run_state.cmd_sync(argparse.Namespace(run_dir=str(run_dir), workflow=str(changed_goal_path)))

    def test_cancel_preserves_run_and_marks_running_node(self):
        run_dir = self.init_run()
        self.start(run_dir, "inspect")
        result = run_state.cmd_cancel(argparse.Namespace(run_dir=str(run_dir), reason="user stopped"))
        self.assertEqual("cancelled", result["status"])
        status = run_state.cmd_status(argparse.Namespace(run_dir=str(run_dir)))
        self.assertEqual("cancelled", status["status"])
        self.assertEqual(["inspect"], status["blocked"])


if __name__ == "__main__":
    unittest.main()
