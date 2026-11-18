"""Probe N5: which source files does a Jet image name in its debug sections?
Lists every NUL-terminated `.jet` name in each `.debug*` and `.jet*` section,
and every other section that holds one. Usage: python3 files.py <image>"""
import re
import struct
import sys

data = open(sys.argv[1], "rb").read()
shoff = struct.unpack_from("<Q", data, 0x28)[0]
shentsize, shnum, shstrndx = struct.unpack_from("<HHH", data, 0x3A)
sections = [struct.unpack_from("<IIQQQQ", data, shoff + i * shentsize) for i in range(shnum)]
names_at = sections[shstrndx][4]
for name, _, _, _, offset, size in sections:
    label = data[names_at + name:data.index(b"\0", names_at + name)].decode()
    found = sorted({m.decode() for m in re.findall(rb"[\w./:-]*\.jet(?=\0|::)", data[offset:offset + size])})
    if found:
        print("{}: {}".format(label, ", ".join(found)))
