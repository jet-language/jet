def build():
    s = ""
    for _ in range(2000000):
        s += "abc,"
    return s


print(f"len={len(build())}")
