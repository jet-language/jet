import csv
import statistics
import sys
from collections import defaultdict


def summary(values):
    ordered = sorted(values)
    n = len(ordered)
    mean = statistics.fmean(values)
    median = ordered[(n - 1) // 2]
    p95 = ordered[((19 * n + 19) // 20) - 1]
    sd = statistics.pstdev(values)
    return f"n={n} mean={mean:.2f} median={median:.2f} p95={p95:.2f} sd={sd:.2f}"


groups = defaultdict(list)
with open(sys.argv[1], newline="") as input_file:
    for row in csv.DictReader(input_file):
        groups[row["group"]].append(float(row["value"]))

all_values = []
for group in sorted(groups):
    all_values.extend(groups[group])
    print(group, summary(groups[group]))
print(f"overall n={len(all_values)} mean={statistics.fmean(all_values):.2f}")
