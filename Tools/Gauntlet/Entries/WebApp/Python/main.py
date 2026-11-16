count = 0


def display():
    return f"count: {count}"


def increment():
    global count
    count += 1


def reset():
    global count
    count = 0


for _ in range(3):
    increment()
print(display())
reset()
print(display())
