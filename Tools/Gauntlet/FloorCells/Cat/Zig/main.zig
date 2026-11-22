const std = @import("std");

pub fn main(init: std.process.Init) !void {
    var in_buf: [65536]u8 = undefined;
    var out_buf: [65536]u8 = undefined;
    var reader = std.Io.File.Reader.init(.stdin(), init.io, &in_buf);
    var writer = std.Io.File.Writer.init(.stdout(), init.io, &out_buf);
    const inp = &reader.interface;
    const out = &writer.interface;

    while (try inp.takeDelimiter('\n')) |raw| {
        const line = if (raw.len > 0 and raw[raw.len - 1] == '\r') raw[0 .. raw.len - 1] else raw;
        try out.writeAll(line);
        try out.writeByte('\n');
    }
    try out.flush();
}
