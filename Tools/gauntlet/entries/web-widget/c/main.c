/* Freestanding C compiled to wasm32: the module exports the triangular kernel
 * and runner.mjs prints the series, as the Rust peer's runner does. */
__attribute__((export_name("triangular")))
int triangular(int n) {
    int total = 0;
    for (int i = 1; i <= n; i++) total += i;
    return total;
}
