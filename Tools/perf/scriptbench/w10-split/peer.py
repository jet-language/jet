text = "\n".join(f"alpha beta gamma delta epsilon {i}" for i in range(20000))
count = long = size = 0
for line in text.split("\n"):
    for word in line.split(" "):
        count += 1
        size += len(word)
        if len(word) > 4:
            long += 1
print(f"words={count} long={long} bytes={size}")
