import random
import sys
from pathlib import Path


WORDS = (
    "amber birch cedar delta ember fern granite hazel ivory juniper kelp linen maple"
    " nickel olive pebble quartz river saffron thistle umber velvet willow xenon yarrow"
).split()


def main() -> None:
    root = Path(sys.argv[1])
    root.mkdir(parents=True, exist_ok=True)
    rng = random.Random(31)
    for index in range(240):
        lines = []
        matched = rng.random() < 0.4
        needle_lines = set()
        if matched:
            needle_lines = set(rng.sample(range(2000), 1 + rng.randrange(4)))
        for line_no in range(2000):
            words = [rng.choice(WORDS) for _ in range(8)]
            if line_no in needle_lines:
                words[rng.randrange(len(words))] = "needle-7f"
            lines.append(" ".join(words))
        (root / f"f{index:03}.txt").write_text("\n".join(lines) + "\n", encoding="ascii")


if __name__ == "__main__":
    main()
