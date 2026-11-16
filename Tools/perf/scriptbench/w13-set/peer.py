with open("w13.txt") as handle:
    seen = set(line.rstrip("\n") for line in handle)
print(f"unique={len(seen)}")
