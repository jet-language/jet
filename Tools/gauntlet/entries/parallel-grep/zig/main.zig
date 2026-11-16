const std = @import("std");

const Scan = struct {
    io: std.Io,
    paths: []const []const u8,
    counts: []u64,
    needle: []const u8,
    next: std.atomic.Value(usize) = .init(0),

    // Non-overlapping needle occurrences, counted line by line so a match never spans a newline.
    fn countText(needle: []const u8, text: []const u8) u64 {
        var count: u64 = 0;
        var lines = std.mem.splitScalar(u8, text, '\n');
        while (lines.next()) |line| count += std.mem.count(u8, line, needle);
        return count;
    }

    fn worker(scan: *Scan) void {
        const allocator = std.heap.smp_allocator;
        while (true) {
            const index = scan.next.fetchAdd(1, .monotonic);
            if (index >= scan.paths.len) return;
            const text = std.Io.Dir.cwd().readFileAlloc(scan.io, scan.paths[index], allocator, .unlimited) catch |err| {
                std.debug.print("cannot read {s}: {t}\n", .{ scan.paths[index], err });
                std.process.exit(1);
            };
            defer allocator.free(text);
            scan.counts[index] = countText(scan.needle, text);
        }
    }
};

fn byPath(_: void, a: []const u8, b: []const u8) bool {
    return std.mem.lessThan(u8, a, b);
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const io = init.io;
    const args = try init.minimal.args.toSlice(allocator);
    const root_arg: []const u8 = if (args.len > 1) args[1] else "files";
    const needle: []const u8 = if (args.len > 2) args[2] else "needle-7f";
    if (needle.len == 0) return error.EmptyNeedle;
    const root = if (root_arg.len > 1) std.mem.trimEnd(u8, root_arg, "/") else root_arg;

    var dir = try std.Io.Dir.cwd().openDir(io, root, .{ .iterate = true });
    defer dir.close(io);
    var walker = try dir.walk(allocator);
    defer walker.deinit();
    var paths: std.ArrayList([]const u8) = .empty;
    while (try walker.next(io)) |entry| {
        if (entry.kind == .file and std.mem.endsWith(u8, entry.basename, ".txt")) {
            try paths.append(allocator, try std.fmt.allocPrint(allocator, "{s}/{s}", .{ root, entry.path }));
        }
    }
    std.mem.sort([]const u8, paths.items, {}, byPath);

    var scan: Scan = .{ .io = io, .paths = paths.items, .counts = try allocator.alloc(u64, paths.items.len), .needle = needle };
    const threads = try allocator.alloc(std.Thread, @max(std.Thread.getCpuCount() catch 1, 1));
    for (threads) |*thread| thread.* = try std.Thread.spawn(.{}, Scan.worker, .{&scan});
    for (threads) |thread| thread.join();

    var output_buffer: [4096]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), io, &output_buffer);
    const out = &stdout.interface;
    var matched: usize = 0;
    var total: u64 = 0;
    for (paths.items, scan.counts) |path, count| {
        if (count == 0) continue;
        try out.print("{s}:{d}\n", .{ path, count });
        matched += 1;
        total += count;
    }
    try out.print("files {d}/{d} total {d}\n", .{ matched, paths.items.len, total });
    try out.flush();
}
