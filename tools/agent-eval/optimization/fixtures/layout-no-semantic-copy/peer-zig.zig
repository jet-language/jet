const std = @import("std");

const Pair = extern struct {
    left: u32,
    right: u32,
};

pub fn main(init: std.process.Init) !void {
    const pair = Pair{ .left = 1, .right = 2 };
    var buffer: [32]u8 = undefined;
    var out = std.Io.File.Writer.init(.stdout(), init.io, &buffer);
    try out.interface.print("{}\n", .{pair.left + pair.right});
    try out.interface.flush();
}
