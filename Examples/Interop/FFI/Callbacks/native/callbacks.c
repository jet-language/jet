#include "callbacks.h"

static source_callback current_callback;
static void *current_context;

void *on_data(source_callback callback, void *ctx) {
    current_callback = callback;
    current_context = ctx;
    return ctx;
}

int64_t emit_async(int64_t value) {
    source_callback callback = current_callback;
    void *ctx = current_context;
    if (callback != 0) {
        callback(ctx, value);
    }
    return value;
}

int unsubscribe(void *ctx) {
    if (ctx == 0 || ctx != current_context) {
        return 1;
    }
    current_callback = 0;
    current_context = 0;
    return 0;
}
