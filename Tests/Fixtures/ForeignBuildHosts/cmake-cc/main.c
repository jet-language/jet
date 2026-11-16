#include "config.h"

#ifndef JET_CC_RESPONSE
#error "CMake response-file flags were not passed"
#endif

int main(void) {
    return JET_CC_FIXTURE == JET_CC_RESPONSE ? 0 : 1;
}
