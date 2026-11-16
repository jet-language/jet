# Writes the numbers 1 to 5, one per line, to argv[1].
import sys

with open(sys.argv[1], "w") as out:
    for n in range(1, 6):
        out.write(f"{n}\n")
