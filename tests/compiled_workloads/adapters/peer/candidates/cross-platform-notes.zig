const std = @import("std");

fn updateSearch(title: []const u8, body: []const u8, query: []const u8, search: *[]const u8) void {
    if (std.mem.indexOf(u8, title, query) != null or std.mem.indexOf(u8, body, query) != null) {
        search.* = "found";
    } else {
        search.* = "not-found";
    }
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.gpa;
    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    if (args.len < 2) return;
    const raw = try std.Io.Dir.cwd().readFileAlloc(io, args[1], allocator, .limited(1024 * 1024));
    defer allocator.free(raw);
    var notes: i64 = 0;
    var focus: []const u8 = "title";
    var search: []const u8 = "not-run";
    var persistence: []const u8 = "not-saved";
    var readonly: []const u8 = "not-run";
    var corrupt: []const u8 = "not-run";
    var active = false;
    var saved = false;
    var read_only_storage = false;
    var unknown = false;
    var query: []const u8 = "";
    var title: []const u8 = "";
    var body: []const u8 = "";
    var lines = std.mem.splitScalar(u8, raw, '\n');
    while (lines.next()) |line| {
        const trimmed = std.mem.trim(u8, line, " \t\r");
        if (trimmed.len == 0) continue;
        const separator = std.mem.indexOfScalar(u8, trimmed, ':') orelse {
            unknown = true;
            continue;
        };
        const key = trimmed[0..separator];
        const value = trimmed[separator + 1..];
        if (std.mem.eql(u8, key, "key")) {
            if (std.mem.eql(u8, value, "add")) {
                notes += 1;
                active = true;
                title = "";
                body = "";
                focus = "title";
            } else if (std.mem.eql(u8, value, "edit")) {
                if (notes > 0) active = true;
                focus = "title";
            } else if (std.mem.eql(u8, value, "search")) {
                updateSearch(title, body, query, &search);
            } else if (std.mem.eql(u8, value, "save")) {
                if (!read_only_storage) {
                    saved = true;
                    persistence = "saved";
                }
            } else if (std.mem.eql(u8, value, "reload")) {
                if (saved) persistence = "reloaded";
            } else if (std.mem.eql(u8, value, "readonly")) {
                read_only_storage = true;
                readonly = "blocked";
            } else if (std.mem.eql(u8, value, "corrupt")) {
                corrupt = "rejected";
            } else {
                unknown = true;
            }
        } else if (std.mem.eql(u8, key, "title")) {
            if (active) {
                title = value;
                if (!std.mem.eql(u8, search, "not-run")) updateSearch(title, body, query, &search);
            }
        } else if (std.mem.eql(u8, key, "body")) {
            if (active) {
                body = value;
                if (!std.mem.eql(u8, search, "not-run")) updateSearch(title, body, query, &search);
            }
        } else if (std.mem.eql(u8, key, "query")) {
            query = value;
            if (!std.mem.eql(u8, search, "not-run")) updateSearch(title, body, query, &search);
        } else {
            unknown = true;
        }
    }
    var buffer: [512]u8 = undefined;
    var writer = std.Io.File.stdout().writer(io, &buffer);
    const out = &writer.interface;
    try out.print("notes={d}\nfocus={s}\nsearch={s}\npersistence={s}\nreadonly={s}\ncorrupt={s}\n", .{ notes, focus, search, persistence, readonly, corrupt });
    if (unknown) try out.print("reject=unknown-key\n", .{});
    try out.flush();
}
