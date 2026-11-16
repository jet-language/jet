const std = @import("std");

const Meeting = struct {
    seconds: i64, // UTC seconds since 1970-01-01
    name: []const u8,

    fn before(_: void, a: Meeting, b: Meeting) bool {
        return a.seconds < b.seconds;
    }
};

fn number(text: []const u8) !i64 {
    return std.fmt.parseInt(i64, text, 10);
}

// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's days_from_civil).
fn daysFromCivil(year: i64, month: i64, day: i64) i64 {
    const y = if (month <= 2) year - 1 else year;
    const era = @divFloor(y, 400);
    const year_of_era = y - era * 400;
    const month_prime = if (month > 2) month - 3 else month + 9;
    const day_of_year = @divTrunc(153 * month_prime + 2, 5) + day - 1;
    const day_of_era = year_of_era * 365 + @divTrunc(year_of_era, 4) - @divTrunc(year_of_era, 100) + day_of_year;
    return era * 146097 + day_of_era - 719468;
}

const Civil = struct { year: i64, month: i64, day: i64 };

// The inverse of daysFromCivil.
fn civilFromDays(days: i64) Civil {
    const shifted = days + 719468;
    const era = @divFloor(shifted, 146097);
    const day_of_era = shifted - era * 146097;
    const year_of_era = @divTrunc(day_of_era - @divTrunc(day_of_era, 1460) + @divTrunc(day_of_era, 36524) - @divTrunc(day_of_era, 146096), 365);
    const day_of_year = day_of_era - (365 * year_of_era + @divTrunc(year_of_era, 4) - @divTrunc(year_of_era, 100));
    const month_prime = @divTrunc(5 * day_of_year + 2, 153);
    const day = day_of_year - @divTrunc(153 * month_prime + 2, 5) + 1;
    const month = if (month_prime < 10) month_prime + 3 else month_prime - 9;
    const year = year_of_era + era * 400 + @as(i64, if (month <= 2) 1 else 0);
    return .{ .year = year, .month = month, .day = day };
}

// RFC3339 with Z or a numeric +HH:MM / -HH:MM offset, as UTC seconds.
fn parseRfc3339(text: []const u8) !i64 {
    if (text.len < 20) return error.BadTimestamp;
    const local = daysFromCivil(try number(text[0..4]), try number(text[5..7]), try number(text[8..10])) * 86_400 +
        try number(text[11..13]) * 3_600 + try number(text[14..16]) * 60 + try number(text[17..19]);
    if (text[19] == 'Z') return local;
    if (text.len < 25 or (text[19] != '+' and text[19] != '-')) return error.BadTimestamp;
    const offset = try number(text[20..22]) * 3_600 + try number(text[23..25]) * 60;
    return if (text[19] == '+') local - offset else local + offset;
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const args = try init.minimal.args.toSlice(allocator);
    if (args.len < 2) {
        std.debug.print("usage: tzreport MEETINGS\n", .{});
        std.process.exit(2);
    }
    const input = try std.Io.Dir.cwd().readFileAlloc(init.io, args[1], allocator, .limited(1024 * 1024));
    var meetings: std.ArrayList(Meeting) = .empty;
    var rows = std.mem.splitScalar(u8, input, '\n');
    while (rows.next()) |row| {
        const line = std.mem.trimEnd(u8, row, "\r");
        if (line.len == 0) continue;
        const bar = std.mem.indexOfScalar(u8, line, '|') orelse return error.BadMeetingLine;
        try meetings.append(allocator, .{ .seconds = try parseRfc3339(line[bar + 1 ..]), .name = line[0..bar] });
    }
    // Stable, so equal instants keep file order.
    std.sort.insertion(Meeting, meetings.items, {}, Meeting.before);

    const weekdays = [_][]const u8{ "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun" };
    var output_buffer: [4096]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &output_buffer);
    const out = &stdout.interface;
    for (meetings.items, 0..) |m, i| {
        const days = @divFloor(m.seconds, 86_400);
        const day_seconds: u64 = @intCast(@mod(m.seconds, 86_400));
        const date = civilFromDays(days);
        const year: u64 = @intCast(date.year);
        const month: u64 = @intCast(date.month);
        const day: u64 = @intCast(date.day);
        try out.print("{s} utc={d:0>4}-{d:0>2}-{d:0>2}T{d:0>2}:{d:0>2}:{d:0>2}Z day={s} gap=", .{
            m.name, year, month, day, day_seconds / 3_600, day_seconds / 60 % 60, day_seconds % 60,
            weekdays[@intCast(@mod(days + 3, 7))],
        });
        if (i == 0) {
            try out.writeAll("-\n");
        } else {
            try out.print("{d}\n", .{@divTrunc(m.seconds - meetings.items[i - 1].seconds, 60)});
        }
    }
    const items = meetings.items;
    try out.print("span {d} minutes {d}\n", .{ @divTrunc(items[items.len - 1].seconds - items[0].seconds, 60), items.len });
    try out.flush();
}
