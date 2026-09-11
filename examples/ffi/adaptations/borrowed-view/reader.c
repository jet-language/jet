#include "reader.h"

unsigned int inspect_bytes(const unsigned char *bytes, unsigned int length) {
    unsigned int nonzero = 0;
    for (unsigned int index = 0; index < length; ++index) {
        nonzero += bytes[index] != 0;
    }
    return nonzero;
}
