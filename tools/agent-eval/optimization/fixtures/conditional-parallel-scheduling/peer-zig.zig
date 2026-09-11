const std = @import("std");

pub fn main(init: std.process.Init) !void {
    const first: i64 = 0;
    const last: i64 = 1023 * 1023;
    var buffer: [64]u8 = undefined;
    var out = std.Io.File.Writer.init(.stdout(), init.io, &buffer);
    try out.interface.print("{}:{}\n", .{ first, last });
    try out.interface.flush();
}
