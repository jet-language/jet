import json
import sys
from urllib.error import HTTPError
from urllib.request import urlopen


def get(url):
    try:
        with urlopen(url) as response:
            return response.status, json.load(response)
    except HTTPError as response:
        return response.code, json.load(response)


def main():
    base = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:18400"
    status, listing = get(f"{base}/items")
    assert status == 200
    print(f"items {len(listing['items'])}")
    for item_id in (2, 5, 99):
        status, item = get(f"{base}/items/{item_id}")
        if status == 404:
            print(f"item {item_id} missing")
        else:
            print(f"item {item['id']} {item['name']} qty={item['qty']}")
    print(f"total-qty {sum(item['qty'] for item in listing['items'])}")


if __name__ == "__main__":
    main()
