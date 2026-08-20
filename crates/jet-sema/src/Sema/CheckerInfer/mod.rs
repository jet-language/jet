// Re-export the parent `Sema` glob so the split-out submodules
// (`expr`/`fallible`/`calls`/`binary`) reach `Checker`, etc. via `use super::*`.
pub(crate) use super::*;
use crate::Syntax;
use crate::AST::Type;

/// One inclusive interval fact used by every integer proof in sema.
///
/// Widths, user refinements, literal checks, and fixed-list indexes all ask
/// the same question: is one value set contained by another? Keep the
/// arithmetic and containment law here so those callers cannot grow their own
/// subtly different `(lo, hi)` checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IntegerInterval {
    pub(crate) lo: i128,
    pub(crate) hi: i128,
}

impl IntegerInterval {
    pub(crate) const fn new(lo: i128, hi: i128) -> Self {
        Self { lo, hi }
    }

    pub(crate) const fn from_bounds(bounds: (i128, i128)) -> Self {
        Self::new(bounds.0, bounds.1)
    }

    /// Value-set containment: a subset needs no conversion or runtime check.
    pub(crate) fn contains_interval(self, subset: Self) -> bool {
        self.lo <= subset.lo && subset.hi <= self.hi
    }

    pub(crate) fn negated(self) -> Option<Self> {
        Some(Self::new(self.hi.checked_neg()?, self.lo.checked_neg()?))
    }

    pub(crate) fn add(self, other: Self) -> Option<Self> {
        Some(Self::new(
            self.lo.checked_add(other.lo)?,
            self.hi.checked_add(other.hi)?,
        ))
    }

    pub(crate) fn sub(self, other: Self) -> Option<Self> {
        Some(Self::new(
            self.lo.checked_sub(other.hi)?,
            self.hi.checked_sub(other.lo)?,
        ))
    }

    pub(crate) fn mul(self, other: Self) -> Option<Self> {
        let products = [
            self.lo.checked_mul(other.lo)?,
            self.lo.checked_mul(other.hi)?,
            self.hi.checked_mul(other.lo)?,
            self.hi.checked_mul(other.hi)?,
        ];
        Some(Self::new(
            *products.iter().min()?,
            *products.iter().max()?,
        ))
    }

    pub(crate) fn fixed_list_indexes(len: u64) -> Option<Self> {
        Some(Self::new(0, i128::from(len).checked_sub(1)?))
    }
}

/// Project the canonical width fact. The width formula belongs to the type's
/// knowledge vector; sema only consumes that fact through this one interval
/// interface.
pub(crate) fn integer_width_interval(signed: bool, bits: u8) -> IntegerInterval {
    IntegerInterval::from_bounds(
        Type::IntN { signed, bits }
            .integer_range()
            .expect("fixed integer widths carry an interval fact"),
    )
}

/// D-INTBIG1: the parser's i64 field is only a fast path. Keep all fixed-width
/// checks on the exact source value so an overflowed token cannot reappear as
/// the lexer sentinel `0`.
pub(crate) fn exact_integer_literal(value: i64, raw: Option<&str>) -> crate::Numeric::CtBigInt {
    raw.and_then(|raw| crate::Numeric::CtBigInt::from_literal(raw).ok())
        .unwrap_or_else(|| crate::Numeric::CtBigInt::from_int(value))
}

pub(crate) fn exact_integer_fits(value: &crate::Numeric::CtBigInt, lo: i128, hi: i128) -> bool {
    let lower = crate::Numeric::CtBigInt::from_literal(&lo.to_string())
        .expect("i128 bounds are valid integer literals");
    let upper = crate::Numeric::CtBigInt::from_literal(&hi.to_string())
        .expect("i128 bounds are valid integer literals");
    value.compare(&lower) != std::cmp::Ordering::Less
        && value.compare(&upper) != std::cmp::Ordering::Greater
}

/// D-TYPE2-DEFAULT1: build the one exact Decimal literal path. The parser and
/// sema use this node for untyped decimal source, while the existing direct
/// Decimal call lowers through the shared Prelude on every execution tier.
pub(crate) fn exact_decimal_literal(text: String, span: crate::Diagnostics::Span) -> crate::AST::Expr {
    crate::AST::Expr::Call(crate::AST::Call {
        name: crate::Syntax::TYPE_DECIMAL.to_string(),
        name_span: span,
        type_args: Vec::new(),
        args: vec![crate::AST::CallArg {
            convention: crate::AST::AccessConvention::Read,
            expr: crate::AST::Expr::Str(
                vec![crate::AST::StrPart::Lit(text)],
                span,
            ),
            span,
            flags: crate::AST::CallArgFlags::default(),
            label: None,
            spread: false,
        }],
        resolved_ret: None,
        range_checked: false,
        widen_approx: false,
    })
}

/// D-ITER1: returns true when `ty` or an immediate inner layer is `Type::Tuple`.
/// Used to decide whether to store `resolved_ret` on a `MethodCall` node so that
/// `Tuples.rs` can collect the JetTup_ shape for `indexed`/`zip`/`partition`.
pub(crate) fn contains_tuple_type(ty: &Type) -> bool {
    match ty {
        Type::Tuple(_) => true,
        Type::List(inner) => matches!(inner.as_ref(), Type::Tuple(_)),
        // D-ITERTOOLS1=A / D-RANGE-EXCL1=C: adapters return `Iter<(…)>`.
        Type::Apply { name, args }
            if name == Syntax::TYPE_ITER && args.len() == 1 =>
        {
            matches!(&args[0], Type::Tuple(_))
        }
        Type::Option(inner) => matches!(inner.as_ref(), Type::Tuple(_)),
        _ => false,
    }
}

/// D-REACT1=B / D-DATARACE1=C: a reactive handle type `Signal<T>`/`Derived<T>`/
/// `Computed<T>` — an Arc-backed shared value whose "copy" shares the same
/// reactive cell. Lock-ordered so task/channel
/// crossings do not lean on rustc `Send`.
pub(crate) fn is_reactive_handle_ty(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Apply { name, .. }
            if name == Syntax::TYPE_SIGNAL
                || name == Syntax::TYPE_DERIVED
                || name == Syntax::TYPE_COMPUTED
    )
}

mod binary;
mod calls;
mod expr;
mod fallible;
