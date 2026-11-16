const std = @import("std");

// The groups in output order; each keeps its values in file order.
const names = [_][]const u8{ "alpha", "beta", "delta", "epsilon", "gamma" };

fn summary(out: *std.Io.Writer, allocator: std.mem.Allocator, name: []const u8, values: []const f64) !void {
    const n = values.len;
    var sum: f64 = 0.0;
    for (values) |value| sum += value;
    const mean = sum / @as(f64, @floatFromInt(n));
    const ordered = try allocator.dupe(f64, values);
    std.mem.sort(f64, ordered, {}, std.sort.asc(f64));
    var squares: f64 = 0.0;
    for (values) |value| {
        const d = value - mean;
        squares += d * d;
    }
    const sd = @sqrt(squares / @as(f64, @floatFromInt(n)));
    try out.print("{s} n={d} mean={d:.2} median={d:.2} p95={d:.2} sd={d:.2}\n", .{ name, n, mean, ordered[(n - 1) / 2], ordered[(19 * n + 19) / 20 - 1], sd });
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const args = try init.minimal.args.toSlice(allocator);
    if (args.len < 2) {
        std.debug.print("usage: datasummary FILE\n", .{});
        std.process.exit(2);
    }
    const input = try std.Io.Dir.cwd().readFileAlloc(init.io, args[1], allocator, .limited(64 * 1024 * 1024));
    var groups: [names.len]std.ArrayList(f64) = @splat(.empty);
    var rows = std.mem.splitScalar(u8, input, '\n');
    _ = rows.next();
    while (rows.next()) |line| {
        if (line.len == 0) continue;
        const comma = std.mem.indexOfScalar(u8, line, ',') orelse return error.MissingComma;
        const group = line[0..comma];
        const index = for (names, 0..) |name, i| {
            if (std.mem.eql(u8, name, group)) break i;
        } else return error.UnknownGroup;
        try groups[index].append(allocator, try std.fmt.parseFloat(f64, line[comma + 1 ..]));
    }

    var output_buffer: [1024]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &output_buffer);
    const out = &stdout.interface;
    var total: f64 = 0.0;
    var count: usize = 0;
    for (names, groups) |name, group| {
        for (group.items) |value| total += value;
        count += group.items.len;
        try summary(out, allocator, name, group.items);
    }
    try out.print("overall n={d} mean={d:.2}\n", .{ count, total / @as(f64, @floatFromInt(count)) });
    try out.flush();
}
