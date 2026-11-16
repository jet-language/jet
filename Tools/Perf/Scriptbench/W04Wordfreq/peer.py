import re
from collections import Counter

rx = re.compile(r"[a-z]+")
counts = Counter()
with open("data/words.txt") as f:
    for line in f:
        counts.update(rx.findall(line.lower()))
print(f"total {sum(counts.values())}")
print(f"distinct {len(counts)}")
for word, n in sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[:10]:
    print(n, word)
