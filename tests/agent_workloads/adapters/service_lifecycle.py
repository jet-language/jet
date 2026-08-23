import os
import shutil
import subprocess
import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: service_lifecycle INPUT_ROOT")

input_root = Path(sys.argv[1]).resolve()
task = os.environ["JET_CORPUS_TASK"]
project = Path.cwd() / "service-project"
home = Path.cwd() / "service-home"
root = Path.cwd() / "service-root"
shutil.rmtree(project, ignore_errors=True)
shutil.rmtree(home, ignore_errors=True)
shutil.rmtree(root, ignore_errors=True)
try:
    shutil.copytree(input_root, project)
    home.mkdir()
    root.mkdir()
    for name in ("systemd-run", "systemctl"):
        (project / "bin" / name).chmod(0o755)
    env = {
        **os.environ,
        "HOME": str(home),
        "JETPACK_ROOT": str(root),
        "JETPACK_FAKE_SYSTEMD_STATE": str(project / "systemd-state"),
        "JETPACK_SERVICE_HEALTH_TIMEOUT_MS": "200" if task == "service-lifecycle-readiness-timeout" else "5000",
        "PATH": f"{project / 'bin'}:{os.environ['PATH']}",
    }

    def run(args):
        return subprocess.run(
            [os.environ["JET_CORPUS_JETPACK"], *args],
            cwd=project,
            env=env,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )

    if task == "service-lifecycle-readiness-timeout":
        failed = run(["services", "up", "timeout", "--no-color"])
        if failed.returncode == 0 or "E1261" not in failed.stderr + failed.stdout:
            raise SystemExit("readiness timeout did not fail with E1261")
        lifecycle = (project / ".jet/services/timeout/lifecycle").read_text()
        if "phase=failed" not in lifecycle or "recovery=startup-failed" not in lifecycle:
            raise SystemExit("failed service lost lifecycle receipt")
        if (project / ".jet/services/timeout/pid").exists():
            raise SystemExit("failed service retained pid")
        child_file = project / ".jet/services/timeout/data/child.pid"
        if child_file.exists():
            child = child_file.read_text().strip()
            stat = Path(f"/proc/{child}/stat")
            if stat.exists() and stat.read_text().split(") ", 1)[1].split()[0] != "Z":
                raise SystemExit("failed service retained descendant")
        if not (project / ".jet/services/timeout/supervisor.error").is_file():
            raise SystemExit("failed service lost supervisor receipt")
        print("service=failed\nerror=E1261\nlimit=bounded\ndescendants=contained\nreceipt=startup-failed")
    else:
        if run(["services", "up", "fixture", "--no-color"]).returncode != 0:
            raise SystemExit("service up failed")
        health = run(["services", "health", "fixture", "--json", "--no-color"])
        if health.returncode != 0 or not all(marker in health.stdout for marker in ("healthy", "linux-systemd-user", "delegated-cgroup")):
            raise SystemExit("service health receipt drifted")
        waited = run(["services", "wait", "fixture", "--no-color"])
        if waited.returncode != 0 or "service `fixture` is ready" not in waited.stderr:
            raise SystemExit("service wait drifted")
        logs = run(["services", "logs", "fixture", "--no-color"])
        if logs.returncode != 0 or "service-started" not in logs.stdout:
            raise SystemExit("service logs drifted")
        if run(["services", "down", "fixture", "--no-color"]).returncode != 0:
            raise SystemExit("service down failed")
        lifecycle = (project / ".jet/services/fixture/lifecycle").read_text()
        if "phase=stopped" not in lifecycle or "recovery=down" not in lifecycle:
            raise SystemExit("service stop receipt drifted")
        if (project / ".jet/services/fixture/pid").exists():
            raise SystemExit("stopped service retained pid")
        print("service=ready\nauthority=linux-systemd-user\ncontainment=delegated-cgroup\nreceipt=health-lifecycle\ncleanup=ok")
finally:
    shutil.rmtree(project, ignore_errors=True)
    shutil.rmtree(home, ignore_errors=True)
    shutil.rmtree(root, ignore_errors=True)
