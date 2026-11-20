// jet-ffi-count-unit: bytes
// jet-ffi-count-meaning: full-extent
// jet-ffi-count-width: 64
// jet-ffi-retention: borrowed-for-call
// jet-ffi-effects: FFI.C
// jet-ffi-ownership: borrowed-for-call
// jet-ffi-failure: preserve
// jet-ffi-copies: none
// jet-ffi-placement: caller
// jet-ffi-cleanup: none
// jet-ffi-obligation: name=abi;basis=contained;checker=pointer-count-fixture;assumptions=declared C ABI
// jet-ffi-obligation: name=layout;basis=contained;checker=pointer-count-fixture;assumptions=byte view has no hidden stride
// jet-ffi-obligation: name=width;basis=enforced;checker=pointer-count-fixture;assumptions=count is uint64_t
// jet-ffi-obligation: name=alignment;basis=contained;checker=pointer-count-fixture;assumptions=uint8 alignment
// jet-ffi-obligation: name=ownership;basis=contained;checker=pointer-count-fixture;assumptions=callee does not retain bytes
// jet-ffi-obligation: name=lifetime;basis=contained;checker=pointer-count-fixture;assumptions=pointer borrowed for call
// jet-ffi-obligation: name=target;basis=contained;checker=pointer-count-fixture;assumptions=host target
// jet-ffi-obligation: name=errors;basis=contained;checker=pointer-count-fixture;assumptions=sum has no error channel
// jet-ffi-obligation: name=cleanup;basis=contained;checker=pointer-count-fixture;assumptions=no resource returned

#include <stddef.h>
#include <stdint.h>
uint64_t sum_bytes(const uint8_t *bytes, uint64_t count);
