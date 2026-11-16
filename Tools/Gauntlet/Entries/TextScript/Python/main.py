#!/usr/bin/env python3
import sys


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "notes.txt"
    with open(path, encoding="utf-8") as source:
        lines = sorted(line.strip() for line in source if line.strip())
    print(f"lines {len(lines)}")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
