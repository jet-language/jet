const std = @import("std");

pub fn main(init: std.process.Init) !void {
    const values = [_]i64{ 1, 2, 3, 4 };
    var total: i64 = 0;
    for (values) |value| total += value * 2;
    var buffer: [32]u8 = undefined;
    var out = std.Io.File.Writer.init(.stdout(), init.io, &buffer);
    try out.interface.print("{}\n", .{total});
    try out.interface.flush();
}
