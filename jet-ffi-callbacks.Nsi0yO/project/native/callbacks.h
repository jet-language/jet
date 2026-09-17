#ifndef JET_FFI_CALLBACKS_H
#define JET_FFI_CALLBACKS_H

#include <stdint.h>

typedef void (*source_callback)(void *ctx, int64_t value);

void *on_data(source_callback callback, void *ctx);
int64_t emit_async(int64_t value);
int unsubscribe(void *ctx);

#endif
