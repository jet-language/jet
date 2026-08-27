const std = @import("std");

fn readU16(data: []const u8, at: *usize) !u16 {
    if (data.len - at.* < 2) return error.UnexpectedEof;
    const value = @as(u16, data[at.*]) | (@as(u16, data[at.* + 1]) << 8);
    at.* += 2;
    return value;
}

fn readU32(data: []const u8, at: *usize) !u32 {
    if (data.len - at.* < 4) return error.UnexpectedEof;
    const value = @as(u32, data[at.*]) |
        (@as(u32, data[at.* + 1]) << 8) |
        (@as(u32, data[at.* + 2]) << 16) |
        (@as(u32, data[at.* + 3]) << 24);
    at.* += 4;
    return value;
}

fn readF64(data: []const u8, at: *usize) !f64 {
    if (data.len - at.* < 8) return error.UnexpectedEof;
    var bits: u64 = 0;
    for (0..8) |shift| bits |= @as(u64, data[at.* + shift]) << @intCast(shift * 8);
    at.* += 8;
    const value: f64 = @bitCast(bits);
    return value;
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const args = try init.minimal.args.toSlice(allocator);
    const path = if (args.len > 1) args[1] else "records.bin";
    const data = try std.Io.Dir.cwd().readFileAlloc(init.io, path, allocator, .limited(64 * 1024 * 1024));
    if (data.len < 8 or !std.mem.eql(u8, data[0..4], "JGB1")) return error.BadHeader;
    var at: usize = 4;
    const count = try readU32(data, &at);
    var sum: f64 = 0.0;
    var hash: u64 = 0xcbf29ce484222325;
    var i: u32 = 0;
    while (i < count) : (i += 1) {
        const id = try readU32(data, &at);
        const value = try readF64(data, &at);
        const name_len = try readU16(data, &at);
        if (data.len - at < name_len) return error.UnexpectedEof;
        if (id % 7 == 0) sum += value;
        for (data[at..][0..name_len]) |byte| hash = (hash ^ byte) *% 0x100000001b3;
        at += name_len;
    }
    var output_buffer: [128]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &output_buffer);
    try stdout.interface.print("records {}\nsum7 {d:.6}\nfnv {x:0>16}\n", .{ count, sum, hash });
    try stdout.interface.flush();
}
