import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: repository_marker_scan INPUT_ROOT")

root = Path(sys.argv[1])
rows = []
for file in root.rglob("*"):
    if not file.is_file():
        continue
    count = sum("agent_workload:" in line for line in file.read_text().splitlines())
    if count:
        rows.append(f"/{file.relative_to(root).as_posix()}|{count}")

print(*sorted(rows), sep="\n")
