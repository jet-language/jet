import random
import sys


def main(out_path):
    rng = random.Random(7)
    ips = [f"192.0.2.{i}" for i in range(1, 51)]
    paths = [f"/api/v1/resource/{i}" for i in range(60)]
    paths += [f"/static/file/{i}" for i in range(140)]
    statuses = (200, 301, 404, 500, 502, 503)
    weights = (70, 10, 10, 4, 3, 3)

    with open(out_path, "w", encoding="ascii", newline="") as out:
        for _ in range(500_000):
            ip = ips[rng.randrange(len(ips))]
            path = paths[rng.randrange(len(paths))]
            pick = rng.randrange(100)
            total = 0
            for status, weight in zip(statuses, weights):
                total += weight
                if pick < total:
                    break
            size = rng.randrange(100, 100_000)
            out.write(
                f'{ip} - - [01/Jan/2026:00:00:00 +0000] "GET {path} HTTP/1.1" '
                f"{status} {size}\n"
            )


if __name__ == "__main__":
    main(sys.argv[1])
