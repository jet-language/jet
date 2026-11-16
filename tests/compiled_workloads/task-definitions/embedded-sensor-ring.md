# Embedded: sensor ring

Build freestanding firmware for the frozen board machine. Decode a bounded
binary sensor frame, write it to a fixed ring buffer, report a checksum over
the board console, and replay the same frames on the host. Include truncated,
bad-CRC, full-buffer, and out-of-range register cases. No allocator or OS
service is allowed. JIT is explicitly not applicable to the firmware target.
