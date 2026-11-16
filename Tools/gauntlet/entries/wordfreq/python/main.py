import sys
from collections import Counter


with open(sys.argv[1], "rb") as input_file:
    data = input_file.read()

data.decode("utf-8")
counts = Counter(data.split())

for word, count in sorted(counts.items(), key=lambda item: (-item[1], item[0]))[:20]:
    print(count, word.decode("utf-8"))
print("distinct", len(counts), "total", sum(counts.values()))
