#include <stdint.h>
typedef struct { int64_t value; } CloseHandle;
static int64_t released;
CloseHandle jet_close_acquire(void) { return (CloseHandle){41}; }
void jet_close_release(CloseHandle handle) { released = handle.value; }
int64_t jet_close_released(void) { return released; }
