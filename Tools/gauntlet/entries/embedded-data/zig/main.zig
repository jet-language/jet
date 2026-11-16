const std = @import("std");

fn readU8(data: []const u8, at: *usize) !u8 {
    if (at.* >= data.len) return error.UnexpectedEof;
    const value = data[at.*];
    at.* += 1;
    return value;
}

fn readU16(data: []const u8, at: *usize) !u16 {
    const low = try readU8(data, at);
    const high = try readU8(data, at);
    return @as(u16, low) | (@as(u16, high) << 8);
}

fn readU32(data: []const u8, at: *usize) !u32 {
    const byte0 = try readU8(data, at);
    const byte1 = try readU8(data, at);
    const byte2 = try readU8(data, at);
    const byte3 = try readU8(data, at);
    return @as(u32, byte0) |
        (@as(u32, byte1) << 8) |
        (@as(u32, byte2) << 16) |
        (@as(u32, byte3) << 24);
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const args = try init.minimal.args.toSlice(allocator);
    const path = if (args.len > 1) args[1] else "telemetry.bin";
    const data = try std.Io.Dir.cwd().readFileAlloc(init.io, path, allocator, .limited(64 * 1024 * 1024));
    if (data.len < 8 or !std.mem.eql(u8, data[0..4], "EDB1")) return error.BadHeader;

    var checksum: u64 = 0;
    for (data) |byte| checksum += byte;

    var at: usize = 4;
    const count = try readU32(data, &at);
    var valid: u64 = 0;
    var channels = [_]u64{ 0, 0, 0, 0 };
    var sample_min: u64 = 65535;
    var sample_max: u64 = 0;
    var sample_sum: u64 = 0;
    var energy_sum: u64 = 0;
    var load_sum: u64 = 0;
    var tick_sum: u64 = 0;

    var index: u32 = 0;
    while (index < count) : (index += 1) {
        const channel: usize = @intCast(try readU8(data, &at));
        const flags = try readU8(data, &at);
        const sample: u64 = @intCast(try readU16(data, &at));
        const tick: u64 = @intCast(try readU32(data, &at));
        const energy: u64 = @intCast(try readU16(data, &at));
        const load: u64 = @intCast(try readU16(data, &at));
        if (channel >= channels.len) return error.BadChannel;
        if (flags == 0) {
            valid += 1;
            channels[channel] += 1;
            if (sample < sample_min) sample_min = sample;
            if (sample > sample_max) sample_max = sample;
            sample_sum += sample;
            energy_sum += energy;
            load_sum += load;
            tick_sum += tick;
        }
    }

    var output_buffer: [256]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &output_buffer);
    try stdout.interface.print(
        "frames {}\nvalid {}\nchannels {} {} {} {}\nsample {} {} {}\nenergy {}\nload {}\nticks {}\nchecksum {}\n",
        .{ count, valid, channels[0], channels[1], channels[2], channels[3], sample_min, sample_max, sample_sum, energy_sum, load_sum, tick_sum, checksum },
    );
    try stdout.interface.flush();
}
