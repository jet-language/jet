import os

files = 0
total = 0
for root, dirs, names in os.walk("w02"):
    for name in names:
        files += 1
        total += os.stat(os.path.join(root, name)).st_size
print(f"files={files} bytes={total}")
