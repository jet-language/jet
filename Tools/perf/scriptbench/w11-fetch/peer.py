from concurrent.futures import ThreadPoolExecutor
from urllib.request import urlopen


def fetch(i):
    with urlopen(f"http://127.0.0.1:18411/item/{i}") as resp:
        resp.read()
        return resp.status


with ThreadPoolExecutor(max_workers=32) as pool:
    codes = list(pool.map(fetch, range(100)))
print(f"ok={codes.count(200)}")
