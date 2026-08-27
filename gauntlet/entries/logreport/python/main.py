#!/usr/bin/env python3
import sys
from collections import Counter
from datetime import datetime


def main() -> None:
    levels = ("DEBUG", "INFO", "WARN", "ERROR")
    level_counts = Counter()
    error_components = Counter()
    first_text = last_text = ""
    first_ms = last_ms = 0
    total = 0
    with open(sys.argv[1], encoding="utf-8") as stream:
        for line in stream:
            timestamp_text, level, component, _message = line.rstrip("\n").split(" ", 3)
            timestamp = datetime.fromisoformat(timestamp_text.replace("Z", "+00:00"))
            timestamp_ms = int(timestamp.timestamp() * 1000)
            if total == 0:
                first_text, first_ms = timestamp_text, timestamp_ms
            last_text, last_ms = timestamp_text, timestamp_ms
            level_counts[level] += 1
            if level == "ERROR":
                error_components[component] += 1
            total += 1
    for level in levels:
        print(f"{level} {level_counts[level]}")
    print("top-error-components:")
    for count, component in sorted(((count, component) for component, count in error_components.items()), key=lambda item: (-item[0], item[1]))[:3]:
        print(f"{count} {component}")
    print(f"span {first_text} .. {last_text} ({(last_ms - first_ms) // 1000}s)")


if __name__ == "__main__":
    main()
