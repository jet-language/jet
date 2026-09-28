import random
import sys


rng = random.Random(42)
vocabulary = [f"w{i:05d}" for i in range(10_000)]
rng.shuffle(vocabulary)
weights = [1.0 / (rank + 1) for rank in range(len(vocabulary))]

with open(sys.argv[1], "w", encoding="ascii", newline="\n") as output:
    for start in range(0, 2_000_000, 15):
        size = min(15, 2_000_000 - start)
        output.write(" ".join(rng.choices(vocabulary, weights=weights, k=size)))
        output.write("\n")
