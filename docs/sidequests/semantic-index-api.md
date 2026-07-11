# Stable Semantic-Index API

**Card:** c96 / c1oixt2m. **Decision:** D-SEMINDEX1=A. **Status:** ready to build.

## Goal

Expose a versioned, public semantic-index API over compiler facts without making
tools depend on private LSP internals.

Initial public facts:

- symbols and definitions;
- references;
- resolved types for names and expressions where already recorded;
- call graph edges;
- effect summaries.

## Build Plan

1. Define a small public crate or module surface, versioned independently from
   internal `Source/LSP/SymbolDB.rs` layout.
2. Build the index from the existing front-end path: loader -> parser -> sema.
   No tool parses Jet itself.
3. Add stable query structs with spans and file paths expressed through
   `jet-foundation` types, not raw LSP JSON.
4. Keep LSP as one consumer of the same facts; do not fork two symbol databases.
5. Add a CLI smoke surface only if needed for tests, e.g. `jet inspect semindex --json`.
6. Tests:
   - symbol lookup;
   - references;
   - call graph;
   - effect facts;
   - schema/version snapshot.

## Constraints

- No new language syntax.
- Public API cannot expose mutable compiler internals.
- Diagnostics still come from the front end; semindex queries report structured
  errors for I/O/project loading only.

## Verification

- Existing LSP tests still pass.
- New semindex tests prove stable JSON/API shape.
- `nix develop -c cargo test`.

