import json
import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: structured_data INPUT_FILE")

try:
    payload = json.loads(Path(sys.argv[1]).read_text())
    events = payload["events"]
    if not isinstance(events, list):
        raise ValueError
    for event in events:
        if not isinstance(event, dict):
            raise ValueError
        if not isinstance(event["service"], str) or not isinstance(event["duration_ms"], int):
            raise ValueError
except (KeyError, TypeError, ValueError, json.JSONDecodeError):
    print("invalid-json")
else:
    summaries = []
    for service in sorted({event["service"] for event in events}):
        rows = [event for event in events if event["service"] == service]
        summaries.append(
            {"service": service, "count": len(rows), "total_ms": sum(row["duration_ms"] for row in rows)}
        )
    print(json.dumps({"total_events": len(events), "summaries": summaries}, separators=(",", ":")))
