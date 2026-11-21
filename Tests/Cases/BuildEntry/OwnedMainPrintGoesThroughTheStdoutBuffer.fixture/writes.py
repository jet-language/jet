# Prints the write and writev calls the `jet perf run` trace argv[1] counted
# (its `syscalls` receipt section), summed over every calling stack.
import json
import sys

with open(sys.argv[1]) as trace:
    content = json.load(trace)["content"]
rows = [section["value"]["syscalls"] for section in content["receipt_sections"] if section["name"] == "syscalls"]
print(sum(row["count"] for section in rows for row in section if row["name"] in ("write", "writev")))
