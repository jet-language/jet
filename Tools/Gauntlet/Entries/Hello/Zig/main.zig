const std = @import("std");

pub fn main(init: std.process.Init) !void {
    var out_buffer: [16]u8 = undefined;
    var out = std.Io.File.Writer.init(.stdout(), init.io, &out_buffer);
    try out.interface.writeAll("hello\n");
    try out.interface.flush();
}
