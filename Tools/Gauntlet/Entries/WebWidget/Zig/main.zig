// Freestanding Zig compiled to wasm32: the module exports the triangular
// kernel and runner.mjs prints the series, as the Rust peer's runner does.
export fn triangular(n: i32) i32 {
    var total: i32 = 0;
    var i: i32 = 1;
    while (i <= n) : (i += 1) total += i;
    return total;
}
