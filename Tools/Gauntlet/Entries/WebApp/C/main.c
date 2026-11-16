/* Freestanding C compiled to wasm32. The module owns the counter and renders
 * its display text into linear memory; runner.mjs mounts the DOM and binds
 * the buttons to these exports, as a browser host page would. */
static unsigned count;
static char label[32];

/* Writes `count: <count>` to label and returns its length. */
static int render(void) {
    __builtin_memcpy(label, "count: ", 7);
    char digits[10];
    int used = 0;
    unsigned value = count;
    do {
        digits[used++] = (char)('0' + value % 10);
        value /= 10;
    } while (value != 0);
    int length = 7;
    while (used > 0) label[length++] = digits[--used];
    return length;
}

__attribute__((export_name("label"))) const char *label_address(void) { return label; }
__attribute__((export_name("mount"))) int mount(void) { count = 0; return render(); }
__attribute__((export_name("increment"))) int increment(void) { count += 1; return render(); }
__attribute__((export_name("reset"))) int reset(void) { count = 0; return render(); }
