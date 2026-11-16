const std = @import("std");
const Io = std.Io;

const workers = 4;
const max_batch = 32;

const Job = struct {
    value: u64,
    results: *Io.Queue(u64),
};

// Takes jobs until the queue closes; each one does the rails' 1ms of work and returns value².
fn worker(io: Io, jobs: *Io.Queue(Job)) void {
    while (jobs.getOneUncancelable(io)) |job| {
        io.sleep(.fromMilliseconds(1), .awake) catch {};
        job.results.putOneUncancelable(io, job.value * job.value) catch return;
    } else |_| {}
}

fn runBatch(io: Io, jobs: *Io.Queue(Job), count: u64) !u64 {
    var result_buffer: [max_batch]u64 = undefined;
    var results: Io.Queue(u64) = .init(&result_buffer);
    for (1..count + 1) |value| try jobs.putOne(io, .{ .value = value, .results = &results });
    var total: u64 = 0;
    for (0..count) |_| total += try results.getOne(io);
    return total;
}

// Answers one connection's command line; returns true when it was `shutdown`.
fn serve(io: Io, stream: Io.net.Stream, jobs: *Io.Queue(Job)) !bool {
    var read_buffer: [256]u8 = undefined;
    var write_buffer: [128]u8 = undefined;
    var reader = stream.reader(io, &read_buffer);
    var writer = stream.writer(io, &write_buffer);
    const out = &writer.interface;
    const line = reader.interface.takeDelimiter('\n') catch null;
    const command = std.mem.trim(u8, line orelse "", &std.ascii.whitespace);
    var stop = false;
    if (std.mem.eql(u8, command, "ready")) {
        try out.writeAll("ready\n");
    } else if (std.mem.eql(u8, command, "shutdown")) {
        try out.writeAll("bye\n");
        stop = true;
    } else if (std.mem.startsWith(u8, command, "batch ")) {
        const count = std.fmt.parseInt(u64, command["batch ".len..], 10) catch 0;
        if (count > 0 and count <= max_batch) {
            try out.print("batch {d} total {d}\n", .{ count, try runBatch(io, jobs, count) });
        } else {
            try out.writeAll("error\n");
        }
    } else {
        try out.writeAll("error\n");
    }
    try out.flush();
    return stop;
}

pub fn main(init: std.process.Init) !void {
    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    if (args.len < 2) {
        std.debug.print("usage: concurrency-service PORT\n", .{});
        std.process.exit(2);
    }
    const address = try Io.net.IpAddress.parseIp4("127.0.0.1", try std.fmt.parseInt(u16, args[1], 10));
    var listener = try address.listen(io, .{ .reuse_address = true });
    defer listener.deinit(io);

    var job_buffer: [max_batch]Job = undefined;
    var jobs: Io.Queue(Job) = .init(&job_buffer);
    var threads: [workers]std.Thread = undefined;
    for (&threads) |*thread| thread.* = try std.Thread.spawn(.{}, worker, .{ io, &jobs });

    var stopping = false;
    while (!stopping) {
        const stream = listener.accept(io) catch break;
        defer stream.close(io);
        stopping = serve(io, stream, &jobs) catch false;
    }
    jobs.close(io);
    for (threads) |thread| thread.join();
}
