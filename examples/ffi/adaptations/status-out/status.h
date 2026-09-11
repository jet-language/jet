// jet-ffi-status-out
// jet-ffi-effects: FFI.C
// jet-ffi-ownership: caller-output
// jet-ffi-failure: status-out
// jet-ffi-copies: caller-buffer-only
// jet-ffi-placement: caller
// jet-ffi-cleanup: none
// jet-ffi-obligation: name=abi;basis=contained;checker=status-out-fixture;assumptions=C ABI
// jet-ffi-obligation: name=layout;basis=contained;checker=status-out-fixture;assumptions=output points to caller storage
// jet-ffi-obligation: name=width;basis=enforced;checker=status-out-fixture;assumptions=output length uses uint32_t
// jet-ffi-obligation: name=alignment;basis=contained;checker=status-out-fixture;assumptions=byte alignment
// jet-ffi-obligation: name=ownership;basis=contained;checker=status-out-fixture;assumptions=caller owns output
// jet-ffi-obligation: name=lifetime;basis=contained;checker=status-out-fixture;assumptions=output lives through call
// jet-ffi-obligation: name=errors;basis=enforced;checker=status-out-fixture;assumptions=negative status is failure
// jet-ffi-obligation: name=cleanup;basis=contained;checker=status-out-fixture;assumptions=no resource returned
// jet-ffi-obligation: name=target;basis=contained;checker=status-out-fixture;assumptions=host target

#include <stdint.h>
int32_t decode_status(const uint8_t *input, uint32_t input_len, uint8_t *output, uint32_t output_cap, uint32_t *written);
