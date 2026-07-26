import subprocess
import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: git_diff_review INPUT_ROOT")

root = Path(sys.argv[1])
completed = subprocess.run(
    [
        "git",
        "-C",
        str(root),
        "-c",
        "core.quotePath=false",
        "diff",
        "--no-index",
        "--no-renames",
        "--name-status",
        "--",
        "before",
        "after",
    ],
    capture_output=True,
    text=True,
    timeout=5,
    check=False,
)
if completed.returncode != 1:
    raise SystemExit(f"git diff exit {completed.returncode}: {completed.stderr.strip()}")

counts = {"A": 0, "D": 0, "M": 0}
kinds = {"A": "added", "D": "deleted", "M": "modified"}
rows = []
for line in completed.stdout.splitlines():
    fields = line.split("\t")
    if len(fields) != 2 or fields[0] not in kinds:
        raise SystemExit(f"bad git name-status row: {line}")
    status, path = fields
    if path.startswith("before/"):
        path = path[len("before/") :]
    elif path.startswith("after/"):
        path = path[len("after/") :]
    else:
        raise SystemExit(f"git path escaped roots: {path}")
    counts[status] += 1
    rows.append(f"{path}|{kinds[status]}")

print(*sorted(rows), sep="\n")
print(f"summary|added={counts['A']}|modified={counts['M']}|deleted={counts['D']}")
