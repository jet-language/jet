// jet-ffi-nullable
// jet-ffi-effects: FFI.C
// jet-ffi-ownership: borrowed-return-or-null
// jet-ffi-failure: null-is-absent
// jet-ffi-copies: none
// jet-ffi-placement: caller
// jet-ffi-cleanup: none
// jet-ffi-obligation: name=abi;basis=contained;checker=nullable-fixture;assumptions=C ABI
// jet-ffi-obligation: name=layout;basis=contained;checker=nullable-fixture;assumptions=NUL terminated bytes
// jet-ffi-obligation: name=width;basis=contained;checker=nullable-fixture;assumptions=pointer width target pinned
// jet-ffi-obligation: name=alignment;basis=contained;checker=nullable-fixture;assumptions=char alignment
// jet-ffi-obligation: name=ownership;basis=contained;checker=nullable-fixture;assumptions=borrowed return is valid for call result
// jet-ffi-obligation: name=lifetime;basis=contained;checker=nullable-fixture;assumptions=library owns returned storage
// jet-ffi-obligation: name=nullability;basis=enforced;checker=nullable-fixture;assumptions=NULL means absent
// jet-ffi-obligation: name=errors;basis=contained;checker=nullable-fixture;assumptions=NULL is the only failure value
// jet-ffi-obligation: name=cleanup;basis=contained;checker=nullable-fixture;assumptions=no close function
// jet-ffi-obligation: name=target;basis=contained;checker=nullable-fixture;assumptions=host target

const char *find_name(unsigned long long id);
