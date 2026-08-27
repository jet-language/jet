#!/usr/bin/env python3
import sys
from datetime import datetime, timezone


def main() -> None:
    meetings = []
    with open(sys.argv[1], encoding="utf-8") as stream:
        for line in stream:
            name, raw = line.rstrip("\n").split("|", 1)
            local = datetime.fromisoformat(raw)
            utc = local.astimezone(timezone.utc)
            meetings.append((utc, name))
    meetings.sort()
    for position, (utc, name) in enumerate(meetings):
        gap = "-" if position == 0 else str(int((utc - meetings[position - 1][0]).total_seconds() // 60))
        print(f"{name} utc={utc.strftime('%Y-%m-%dT%H:%M:%SZ')} day={utc.strftime('%a')} gap={gap}")
    span = int((meetings[-1][0] - meetings[0][0]).total_seconds() // 60)
    print(f"span {span} minutes {len(meetings)}")


if __name__ == "__main__":
    main()
