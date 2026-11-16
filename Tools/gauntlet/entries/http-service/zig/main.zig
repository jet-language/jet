const std = @import("std");

const json_header = [_]std.http.Header{.{ .name = "Content-Type", .value = "application/json" }};

fn reply(request: *std.http.Server.Request, status: std.http.Status, body: []const u8) !void {
    try request.respond(body, .{ .status = status, .keep_alive = false, .extra_headers = &json_header });
}

// Answers one request; returns true when it was GET /shutdown. Stored pairs live in `gpa`;
// reply text lives in `scratch`, which the caller frees after the request.
fn handle(gpa: std.mem.Allocator, scratch: std.mem.Allocator, values: *std.StringHashMapUnmanaged([]const u8), request: *std.http.Server.Request) !bool {
    const method = request.head.method;
    const target = request.head.target;
    const path = target[0 .. std.mem.indexOfScalar(u8, target, '?') orelse target.len];
    const key: ?[]const u8 = if (std.mem.startsWith(u8, path, "/kv/")) path[4..] else null;
    if (method == .GET and std.mem.eql(u8, path, "/health")) {
        try reply(request, .ok, "{\"status\":\"ok\"}");
    } else if (method == .GET and std.mem.eql(u8, path, "/shutdown")) {
        try reply(request, .ok, "{\"bye\":true}");
        return true;
    } else if (method == .PUT and key != null) {
        // The body reader reuses the head's memory, so the key is copied out first.
        const owned_key = try scratch.dupe(u8, key.?);
        var body_buffer: [4096]u8 = undefined;
        const value = try request.readerExpectNone(&body_buffer).allocRemaining(gpa, .unlimited);
        const slot = try values.getOrPut(gpa, owned_key);
        if (slot.found_existing) {
            gpa.free(slot.value_ptr.*);
        } else {
            slot.key_ptr.* = try gpa.dupe(u8, owned_key);
        }
        slot.value_ptr.* = value;
        try reply(request, .ok, try std.fmt.allocPrint(scratch, "{{\"stored\":\"{s}\"}}", .{owned_key}));
    } else if (method == .GET and key != null and values.get(key.?) != null) {
        try reply(request, .ok, try std.fmt.allocPrint(scratch, "{{\"key\":\"{s}\",\"value\":\"{s}\"}}", .{ key.?, values.get(key.?).? }));
    } else {
        try reply(request, .not_found, "{\"error\":\"not found\"}");
    }
    return false;
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.gpa;
    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    if (args.len < 2) {
        std.debug.print("usage: http-service PORT\n", .{});
        std.process.exit(2);
    }
    const address = try std.Io.net.IpAddress.parseIp4("127.0.0.1", try std.fmt.parseInt(u16, args[1], 10));
    var listener = try address.listen(io, .{ .reuse_address = true });
    defer listener.deinit(io);
    var values: std.StringHashMapUnmanaged([]const u8) = .empty;

    var stopping = false;
    while (!stopping) {
        const stream = try listener.accept(io);
        defer stream.close(io);
        var read_buffer: [8192]u8 = undefined;
        var write_buffer: [4096]u8 = undefined;
        var reader = stream.reader(io, &read_buffer);
        var writer = stream.writer(io, &write_buffer);
        var server = std.http.Server.init(&reader.interface, &writer.interface);
        var request = server.receiveHead() catch continue;
        var scratch: std.heap.ArenaAllocator = .init(allocator);
        defer scratch.deinit();
        stopping = try handle(allocator, scratch.allocator(), &values, &request);
    }
}
