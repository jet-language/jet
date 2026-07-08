//! AST nodes. Grows with each milestone; keep nodes small and keep spans on
//! anything an error might need to point at.

use crate::Diagnostics::Span;
include!("AST/types.rs");
include!("AST/program_imports.rs");
include!("AST/items.rs");
include!("AST/patterns.rs");
include!("AST/statements.rs");
include!("AST/lvalues.rs");
include!("AST/expressions.rs");
include!("AST/comptime.rs");
include!("AST/ffi.rs");
