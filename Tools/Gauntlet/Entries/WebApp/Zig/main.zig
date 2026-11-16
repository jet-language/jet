// Freestanding Zig compiled to wasm32. The module owns the counter and
// renders its display text into linear memory; runner.mjs mounts the DOM and
// binds the buttons to these exports, as a browser host page would.
const std = @import("std");

var count: u32 = 0;
var text: [32]u8 = undefined;

// Writes `count: <count>` to text and returns its length.
fn render() usize {
    return (std.fmt.bufPrint(&text, "count: {d}", .{count}) catch unreachable).len;
}

export fn label() [*]const u8 {
    return &text;
}

export fn mount() usize {
    count = 0;
    return render();
}

export fn increment() usize {
    count += 1;
    return render();
}

export fn reset() usize {
    count = 0;
    return render();
}
