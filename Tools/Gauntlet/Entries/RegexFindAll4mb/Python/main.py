import sys


def main(path):
    with open(path, encoding="ascii") as source:
        matches = source.read().count("struct")
    print(f"matches {matches}")


if __name__ == "__main__":
    main(sys.argv[1])
