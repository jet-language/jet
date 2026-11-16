import re

rx = re.compile(r"status=5\d\d ms=(\d+)")
count = total = 0
with open("data/log100m.txt") as f:
    for line in f:
        m = rx.search(line)
        if m:
            count += 1
            total += int(m.group(1))
print(f"matches {count}")
print(f"ms_sum {total}")
