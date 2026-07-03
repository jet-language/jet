//! D-EXPANDCLI1=A (card #183): facts recorded during ordinary checking for
//! `jet expand`'s `refs` lens. Never a second analysis (I2/I3/I8) — this is
//! the same owner resolution `CheckerOwnership::check_stored_ref_fields`
//! already computes to police E2302/E0207/E2306; the only change here is
//! that a successful resolution is *also* remembered instead of thrown away
//! once the diagnostic checks pass.
//!
//! The `inline` lens needs no side table at all: `Func::is_inline` /
//! `is_inline_always` on an already-checked bundle already means "sema
//! proved this contract holds" (an `@InlineAlways` fn that couldn't inline
//! would have failed E0917/E0918/E0919 and the bundle wouldn't be here), so
//! `jet expand`'s driver-side renderer reads those AST fields directly.

use crate::Diagnostics::Span;

/// D-REF-SHORTHAND1: how a `&T` stored-ref field's owner was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefOwnerHow {
    /// `#Ref(label)` named the owner explicitly.
    Labeled,
    /// Exactly one in-scope candidate of the referent type — inferred.
    Inferred,
}

/// One `&T` stored-ref field's resolved owner at one struct-construction site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefFact {
    /// The struct type being constructed (`Type.{ … }`).
    pub struct_name: String,
    /// The `&T` field name being filled.
    pub field: String,
    /// The resolved owner — the in-scope value this field's reference points into.
    pub owner: String,
    pub how: RefOwnerHow,
    /// Span of the field initializer (`fname: value`) in the struct literal.
    pub span: Span,
    /// Index into `ProgramBundle::modules` for the file this site lives in.
    pub module_idx: usize,
}
