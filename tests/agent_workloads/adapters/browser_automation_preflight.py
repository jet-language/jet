import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: browser_automation_preflight INPUT_FILE")

available_capabilities = {"profile", "timeout"}
profiles = {"bidi-2025.5", "bidi-2024.11"}


def has_capability(name):
    return name in available_capabilities


def parse_timeout(value):
    raw = value.strip()
    if not raw:
        return None
    if raw[0] in "+-":
        digits = raw[1:]
    else:
        digits = raw
    if not digits or not all("0" <= character <= "9" for character in digits):
        return None
    try:
        milliseconds = int(raw)
    except ValueError:
        return None
    return milliseconds if 1 <= milliseconds <= 600000 else None


for line_number, line in enumerate(Path(sys.argv[1]).read_text().splitlines(), 1):
    if line_number == 1 or not line:
        continue
    fields = line.split("\t")
    if len(fields) != 2:
        raise SystemExit(f"malformed browser row {line_number}")
    operation, value = fields
    if operation == "profile":
        accepted = has_capability("profile") and value in profiles
    elif operation == "timeout":
        accepted = has_capability("timeout") and parse_timeout(value) is not None
    elif operation == "connect":
        accepted = False
    else:
        raise SystemExit(f"unknown browser operation {operation}")
    print(f"{operation}|{value}|{'accepted' if accepted else 'rejected'}")
