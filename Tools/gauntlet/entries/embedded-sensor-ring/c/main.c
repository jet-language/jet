#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int64_t crc16(int64_t word) {
    int64_t crc = 65535;
    int64_t value = word;
    for (int bit = 0; bit < 16; ++bit) {
        int64_t top = crc / 32768;
        int64_t incoming = value % 2;
        crc = (crc * 2) % 65536;
        if ((top + incoming) % 2 == 1) crc = (crc + 4129) % 65536;
        value /= 2;
    }
    return crc;
}

static int64_t signed_div8(int64_t value) {
    return value >= 0 ? value / 8 : -((-value) / 8);
}

static int64_t clamp(int64_t value, int64_t low, int64_t high) {
    if (value < low) return low;
    if (value > high) return high;
    return value;
}

int main(int argc, char **argv) {
    int64_t ticks = strtoll(argv[1], NULL, 10);
    int64_t ring[16] = {0};
    int ring_position = 0;
    int64_t running_sum = 0;
    int64_t integrator = 0;
    int64_t actuator = 0;
    int64_t watchdog = 0;
    int64_t watchdog_resets = 0;
    int64_t accepted = 0;
    int64_t rejected = 0;
    int64_t faults = 0;
    int64_t control_reg = 4095;
    int64_t status_reg = 0;
    int64_t mmio_checksum = 0;

    for (int64_t tick = 0; tick < ticks; ++tick) {
        int64_t raw = (tick * 73 + 19) % 4096;
        int64_t frame = (raw * 17 + tick * 31 + 7) % 65536;
        int64_t expected_crc = crc16(frame);
        int64_t received_crc = expected_crc;
        if (tick % 127 == 0) received_crc = (expected_crc + 1) % 65536;
        watchdog += 1;

        if (received_crc != expected_crc) {
            rejected += 1;
            faults += 1;
            status_reg = 2;
        } else {
            int64_t sample = (raw * 3 + 11) % 4096;
            int64_t old = ring[ring_position];
            ring[ring_position] = sample;
            ring_position += 1;
            if (ring_position == 16) ring_position = 0;
            running_sum += sample - old;
            int64_t mean = running_sum / 16;
            int64_t error = 2048 - mean;
            integrator += error;
            integrator = clamp(integrator, -8192, 8192);
            int64_t command = error * 4 + signed_div8(integrator);
            command = clamp(command, -4095, 4095);
            actuator = command;
            accepted += 1;
            status_reg = 1;
        }

        control_reg = actuator + 4095;
        mmio_checksum = (mmio_checksum * 33 + status_reg * 257 + control_reg + received_crc) % 1000000007;
        if (watchdog == 64) {
            watchdog = 0;
            watchdog_resets += 1;
        }
    }

    int64_t ring_checksum = 0;
    for (int index = 0; index < 16; ++index) {
        ring_checksum = (ring_checksum * 131 + ring[index]) % 1000000007;
    }
    printf("ticks %lld\n", (long long)ticks);
    printf("accepted %lld rejected %lld\n", (long long)accepted, (long long)rejected);
    printf("watchdog_resets %lld faults %lld\n", (long long)watchdog_resets, (long long)faults);
    printf("actuator %lld\n", (long long)actuator);
    printf("mmio_checksum %lld\n", (long long)mmio_checksum);
    printf("ring_checksum %lld\n", (long long)ring_checksum);
}
