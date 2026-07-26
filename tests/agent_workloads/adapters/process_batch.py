import subprocess
import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: process_batch INPUT_FILE")

for line_number, line in enumerate(Path(sys.argv[1]).read_text().splitlines()[1:], 2):
    if not line:
        continue
    fields = line.split("\t")
    if len(fields) != 4:
        raise SystemExit(f"bad process row {line_number}")
    label, program, argument, timeout_text = fields
    try:
        completed = subprocess.run(
            [program, argument],
            capture_output=True,
            text=True,
            timeout=int(timeout_text) / 1000,
            check=False,
        )
    except subprocess.TimeoutExpired:
        print(f"{label}|timeout")
    else:
        print(f"{label}|exit={completed.returncode}|stdout={completed.stdout.strip()}")
