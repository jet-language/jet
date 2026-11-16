const std = @import("std");

fn readAll(io: std.Io, allocator: std.mem.Allocator, file: std.Io.File) ![]u8 {
    var buffer: [4096]u8 = undefined;
    var reader = file.readerStreaming(io, &buffer);
    return reader.interface.allocRemaining(allocator, .unlimited);
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const io = init.io;
    var output_buffer: [1024]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), io, &output_buffer);
    const out = &stdout.interface;

    // calc: capture a child's stdout.
    const calc = try std.process.run(allocator, io, .{ .argv = &.{ "python3", "-c", "print(6*7)" } });
    try out.print("calc {s}\n", .{std.mem.trim(u8, calc.stdout, &std.ascii.whitespace)});

    // sorted: python's lines piped through sort.
    var source = try std.process.spawn(io, .{ .argv = &.{ "python3", "-c", "print('b');print('a');print('c')" }, .stdout = .pipe });
    var sorter = try std.process.spawn(io, .{ .argv = &.{"sort"}, .stdin = .{ .file = source.stdout.? }, .stdout = .pipe });
    source.stdout.?.close(io);
    source.stdout = null;
    const sorted = try readAll(io, allocator, sorter.stdout.?);
    _ = try source.wait(io);
    _ = try sorter.wait(io);
    try out.writeAll("sorted ");
    var lines = std.mem.tokenizeScalar(u8, std.mem.trim(u8, sorted, &std.ascii.whitespace), '\n');
    var first = true;
    while (lines.next()) |line| : (first = false) try out.print("{s}{s}", .{ if (first) "" else ",", line });
    try out.writeAll("\n");

    // slow: a child that outlives a 300ms deadline is killed.
    const deadline: std.Io.Timeout = .{ .duration = .{ .raw = .fromMilliseconds(300), .clock = .awake } };
    if (std.process.run(allocator, io, .{ .argv = &.{ "python3", "-c", "import time;time.sleep(5)" }, .timeout = deadline })) |_| {} else |err| switch (err) {
        error.Timeout => try out.writeAll("slow timeout\n"),
        else => |e| return e,
    }

    // exit: a checked run observes the child's exit code.
    const checked = try std.process.run(allocator, io, .{ .argv = &.{ "python3", "-c", "import sys;sys.exit(3)" } });
    switch (checked.term) {
        .exited => |code| if (code != 0) try out.print("exit {d}\n", .{code}),
        else => {},
    }
    try out.flush();
}
