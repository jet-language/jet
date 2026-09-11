const std = @import("std");

const SIDE: usize = 4096;
const ELEMENTS: usize = SIDE * SIDE;

pub fn main(init: std.process.Init) !void {
    const allocator = std.heap.page_allocator;
    const left = try allocator.alloc(f64, ELEMENTS);
    defer allocator.free(left);
    const right = try allocator.alloc(f64, ELEMENTS);
    defer allocator.free(right);
    const mapped = try allocator.alloc(f64, ELEMENTS);
    defer allocator.free(mapped);

    for (left, right) |*a, *b| {
        a.* = 1.0;
        b.* = 2.0;
    }
    for (mapped, left, right) |*out, a, b| {
        out.* = a + b;
    }

    const corner = mapped[ELEMENTS - 1];
    var checksum: f64 = 0.0;
    for (mapped) |value| checksum += value;
    if (corner != 3.0 or checksum != 50331648.0) return error.InvalidResult;
    const checksum_int: u64 = @intFromFloat(checksum);

    var out_buffer: [128]u8 = undefined;
    var out = std.Io.File.Writer.init(.stdout(), init.io, &out_buffer);
    try out.interface.print(
        "shape:[4096, 4096] numel:{} corner:{d:.1} checksum:{}\n",
        .{ ELEMENTS, corner, checksum_int },
    );
    try out.interface.flush();
}
