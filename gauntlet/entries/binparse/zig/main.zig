const std = @import("std");

pub fn main() !void {
    var args = std.process.args(); defer args.deinit();
    _ = args.next();
    const path = args.next() orelse "records.bin";
    const file = try std.fs.cwd().openFile(path, .{}); defer file.close();
    var reader = file.reader();
    var magic: [4]u8 = undefined; try reader.readNoEof(&magic);
    if (!std.mem.eql(u8, &magic, "JGB1")) return error.BadMagic;
    const count = try reader.readInt(u32, .little);
    var sum: f64 = 0.0;
    var hash: u64 = 0xcbf29ce484222325;
    var name: [24]u8 = undefined;
    var i: u32 = 0;
    while (i < count) : (i += 1) {
        const id = try reader.readInt(u32, .little);
        const value = @bitCast(try reader.readInt(u64, .little));
        const name_len = try reader.readInt(u16, .little);
        if (id % 7 == 0) sum += value;
        try reader.readNoEof(name[0..name_len]);
        for (name[0..name_len]) |byte| hash = (hash ^ byte) *% 0x100000001b3;
    }
    var out = std.io.getStdOut().writer();
    try out.print("records {d}\nsum7 {d:.6}\nfnv {x:0>16}\n", .{ count, sum, hash });
}
