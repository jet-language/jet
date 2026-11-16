const std = @import("std");

fn byBytes(_: void, a: []const u8, b: []const u8) bool {
    return std.mem.lessThan(u8, a, b);
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const args = try init.minimal.args.toSlice(allocator);
    const path = if (args.len > 1) args[1] else "notes.txt";
    const input = try std.Io.Dir.cwd().readFileAlloc(init.io, path, allocator, .limited(64 * 1024 * 1024));

    var lines: std.ArrayList([]const u8) = .empty;
    var rows = std.mem.splitScalar(u8, input, '\n');
    while (rows.next()) |row| {
        const line = std.mem.trim(u8, row, &std.ascii.whitespace);
        if (line.len != 0) try lines.append(allocator, line);
    }
    std.mem.sort([]const u8, lines.items, {}, byBytes);

    var output_buffer: [4096]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &output_buffer);
    const out = &stdout.interface;
    try out.print("lines {d}\n", .{lines.items.len});
    for (lines.items) |line| try out.print("{s}\n", .{line});
    try out.flush();
}
