import csv
import random
import sys


def main(path):
    rng = random.Random(5)
    groups = ["alpha", "beta", "gamma", "delta", "epsilon"]
    means = {"alpha": 10.0, "beta": 20.0, "gamma": 30.0, "delta": 40.0, "epsilon": 50.0}
    with open(path, "w", newline="") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(["group", "value"])
        for _ in range(50_000):
            group = rng.choice(groups)
            writer.writerow([group, f"{rng.gauss(means[group], 3.0):.3f}"])


if __name__ == "__main__":
    main(sys.argv[1])
