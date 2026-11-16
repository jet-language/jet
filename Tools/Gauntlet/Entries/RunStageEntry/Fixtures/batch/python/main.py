# Prints the first argv[1] lines of words.txt.
import sys

with open("words.txt") as words:
    for line in words.readlines()[: int(sys.argv[1])]:
        print(line, end="")
