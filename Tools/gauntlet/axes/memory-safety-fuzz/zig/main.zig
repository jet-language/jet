const std = @import("std");

const record_bytes: usize = 64;
const metadata_bytes: usize = 4;
const payload_bytes: usize = record_bytes - metadata_bytes;
const kind_count: usize = 5;

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const data = try std.Io.Dir.cwd().readFileAlloc(init.io, "fuzz-input.bin", allocator, .limited(1024 * 1024));
    if (data.len == 0 or data.len % record_bytes != 0) return error.InvalidFuzzCorpus;

    var counts: [kind_count]usize = .{ 0, 0, 0, 0, 0 };
    var checksum: u32 = 0;
    var semantic: u32 = 0;
    var offset: usize = 0;
    while (offset < data.len) : (offset += record_bytes) {
        const kind = @as(usize, data[offset]);
        if (kind >= kind_count) return error.InvalidFuzzCase;
        const declared_length = @as(usize, data[offset + 1]);
        const requested_index = @as(usize, data[offset + 2]);
        const bounded_length = @min(declared_length, payload_bytes);
        const safe_index = @min(requested_index, payload_bytes);
        var value: u32 = 0;
        counts[kind] += 1;

        var index: usize = 0;
        while (index < record_bytes) : (index += 1) checksum +%= @as(u32, data[offset + index]);
        if (kind == 0 or kind == 2) {
            index = 0;
            while (index < bounded_length) : (index += 1)
                value +%= @as(u32, data[offset + metadata_bytes + index]);
        } else if (requested_index < payload_bytes) {
            value = @as(u32, data[offset + metadata_bytes + requested_index]);
            if (kind == 4) value ^= 0xa5;
        }
        semantic +%= value;
        semantic +%= @as(u32, @intCast((kind + 1) * 257 + bounded_length + safe_index));
    }

    var output_buffer: [256]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &output_buffer);
    try stdout.interface.print(
        "cases {} valid {} boundary {} oob {} use_after_free {} wrong_output {} bytes {} checksum {} semantic {}\n",
        .{
            data.len / record_bytes,
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            counts[4],
            data.len,
            checksum,
            semantic,
        },
    );
    try stdout.interface.flush();
}
