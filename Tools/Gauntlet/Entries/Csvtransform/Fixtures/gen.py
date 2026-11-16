import csv
import random
import sys
from datetime import date, timedelta


REGIONS = ["Central", "East", "Midwest", "North", "Northeast", "South", "Southeast", "West"]
CATEGORIES = [
    "hardware",
    "software",
    "services",
    "supplies",
    "training",
    "kits, deluxe",
    "parts, premium",
    "bundles, team",
    "tools, field",
    "cases, rugged",
] + [f"category-{i:02d}" for i in range(15)]


def main(out_path):
    rng = random.Random(21)
    with open(out_path, "w", newline="", encoding="utf-8") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(["id", "region", "category", "amount", "date"])
        for row_id in range(200_000):
            amount = rng.randint(-5000, 95000) / 100.0
            day = date(2026, 1, 1) + timedelta(days=rng.randrange(365))
            writer.writerow(
                [
                    row_id,
                    rng.choice(REGIONS),
                    rng.choice(CATEGORIES),
                    f"{amount:.2f}",
                    day.isoformat(),
                ]
            )


if __name__ == "__main__":
    main(sys.argv[1])
