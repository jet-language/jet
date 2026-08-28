const std = @import("std");

const Group = struct {
    name: []const u8,
    count: usize,
    sum: f64,
};

fn nextField(line: []const u8, at: *usize) ?[]const u8 {
    if (at.* > line.len) return null;
    if (at.* == line.len) {
        at.* += 1;
        return line[line.len..];
    }
    const start = at.*;
    if (line[start] == '"') {
        const value_start = start + 1;
        var index = value_start;
        while (index < line.len) : (index += 1) {
            if (line[index] != '"') continue;
            if (index + 1 < line.len and line[index + 1] == '"') {
                index += 1;
                continue;
            }
            const end = index;
            index += 1;
            if (index < line.len and line[index] == ',') {
                index += 1;
            } else if (index < line.len) {
                return null;
            }
            at.* = index;
            return line[value_start..end];
        }
        return null;
    }
    var index = start;
    while (index < line.len and line[index] != ',') : (index += 1) {}
    at.* = if (index < line.len) index + 1 else index;
    return line[start..index];
}

fn addGroup(groups: *std.ArrayList(Group), allocator: std.mem.Allocator, name: []const u8, amount: f64) !void {
    for (groups.items) |*group| {
        if (std.mem.eql(u8, group.name, name)) {
            group.count += 1;
            group.sum += amount;
            return;
        }
    }
    try groups.append(allocator, .{ .name = name, .count = 1, .sum = amount });
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const args = try init.minimal.args.toSlice(allocator);
    const path = if (args.len > 1) args[1] else "sales.csv";
    const input = try std.Io.Dir.cwd().readFileAlloc(init.io, path, allocator, .limited(64 * 1024 * 1024));
    var groups = std.ArrayList(Group).empty;
    var total_count: usize = 0;
    var total_sum: f64 = 0.0;
    var rows = std.mem.splitScalar(u8, input, '\n');
    _ = rows.next();
    while (rows.next()) |raw_line| {
        const line = std.mem.trimEnd(u8, raw_line, "\r");
        if (line.len == 0) continue;
        var at: usize = 0;
        _ = nextField(line, &at) orelse continue;
        const region = nextField(line, &at) orelse continue;
        _ = nextField(line, &at) orelse continue;
        const amount_text = nextField(line, &at) orelse continue;
        const amount = std.fmt.parseFloat(f64, amount_text) catch continue;
        if (amount <= 0.0) continue;
        try addGroup(&groups, allocator, region, amount);
        total_count += 1;
        total_sum += amount;
    }
    std.mem.sort(Group, groups.items, {}, struct {
        fn lessThan(_: void, left: Group, right: Group) bool {
            return std.mem.order(u8, left.name, right.name) == .lt;
        }
    }.lessThan);

    var output_buffer: [1024]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &output_buffer);
    for (groups.items) |group| try stdout.interface.print("{s} n={} sum={d:.2}\n", .{ group.name, group.count, group.sum });
    try stdout.interface.print("total n={} sum={d:.2}\n", .{ total_count, total_sum });
    try stdout.interface.flush();
}
