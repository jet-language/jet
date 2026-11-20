// jet-ffi-fallible-close
// jet-ffi-status-out
// jet-ffi-effects: FFI.C
// jet-ffi-ownership: owned-handle
// jet-ffi-failure: close-status-preserved
// jet-ffi-copies: none
// jet-ffi-placement: caller
// jet-ffi-cleanup: fallible-close
// jet-ffi-obligation: name=abi;basis=contained;checker=close-fixture;assumptions=C ABI
// jet-ffi-obligation: name=layout;basis=contained;checker=close-fixture;assumptions=opaque handle only
// jet-ffi-obligation: name=width;basis=enforced;checker=close-fixture;assumptions=handle fits uintptr_t
// jet-ffi-obligation: name=alignment;basis=contained;checker=close-fixture;assumptions=opaque pointer alignment
// jet-ffi-obligation: name=ownership;basis=enforced;checker=close-fixture;assumptions=open transfers ownership
// jet-ffi-obligation: name=lifetime;basis=enforced;checker=close-fixture;assumptions=close consumes handle
// jet-ffi-obligation: name=errors;basis=enforced;checker=close-fixture;assumptions=close returns status
// jet-ffi-obligation: name=cleanup;basis=enforced;checker=close-fixture;assumptions=close may fail
// jet-ffi-obligation: name=target;basis=contained;checker=close-fixture;assumptions=host target

typedef struct Handle Handle;
Handle *open_handle(void);
int close_handle(Handle *handle);
int handle_value(const Handle *handle);
