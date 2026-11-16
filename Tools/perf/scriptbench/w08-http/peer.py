from urllib.request import urlopen

total = 0
for i in range(200):
    with urlopen(f"http://127.0.0.1:18080/item/{i}") as resp:
        total += len(resp.read())
print("requests 200")
print(f"bytes {total}")
