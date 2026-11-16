import csv

count = qty = value = 0
with open("w05.csv", newline="") as handle:
    rows = csv.reader(handle)
    next(rows)
    for row in rows:
        q = int(row[3])
        count += 1
        qty += q
        value += q * int(row[4])
print(f"rows={count} qty={qty} value={value}")
