import csv
import sys


def main(path):
    groups = {}
    total_count = 0
    total_sum = 0.0
    with open(path, newline="", encoding="utf-8") as source:
        rows = csv.DictReader(source)
        for row in rows:
            amount = float(row["amount"])
            if amount <= 0.0:
                continue
            region = row["region"]
            count, total = groups.get(region, (0, 0.0))
            groups[region] = (count + 1, total + amount)
            total_count += 1
            total_sum += amount
    for region in sorted(groups):
        count, total = groups[region]
        print(f"{region} n={count} sum={total:.2f}")
    print(f"total n={total_count} sum={total_sum:.2f}")


if __name__ == "__main__":
    main(sys.argv[1])
