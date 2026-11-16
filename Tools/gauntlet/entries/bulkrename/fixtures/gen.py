import random
import sys
from pathlib import Path


def main() -> None:
    root = Path(sys.argv[1])
    root.mkdir(parents=True, exist_ok=True)
    rng = random.Random(3)
    for number in rng.sample(range(1, 100000), 300):
        (root / f"IMG_{number}.jpeg").touch()
    for number in range(1, 61):
        (root / f"IMG_{number}.png").touch()
    for number in range(1, 41):
        (root / f"notes-{number}.txt").touch()


if __name__ == "__main__":
    main()
