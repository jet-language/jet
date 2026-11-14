const std = @import("std");

const Item = struct {
    id: i64,
    name: []const u8,
    qty: i64,
};

const ItemList = struct {
    items: []Item,
};

const Response = struct {
    status: std.http.Status,
    body: []const u8,
};

fn get(client: *std.http.Client, allocator: std.mem.Allocator, url: []const u8) !Response {
    var body: std.Io.Writer.Allocating = .init(allocator);
    const result = try client.fetch(.{ .location = .{ .url = url }, .response_writer = &body.writer });
    return .{ .status = result.status, .body = body.written() };
}

fn parse(comptime T: type, allocator: std.mem.Allocator, body: []const u8) !T {
    return std.json.parseFromSliceLeaky(T, allocator, body, .{ .ignore_unknown_fields = true, .allocate = .alloc_always });
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const args = try init.minimal.args.toSlice(allocator);
    const base: []const u8 = if (args.len > 1) args[1] else "http://127.0.0.1:18400";
    var client: std.http.Client = .{ .allocator = allocator, .io = init.io };
    defer client.deinit();

    var output_buffer: [1024]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &output_buffer);
    const out = &stdout.interface;

    const listing_response = try get(&client, allocator, try std.fmt.allocPrint(allocator, "{s}/items", .{base}));
    if (listing_response.status != .ok) return error.ListingRequestFailed;
    const listing = try parse(ItemList, allocator, listing_response.body);
    try out.print("items {d}\n", .{listing.items.len});
    for ([_]u32{ 2, 5, 99 }) |id| {
        const response = try get(&client, allocator, try std.fmt.allocPrint(allocator, "{s}/items/{d}", .{ base, id }));
        if (response.status == .not_found) {
            try out.print("item {d} missing\n", .{id});
        } else {
            const item = try parse(Item, allocator, response.body);
            try out.print("item {d} {s} qty={d}\n", .{ item.id, item.name, item.qty });
        }
    }
    var total: i64 = 0;
    for (listing.items) |item| total += item.qty;
    try out.print("total-qty {d}\n", .{total});
    try out.flush();
}
