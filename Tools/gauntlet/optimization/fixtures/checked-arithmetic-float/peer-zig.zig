const std = @import("std");

pub fn main(init: std.process.Init) !void {
    const left: f64 = (1.0e16 + -1.0e16) + 1.0;
    var buffer: [64]u8 = undefined;
    var out = std.Io.File.Writer.init(.stdout(), init.io, &buffer);
    try out.interface.print("{}\n{}\n", .{ left == 1.0, @as(i64, 123) });
    try out.interface.flush();
}
