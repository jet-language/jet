# `add NAME` appends NAME to items.txt; `list` prints items.txt.
import sys

if sys.argv[1] == "add":
    with open("items.txt", "a") as items:
        items.write(sys.argv[2] + "\n")
    print("added " + sys.argv[2])
else:
    with open("items.txt") as items:
        print(items.read(), end="")
