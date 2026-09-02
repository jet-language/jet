import struct
import sys


COUNT = 1_000_000


def main(path):
    with open(path, "wb") as out:
        out.write(b"EDB1")
        out.write(struct.pack("<I", COUNT))
        for index in range(COUNT):
            channel = index % 4
            flags = (index // 4) % 2
            sample = (index % 4095) + 1
            tick = 1_000_000 + index * 3
            energy = (index * 5 + 17) % 1000
            load = (index * 7 + 23) % 65_536
            out.write(struct.pack("<BBHIHH", channel, flags, sample, tick, energy, load))


if __name__ == "__main__":
    main(sys.argv[1])
