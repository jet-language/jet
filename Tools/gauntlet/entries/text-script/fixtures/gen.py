#!/usr/bin/env python3
import sys


def main(path):
    with open(path, "w", encoding="utf-8", newline="") as output:
        output.write("  zebra  \n\n apple\nbanana  \n apple \n\tpear\t\n")


if __name__ == "__main__":
    main(sys.argv[1])
