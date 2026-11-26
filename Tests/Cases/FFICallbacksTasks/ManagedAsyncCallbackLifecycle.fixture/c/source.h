#ifndef MANAGED_CALLBACK_SOURCE_H
#define MANAGED_CALLBACK_SOURCE_H

#include <stdint.h>

typedef void (*source_callback)(void *ctx, int64_t value);

void *on_data(source_callback callback, void *ctx);
int64_t emit_async(int64_t value);
int unsubscribe(void *ctx);

int64_t start_inflight_pthread(int64_t value);
int64_t wait_inflight_entered(void);
int64_t mark_inflight_entered_and_delay(void);
int64_t mark_inflight_done(void);
int64_t read_inflight_done(void);
int64_t join_inflight_pthread(void);
int64_t invoke_stale_callback(int64_t value);
int64_t read_unsub_count(void);

#endif
