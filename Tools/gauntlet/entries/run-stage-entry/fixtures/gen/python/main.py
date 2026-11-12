# Prints the sum of the numbers in fixtures/data.txt.
with open("fixtures/data.txt") as data:
    print(sum(int(line) for line in data))
