import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: incident_report INPUT_FILE")

incidents = []
rejects = []
for line_number, line in enumerate(Path(sys.argv[1]).read_text().splitlines()[1:], 2):
    if not line:
        continue
    fields = line.split("\t")
    if len(fields) != 3:
        rejects.append(f"reject|{line_number}|field-count")
        continue
    service, status, _duration = fields
    if status not in {"ok", "error"}:
        rejects.append(f"reject|{line_number}|status")
    elif not service:
        rejects.append(f"reject|{line_number}|service")
    else:
        incidents.append((service, status))

rows = [f"accepted|{len(incidents)}", f"rejected|{len(rejects)}", *rejects]
for service in sorted({service for service, _status in incidents}):
    ok = sum(item == (service, "ok") for item in incidents)
    errors = sum(item == (service, "error") for item in incidents)
    rows.append(f"{service}|ok={ok}|error={errors}")
print(*rows, sep="\n")
