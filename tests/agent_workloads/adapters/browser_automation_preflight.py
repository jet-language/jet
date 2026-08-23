import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: browser_automation_preflight INPUT_FILE")

profiles = {"bidi-2025.5", "bidi-2024.11"}
for line_number, line in enumerate(Path(sys.argv[1]).read_text().splitlines(), 1):
    if line_number == 1 or not line:
        continue
    operation, value = line.split("\t")
    if operation == "profile":
        accepted = value in profiles
    elif operation == "timeout":
        accepted = value == "500"
    elif operation == "connect":
        accepted = False
    else:
        raise SystemExit(f"unknown browser operation {operation}")
    print(f"{operation}|{value}|{'accepted' if accepted else 'rejected'}")
