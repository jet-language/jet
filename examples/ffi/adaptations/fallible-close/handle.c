#include "handle.h"
#include <stdlib.h>

struct Handle { int value; };

Handle *open_handle(void) {
    Handle *handle = (Handle *)malloc(sizeof(Handle));
    if (handle != 0) handle->value = 42;
    return handle;
}

int close_handle(Handle *handle) {
    if (handle == 0) return -1;
    int status = handle->value == 42 ? 0 : 1;
    free(handle);
    return status;
}

int handle_value(const Handle *handle) {
    return handle == 0 ? -1 : handle->value;
}
