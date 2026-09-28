import struct
import sys
from pathlib import Path


FRAME = struct.Struct("<BBHIHH")


def main(path):
    data = Path(path).read_bytes()
    if len(data) < 8 or data[:4] != b"EDB1":
        raise ValueError("bad input")
    count = struct.unpack_from("<I", data, 4)[0]
    checksum = sum(data)
    offset = 8
    valid = 0
    channels = [0, 0, 0, 0]
    sample_min = 65_535
    sample_max = 0
    sample_sum = 0
    energy_sum = 0
    load_sum = 0
    tick_sum = 0
    for _ in range(count):
        channel, flags, sample, tick, energy, load = FRAME.unpack_from(data, offset)
        offset += FRAME.size
        if channel >= len(channels):
            raise ValueError("bad channel")
        if flags == 0:
            valid += 1
            channels[channel] += 1
            sample_min = min(sample_min, sample)
            sample_max = max(sample_max, sample)
            sample_sum += sample
            energy_sum += energy
            load_sum += load
            tick_sum += tick
    print(f"frames {count}")
    print(f"valid {valid}")
    print(f"channels {channels[0]} {channels[1]} {channels[2]} {channels[3]}")
    print(f"sample {sample_min} {sample_max} {sample_sum}")
    print(f"energy {energy_sum}")
    print(f"load {load_sum}")
    print(f"ticks {tick_sum}")
    print(f"checksum {checksum}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "telemetry.bin")
