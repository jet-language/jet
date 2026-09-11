const std = @import("std");

pub fn main(init: std.process.Init) !void {
    const value: i64 = 3;
    const factor: i64 = 4;
    var buffer: [32]u8 = undefined;
    var out = std.Io.File.Writer.init(.stdout(), init.io, &buffer);
    try out.interface.print("{}\n", .{value * factor});
    try out.interface.flush();
}
