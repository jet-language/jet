import argparse
import copy
import importlib.util
import json
import subprocess
import sys
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

    def report(self, status="passed", *, acceptance=None, artifacts=None, changed=None, summary="proof recorded"):
        return {
            "status": status,
            "summary": summary,
            "acceptance": acceptance or [],
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
        self.start(run_dir, "implement", "writer")
        (self.workspace / "src" / "feature.txt").write_text("feature\n", encoding="utf-8")
        (self.workspace / "tests" / "test_feature.txt").write_text("proof\n", encoding="utf-8")
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
        self.start(run_dir, "review", "sol-review")
        result = self.finish(run_dir, "review", self.report(acceptance=self.workflow["acceptance"]))
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
        with self.assertRaisesRegex(run_state.CodeflowError, "write nodes must be ordered"):
            run_state.validate_workflow(overlap)

    def test_write_requires_fresh_downstream_review(self):
        workflow = copy.deepcopy(self.workflow)
        workflow["nodes"][-1]["fresh_context"] = False
        with self.assertRaisesRegex(run_state.CodeflowError, "fresh Sol review"):
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
        self.start(run_dir, "review", "sol-review")
        self.finish(run_dir, "review", self.report(acceptance=self.workflow["acceptance"]))
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
            argparse.Namespace(run_dir=str(run_dir), node_id="inspect", allow_blocked=False)
        )
        self.start(run_dir, "inspect")
        self.finish(run_dir, "inspect", failure, "failure-two.json")
        with self.assertRaisesRegex(run_state.CodeflowError, "same failure"):
            run_state.cmd_retry(
                argparse.Namespace(run_dir=str(run_dir), node_id="inspect", allow_blocked=False)
            )

    def test_repair_cycle_budget_is_enforced(self):
        workflow = copy.deepcopy(self.workflow)
        workflow["limits"]["max_cycles"] = 1
        run_dir = self.init_run(workflow, "cycle-budget")
        self.start(run_dir, "inspect")
        self.finish(run_dir, "inspect", self.report(status="failed", summary="first failure"), "cycle-one.json")
        run_state.cmd_retry(argparse.Namespace(run_dir=str(run_dir), node_id="inspect", allow_blocked=False))
        self.start(run_dir, "inspect")
        second = self.report(status="failed", summary="different failure")
        second["evidence"] = ["different evidence"]
        self.finish(run_dir, "inspect", second, "cycle-two.json")
        with self.assertRaisesRegex(run_state.CodeflowError, "repair-cycle budget"):
            run_state.cmd_retry(argparse.Namespace(run_dir=str(run_dir), node_id="inspect", allow_blocked=False))

    def test_passed_report_rejects_bad_checks_and_unknown_acceptance(self):
        run_dir = self.init_run()
        self.start(run_dir, "inspect")
        bad_check = self.report()
        bad_check["checks"][0]["status"] = "failed"
        with self.assertRaisesRegex(run_state.CodeflowError, "cannot contain failed"):
            self.finish(run_dir, "inspect", bad_check, "bad-check.json")
        unknown = self.report(acceptance=["invented criterion"])
        with self.assertRaisesRegex(run_state.CodeflowError, "unknown acceptance"):
            self.finish(run_dir, "inspect", unknown, "bad-acceptance.json")

    def test_completion_requires_acceptance_coverage(self):
        run_dir = self.init_run()
        self.pass_inspect(run_dir)
        self.pass_implementation(run_dir)
        self.start(run_dir, "review", "sol-review")
        result = self.finish(run_dir, "review", self.report())
        self.assertEqual("active", result["run_status"])
        status = run_state.cmd_status(argparse.Namespace(run_dir=str(run_dir)))
        self.assertEqual(self.workflow["acceptance"], status["uncovered_acceptance"])

    def test_cli_transactions_preserve_concurrent_finishes(self):
        workflow = copy.deepcopy(self.workflow)
        workflow["nodes"] = [
            {
                "id": "left",
                "kind": "investigate",
                "mode": "read",
                "objective": "Inspect left",
                "depends_on": [],
                "paths": ["src"],
                "acceptance": ["Return left evidence"],
                "forbidden": ["edit files", "spawn subagents"],
                "fresh_context": True,
            },
            {
                "id": "right",
                "kind": "investigate",
                "mode": "read",
                "objective": "Inspect right",
                "depends_on": [],
                "paths": ["tests"],
                "acceptance": ["Return right evidence"],
                "forbidden": ["edit files", "spawn subagents"],
                "fresh_context": True,
            },
        ]
        workflow["acceptance"] = ["left covered", "right covered"]
        run_dir = self.init_run(workflow, "parallel-run")
        self.start(run_dir, "left", "left-worker")
        self.start(run_dir, "right", "right-worker")
        left = self.write_json("left.json", self.report(acceptance=["left covered"]))
        right = self.write_json("right.json", self.report(acceptance=["right covered"]))
        commands = [
            [sys.executable, str(SCRIPT), "finish", str(run_dir), "left", str(left)],
            [sys.executable, str(SCRIPT), "finish", str(run_dir), "right", str(right)],
        ]
        processes = [subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) for command in commands]
        results = [process.communicate(timeout=10) for process in processes]
        self.assertEqual([0, 0], [process.returncode for process in processes], results)
        status = run_state.cmd_status(argparse.Namespace(run_dir=str(run_dir)))
        self.assertEqual("complete", status["status"])
        self.assertEqual({"passed": 2}, status["counts"])

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

    def test_sol_is_single_and_distinct_from_writer(self):
        run_dir = self.init_run()
        self.pass_inspect(run_dir)
        self.pass_implementation(run_dir)
        state_path = run_dir / "run.json"
        state = json.loads(state_path.read_text(encoding="utf-8"))
        state["reviewer_worker"] = "native-sol-thread"
        run_state.atomic_write_json(state_path, state)
        with self.assertRaisesRegex(run_state.CodeflowError, "single Sol agent thread"):
            self.start(run_dir, "review", "second-reviewer-thread")

        state = json.loads(state_path.read_text(encoding="utf-8"))
        state["nodes"]["implement"]["worker"] = "native-sol-thread"
        run_state.atomic_write_json(state_path, state)
        with self.assertRaisesRegex(run_state.CodeflowError, "different worker"):
            self.start(run_dir, "review", "native-sol-thread")

    def test_resume_checks_report_integrity_and_noop_is_stable(self):
        run_dir = self.init_run()
        self.pass_inspect(run_dir)
        self.pass_implementation(run_dir)
        self.start(run_dir, "review", "sol-review")
        self.finish(run_dir, "review", self.report(acceptance=self.workflow["acceptance"]))
        state_before = json.loads((run_dir / "run.json").read_text(encoding="utf-8"))
        completions_before = sum(event["event"] == "run-complete" for event in state_before["events"])
        resumed = run_state.cmd_resume(argparse.Namespace(run_dir=str(run_dir), stale_after=0))
        state_after = json.loads((run_dir / "run.json").read_text(encoding="utf-8"))
        completions_after = sum(event["event"] == "run-complete" for event in state_after["events"])
        self.assertEqual([], resumed["invalidated"])
        self.assertEqual(completions_before, completions_after)

        report_path = run_dir / state_after["nodes"]["review"]["report"]
        tampered = json.loads(report_path.read_text(encoding="utf-8"))
        tampered["summary"] = "tampered after completion"
        report_path.write_text(json.dumps(tampered), encoding="utf-8")
        resumed = run_state.cmd_resume(argparse.Namespace(run_dir=str(run_dir), stale_after=0))
        self.assertEqual(["review"], resumed["invalidated"])
        self.assertEqual(["review"], run_state.cmd_ready(argparse.Namespace(run_dir=str(run_dir)))["ready"])
        started = self.start(run_dir, "review", "sol-review")
        self.assertEqual(2, started["execution"])
        result = self.finish(run_dir, "review", self.report(acceptance=self.workflow["acceptance"]), "review-recovered.json")
        self.assertEqual("complete", result["run_status"])

    def test_actual_out_of_scope_write_is_rejected(self):
        run_dir = self.init_run()
        self.pass_inspect(run_dir)
        self.start(run_dir, "implement", "writer")
        (self.workspace / "src" / "feature.txt").write_text("feature\n", encoding="utf-8")
        (self.workspace / "outside.txt").write_text("unexpected\n", encoding="utf-8")
        report = self.report(artifacts=["src/feature.txt"], changed=["src/feature.txt"])
        with self.assertRaisesRegex(run_state.CodeflowError, "actual changes outside write scope"):
            self.finish(run_dir, "implement", report, "actual-scope.json")

    def test_stale_final_attempt_can_be_recovered(self):
        workflow = copy.deepcopy(self.workflow)
        workflow["nodes"][0]["max_attempts"] = 1
        workflow["limits"]["max_cycles"] = 1
        run_dir = self.init_run(workflow, "stale-final")
        first = self.start(run_dir, "inspect")
        self.assertEqual(1, first["execution"])
        state_path = run_dir / "run.json"
        state = json.loads(state_path.read_text(encoding="utf-8"))
        state["nodes"]["inspect"]["started_at"] = (datetime.now(timezone.utc) - timedelta(hours=2)).isoformat()
        run_state.atomic_write_json(state_path, state)
        run_state.cmd_resume(argparse.Namespace(run_dir=str(run_dir), stale_after=60))
        second = self.start(run_dir, "inspect")
        self.assertEqual(1, second["attempt"])
        self.assertEqual(2, second["execution"])
        state = json.loads(state_path.read_text(encoding="utf-8"))
        state["nodes"]["inspect"]["started_at"] = (datetime.now(timezone.utc) - timedelta(hours=2)).isoformat()
        run_state.atomic_write_json(state_path, state)
        run_state.cmd_resume(argparse.Namespace(run_dir=str(run_dir), stale_after=60))
        status = run_state.cmd_status(argparse.Namespace(run_dir=str(run_dir)))
        self.assertEqual(["inspect"], status["blocked"])
        self.assertEqual([], status["ready"])

    def test_symlink_scope_and_control_character_are_rejected(self):
        outside = self.root / "outside"
        outside.mkdir()
        (self.workspace / "src" / "link").symlink_to(outside, target_is_directory=True)
        workflow = copy.deepcopy(self.workflow)
        workflow["nodes"][1]["paths"] = ["src/link/feature.txt"]
        workflow["nodes"][1]["write_scope"] = ["src/link/feature.txt"]
        workflow["nodes"][2]["paths"] = ["src/link/feature.txt"]
        with self.assertRaisesRegex(run_state.CodeflowError, "escapes workspace"):
            run_state.validate_workflow(workflow)

        (self.workspace / ".codeflow").mkdir()
        (self.workspace / "ledger-link").symlink_to(self.workspace / ".codeflow", target_is_directory=True)
        ledger = copy.deepcopy(self.workflow)
        ledger["nodes"][1]["paths"] = ["ledger-link/result.json"]
        ledger["nodes"][1]["write_scope"] = ["ledger-link/result.json"]
        ledger["nodes"][2]["paths"] = ["ledger-link/result.json"]
        with self.assertRaisesRegex(run_state.CodeflowError, "resolves into the Codeflow ledger"):
            run_state.validate_workflow(ledger)
        with self.assertRaisesRegex(run_state.CodeflowError, "control character"):
            run_state.normalize_rel_path("src/bad\x00name", "test")

    def test_sync_is_recoverable_if_export_is_stale(self):
        run_dir = self.init_run()
        adapted = copy.deepcopy(self.workflow)
        adapted["nodes"].insert(
            1,
            {
                "id": "extra",
                "kind": "investigate",
                "mode": "read",
                "objective": "Inspect another path",
                "depends_on": [],
                "paths": ["tests"],
                "acceptance": ["Return evidence"],
                "forbidden": ["edit files", "spawn subagents"],
                "fresh_context": True,
            },
        )
        path = self.write_json("sync-recovery.json", adapted)
        run_state.cmd_sync(argparse.Namespace(run_dir=str(run_dir), workflow=str(path)))
        (run_dir / "workflow.json").write_text("{}", encoding="utf-8")
        status = run_state.cmd_status(argparse.Namespace(run_dir=str(run_dir)))
        self.assertEqual("active", status["status"])
        repaired = json.loads((run_dir / "workflow.json").read_text(encoding="utf-8"))
        self.assertEqual(4, len(repaired["nodes"]))


if __name__ == "__main__":
    unittest.main()
