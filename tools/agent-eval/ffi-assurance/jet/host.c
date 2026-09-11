#include "ffiassurance.h"

#include <stdio.h>
#include <string.h>

int main(void) {
    const char *result = ffi_assurance_probe("ffi-assurance");
    if (result == NULL) return 2;
    puts(result);
    return strcmp(result, "ok") == 0 ? 0 : 1;
}
