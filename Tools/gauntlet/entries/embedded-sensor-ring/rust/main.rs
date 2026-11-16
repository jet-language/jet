use std::env;

fn crc16(word: i64) -> i64 {
    let mut crc = 65_535;
    let mut value = word;
    let mut bit = 0;
    while bit < 16 {
        let top = crc / 32_768;
        let incoming = value % 2;
        crc = (crc * 2) % 65_536;
        if (top + incoming) % 2 == 1 {
            crc = (crc + 4_129) % 65_536;
        }
        value /= 2;
        bit += 1;
    }
    crc
}

fn signed_div8(value: i64) -> i64 {
    if value >= 0 { value / 8 } else { -((-value) / 8) }
}

fn clamp(value: i64, low: i64, high: i64) -> i64 {
    value.clamp(low, high)
}

fn main() {
    let ticks: i64 = env::args().nth(1).unwrap().parse().unwrap();
    let mut ring = [0i64; 16];
    let mut ring_position = 0usize;
    let mut running_sum = 0i64;
    let mut integrator = 0i64;
    let mut actuator = 0i64;
    let mut watchdog = 0i64;
    let mut watchdog_resets = 0i64;
    let mut accepted = 0i64;
    let mut rejected = 0i64;
    let mut faults = 0i64;
    let mut control_reg = 4_095i64;
    let mut status_reg = 0i64;
    let mut mmio_checksum = 0i64;

    let mut tick = 0i64;
    while tick < ticks {
        let raw = (tick * 73 + 19) % 4_096;
        let frame = (raw * 17 + tick * 31 + 7) % 65_536;
        let expected_crc = crc16(frame);
        let mut received_crc = expected_crc;
        if tick % 127 == 0 {
            received_crc = (expected_crc + 1) % 65_536;
        }
        watchdog += 1;

        if received_crc != expected_crc {
            rejected += 1;
            faults += 1;
            status_reg = 2;
        } else {
            let sample = (raw * 3 + 11) % 4_096;
            let old = ring[ring_position];
            ring[ring_position] = sample;
            ring_position += 1;
            if ring_position == 16 {
                ring_position = 0;
            }
            running_sum += sample - old;
            let mean = running_sum / 16;
            let error = 2_048 - mean;
            integrator += error;
            integrator = clamp(integrator, -8_192, 8_192);
            let mut command = error * 4 + signed_div8(integrator);
            command = clamp(command, -4_095, 4_095);
            actuator = command;
            accepted += 1;
            status_reg = 1;
        }

        control_reg = actuator + 4_095;
        mmio_checksum = (mmio_checksum * 33 + status_reg * 257 + control_reg + received_crc) % 1_000_000_007;
        if watchdog == 64 {
            watchdog = 0;
            watchdog_resets += 1;
        }
        tick += 1;
    }

    let mut ring_checksum = 0i64;
    for sample in ring {
        ring_checksum = (ring_checksum * 131 + sample) % 1_000_000_007;
    }
    println!("ticks {ticks}");
    println!("accepted {accepted} rejected {rejected}");
    println!("watchdog_resets {watchdog_resets} faults {faults}");
    println!("actuator {actuator}");
    println!("mmio_checksum {mmio_checksum}");
    println!("ring_checksum {ring_checksum}");
}
