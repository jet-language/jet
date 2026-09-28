import sys


CORPUS_BYTES = 4 * 1024 * 1024
MATCHES = 31_476
NEEDLE = b"struct"
MASK = 0xFFFFFFFF
# Deterministic printable source-like filler. The assertion below ensures its
# random bytes do not add matches to the injected corpus.
ALPHABET = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_{}[]()<>:=+-*/.,; \n"


def main(out_path):
    data = bytearray(CORPUS_BYTES)
    state = 0x20564D31
    for index in range(CORPUS_BYTES):
        state ^= (state << 13) & MASK
        state ^= state >> 17
        state ^= (state << 5) & MASK
        state &= MASK
        data[index] = ALPHABET[state % len(ALPHABET)]

    assert data.count(NEEDLE) == 0
    for index in range(MATCHES):
        offset = index * (CORPUS_BYTES - len(NEEDLE)) // MATCHES
        data[offset : offset + len(NEEDLE)] = NEEDLE

    assert len(data) == CORPUS_BYTES
    assert data.count(NEEDLE) == MATCHES
    with open(out_path, "wb") as output:
        output.write(data)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: gen.py OUTPUT")
    main(sys.argv[1])
