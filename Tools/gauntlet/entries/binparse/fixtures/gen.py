import random
import struct
import sys


def main(path):
    rng = random.Random(99)
    count = 300_000
    ids = list(range(count))
    rng.shuffle(ids)
    with open(path, "wb") as out:
        out.write(b"JGB1")
        out.write(struct.pack("<I", count))
        for record_id in ids:
            value = rng.uniform(-1000.0, 1000.0)
            suffix_len = rng.randrange(1, 20)
            suffix = "".join(rng.choice("abcdefghijklmnopqrstuvwxyz0123456789") for _ in range(suffix_len))
            name = ("item-" + suffix).encode("ascii")
            out.write(struct.pack("<I d H", record_id, value, len(name)))
            out.write(name)


if __name__ == "__main__":
    main(sys.argv[1])
