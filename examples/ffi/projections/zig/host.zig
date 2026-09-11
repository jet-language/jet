const projection = @import("target/bindings/projection_zig.zig");

pub fn main() !void {
    const initial = [_]u8{ 10, 20, 30 };
    const replacement = [_]u8{ 40, 50, 60 };
    var document = projection.ResourceDocument{};
    if (document.open(initial[0..]) != projection.RESOURCE_OK) return error.OpenFailed;
    var stale: projection.ResourceView = undefined;
    if (document.bytes(&stale) != projection.RESOURCE_OK) return error.BytesFailed;
    var value: i64 = 0;
    if (document.at(&stale, 1, &value) != projection.RESOURCE_OK or value != 20) return error.ReadFailed;
    if (document.replace(replacement[0..]) != projection.RESOURCE_OK) return error.ReplaceFailed;
    if (document.at(&stale, 1, &value) != projection.RESOURCE_EXPIRED_VIEW) return error.StaleView;
    var fresh: projection.ResourceView = undefined;
    if (document.bytes(&fresh) != projection.RESOURCE_OK) return error.BytesFailed;
    if (document.at(&fresh, 1, &value) != projection.RESOURCE_OK or value != 50) return error.ReadFailed;
    if (document.close() != projection.RESOURCE_OK) return error.CloseFailed;
    if (document.close() != projection.RESOURCE_CLOSED) return error.DoubleClose;
    if (projection.add(41) != 42) return error.UnexpectedResult;
}
