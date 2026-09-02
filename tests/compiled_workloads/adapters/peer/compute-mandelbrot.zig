// #1414 peer adapter: independent Zig stdlib Mandelbrot implementation.
// Codeberg Zig d03a147ea0a590ca711b3db07106effc559b0fc6 identifies the
// toolchain lineage; this source does not copy an incumbent implementation.
const std = @import("std");
const Config = struct {
    width: i64 = 0,
    height: i64 = 0,
    stride: i64 = 0,
    iterations: i64 = 0,
};

fn reject(out: anytype, label: []const u8, named: bool) !void {
    if (named) try out.print("case={s}\n", .{label});
    try out.print("reject=invalid-number\nsamples=0\nchecksum=0\n", .{});
}

fn escapeCount(cx: f64, cy: f64, maximum: i64) i64 {
    @setFloatMode(.strict);
    var zx: f64 = 0.0;
    var zy: f64 = 0.0;
    var i: i64 = 0;
    while (i < maximum) : (i += 1) {
        const zx_squared = zx * zx;
        const zy_squared = zy * zy;
        const difference = zx_squared - zy_squared;
        const next_x = difference + cx;
        const product = zx * zy;
        const doubled = product * 2.0;
        const next_y = doubled + cy;
        zx = next_x;
        zy = next_y;
        const magnitude_x = zx * zx;
        const magnitude_y = zy * zy;
        const magnitude_squared = magnitude_x + magnitude_y;
        if (magnitude_squared > 4.0) return i + 1;
    }
    return maximum;
}

fn emitCase(
    out: anytype,
    label: []const u8,
    named: bool,
    config: Config,
    parse_failed: bool,
    seen: u8,
) !void {
    @setFloatMode(.strict);
    if (parse_failed or seen != 0b1111) {
        try reject(out, label, named);
        return;
    }
    if (config.width <= 0 or config.width > 16_000 or
        config.height <= 0 or config.height > 16_000 or
        config.stride <= 0 or config.stride > 16_000 or
        config.iterations <= 0 or config.iterations > 64)
    {
        try reject(out, label, named);
        return;
    }
    const samples_x = (config.width + config.stride - 1) / config.stride;
    const samples_y = (config.height + config.stride - 1) / config.stride;
    if (samples_x <= 0 or samples_y <= 0 or samples_x > 25_000_000 / samples_y) {
        try reject(out, label, named);
        return;
    }
    const expected_samples = samples_x * samples_y;
    var samples: i64 = 0;
    var checksum: i64 = 0;
    var y: i64 = 0;
    while (y < config.height) : (y += config.stride) {
        const cy_ratio: f64 = @as(f64, @floatFromInt(y)) /
            @as(f64, @floatFromInt(config.height));
        const cy = cy_ratio * 2.0 - 1.0;
        var x: i64 = 0;
        while (x < config.width) : (x += config.stride) {
            const cx_ratio: f64 = @as(f64, @floatFromInt(x)) /
                @as(f64, @floatFromInt(config.width));
            const cx = cx_ratio * 3.5 - 2.5;
            const count = escapeCount(cx, cy, config.iterations);
            checksum = (checksum + count) % 1_000_000_007;
            samples += 1;
        }
    }
    if (samples != expected_samples) {
        try reject(out, label, named);
        return;
    }
    if (named) try out.print("case={s}\n", .{label});
    try out.print("samples={d}\nchecksum={d}\n", .{ samples, checksum });
}

fn runSingle(out: anytype, raw: []const u8) !void {
    var config = Config{};
    var seen: u8 = 0;
    var parse_failed = false;
    var lines = std.mem.splitScalar(u8, raw, '\n');
    while (lines.next()) |line| {
        const trimmed = std.mem.trim(u8, line, " \t\r");
        if (trimmed.len == 0) continue;
        var fields = std.mem.splitScalar(u8, trimmed, '=');
        const key_raw = fields.next();
        const value_raw = fields.next();
        if (key_raw == null or value_raw == null or fields.next() != null) {
            parse_failed = true;
            continue;
        }
        const key = std.mem.trim(u8, key_raw.?, " \t\r");
        const value = std.mem.trim(u8, value_raw.?, " \t\r");
        const parsed = std.fmt.parseInt(i64, value, 10) catch {
            parse_failed = true;
            continue;
        };
        if (std.mem.eql(u8, key, "width")) {
            if (seen & 1 != 0) parse_failed = true;
            seen |= 1;
            config.width = parsed;
        } else if (std.mem.eql(u8, key, "height")) {
            if (seen & 2 != 0) parse_failed = true;
            seen |= 2;
            config.height = parsed;
        } else if (std.mem.eql(u8, key, "stride")) {
            if (seen & 4 != 0) parse_failed = true;
            seen |= 4;
            config.stride = parsed;
        } else if (std.mem.eql(u8, key, "iterations")) {
            if (seen & 8 != 0) parse_failed = true;
            seen |= 8;
            config.iterations = parsed;
        } else {
            parse_failed = true;
        }
    }
    try emitCase(out, "", false, config, parse_failed, seen);
}

fn runBatch(out: anytype, raw: []const u8) !void {
    var config = Config{};
    var seen: u8 = 0;
    var parse_failed = false;
    var active = false;
    var label: []const u8 = "";
    var lines = std.mem.splitScalar(u8, raw, '\n');
    while (lines.next()) |line| {
        const trimmed = std.mem.trim(u8, line, " \t\r");
        if (trimmed.len == 0 or trimmed[0] == '#') continue;
        var fields = std.mem.splitScalar(u8, trimmed, '=');
        const key_raw = fields.next();
        const value_raw = fields.next();
        if (key_raw == null or value_raw == null or fields.next() != null) {
            parse_failed = true;
            continue;
        }
        const key = std.mem.trim(u8, key_raw.?, " \t\r");
        if (std.mem.eql(u8, key, "case")) {
            if (active) try emitCase(out, label, true, config, parse_failed, seen);
            label = std.mem.trim(u8, value_raw.?, " \t\r");
            config = Config{};
            seen = 0;
            parse_failed = label.len == 0;
            active = true;
            continue;
        }
        if (!active) {
            parse_failed = true;
            continue;
        }
        const value = std.mem.trim(u8, value_raw.?, " \t\r");
        const parsed = std.fmt.parseInt(i64, value, 10) catch {
            parse_failed = true;
            continue;
        };
        if (std.mem.eql(u8, key, "width")) {
            if (seen & 1 != 0) parse_failed = true;
            seen |= 1;
            config.width = parsed;
        } else if (std.mem.eql(u8, key, "height")) {
            if (seen & 2 != 0) parse_failed = true;
            seen |= 2;
            config.height = parsed;
        } else if (std.mem.eql(u8, key, "stride")) {
            if (seen & 4 != 0) parse_failed = true;
            seen |= 4;
            config.stride = parsed;
        } else if (std.mem.eql(u8, key, "iterations")) {
            if (seen & 8 != 0) parse_failed = true;
            seen |= 8;
            config.iterations = parsed;
        } else {
            parse_failed = true;
        }
    }
    if (active) {
        try emitCase(out, label, true, config, parse_failed, seen);
    } else {
        try reject(out, "batch", true);
    }
}

pub fn main(init: std.process.Init) !void {
    @setFloatMode(.strict);
    const allocator = init.gpa;
    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    var stdout_buffer: [256]u8 = undefined;
    var stdout_writer = std.Io.File.stdout().writer(io, &stdout_buffer);
    const out = &stdout_writer.interface;
    defer out.flush() catch {};
    if (args.len < 2) {
        try reject(out, "input", true);
        return;
    }
    const raw = std.Io.Dir.cwd().readFileAlloc(
        io,
        args[1],
        allocator,
        .limited(1024 * 1024),
    ) catch {
        try reject(out, "input", true);
        return;
    };
    defer allocator.free(raw);
    if (std.mem.indexOf(u8, raw, "case=") != null) {
        try runBatch(out, raw);
    } else {
        try runSingle(out, raw);
    }
}
