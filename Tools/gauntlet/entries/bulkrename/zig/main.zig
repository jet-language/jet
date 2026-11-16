const std = @import("std");

const Photo = struct {
    number: u32,
    name: []const u8,

    // Numeric suffix order; equal numbers (IMG_7 and IMG_07) keep name order.
    fn before(_: void, a: Photo, b: Photo) bool {
        if (a.number != b.number) return a.number < b.number;
        return std.mem.lessThan(u8, a.name, b.name);
    }
};

// The number of IMG_<digits>.jpeg, or null when the name is not one or overflows u32.
fn photoNumber(name: []const u8) ?u32 {
    if (name.len < 10 or !std.mem.startsWith(u8, name, "IMG_") or !std.mem.endsWith(u8, name, ".jpeg")) return null;
    const raw = name[4 .. name.len - 5];
    for (raw) |c| if (!std.ascii.isDigit(c)) return null;
    return std.fmt.parseInt(u32, raw, 10) catch null;
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const io = init.io;
    const args = try init.minimal.args.toSlice(allocator);
    if (args.len < 2) {
        std.debug.print("usage: bulkrename DIR\n", .{});
        std.process.exit(2);
    }
    var dir = try std.Io.Dir.cwd().openDir(io, args[1], .{ .iterate = true });
    defer dir.close(io);

    var photos: std.ArrayList(Photo) = .empty;
    var files: usize = 0;
    var entries = dir.iterate();
    while (try entries.next(io)) |entry| {
        // statFile follows a symlink, so a link to a file counts as a file, as in the other rails.
        const info = dir.statFile(io, entry.name, .{}) catch continue;
        if (info.kind != .file) continue;
        files += 1;
        const number = photoNumber(entry.name) orelse continue;
        try photos.append(allocator, .{ .number = number, .name = try allocator.dupe(u8, entry.name) });
    }
    std.mem.sort(Photo, photos.items, {}, Photo.before);

    var output_buffer: [4096]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), io, &output_buffer);
    for (photos.items, 1..) |photo, index| {
        var target_buffer: [32]u8 = undefined;
        const target = try std.fmt.bufPrint(&target_buffer, "photo-{d:0>4}.jpg", .{index});
        try dir.rename(photo.name, dir, target, io);
        try stdout.interface.print("renamed {s} -> {s}\n", .{ photo.name, target });
    }
    try stdout.interface.print("renamed {d} skipped {d}\n", .{ photos.items.len, files - photos.items.len });
    try stdout.interface.flush();
}
