const std = @import("std");

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const args = try init.minimal.args.toSlice(allocator);
    const path = if (args.len > 1) args[1] else "corpus.txt";
    const text = try std.Io.Dir.cwd().readFileAlloc(init.io, path, allocator, .limited(8 * 1024 * 1024));
    const needle = "struct";
    var matches: usize = 0;
    var offset: usize = 0;
    while (offset < text.len) {
        const relative = std.mem.indexOf(u8, text[offset..], needle) orelse break;
        matches += 1;
        offset += relative + needle.len;
    }

    var output_buffer: [64]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &output_buffer);
    try stdout.interface.print("matches {}\n", .{matches});
    try stdout.interface.flush();
}
