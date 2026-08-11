use super::{AccessConvention, BinOp, Expr, Lambda, ParamZone, Type};
use std::any::Any;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Names::{mangle, mangle_path, user_type_rust};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// D-MEM-VIEWRET1=B: compiler-inferred public owner of a returned/stored view.
/// Parameter positions are stable across renames and generic instantiation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewSource {
    Receiver,
    Parameter(usize),
    Static { module_path: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewSourceProjection {
    Field(String),
    Index,
    Range,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewSourcePath {
    pub source: ViewSource,
    pub projections: Vec<ViewSourceProjection>,
}

/// D-MEMPROVENANCE2=A: one returned-view slot may come from any source path
/// in this bounded, deterministic set. Access stays a property of the slot:
/// every path must provide the same read or write capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewProvenance {
    pub sources: BTreeSet<ViewSourcePath>,
    pub mutable: bool,
}

/// Canonical returned-view contract keyed by the output slot that contains the
/// view. An empty path is a direct `View` return; aggregate slots use their
/// nested field path. BTreeMap makes API fingerprints and fixed points stable.
pub type ViewProvenanceMap = BTreeMap<Vec<String>, ViewProvenance>;

/// Shared, replaceable view summary used by the body-analysis fixed point.
/// A plain `OnceLock` freezes the first partial result in recursive call
/// graphs; this cell publishes each larger iteration until convergence.
#[derive(Debug, Clone, Default)]
pub struct ViewProvenanceCell(
    std::sync::Arc<std::sync::RwLock<Option<ViewProvenanceMap>>>,
);

impl ViewProvenanceCell {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self) -> Option<ViewProvenanceMap> {
        self.0
            .read()
            .expect("view provenance lock poisoned")
            .clone()
    }

    pub fn set(&self, provenance: ViewProvenanceMap) {
        *self
            .0
            .write()
            .expect("view provenance lock poisoned") = Some(provenance);
    }
}

pub fn canonical_view_provenance_map(map: &ViewProvenanceMap) -> String {
    map.iter()
        .map(|(path, provenance)| {
            let slot = if path.is_empty() {
                "$".to_string()
            } else {
                path.join(".")
            };
            format!("{slot}={}", provenance.canonical())
        })
        .collect::<Vec<_>>()
        .join("|")
}

impl ViewProvenance {
    pub fn canonical(&self) -> String {
        let access = if self.mutable { "write" } else { "read" };
        if let Some(source) = self.sources.iter().next().filter(|_| self.sources.len() == 1) {
            let canonical = source.canonical();
            let (owner, path) = canonical
                .split_once(";path:")
                .expect("view source canonical form includes a path");
            return format!("{owner};access:{access};path:{path}");
        }
        let sources = self
            .sources
            .iter()
            .map(ViewSourcePath::canonical)
            .collect::<Vec<_>>()
            .join(",");
        format!("one_of({sources});access:{access}")
    }
}

impl ViewSourcePath {
    pub fn canonical(&self) -> String {
        let source = match &self.source {
            ViewSource::Receiver => "receiver".to_string(),
            ViewSource::Parameter(index) => format!("parameter:{index}"),
            ViewSource::Static { module_path, name } => format!("static:{module_path}::{name}"),
        };
        let path = self.projections.iter().map(|projection| match projection {
            ViewSourceProjection::Field(name) => format!("field:{name}"),
            ViewSourceProjection::Index => "index".to_string(),
            ViewSourceProjection::Range => "range".to_string(),
        }).collect::<Vec<_>>().join("/");
        format!("{source};path:{path}")
    }
}

/// Semantic signature of a function — the compiler's internal view after
/// registration. Lives in `AST` so that `Traits`, `Codegen`, and `Sema` can
/// all depend on it without creating cycles.
#[derive(Debug, Clone)]
pub struct FuncSig {
    pub params: Vec<(AccessConvention, Type)>,
    /// D-CALLDUAL1=E: the function has a `#Root` first parameter and may be
    /// selected by the receiver-first call spelling.
    pub root_param: bool,
    pub return_type: Option<Type>,
    /// Sema-proved stable source for a returned view. Callers compose the
    /// parameter index onto the corresponding actual argument place.
    pub return_view_provenance: ViewProvenanceCell,
    /// S50: declared in `extern rust`, implemented by the FFI bridge.
    pub is_extern: bool,
    /// S58 (E2-M13): `#Unsafe fn` — calling it requires an enclosing `#Unsafe`
    /// block (E3103).
    pub is_unsafe: bool,
    /// S60 (E2-M16): `pure fn` — this function is free of ambient I/O and
    /// non-determinism. Call sites inside a `pure fn` must also be pure (E3401).
    pub is_pure: bool,
    /// D-CABI-CALLBACK1: body contains only allocation-free, panic-free scalar
    /// computation and has no generic parameters or runtime/global access.
    pub is_foreign_thread_safe: bool,
    /// Compatibility bit for the retired sanitizer spelling.
    pub is_sanitizer: bool,
    /// D-MUSTUSE1 (c18iwxqx): `#MustUse fn` / method — return value cannot be
    /// silently ignored as a bare expression statement (E0419).
    pub is_must_use: bool,
    /// Card #436: true only for a C-boundary extern fn (`#Extern`/`#Bindgen
    /// module c.<lib>`, `CModule`), false for everything else including
    /// `extern rust`. A `String` argument to one of these crosses through
    /// `CString::new` in codegen — which fails on an embedded NUL byte — so
    /// call-site checking (E3211, `direct_calls.rs`) only applies here.
    pub is_c_abi: bool,
    /// D-CABI-PLATFORM1: explicit alternate ABI; such functions are direct-call-only.
    pub c_abi_name: Option<String>,
    /// Narrow compiler-owned effect for a generated foreign binding. `None`
    /// keeps ordinary extern calls maximally effectful.
    pub foreign_effect_root: Option<String>,
    /// S61: parameter names and default-value presence, parallel to `params`.
    /// Empty for extern/built-in functions.
    pub param_info: Vec<(String, bool)>,
    /// D-APILABEL1=A: the public call contract, parallel to `params` — the
    /// label a caller writes and the zone that decides whether they may (or
    /// must) write it. It is projected as a callable obligation; the local
    /// names in `param_info` are not. Empty alongside `param_info`.
    pub param_call: Vec<(String, ParamZone)>,
    /// S61: default expressions for parameters that have them, parallel to `params`.
    pub defaults: Vec<Option<Expr>>,
    /// D-VARIADIC1: parallel to `params` — true when that parameter is variadic.
    pub param_variadic: Vec<bool>,
    /// D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): the trailing variadic parameter's
    /// resolved trait-bound list (`Param::variadic_trait_bounds`), or `None` for
    /// a non-variadic function or a plain D-VARIADIC1 homogeneous-concrete-type
    /// variadic. Call-site checking (E1313) and codegen's per-arity
    /// monomorphization both key off this.
    pub variadic_bounds: Option<Vec<String>>,
    /// D-MEMPROVENANCE3=A: parallel to `params` — optional `from` source names
    /// on each parameter (call-boundary requirement).
    pub param_view_from_names: Vec<Option<Vec<String>>>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared data types — placed in AST so every seam crate can depend on them
// without creating cross-seam dep cycles.
// ─────────────────────────────────────────────────────────────────────────────

// ── CtValue / CtKey ──────────────────────────────────────────────────────────

/// Width-preserving compile-time float. Every operation stays in its native
/// Rust width so tier-0 never computes in f64 and casts afterward (D-FLOATW1).
#[derive(Clone, Copy, PartialEq)]
pub enum CtFloat {
    F32(f32),
    F64(f64),
}

impl fmt::Debug for CtFloat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::F32(value) => value.fmt(f),
            Self::F64(value) => value.fmt(f),
        }
    }
}

macro_rules! unary_math {
    ($value:expr, $method:ident) => {
        match $value {
            CtFloat::F32(value) => CtFloat::F32(value.$method()),
            CtFloat::F64(value) => CtFloat::F64(value.$method()),
        }
    };
}

macro_rules! binary_math {
    ($left:expr, $right:expr, $method:ident) => {
        match ($left, $right) {
            (CtFloat::F32(left), CtFloat::F32(right)) => Some(CtFloat::F32(left.$method(right))),
            (CtFloat::F64(left), CtFloat::F64(right)) => Some(CtFloat::F64(left.$method(right))),
            _ => None,
        }
    };
}

impl CtFloat {
    pub fn literal(value: f64, is_f32: bool) -> Self {
        if is_f32 {
            Self::F32(value as f32)
        } else {
            Self::F64(value)
        }
    }

    pub fn f32(value: f32) -> Self {
        Self::F32(value)
    }

    pub fn f64(value: f64) -> Self {
        Self::F64(value)
    }

    pub fn jet_type(self) -> Type {
        match self {
            Self::F32(_) => Type::Float32,
            Self::F64(_) => Type::Float,
        }
    }

    pub fn render(self) -> String {
        format!("{self:?}")
    }

    pub fn to_json(self) -> String {
        let rendered = self.render();
        if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
            rendered
        } else {
            format!("{rendered}.0")
        }
    }

    pub fn serialize(self) -> String {
        match self {
            Self::F32(value) => serialize_float(value, "f32", "f32"),
            Self::F64(value) => serialize_float(value, "f64", "f64"),
        }
    }

    pub fn as_f64(self) -> f64 {
        match self {
            Self::F32(value) => value as f64,
            Self::F64(value) => value,
        }
    }

    pub fn as_f32(self) -> f32 {
        match self {
            Self::F32(value) => value,
            Self::F64(value) => value as f32,
        }
    }

    pub fn neg(self) -> Self {
        match self {
            Self::F32(value) => Self::F32(-value),
            Self::F64(value) => Self::F64(-value),
        }
    }

    pub fn binop(self, op: BinOp, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::F32(left), Self::F32(right)) => Some(Self::F32(match op {
                BinOp::Add => left + right,
                BinOp::Sub => left - right,
                BinOp::Mul => left * right,
                BinOp::Div => left / right,
                // D-EXPSEM1=A: `^` on floats is the floating-point power.
                BinOp::Pow => left.powf(right),
                _ => return None,
            })),
            (Self::F64(left), Self::F64(right)) => Some(Self::F64(match op {
                BinOp::Add => left + right,
                BinOp::Sub => left - right,
                BinOp::Mul => left * right,
                BinOp::Div => left / right,
                // D-EXPSEM1=A: `^` on floats is the floating-point power.
                BinOp::Pow => left.powf(right),
                _ => return None,
            })),
            _ => None,
        }
    }

    pub fn partial_cmp(self, other: Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::F32(left), Self::F32(right)) => left.partial_cmp(&right),
            (Self::F64(left), Self::F64(right)) => left.partial_cmp(&right),
            _ => None,
        }
    }

    pub fn abs(self) -> Self {
        match self {
            Self::F32(value) => Self::F32(value.abs()),
            Self::F64(value) => Self::F64(value.abs()),
        }
    }

    pub fn is_nan(self) -> bool {
        match self {
            Self::F32(value) => value.is_nan(),
            Self::F64(value) => value.is_nan(),
        }
    }

    pub fn is_infinite(self) -> bool {
        match self {
            Self::F32(value) => value.is_infinite(),
            Self::F64(value) => value.is_infinite(),
        }
    }

    pub fn is_finite(self) -> bool {
        match self {
            Self::F32(value) => value.is_finite(),
            Self::F64(value) => value.is_finite(),
        }
    }

    pub fn round_i64(self) -> i64 {
        match self {
            Self::F32(value) => value.round() as i64,
            Self::F64(value) => value.round() as i64,
        }
    }

    pub fn trunc_i64(self) -> i64 {
        match self {
            Self::F32(value) => value.trunc() as i64,
            Self::F64(value) => value.trunc() as i64,
        }
    }

    pub fn sign(self) -> i64 {
        match self {
            Self::F32(value) => i64::from(value > 0.0) - i64::from(value < 0.0),
            Self::F64(value) => i64::from(value > 0.0) - i64::from(value < 0.0),
        }
    }

    pub fn to_bits_i64(self) -> i64 {
        match self {
            Self::F32(value) => value.to_bits() as i64,
            Self::F64(value) => value.to_bits() as i64,
        }
    }

    pub fn sqrt(self) -> Self { unary_math!(self, sqrt) }
    pub fn floor(self) -> Self { unary_math!(self, floor) }
    pub fn ceil(self) -> Self { unary_math!(self, ceil) }
    pub fn log2(self) -> Self { unary_math!(self, log2) }
    pub fn log10(self) -> Self { unary_math!(self, log10) }
    pub fn sin(self) -> Self { unary_math!(self, sin) }
    pub fn cos(self) -> Self { unary_math!(self, cos) }
    pub fn tan(self) -> Self { unary_math!(self, tan) }
    pub fn asin(self) -> Self { unary_math!(self, asin) }
    pub fn acos(self) -> Self { unary_math!(self, acos) }
    pub fn atan(self) -> Self { unary_math!(self, atan) }
    pub fn sinh(self) -> Self { unary_math!(self, sinh) }
    pub fn cosh(self) -> Self { unary_math!(self, cosh) }
    pub fn tanh(self) -> Self { unary_math!(self, tanh) }
    pub fn exp(self) -> Self { unary_math!(self, exp) }
    pub fn ln(self) -> Self { unary_math!(self, ln) }
    pub fn trunc(self) -> Self { unary_math!(self, trunc) }
    pub fn fract(self) -> Self { unary_math!(self, fract) }
    pub fn acosh(self) -> Self { unary_math!(self, acosh) }
    pub fn asinh(self) -> Self { unary_math!(self, asinh) }
    pub fn atanh(self) -> Self { unary_math!(self, atanh) }
    pub fn cbrt(self) -> Self { unary_math!(self, cbrt) }
    pub fn exp2(self) -> Self { unary_math!(self, exp2) }
    pub fn exp_m1(self) -> Self { unary_math!(self, exp_m1) }
    pub fn ln_1p(self) -> Self { unary_math!(self, ln_1p) }
    pub fn signum(self) -> Self { unary_math!(self, signum) }
    pub fn to_degrees(self) -> Self { unary_math!(self, to_degrees) }
    pub fn to_radians(self) -> Self { unary_math!(self, to_radians) }

    pub fn powf(self, other: Self) -> Option<Self> { binary_math!(self, other, powf) }
    pub fn min(self, other: Self) -> Option<Self> { binary_math!(self, other, min) }
    pub fn max(self, other: Self) -> Option<Self> { binary_math!(self, other, max) }
    pub fn atan2(self, other: Self) -> Option<Self> { binary_math!(self, other, atan2) }
    pub fn hypot(self, other: Self) -> Option<Self> { binary_math!(self, other, hypot) }
    pub fn copysign(self, other: Self) -> Option<Self> { binary_math!(self, other, copysign) }
    pub fn log(self, other: Self) -> Option<Self> { binary_math!(self, other, log) }

    /// One rounding for a multiply and an add together, so the product keeps
    /// its full width before the sum. Mixing widths is refused, as it is for
    /// every other width-generic operation.
    pub fn mul_add(self, factor: Self, addend: Self) -> Option<Self> {
        match (self, factor, addend) {
            (CtFloat::F64(a), CtFloat::F64(b), CtFloat::F64(c)) => Some(CtFloat::F64(a.mul_add(b, c))),
            (CtFloat::F32(a), CtFloat::F32(b), CtFloat::F32(c)) => Some(CtFloat::F32(a.mul_add(b, c))),
            _ => None,
        }
    }

    pub fn clamp(self, low: Self, high: Self) -> Option<Self> {
        match (self, low, high) {
            (Self::F32(value), Self::F32(low), Self::F32(high)) => {
                Some(Self::F32(value.clamp(low, high)))
            }
            (Self::F64(value), Self::F64(low), Self::F64(high)) => {
                Some(Self::F64(value.clamp(low, high)))
            }
            _ => None,
        }
    }

    pub fn lerp(self, other: Self, t: Self) -> Option<Self> {
        match (self, other, t) {
            (Self::F32(left), Self::F32(right), Self::F32(t)) => {
                Some(Self::F32(left + (right - left) * t))
            }
            (Self::F64(left), Self::F64(right), Self::F64(t)) => {
                Some(Self::F64(left + (right - left) * t))
            }
            _ => None,
        }
    }
}

fn serialize_float<T: fmt::Debug + PartialOrd + Copy>(value: T, suffix: &str, module: &str) -> String
where
    f64: From<T>,
{
    let widened = f64::from(value);
    if widened.is_nan() {
        format!("{module}::NAN")
    } else if widened == f64::INFINITY {
        format!("{module}::INFINITY")
    } else if widened == f64::NEG_INFINITY {
        format!("{module}::NEG_INFINITY")
    } else {
        format!("{value:?}{suffix}")
    }
}

/// A fully-evaluated compile-time value.
#[derive(Clone, Debug, PartialEq)]
pub enum CtValue {
    Int(i64),
    Float(CtFloat),
    Bool(bool),
    Char(char),
    Str(String),
    /// D-BIGINT1: arbitrary-precision integer, comptime/REPL tier-0 mirror of
    /// AOT's `JetBigInt` (crate::Numeric::CtBigInt keeps the same limb algorithm
    /// so results print identically on both tiers — R12 parity).
    BigInt(crate::Numeric::CtBigInt),
    /// `[U8]` byte buffer (D-CTIO1 `embed_bytes`).
    Bytes(Vec<u8>),
    List(Vec<CtValue>),
    Map(BTreeMap<CtKey, CtValue>),
    Struct {
        type_name: String,
        fields: Vec<(String, CtValue)>,
    },
    Enum {
        type_name: String,
        variant: String,
        args: Vec<(Option<String>, CtValue)>,
    },
    /// D-FAIL-CARRIER1=A — the payload side of the one outcome carrier. `T?`
    /// and `T ? E` are two views of this carrier, so a present payload has one
    /// spelling, not one per view. Mirrors the prelude's `Ok` on
    /// `JetOutcome<T, E>`.
    Present(Box<CtValue>),
    /// D-FAIL-CARRIER1=A — the stop side of the one outcome carrier. The report
    /// says whether the stop is clean or told.
    Failed(CtReport),
    Unit,
    /// c139 JIT/interpreter-parity: a lambda value (`(x) => x > 3`) captured
    /// at the point it's created, so the interpreter can invoke it later —
    /// passed to a higher-order method (`.filter`/`.map`/`.each`/`.sort_by`/
    /// `Option.lift2`), stored, or returned. See `ClosureData` below.
    Closure(std::sync::Arc<ClosureData>),
}

/// The one `[U8]` conversion and rejection voice shared by every evaluator.
pub fn as_bytes(value: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    let reject = || Diagnostic::e0956_unsupported("this `as_bytes` call", span);

    match value {
        CtValue::Bytes(bytes) => Ok(bytes.clone()),
        CtValue::List(items) => {
            let mut bytes = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    CtValue::Int(value) if (0..=255).contains(value) => {
                        bytes.push(*value as u8)
                    }
                    _ => return Err(reject()),
                }
            }
            Ok(bytes)
        }
        _ => Err(reject()),
    }
}

/// D-FAIL-CARRIER1=A — the report on the stop side of the one outcome carrier.
///
/// `T?` and `T ? E` stop the same way; they differ only in what the report has
/// to say. A clean report says nothing but the payload it lacks, and is the
/// comptime twin of the prelude's zero-sized `JetAbsent`. A told report is the
/// error value on `T ? E`'s stop side.
#[derive(Clone, Debug, PartialEq)]
pub enum CtReport {
    /// The clean report: an absence, which is not a failure.
    Clean(Box<Type>),
    /// The told report: the failure the caller reads.
    Told(Box<CtValue>),
}

impl CtValue {
    /// Stop with a clean report: the `T?` view of the carrier, where the
    /// missing payload would have had type `ty`.
    pub fn absent(ty: impl Into<Box<Type>>) -> CtValue {
        CtValue::Failed(CtReport::Clean(ty.into()))
    }

    /// Stop with a told report: the `T ? E` view of the carrier.
    pub fn failed(report: Box<CtValue>) -> CtValue {
        CtValue::Failed(CtReport::Told(report))
    }

    /// Does this outcome carry a payload?
    pub fn is_present(&self) -> bool {
        matches!(self, CtValue::Present(_))
    }

    /// Did this outcome stop with a clean report?
    pub fn is_clean_stop(&self) -> bool {
        matches!(self, CtValue::Failed(CtReport::Clean(_)))
    }

    /// The told report, when this outcome stopped with one.
    pub fn told_report(&self) -> Option<&CtValue> {
        match self {
            CtValue::Failed(CtReport::Told(report)) => Some(report),
            _ => None,
        }
    }

    /// Interpreter/deopt adapter for D-FAIL-ERROR1=A. This only marshals the
    /// checked TIR shape; construction, field meaning, and rendering stay in
    /// the shared Prelude's `JetErr` implementation.
    pub fn to_jet_err(&self) -> Option<crate::Outcome::JetErr> {
        use crate::Outcome::{jet_err, JetAbsent};

        let CtValue::Struct { type_name, fields } = self else {
            return None;
        };
        if type_name != crate::Syntax::TYPE_ERR {
            return None;
        }
        let field = |name: &str| {
            fields
                .iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value)
        };
        let CtValue::Str(message) = field("message")? else {
            return None;
        };
        let code = match field("code") {
            Some(CtValue::Present(value)) => match value.as_ref() {
                CtValue::Str(code) => Ok(code.clone()),
                _ => return None,
            },
            Some(CtValue::Failed(CtReport::Clean(_))) => Err(JetAbsent),
            _ => return None,
        };
        let cause = match field("cause") {
            Some(CtValue::Present(value)) => value.to_jet_err().ok_or(JetAbsent),
            Some(CtValue::Failed(CtReport::Clean(_))) => Err(JetAbsent),
            _ => return None,
        };
        Some(jet_err(message.clone(), code, cause))
    }

    /// Interpreter/deopt adapter for the core crypto error shape. The native
    /// bridge carries the same stable Display text in `JetCryptoError`; keep
    /// the tier-0 carrier as a message-only view rather than exposing its
    /// Rust-shaped struct rendering at an entry boundary.
    pub fn crypto_error_message(&self) -> Option<String> {
        let CtValue::Struct { type_name, fields } = self else {
            return None;
        };
        if type_name != "CryptoError" {
            return None;
        }
        match fields.iter().find(|(field, _)| field == "message") {
            Some((_, CtValue::Str(message))) => Some(message.clone()),
            _ => None,
        }
    }

    /// Interpreter/deopt adapter from the Prelude-owned error shape. This is
    /// the inverse of `to_jet_err`; it does not recreate error semantics.
    pub fn from_jet_err(error: &crate::Outcome::JetErr) -> CtValue {
        let code = match crate::Outcome::jet_err_code(error) {
            Ok(code) => CtValue::Present(Box::new(CtValue::Str(code))),
            Err(_) => CtValue::absent(Type::String),
        };
        let cause = match crate::Outcome::jet_err_cause(error) {
            Ok(cause) => CtValue::Present(Box::new(CtValue::from_jet_err(&cause))),
            Err(_) => CtValue::absent(Type::Named(crate::Syntax::TYPE_ERR.to_string())),
        };
        CtValue::Struct {
            type_name: crate::Syntax::TYPE_ERR.to_string(),
            fields: vec![
                (
                    "message".to_string(),
                    CtValue::Str(crate::Outcome::jet_err_message(error)),
                ),
                ("code".to_string(), code),
                ("cause".to_string(), cause),
            ],
        }
    }
}

/// A compiler-private opaque value that can cross the `CtValue` boundary while
/// retaining identity. It is deliberately not a language value: only code
/// that created the token can downcast it.
#[derive(Clone)]
pub struct CtOpaque(std::sync::Arc<dyn Any + Send + Sync>);

impl CtOpaque {
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        Self(std::sync::Arc::new(value))
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

impl fmt::Debug for CtOpaque {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<opaque>")
    }
}

/// c139: a closure's captured state — the AST `Lambda` node plus every
/// binding in scope where it was created (a tree-walker over-captures rather
/// than tracking free variables, which is simpler and behaviorally
/// equivalent here). `PartialEq` is reference identity (the `Arc`'s shared
/// allocation address) rather than structural — closures aren't
/// meaningfully comparable, and nothing in the language surface needs them
/// to be; deriving `PartialEq` on the whole AST just to satisfy `CtValue`'s
/// derive would be a much larger, unrelated change. `Arc` (not `Rc`) so
/// `CtValue` stays `Send + Sync`, matching every other variant.
#[derive(Clone, Debug)]
pub struct ClosureData {
    pub lambda: Lambda,
    pub captured: std::collections::HashMap<String, CtValue>,
    /// Contextual return type retained by tier-0 when a lambda is bound as a
    /// typed function value. Parsed comptime bodies run before full sema body
    /// elaboration, so the value boundary is the width authority here.
    pub return_type: Option<Type>,
    /// Compiler-private identity carrier for values that need to retain a
    /// shared runtime handle across the `CtValue` boundary.
    pub opaque: Option<CtOpaque>,
}

impl PartialEq for ClosureData {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

/// Orderable map key (S38: maps are `BTreeMap`, so keys must be `Ord`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CtKey {
    Int(i64),
    Str(String),
    Bool(bool),
    Char(char),
}

impl CtKey {
    pub fn from_value(v: CtValue) -> Option<CtKey> {
        match v {
            CtValue::Int(n) => Some(CtKey::Int(n)),
            CtValue::Str(s) => Some(CtKey::Str(s)),
            CtValue::Bool(b) => Some(CtKey::Bool(b)),
            CtValue::Char(c) => Some(CtKey::Char(c)),
            _ => None,
        }
    }
    pub fn to_value(&self) -> CtValue {
        match self {
            CtKey::Int(n) => CtValue::Int(*n),
            CtKey::Str(s) => CtValue::Str(s.clone()),
            CtKey::Bool(b) => CtValue::Bool(*b),
            CtKey::Char(c) => CtValue::Char(*c),
        }
    }
    pub(crate) fn jet_type(&self) -> Type {
        match self {
            CtKey::Int(_) => Type::Int,
            CtKey::Str(_) => Type::String,
            CtKey::Bool(_) => Type::Bool,
            CtKey::Char(_) => Type::Char,
        }
    }
    pub(crate) fn jet_show(&self) -> String {
        self.to_value().jet_show()
    }
}

impl CtValue {
    pub fn jet_type(&self) -> Type {
        match self {
            CtValue::Int(_) => Type::Int,
            CtValue::Float(value) => value.jet_type(),
            CtValue::Bool(_) => Type::Bool,
            CtValue::Char(_) => Type::Char,
            CtValue::Str(_) => Type::String,
            CtValue::BigInt(_) => Type::Named(crate::Syntax::TYPE_BIGINT.to_string()),
            CtValue::Bytes(_) => Type::List(Box::new(Type::IntN {
                signed: false,
                bits: 8,
            })),
            CtValue::List(xs) => {
                let inner = xs.first().map(|x| x.jet_type()).unwrap_or(Type::Int);
                Type::List(Box::new(inner))
            }
            CtValue::Map(m) => {
                let (k, v) = m
                    .iter()
                    .next()
                    .map(|(k, v)| (k.jet_type(), v.jet_type()))
                    .unwrap_or((Type::String, Type::Int));
                Type::Map {
                    key: Box::new(k),
                    key_span: None,
                    value: Box::new(v),
                }
            }
            // D-FAIL-CARRIER1=A: a payload alone does not say which view of the
            // carrier a caller asked for, so the optional view — the one with
            // the clean report — is the answer. Sema owns the declared type;
            // this is only the value's own shape.
            CtValue::Present(inner) => Type::Option(Box::new(inner.jet_type())),
            CtValue::Failed(CtReport::Clean(t)) => Type::Option(t.clone()),
            CtValue::Failed(CtReport::Told(e)) => Type::Result {
                ok: Box::new(Type::Int),
                err: Box::new(e.jet_type()),
            },
            CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } => {
                Type::Named(type_name.clone())
            }
            CtValue::Unit => Type::Named(String::new()),
            // c139: no return-type info is threaded to a closure value at
            // runtime (the interpreter is untyped past sema); the exact
            // signature is never read back from `jet_type()` for a closure
            // in practice (unlike `None(Type)`, which needs it to pick an
            // empty list/option's element type).
            CtValue::Closure(_) => Type::Fn {
                params: Vec::new(),
                ret: None,
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            },
        }
    }

    pub fn jet_show(&self) -> String {
        match self {
            CtValue::Int(n) => n.to_string(),
            CtValue::Float(value) => value.render(),
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => c.to_string(),
            CtValue::Str(s) => s.clone(),
            CtValue::BigInt(b) => b.to_string_rep(),
            CtValue::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                format!("[{}]", parts.join(", "))
            }
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.jet_show()).collect();
                format!("[{}]", parts.join(", "))
            }
            CtValue::Map(m) => {
                let parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.jet_show(), v.jet_show()))
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            CtValue::Present(v) => v.jet_show(),
            CtValue::Failed(CtReport::Clean(_)) => "null".to_string(),
            CtValue::Failed(CtReport::Told(_)) => "err".to_string(),
            CtValue::Struct { type_name, fields } => {
                if type_name == crate::Syntax::TYPE_ERR {
                    if let Some(error) = self.to_jet_err() {
                        return crate::Outcome::jet_render_err(&error);
                    }
                }
                // AOT `JetShow` for user structs is `format!("{:?}", self)`
                // (`Codegen::Items`) → `__jet_Name { __jet_field: … }`. Match that
                // here (I2 / #777 corpus differential). Table/Series/LazyFrame
                // hide the internal `elem_type` schema tag AOT never prints.
                let filtered: Vec<(String, CtValue)> = fields
                    .iter()
                    .filter(|(n, _)| {
                        !(matches!(type_name.as_str(), "Table" | "Series" | "LazyFrame")
                            && n == "elem_type")
                    })
                    .cloned()
                    .collect();
                CtValue::Struct {
                    type_name: type_name.clone(),
                    fields: filtered,
                }
                .debug_rust()
            }
            CtValue::Enum {
                type_name,
                variant,
                args,
            } if type_name == "Loadable" => match (variant.as_str(), args.first()) {
                ("Idle" | "Loading", _) => variant.clone(),
                ("Loaded" | "Failed", Some((_, value))) => {
                    format!("{}({})", variant, value.jet_show())
                }
                _ => variant.clone(),
            },
            // The compiled program's Rust `#[derive(Debug)]` output for a user
            // enum: the variant's Rust identifier is `__jet_<Variant>` (S34,
            // `Codegen::mangle_variant`), and a payload prints tuple-style with
            // each arg in Rust `{:?}` form — matching that exactly here keeps
            // `jet dev` byte-identical to the compiled binary (I2).
            CtValue::Enum { variant, args, .. } => {
                let mangled = mangle_path(variant);
                if args.is_empty() {
                    mangled
                } else if args.iter().all(|(label, _)| label.is_some()) {
                    let parts: Vec<String> = args
                        .iter()
                        .map(|(label, v)| {
                            format!("{}: {}", label.as_deref().unwrap_or(""), v.debug_rust())
                        })
                        .collect();
                    format!("{} {{ {} }}", mangled, parts.join(", "))
                } else {
                    let parts: Vec<String> = args.iter().map(|(_, v)| v.debug_rust()).collect();
                    format!("{}({})", mangled, parts.join(", "))
                }
            }
            CtValue::Unit => String::new(),
            // c139: never reached in practice — a closure is a callable, not
            // a printable value, so no example interpolates one directly.
            CtValue::Closure(_) => "<closure>".to_string(),
        }
    }

    /// Rust `{:?}` (Debug) rendering of this value — used to format a value
    /// nested inside an enum-variant payload the same way the compiled
    /// program's derived `Debug` impl would (I2). Distinct from `jet_show`,
    /// which is the user-facing `Display`-style rendering `print` uses at the
    /// top level (e.g. an unquoted string).
    pub fn debug_rust(&self) -> String {
        match self {
            CtValue::Int(n) => n.to_string(),
            CtValue::Float(value) => value.render(),
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => format!("{:?}", c),
            CtValue::Str(s) => format!("{:?}", s),
            CtValue::BigInt(b) => b.to_string_rep(),
            CtValue::Bytes(bs) => format!("{:?}", bs),
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.debug_rust()).collect();
                format!("[{}]", parts.join(", "))
            }
            CtValue::Map(m) => {
                let parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.to_value().debug_rust(), v.debug_rust()))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            // D-FAIL-CARRIER1=A: one carrier, so one pair of Rust spellings.
            CtValue::Present(v) => format!("Ok({})", v.debug_rust()),
            CtValue::Failed(CtReport::Clean(_)) => "Err(JetAbsent)".to_string(),
            CtValue::Failed(CtReport::Told(v)) => format!("Err({})", v.debug_rust()),
            CtValue::Struct { type_name, fields } => {
                let ty = crate::Syntax::generated_suffix(type_name);
                let mangled = crate::Syntax::generated_path(ty);
                if fields.is_empty() {
                    mangled
                } else {
                    let parts: Vec<String> = fields
                        .iter()
                        .map(|(n, v)| {
                            let field = crate::Syntax::generated_suffix(n);
                            format!("{}: {}", ct_mangle(field), v.debug_rust())
                        })
                        .collect();
                    format!("{} {{ {} }}", mangled, parts.join(", "))
                }
            }
            CtValue::Enum {
                type_name,
                variant,
                args,
            } if type_name == "Loadable" => match (variant.as_str(), args.first()) {
                ("Idle" | "Loading", _) => variant.clone(),
                ("Loaded" | "Failed", Some((_, value))) => {
                    format!("{}({})", variant, value.debug_rust())
                }
                _ => variant.clone(),
            },
            CtValue::Enum { variant, args, .. } => {
                let mangled = mangle_path(variant);
                if args.is_empty() {
                    mangled
                } else if args.iter().all(|(label, _)| label.is_some()) {
                    let parts: Vec<String> = args
                        .iter()
                        .map(|(label, v)| {
                            format!("{}: {}", label.as_deref().unwrap_or(""), v.debug_rust())
                        })
                        .collect();
                    format!("{} {{ {} }}", mangled, parts.join(", "))
                } else {
                    let parts: Vec<String> = args.iter().map(|(_, v)| v.debug_rust()).collect();
                    format!("{}({})", mangled, parts.join(", "))
                }
            }
            CtValue::Unit => "()".to_string(),
            CtValue::Closure(_) => "<closure>".to_string(),
        }
    }

    pub fn render_pretty(&self) -> String {
        let mut out = String::new();
        self.render_pretty_inner(&mut out, 0);
        out
    }

    fn render_pretty_inner(&self, out: &mut String, depth: usize) {
        let indent = "  ".repeat(depth);
        let inner_indent = "  ".repeat(depth + 1);
        match self {
            CtValue::Int(n) => out.push_str(&n.to_string()),
            CtValue::Float(value) => out.push_str(&value.render()),
            CtValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            CtValue::Char(c) => {
                out.push('\'');
                out.push(*c);
                out.push('\'');
            }
            CtValue::Str(s) => {
                out.push('"');
                out.push_str(s);
                out.push('"');
            }
            CtValue::BigInt(b) => out.push_str(&b.to_string_rep()),
            CtValue::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                out.push('[');
                out.push_str(&parts.join(", "));
                out.push(']');
            }
            CtValue::List(xs) => {
                if xs.is_empty() {
                    out.push_str("[]");
                } else {
                    out.push_str("[\n");
                    for item in xs {
                        out.push_str(&inner_indent);
                        item.render_pretty_inner(out, depth + 1);
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push(']');
                }
            }
            CtValue::Map(m) => {
                if m.is_empty() {
                    out.push_str("{}");
                } else {
                    out.push_str("{\n");
                    for (k, v) in m {
                        out.push_str(&inner_indent);
                        out.push_str(&k.jet_show());
                        out.push_str(": ");
                        v.render_pretty_inner(out, depth + 1);
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push('}');
                }
            }
            CtValue::Struct { type_name, fields } => {
                if fields.is_empty() {
                    out.push_str(type_name);
                    out.push_str(" {}");
                } else {
                    out.push_str(type_name);
                    out.push_str(" {\n");
                    for (name, value) in fields {
                        out.push_str(&inner_indent);
                        out.push_str(name);
                        out.push_str(": ");
                        value.render_pretty_inner(out, depth + 1);
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push('}');
                }
            }
            CtValue::Enum { type_name, .. } if type_name == "Loadable" => {
                out.push_str(&self.jet_show())
            }
            CtValue::Enum {
                type_name,
                variant,
                args,
            } => {
                out.push_str(type_name);
                out.push_str("::");
                out.push_str(variant);
                if !args.is_empty() {
                    out.push('(');
                    let mut first = true;
                    for (label, v) in args {
                        if !first {
                            out.push_str(", ");
                        }
                        first = false;
                        if let Some(lbl) = label {
                            out.push_str(lbl);
                            out.push_str(": ");
                        }
                        v.render_pretty_inner(out, depth);
                    }
                    out.push(')');
                }
            }
            // D-FAIL-CARRIER1=A: one carrier, so one pair of spellings.
            CtValue::Present(v) => {
                out.push_str("Ok(");
                v.render_pretty_inner(out, depth);
                out.push(')');
            }
            CtValue::Failed(CtReport::Clean(_)) => out.push_str("Err(JetAbsent)"),
            CtValue::Failed(CtReport::Told(e)) => {
                out.push_str("Err(");
                e.render_pretty_inner(out, depth);
                out.push(')');
            }
            CtValue::Unit => out.push_str("()"),
            CtValue::Closure(_) => out.push_str("<closure>"),
        }
    }

    pub fn to_json(&self) -> String {
        match self {
            CtValue::Int(n) => n.to_string(),
            CtValue::Float(value) => value.to_json(),
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => format!("\"{}\"", c),
            CtValue::Str(s) => {
                let mut out = String::from('"');
                for ch in s.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            // A `BigInt` doesn't fit in a JSON number without losing
            // precision in most readers, so it round-trips as a string —
            // matching how AOT's `#[Codable]` handles arbitrary precision.
            CtValue::BigInt(b) => format!("\"{}\"", b.to_string_rep()),
            CtValue::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                format!("[{}]", parts.join(","))
            }
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.to_json()).collect();
                format!("[{}]", parts.join(","))
            }
            CtValue::Map(m) => {
                let parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}:{}", k.to_value().to_json(), v.to_json()))
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
            CtValue::Struct { fields, .. } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("\"{}\":{}", n, v.to_json()))
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
            CtValue::Enum { variant, args, .. } => {
                if args.is_empty() {
                    format!("\"{}\"", variant)
                } else {
                    let parts: Vec<String> = args
                        .iter()
                        .map(|(label, v)| {
                            if let Some(lbl) = label {
                                format!("\"{}\":{}", lbl, v.to_json())
                            } else {
                                v.to_json()
                            }
                        })
                        .collect();
                    if args.iter().all(|(label, _)| label.is_some()) {
                        format!("{{\"{}\":{{{}}}}}", variant, parts.join(","))
                    } else {
                        format!("{{\"{}\":[{}]}}", variant, parts.join(","))
                    }
                }
            }
            // D-FAIL-CARRIER1=A: one carrier, so a payload encodes as itself and
            // a clean stop as null; only a told report needs a tag.
            CtValue::Present(v) => v.to_json(),
            CtValue::Failed(CtReport::Clean(_)) => "null".to_string(),
            CtValue::Failed(CtReport::Told(e)) => format!("{{\"err\":{}}}", e.to_json()),
            CtValue::Unit => "null".to_string(),
            // c139: a closure has no JSON representation — unreachable in
            // practice (a value routed through the encoder never contains
            // one; codable derives never target a `fn`-typed field).
            CtValue::Closure(_) => "null".to_string(),
        }
    }

    pub fn serialize(&self) -> String {
        match self {
            CtValue::Int(n) => format!("{}i64", n),
            CtValue::Float(value) => value.serialize(),
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => format!("{:?}", c),
            CtValue::Str(s) => format!("{:?}.to_string()", s),
            // A `comptime`-computed `BigInt` is baked into the generated
            // program as a call into the same prelude constructor AOT code
            // uses for `BigInt("…")` (`jet_std::JetBigInt::from_str`), fed
            // its canonical decimal string — never re-deriving the limbs in
            // codegen (I3: codegen stays dumb).
            CtValue::BigInt(b) => format!(
                "jet_std::JetBigInt::from_str({:?}).unwrap()",
                b.to_string_rep()
            ),
            CtValue::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| format!("{}u8", b)).collect();
                format!("vec![{}]", parts.join(", "))
            }
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.serialize()).collect();
                format!("vec![{}]", parts.join(", "))
            }
            CtValue::Map(m) => {
                if m.is_empty() {
                    "JetMap::new()".to_string()
                } else {
                    let mut s = String::from("{ let mut _m = JetMap::new(); ");
                    for (k, v) in m {
                        s.push_str(&format!(
                            "_m.insert(({}), {}); ",
                            k.to_value().serialize(),
                            v.serialize()
                        ));
                    }
                    s.push_str("_m }");
                    s
                }
            }
            // D-FAIL-CARRIER1=A: one carrier, so one pair of Rust spellings.
            CtValue::Present(v) => format!("Ok({})", v.serialize()),
            CtValue::Failed(CtReport::Clean(_)) => "Err(JetAbsent)".to_string(),
            CtValue::Failed(CtReport::Told(e)) => format!("Err({})", e.serialize()),
            CtValue::Struct { type_name, fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("{}: {}", ct_mangle(n), v.serialize()))
                    .collect();
                format!("{} {{ {} }}", user_type_rust(type_name), parts.join(", "))
            }
            CtValue::Enum {
                type_name,
                variant,
                args,
            } if type_name == "RemoveBy" => {
                debug_assert!(args.is_empty(), "RemoveBy variants are unit values");
                format!("JetRemoveBy::{}", crate::Syntax::generated_suffix(variant))
            }
            CtValue::Enum {
                type_name,
                variant,
                args,
            } if type_name == "Loadable" => match (variant.as_str(), args.first()) {
                ("Idle", _) => "JetLoadable::<(), ()>::Idle".to_string(),
                ("Loading", _) => "JetLoadable::<(), ()>::Loading".to_string(),
                ("Loaded", Some((_, value))) => {
                    format!("JetLoadable::<_, ()>::Loaded({})", value.serialize())
                }
                ("Failed", Some((_, value))) => {
                    format!("JetLoadable::<(), _>::Failed({})", value.serialize())
                }
                _ => unreachable!("invalid Loadable comptime value"),
            },
            CtValue::Enum {
                type_name,
                variant,
                args,
            } => {
                // Anonymous unions use compiler-owned bare Rust variants;
                // ordinary user enums keep the `__jet_` namespace prefix.
                let variant = if type_name.starts_with("__JetUnion_") {
                    variant.clone()
                } else {
                    ct_mangle(variant)
                };
                let prefix = format!("{}::{}", user_type_rust(type_name), variant);
                if args.is_empty() {
                    prefix
                } else if args.iter().all(|(label, _)| label.is_none()) {
                    let parts: Vec<String> = args.iter().map(|(_, v)| v.serialize()).collect();
                    format!("{}({})", prefix, parts.join(", "))
                } else {
                    let parts: Vec<String> = args
                        .iter()
                        .filter_map(|(label, v)| {
                            label
                                .as_ref()
                                .map(|name| format!("{}: {}", ct_mangle(name), v.serialize()))
                        })
                        .collect();
                    format!("{} {{ {} }}", prefix, parts.join(", "))
                }
            }
            CtValue::Unit => "()".to_string(),
            // c139: a closure can't be baked into a Rust literal — but it
            // also can't reach here: a `comptime` binding's value must be an
            // encodable type (sema rejects a function-typed `comptime` const
            // before evaluation), and this method exists only to serialize a
            // `comptime` binding's final value for codegen.
            CtValue::Closure(_) => {
                unreachable!("a closure value can't be a comptime binding's serialized result")
            }
        }
    }
}

fn ct_mangle(name: &str) -> String {
    mangle(name)
}

#[cfg(test)]
mod tests {
    use super::{as_bytes, CtFloat, CtValue};
    use crate::AST::{BinOp, Type};
    use crate::Diagnostics::Span;

    #[test]
    fn ct_float_preserves_width_native_equality_and_rounding() {
        let rounded = CtFloat::f32(16_777_217.0);
        let wide = CtFloat::f64(16_777_217.0);

        assert_eq!(rounded.jet_type(), Type::Float32);
        assert_eq!(wide.jet_type(), Type::Float);
        assert_ne!(rounded, wide);
        assert_eq!(rounded.render(), "16777216.0");
        assert_eq!(rounded.binop(BinOp::Add, CtFloat::f32(1.0)), Some(rounded));
        assert_eq!(CtFloat::f32(0.0), CtFloat::f32(-0.0));
        assert_ne!(CtFloat::f32(f32::NAN), CtFloat::f32(f32::NAN));
    }

    #[test]
    fn ct_float_render_json_and_rust_serialization_are_width_native() {
        let nested = CtValue::List(vec![
            CtValue::Float(CtFloat::f32(16_777_217.0)),
            CtValue::Float(CtFloat::f64(16_777_217.0)),
        ]);

        assert_eq!(format!("{:?}", CtFloat::f32(-0.0)), "-0.0");
        assert_eq!(CtValue::Float(CtFloat::f32(1.0)).to_json(), "1.0");
        assert_eq!(nested.to_json(), "[16777216.0,16777217.0]");
        assert_eq!(nested.serialize(), "vec![16777216.0f32, 16777217.0f64]");
        assert_eq!(CtFloat::f32(f32::NAN).serialize(), "f32::NAN");
        assert_eq!(CtFloat::f64(f64::NEG_INFINITY).serialize(), "f64::NEG_INFINITY");
    }

    #[test]
    fn loadable_uses_aot_debug_inside_user_enum_payload() {
        let loaded = CtValue::Enum {
            type_name: "Loadable".to_string(),
            variant: "Loaded".to_string(),
            args: vec![(None, CtValue::Int(7))],
        };
        let wrapped = CtValue::Enum {
            type_name: "Wrapper".to_string(),
            variant: "Ready".to_string(),
            args: vec![(None, loaded)],
        };

        assert_eq!(wrapped.jet_show(), "__jet_Ready(Loaded(7))");
    }

    #[test]
    fn anonymous_union_serialization_uses_bare_generated_variant() {
        let value = CtValue::Enum {
            type_name: "__JetUnion_Int_String".to_string(),
            variant: "Int".to_string(),
            args: vec![(None, CtValue::Int(3))],
        };

        assert_eq!(
            value.serialize(),
            "__jet___JetUnion_Int_String::Int(3i64)"
        );
    }

    #[test]
    fn as_bytes_uses_one_shared_rejection() {
        let span = Span::new(4, 7);
        for value in [
            CtValue::Int(1),
            CtValue::List(vec![CtValue::Int(256)]),
            CtValue::List(vec![CtValue::Str("not a byte".to_string())]),
        ] {
            let error = as_bytes(&value, span)
                .expect_err("invalid byte value must be rejected");
            assert_eq!(error.code, "E0956");
            assert_eq!(error.what, "this `as_bytes` call can't run at compile time yet");
            assert_eq!(
                error.why,
                "the canonical TIR evaluator doesn't cover this construct yet"
            );
            assert_eq!(error.fix, "use a simpler form, or run via `jet build` / `jet run`");
        }
    }

    #[test]
    fn default_err_ct_value_uses_the_prelude_shape_and_renderer() {
        let cause = CtValue::Struct {
            type_name: crate::Syntax::TYPE_ERR.to_string(),
            fields: vec![
                ("message".to_string(), CtValue::Str("bad input".to_string())),
                ("code".to_string(), CtValue::absent(Type::String)),
                (
                    "cause".to_string(),
                    CtValue::absent(Type::Named(crate::Syntax::TYPE_ERR.to_string())),
                ),
            ],
        };
        let value = CtValue::Struct {
            type_name: crate::Syntax::TYPE_ERR.to_string(),
            fields: vec![
                ("message".to_string(), CtValue::Str("config failed".to_string())),
                (
                    "code".to_string(),
                    CtValue::Present(Box::new(CtValue::Str("CFG404".to_string()))),
                ),
                ("cause".to_string(), CtValue::Present(Box::new(cause))),
            ],
        };

        let error = value.to_jet_err().expect("checked Err shape must marshal");
        assert_eq!(crate::Outcome::jet_err_message(&error), "config failed");
        assert_eq!(value.jet_show(), "Error [CFG404]: config failed\n  cause: bad input");
        assert_eq!(CtValue::from_jet_err(&error), value);
    }
}
