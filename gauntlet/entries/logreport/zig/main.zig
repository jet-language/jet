const std = @import("std");

const levels = [_][]const u8{ "DEBUG", "INFO", "WARN", "ERROR" };
const components = [_][]const u8{ "api", "auth", "cache", "db", "jobs", "mailer", "payments", "queue", "search", "storage", "worker", "web" };

fn parseTimestamp(text: []const u8) i64 {
    const digit = struct {
        fn get(value: []const u8, start: usize, length: usize) i64 {
            var result: i64 = 0;
            for (value[start .. start + length]) |byte| result = result * 10 + @as(i64, byte - '0');
            return result;
        }
    }.get;
    const year = digit(text, 0, 4);
    const month = digit(text, 5, 2);
    const day = digit(text, 8, 2);
    const hour = digit(text, 11, 2);
    const minute = digit(text, 14, 2);
    const second = digit(text, 17, 2);
    const days_before_year = 365 * (year - 1970) + @divTrunc(year - 1969, 4) - @divTrunc(year - 1901, 100) + @divTrunc(year - 1601, 400);
    const month_days = [_]i64{ 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334 };
    const leap = @mod(year, 4) == 0 and (@mod(year, 100) != 0 or @mod(year, 400) == 0);
    const leap_day: i64 = if (leap and month > 2) 1 else 0;
    const days = days_before_year + month_days[@intCast(month - 1)] + day - 1 + leap_day;
    return days * 86_400 + hour * 3_600 + minute * 60 + second;
}

fn indexOf(values: []const []const u8, wanted: []const u8) usize {
    for (values, 0..) |value, index| if (std.mem.eql(u8, value, wanted)) return index;
    return 0;
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const args = try init.minimal.args.toSlice(allocator);
    const path = if (args.len > 1) args[1] else "app.log";
    var file = try std.Io.Dir.cwd().openFile(init.io, path, .{});
    defer file.close(init.io);
    var read_buffer: [64 * 1024]u8 = undefined;
    var reader = file.readerStreaming(init.io, &read_buffer);
    var level_counts = [_]i64{ 0, 0, 0, 0 };
    var error_counts = [_]i64{ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 };
    var first_text: []const u8 = "";
    var last_text: []const u8 = "";
    var first_seconds: i64 = 0;
    var last_seconds: i64 = 0;
    var total: i64 = 0;

    while (true) {
        const raw_line = reader.interface.takeDelimiterExclusive('\n') catch |err| switch (err) {
            error.EndOfStream => break,
            else => return err,
        };
        const line = std.mem.trimEnd(u8, raw_line, "\r");
        var fields = std.mem.splitScalar(u8, line, ' ');
        const timestamp_text = fields.next() orelse continue;
        const level = fields.next() orelse continue;
        const component = fields.next() orelse continue;
        const timestamp = parseTimestamp(timestamp_text);
        if (total == 0) {
            first_text = try allocator.dupe(u8, timestamp_text);
            first_seconds = timestamp;
        }
        last_text = try allocator.dupe(u8, timestamp_text);
        last_seconds = timestamp;
        level_counts[indexOf(&levels, level)] += 1;
        if (std.mem.eql(u8, level, "ERROR")) error_counts[indexOf(&components, component)] += 1;
        total += 1;
    }

    var output_buffer: [1024]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &output_buffer);
    for (levels, 0..) |level, index| try stdout.interface.print("{s} {}\n", .{ level, level_counts[index] });
    try stdout.interface.print("top-error-components:\n", .{});
    var selected = [_]bool{ false, false, false, false, false, false, false, false, false, false, false, false };
    for (0..3) |_| {
        var best_index: usize = 0;
        var best_count: i64 = -1;
        for (components, 0..) |component, index| {
            if (!selected[index] and error_counts[index] > best_count) {
                best_index = index;
                best_count = error_counts[index];
            }
            _ = component;
        }
        selected[best_index] = true;
        try stdout.interface.print("{} {s}\n", .{ best_count, components[best_index] });
    }
    try stdout.interface.print("span {s} .. {s} ({}s)\n", .{ first_text, last_text, last_seconds - first_seconds });
    try stdout.interface.flush();
}
