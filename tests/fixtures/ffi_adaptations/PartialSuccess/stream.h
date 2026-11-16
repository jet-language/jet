// jet-ffi-status-out
// jet-ffi-partial-success
// jet-ffi-count-unit: bytes
// jet-ffi-count-meaning: prefix
// jet-ffi-count-width: 32
// jet-ffi-retention: borrowed-for-call
// jet-ffi-effects: FFI.C
// jet-ffi-ownership: caller-output
// jet-ffi-failure: status-preserves-partial
// jet-ffi-copies: caller-buffer-only
// jet-ffi-placement: caller
// jet-ffi-cleanup: none
// jet-ffi-obligation: name=abi;basis=contained;checker=partial-fixture;assumptions=C ABI
// jet-ffi-obligation: name=layout;basis=contained;checker=partial-fixture;assumptions=contiguous output bytes
// jet-ffi-obligation: name=width;basis=enforced;checker=partial-fixture;assumptions=uint32_t prefix count
// jet-ffi-obligation: name=alignment;basis=contained;checker=partial-fixture;assumptions=byte alignment
// jet-ffi-obligation: name=ownership;basis=contained;checker=partial-fixture;assumptions=caller output buffer
// jet-ffi-obligation: name=lifetime;basis=contained;checker=partial-fixture;assumptions=borrowed during call
// jet-ffi-obligation: name=errors;basis=enforced;checker=partial-fixture;assumptions=status retains partial bytes
// jet-ffi-obligation: name=cleanup;basis=contained;checker=partial-fixture;assumptions=no resource returned
// jet-ffi-obligation: name=target;basis=contained;checker=partial-fixture;assumptions=host target

#include <stdint.h>
int32_t read_chunk(const uint8_t *input, uint32_t input_len, uint8_t *output, uint32_t output_cap, uint32_t *read_len);
