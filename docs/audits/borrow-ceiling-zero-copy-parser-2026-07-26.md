# Zero-copy parser borrow-ceiling audit

Card #745 ports a parser that returns a token and its remaining input as
zero-copy views into one caller-owned `String`.

## Classification

- D-MEM-VIEWRET1 covers the returned `Token` fields. Sema records parameter-0
  provenance for both `View<str>` slots.
- D-SHAPE-PLACE1 protects the caller's source after the parser returns. E0212
  stops replacement while either token view is live.
- `Shared<T>` does not apply. The parser has one caller-owned source, not shared
  mutable ownership.
- `Pool<T>` and `Id<T>` do not apply. The parser does not need stable
  many-object identities.

## Result

The existing production path accepts the valid parser and composes provenance
through its wrapper. Generated Rust gives both token fields one hidden lifetime
tied to the source parameter. No user-written lifetime syntax, raw reference
field, fallback, or second view mechanism is present.

The audit found no valid-code rejection in the ownership checker. The checker
therefore needs no production change. Focused tests cover the successful port,
a parser-owned source failure, hostile source replacement through the wrapper,
and exact native output from the executable memory example.
