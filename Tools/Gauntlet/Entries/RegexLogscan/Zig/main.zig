const std = @import("std");

// Zig's standard library has no regex engine, so this rail matches
// ^([0-9.]+) - - \[[^]]+\] "GET (/api/[^ ]+) HTTP/1[.]1" 5[0-9][0-9]
// by hand and returns the client IP of a matching line.
fn matchClient(line: []const u8) ?[]const u8 {
    var at: usize = 0;
    while (at < line.len and (std.ascii.isDigit(line[at]) or line[at] == '.')) at += 1;
    if (at == 0) return null;
    const ip = line[0..at];
    var rest = line[at..];
    if (!std.mem.startsWith(u8, rest, " - - [")) return null;
    rest = rest[6..];
    const close = std.mem.indexOfScalar(u8, rest, ']') orelse return null;
    if (close == 0) return null;
    rest = rest[close + 1 ..];
    if (!std.mem.startsWith(u8, rest, " \"GET /api/")) return null;
    rest = rest[11..];
    const space = std.mem.indexOfScalar(u8, rest, ' ') orelse return null;
    if (space == 0) return null;
    rest = rest[space..];
    if (!std.mem.startsWith(u8, rest, " HTTP/1.1\" 5")) return null;
    rest = rest[12..];
    if (rest.len < 3 or !std.ascii.isDigit(rest[0]) or !std.ascii.isDigit(rest[1]) or rest[2] != ' ') return null;
    return ip;
}

const Client = struct {
    ip: []const u8,
    count: u32,

    fn before(_: void, a: Client, b: Client) bool {
        if (a.count != b.count) return a.count > b.count;
        return std.mem.lessThan(u8, a.ip, b.ip);
    }
};

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const args = try init.minimal.args.toSlice(allocator);
    if (args.len < 2) std.process.exit(2);
    const data = try std.Io.Dir.cwd().readFileAlloc(init.io, args[1], allocator, .limited(1 << 30));

    var counts: std.StringHashMapUnmanaged(u32) = .empty;
    var matches: u64 = 0;
    var lines = std.mem.splitScalar(u8, data, '\n');
    while (lines.next()) |line| {
        const ip = matchClient(line) orelse continue;
        const slot = try counts.getOrPut(allocator, ip);
        if (!slot.found_existing) slot.value_ptr.* = 0;
        slot.value_ptr.* += 1;
        matches += 1;
    }

    const ranked = try allocator.alloc(Client, counts.count());
    var entries = counts.iterator();
    var i: usize = 0;
    while (entries.next()) |entry| : (i += 1) ranked[i] = .{ .ip = entry.key_ptr.*, .count = entry.value_ptr.* };
    std.mem.sort(Client, ranked, {}, Client.before);

    var output_buffer: [512]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &output_buffer);
    try stdout.interface.print("matches {d}\n", .{matches});
    for (ranked[0..@min(5, ranked.len)]) |client| try stdout.interface.print("{d} {s}\n", .{ client.count, client.ip });
    try stdout.interface.flush();
}
