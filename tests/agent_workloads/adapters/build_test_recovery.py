import shutil
import subprocess
import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: build_test_recovery INPUT_ROOT")

project = Path("project")
shutil.copytree(sys.argv[1], project)
try:
    invalid = subprocess.run(
        ["python3", "-m", "py_compile", str(project / "invalid.py")],
        capture_output=True,
        check=False,
    )
    if invalid.returncode == 0:
        raise SystemExit("invalid source passed")

    checked = subprocess.run(
        ["python3", "-m", "py_compile", str(project / "valid.py")],
        capture_output=True,
        check=False,
    )
    if checked.returncode != 0:
        raise SystemExit("valid source did not build")

    tested = subprocess.run(
        ["python3", str(project / "valid.py")],
        capture_output=True,
        text=True,
        check=False,
    )
    if tested.returncode != 0:
        raise SystemExit("valid source test failed")
    print("recovery=ok")
    print(f"test={tested.stdout.strip()}")
finally:
    shutil.rmtree(project)
