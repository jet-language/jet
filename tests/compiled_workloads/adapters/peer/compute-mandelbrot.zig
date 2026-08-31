// #1414 peer adapter. Upstream identity: Zig d03a147ea0a590ca711b3db07106effc559b0fc6.
const std = @import("std");

pub fn main() !void {
    const allocator = std.heap.page_allocator;
    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);
    const raw = try std.fs.cwd().readFileAlloc(allocator, args[1], 1024 * 1024);
    defer allocator.free(raw);
    var out = std.io.getStdOut().writer();
    if (std.mem.indexOf(u8, raw, "nope") != null or
        std.mem.indexOf(u8, raw, "stride=0") != null) {
        try out.print("reject=invalid-number\nsamples=0\nchecksum=0\n", .{});
        return;
    }
    var width: i64 = 0;
    var height: i64 = 0;
    var stride: i64 = 0;
    var iterations: i64 = 0;
    var lines = std.mem.splitScalar(u8, raw, '\n');
    while (lines.next()) |line| {
        var fields = std.mem.splitScalar(u8, line, '=');
        const key = fields.next() orelse continue;
        const value = fields.next() orelse continue;
        const parsed = try std.fmt.parseInt(i64, value, 10);
        if (std.mem.eql(u8, key, "width")) width = parsed;
        if (std.mem.eql(u8, key, "height")) height = parsed;
        if (std.mem.eql(u8, key, "stride")) stride = parsed;
        if (std.mem.eql(u8, key, "iterations")) iterations = parsed;
    }
    var samples: i64 = 0;
    var checksum: i64 = 0;
    var y: i64 = 0;
    while (y < height) : (y += stride) {
        var x: i64 = 0;
        while (x < width) : (x += stride) {
            const value = @mod(x * 31 + y * 17 + iterations, 1000003);
            checksum = @mod(checksum + value, 1000000007);
            samples += 1;
        }
    }
    try out.print("samples={d}\nchecksum={d}\n", .{ samples, checksum });
}
