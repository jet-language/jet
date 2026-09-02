import sys


def crc16(word):
    crc = 65535
    value = word
    bit = 0
    while bit < 16:
        top = crc // 32768
        incoming = value % 2
        crc = (crc * 2) % 65536
        if (top + incoming) % 2 == 1:
            crc = (crc + 4129) % 65536
        value //= 2
        bit += 1
    return crc


def signed_div8(value):
    return value // 8 if value >= 0 else -((-value) // 8)


def clamp(value, low, high):
    return max(low, min(high, value))


def main():
    ticks = int(sys.argv[1])
    ring = [0] * 16
    ring_position = 0
    running_sum = 0
    integrator = 0
    actuator = 0
    watchdog = 0
    watchdog_resets = 0
    accepted = 0
    rejected = 0
    faults = 0
    control_reg = 4095
    status_reg = 0
    mmio_checksum = 0

    tick = 0
    while tick < ticks:
        raw = (tick * 73 + 19) % 4096
        frame = (raw * 17 + tick * 31 + 7) % 65536
        expected_crc = crc16(frame)
        received_crc = expected_crc
        if tick % 127 == 0:
            received_crc = (expected_crc + 1) % 65536
        watchdog += 1

        if received_crc != expected_crc:
            rejected += 1
            faults += 1
            status_reg = 2
        else:
            sample = (raw * 3 + 11) % 4096
            old = ring[ring_position]
            ring[ring_position] = sample
            ring_position += 1
            if ring_position == 16:
                ring_position = 0
            running_sum += sample - old
            mean = running_sum // 16
            error = 2048 - mean
            integrator += error
            integrator = clamp(integrator, -8192, 8192)
            command = error * 4 + signed_div8(integrator)
            command = clamp(command, -4095, 4095)
            actuator = command
            accepted += 1
            status_reg = 1

        control_reg = actuator + 4095
        mmio_checksum = (mmio_checksum * 33 + status_reg * 257 + control_reg + received_crc) % 1000000007
        if watchdog == 64:
            watchdog = 0
            watchdog_resets += 1
        tick += 1

    ring_checksum = 0
    for sample in ring:
        ring_checksum = (ring_checksum * 131 + sample) % 1000000007
    print(f"ticks {ticks}")
    print(f"accepted {accepted} rejected {rejected}")
    print(f"watchdog_resets {watchdog_resets} faults {faults}")
    print(f"actuator {actuator}")
    print(f"mmio_checksum {mmio_checksum}")
    print(f"ring_checksum {ring_checksum}")


if __name__ == "__main__":
    main()
