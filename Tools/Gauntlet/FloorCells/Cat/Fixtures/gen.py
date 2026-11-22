import sys


def main() -> None:
    with open(sys.argv[1], "w", encoding="ascii", newline="\n") as out:
        for i in range(200_000):
            out.write(f"line-{i:06d} payload alpha beta gamma delta 0123456789\n")


if __name__ == "__main__":
    main()
