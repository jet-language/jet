import re
import sys
from collections import Counter


PATTERN = re.compile(
    r'^([0-9.]+) - - \[[^]]+\] "GET (/api/[^ ]+) HTTP/1\.1" 5[0-9]{2} '
)


def main(path):
    counts = Counter()
    matches = 0
    with open(path, encoding="ascii") as source:
        for line in source:
            found = PATTERN.match(line)
            if found:
                counts[found.group(1)] += 1
                matches += 1

    print(f"matches {matches}")
    for count, ip in sorted(
        ((count, ip) for ip, count in counts.items()), key=lambda item: (-item[0], item[1])
    )[:5]:
        print(f"{count} {ip}")


if __name__ == "__main__":
    main(sys.argv[1])
