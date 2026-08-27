const std = @import("std");

const Entry = struct { word: []const u8, count: usize };

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const args = try init.minimal.args.toSlice(allocator);
    const input = try std.Io.Dir.cwd().readFileAlloc(init.io, args[1], allocator, .limited(64 * 1024 * 1024));
    var counts = std.StringHashMap(usize).init(allocator);
    var words = std.ArrayList(Entry).empty;
    var tokens = std.mem.tokenizeAny(u8, input, " \n\t\r\x0c\x0b");
    var total: usize = 0;
    while (tokens.next()) |word| {
        const result = try counts.getOrPut(word);
        if (!result.found_existing) result.value_ptr.* = 0;
        result.value_ptr.* += 1;
        total += 1;
    }
    var iterator = counts.iterator();
    while (iterator.next()) |item| try words.append(allocator, .{ .word = item.key_ptr.*, .count = item.value_ptr.* });
    std.mem.sort(Entry, words.items, {}, struct {
        fn lessThan(_: void, left: Entry, right: Entry) bool {
            if (left.count != right.count) return left.count > right.count;
            return std.mem.order(u8, left.word, right.word) == .lt;
        }
    }.lessThan);
    var stdout_buffer: [1024]u8 = undefined;
    var stdout_file_writer: std.Io.File.Writer = .init(.stdout(), init.io, &stdout_buffer);
    const output = &stdout_file_writer.interface;
    for (words.items[0..20]) |entry| try output.print("{} {s}\n", .{ entry.count, entry.word });
    try output.print("distinct {} total {}\n", .{ words.items.len, total });
    try output.flush();
}
