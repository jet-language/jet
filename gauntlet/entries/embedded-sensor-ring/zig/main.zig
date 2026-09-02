const std = @import("std");

fn crc16(word: i64) i64 {
    var crc: i64 = 65535;
    var value = word;
    var bit: i64 = 0;
    while (bit < 16) : (bit += 1) {
        const top = @divTrunc(crc, 32768);
        const incoming = @mod(value, 2);
        crc = @mod(crc * 2, 65536);
        if (@mod(top + incoming, 2) == 1) crc = @mod(crc + 4129, 65536);
        value = @divTrunc(value, 2);
    }
    return crc;
}

fn signedDiv8(value: i64) i64 {
    if (value >= 0) return @divTrunc(value, 8);
    return -@divTrunc(-value, 8);
}

fn clamp(value: i64, low: i64, high: i64) i64 {
    if (value < low) return low;
    if (value > high) return high;
    return value;
}

pub fn main(init: std.process.Init) !void {
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    const ticks = try std.fmt.parseInt(i64, args[1], 10);
    var ring = [_]i64{0} ** 16;
    var ring_position: usize = 0;
    var running_sum: i64 = 0;
    var integrator: i64 = 0;
    var actuator: i64 = 0;
    var watchdog: i64 = 0;
    var watchdog_resets: i64 = 0;
    var accepted: i64 = 0;
    var rejected: i64 = 0;
    var faults: i64 = 0;
    var control_reg: i64 = 4095;
    var status_reg: i64 = 0;
    var mmio_checksum: i64 = 0;

    var tick: i64 = 0;
    while (tick < ticks) : (tick += 1) {
        const raw = @mod(tick * 73 + 19, 4096);
        const frame = @mod(raw * 17 + tick * 31 + 7, 65536);
        const expected_crc = crc16(frame);
        var received_crc = expected_crc;
        if (@mod(tick, 127) == 0) received_crc = @mod(expected_crc + 1, 65536);
        watchdog += 1;

        if (received_crc != expected_crc) {
            rejected += 1;
            faults += 1;
            status_reg = 2;
        } else {
            const sample = @mod(raw * 3 + 11, 4096);
            const old = ring[ring_position];
            ring[ring_position] = sample;
            ring_position += 1;
            if (ring_position == 16) ring_position = 0;
            running_sum += sample - old;
            const mean = @divTrunc(running_sum, 16);
            const controller_error = 2048 - mean;
            integrator += controller_error;
            integrator = clamp(integrator, -8192, 8192);
            var command = controller_error * 4 + signedDiv8(integrator);
            command = clamp(command, -4095, 4095);
            actuator = command;
            accepted += 1;
            status_reg = 1;
        }

        control_reg = actuator + 4095;
        mmio_checksum = @mod(mmio_checksum * 33 + status_reg * 257 + control_reg + received_crc, 1000000007);
        if (watchdog == 64) {
            watchdog = 0;
            watchdog_resets += 1;
        }
    }

    var ring_checksum: i64 = 0;
    for (ring) |sample| ring_checksum = @mod(ring_checksum * 131 + sample, 1000000007);
    var out_buffer: [256]u8 = undefined;
    var out = std.Io.File.Writer.init(.stdout(), init.io, &out_buffer);
    try out.interface.print("ticks {d}\naccepted {d} rejected {d}\nwatchdog_resets {d} faults {d}\nactuator {d}\nmmio_checksum {d}\nring_checksum {d}\n", .{ ticks, accepted, rejected, watchdog_resets, faults, actuator, mmio_checksum, ring_checksum });
    try out.interface.flush();
}
