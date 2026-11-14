const std = @import("std");

const Task = struct {
    id: i64,
    text: []const u8,
    done: bool = false,
};

const Store = struct {
    tasks: []Task = &.{},
};

const store_path = "tasks.json";

// A missing or unreadable store is an empty one, as in the other rails.
fn load(io: std.Io, allocator: std.mem.Allocator) Store {
    const bytes = std.Io.Dir.cwd().readFileAlloc(io, store_path, allocator, .limited(16 * 1024 * 1024)) catch return .{};
    return std.json.parseFromSliceLeaky(Store, allocator, bytes, .{ .ignore_unknown_fields = true, .allocate = .alloc_always }) catch .{};
}

fn save(io: std.Io, allocator: std.mem.Allocator, store: Store) !void {
    const bytes = try std.json.Stringify.valueAlloc(allocator, store, .{});
    try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = store_path, .data = bytes });
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const io = init.io;
    const args = try init.minimal.args.toSlice(allocator);
    const command: []const u8 = if (args.len > 1) args[1] else "";
    const argument: ?[]const u8 = if (args.len > 2) args[2] else null;
    var store = load(io, allocator);

    var output_buffer: [4096]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), io, &output_buffer);
    const out = &stdout.interface;
    if (std.mem.eql(u8, command, "add")) {
        const text = argument orelse "";
        var tasks: std.ArrayList(Task) = .fromOwnedSlice(store.tasks);
        const id: i64 = @intCast(tasks.items.len + 1);
        try tasks.append(allocator, .{ .id = id, .text = text });
        store.tasks = tasks.items;
        try save(io, allocator, store);
        try out.print("added {d} {s}\n", .{ id, text });
    } else if (std.mem.eql(u8, command, "done")) {
        const id: i64 = if (argument) |text| std.fmt.parseInt(i64, text, 10) catch -1 else -1;
        for (store.tasks) |*task| {
            if (task.id == id) {
                task.done = true;
                try save(io, allocator, store);
                try out.print("done {d}\n", .{id});
                break;
            }
        } else try out.print("no task {d}\n", .{id});
    } else if (std.mem.eql(u8, command, "list")) {
        var open: usize = 0;
        var done: usize = 0;
        for (store.tasks) |task| {
            try out.print("[{s}] {d} {s}\n", .{ if (task.done) "x" else " ", task.id, task.text });
            if (task.done) done += 1 else open += 1;
        }
        try out.print("open {d} done {d}\n", .{ open, done });
    }
    try out.flush();
}
