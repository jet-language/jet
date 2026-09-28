// jet-ffi-count-unit: bytes
// jet-ffi-count-meaning: full-extent
// jet-ffi-count-width: 32
// jet-ffi-retention: borrowed-for-call
// jet-ffi-borrowed-view
// jet-ffi-effects: FFI.C
// jet-ffi-ownership: borrowed-view
// jet-ffi-failure: preserve
// jet-ffi-copies: none
// jet-ffi-placement: caller
// jet-ffi-cleanup: none
// jet-ffi-obligation: name=abi;basis=contained;checker=borrowed-view-fixture;assumptions=C ABI
// jet-ffi-obligation: name=layout;basis=contained;checker=borrowed-view-fixture;assumptions=bytes are contiguous
// jet-ffi-obligation: name=width;basis=enforced;checker=borrowed-view-fixture;assumptions=count is uint32_t
// jet-ffi-obligation: name=alignment;basis=contained;checker=borrowed-view-fixture;assumptions=uint8 alignment
// jet-ffi-obligation: name=ownership;basis=contained;checker=borrowed-view-fixture;assumptions=callee borrows only for call
// jet-ffi-obligation: name=lifetime;basis=enforced;checker=borrowed-view-fixture;assumptions=callee does not retain pointer
// jet-ffi-obligation: name=errors;basis=contained;checker=borrowed-view-fixture;assumptions=no error channel
// jet-ffi-obligation: name=cleanup;basis=contained;checker=borrowed-view-fixture;assumptions=no resource returned
// jet-ffi-obligation: name=target;basis=contained;checker=borrowed-view-fixture;assumptions=host target

unsigned int inspect_bytes(const unsigned char *bytes, unsigned int length);
