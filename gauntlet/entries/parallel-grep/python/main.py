from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import sys


def count_file(path_and_needle):
    path, needle = path_and_needle
    count = sum(line.count(needle) for line in path.read_text(encoding="ascii").splitlines())
    return str(path), count


def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "files")
    needle = sys.argv[2] if len(sys.argv) > 2 else "needle-7f"
    paths = sorted(root.glob("*.txt"))
    with ThreadPoolExecutor() as pool:
        results = list(pool.map(count_file, ((path, needle) for path in paths)))
    matches = sorted((path, count) for path, count in results if count > 0)
    for path, count in matches:
        print(f"{path}:{count}")
    print(f"files {len(matches)}/{len(paths)} total {sum(count for _, count in matches)}")


if __name__ == "__main__":
    main()
