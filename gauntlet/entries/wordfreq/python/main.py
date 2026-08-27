import sys
from collections import Counter


with open(sys.argv[1], encoding="ascii") as input_file:
    counts = Counter(input_file.read().split())

for word, count in sorted(counts.items(), key=lambda item: (-item[1], item[0]))[:20]:
    print(count, word)
print("distinct", len(counts), "total", sum(counts.values()))
