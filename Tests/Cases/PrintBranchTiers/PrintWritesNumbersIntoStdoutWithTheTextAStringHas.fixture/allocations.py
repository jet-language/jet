# Prints the heap allocations the `jet perf run --lens=alloc` trace argv[1]
# counted (its `allocations` receipt section).
import json
import sys

with open(sys.argv[1]) as trace:
    content = json.load(trace)["content"]
print(sum(section["value"]["allocations"] for section in content["receipt_sections"] if section["name"] == "allocations"))
