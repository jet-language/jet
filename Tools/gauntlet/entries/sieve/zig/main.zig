const std = @import("std");

fn sieve(allocator: std.mem.Allocator, n: usize) ![]u8 {
    const prime = try allocator.alloc(u8, n);
    @memset(prime, 1);
    if (n > 0) prime[0] = 0;
    if (n > 1) prime[1] = 0;
    var p: usize = 3;
    while (p * p < n) : (p += 2) {
        if (prime[p] == 1) {
            var multiple = p * p;
            while (multiple < n) : (multiple += p * 2) {
                prime[multiple] = 0;
            }
        }
    }
    return prime;
}

pub fn main(init: std.process.Init) !void {
    const allocator = std.heap.page_allocator;
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    const n = try std.fmt.parseInt(usize, args[1], 10);
    const prime = try sieve(allocator, n);
    var count: usize = 0;
    var largest: usize = 0;
    if (n > 2) {
        count = 1;
        largest = 2;
        var i: usize = 3;
        while (i < n) : (i += 2) {
            if (prime[i] == 1) {
                count += 1;
                largest = i;
            }
        }
    }
    var out_buffer: [128]u8 = undefined;
    var out = std.Io.File.Writer.init(.stdout(), init.io, &out_buffer);
    try out.interface.print("count {d}\nlargest {d}\n", .{ count, largest });
    try out.interface.flush();
}
