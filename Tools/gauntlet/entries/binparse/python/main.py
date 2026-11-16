import struct
import sys


with open(sys.argv[1] if len(sys.argv) > 1 else "records.bin", "rb") as source:
    data = source.read()

if data[:4] != b"JGB1":
    raise ValueError("bad magic")
count = struct.unpack_from("<I", data, 4)[0]
at = 8
total = 0.0
hashed = 0xCBF29CE484222325
for _ in range(count):
    record_id, value, name_len = struct.unpack_from("<IdH", data, at)
    at += 14
    if record_id % 7 == 0:
        total += value
    for byte in data[at:at + name_len]:
        hashed = ((hashed ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    at += name_len

print(f"records {count}")
print(f"sum7 {total:.6f}")
print(f"fnv {hashed:016x}")
