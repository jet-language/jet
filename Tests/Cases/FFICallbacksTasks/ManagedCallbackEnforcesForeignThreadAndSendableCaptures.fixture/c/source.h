#ifndef MANAGED_CALLBACK_SAFETY_H
#define MANAGED_CALLBACK_SAFETY_H

#include <stdint.h>

typedef void (*source_callback)(void *ctx, int64_t value);

void *on_data(source_callback callback, void *ctx);
int unsubscribe(void *ctx);

#endif
