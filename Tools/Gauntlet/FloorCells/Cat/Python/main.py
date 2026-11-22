import sys


def main() -> None:
    write = sys.stdout.write
    for line in sys.stdin:
        write(line.rstrip("\r\n") + "\n")


if __name__ == "__main__":
    main()
