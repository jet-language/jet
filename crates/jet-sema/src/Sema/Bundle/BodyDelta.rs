//! #2517 stage S3R: the checked-body delta of one function.
//!
//! Body checking rewrites a function in place: it elaborates value-block
//! tails, fills receiver, binding and literal types, and inserts `Ok`,
//! `Place` and `Copy` wrappers. Item reuse therefore stores the parts of a
//! function the checker rewrites (parameters, return type, body statements,
//! reactive upgrade lines) as they were after the check, and installs them on
//! a reused function instead of checking it again.
//!
//! The encoding is lossless for the node kinds it lists and refuses every
//! other kind; a sema-filled field it does not carry must be empty. A delta is
//! recorded only when applying it to the parsed function rebuilds the checked
//! function exactly (`Debug` equality, see `ItemReuse::record`), so a missing
//! node kind costs reuse and never changes a result.
//!
//! Spans are stored relative to the function's start, so they move with the
//! item. Only spans the parser gave this function may be stored: a span sema
//! copied from another declaration (a callee's default argument, a type from
//! its signature) belongs to another item or file and would not move with
//! this one, so a delta holding one is refused. The synthesized empty span at
//! offset 0 is position independent and stored as it is.
//!
//! The same codec carries compile-time values (`encode_value`), which
//! `ComptimeReuse` stores for evaluated constants.

use crate::Diagnostics::Span;
use crate::Numeric::CtBigInt;
use crate::AST::{
    AccessConvention, ArithmeticMode, ArithmeticOperation, ArithmeticPolicyFact, BinOp, BindName,
    BindPattern, Binding, ByteTextPart, Call, CallArg, CallArgFlags, CtFloat, CtKey, CtReport,
    CtValue, EnumLitArg, Expr, ForKind, Func, IndexKind, InternalTag, LValue, OrFallback, Param,
    ParamZone, PatSlot, Pattern, PlaceAccess, Stmt, StrFormat, StrPart, StructPatField, SwitchArm,
    TagMarker, TryConvert, Type, TypedLitBody, UnOp, UnitFormat,
};
use std::collections::{BTreeMap, HashSet};

/// Why a checked function cannot be stored as a delta.
pub(super) type Refusal = &'static str;

/// The rewritable parts of `checked`, relative to its start. `pristine` is
/// the function as parsed; it names the spans the delta may hold.
pub(super) fn encode(pristine: &Func, checked: &Func) -> Result<Vec<u8>, Refusal> {
    // Publishing a view provenance also writes the shared signature table,
    // which a reused body would skip.
    if checked.return_view_provenance.is_some() {
        return Err("view provenance");
    }
    let mut authored = Writer {
        bytes: Vec::new(),
        start: pristine.span.start,
        end: pristine.span.end,
        spans: Spans::Collect(HashSet::new()),
    };
    pristine.params.put(&mut authored)?;
    pristine.return_type.put(&mut authored)?;
    pristine.body.put(&mut authored)?;
    let authored = match authored.spans {
        Spans::Collect(spans) | Spans::Require(spans) => spans,
    };
    let mut writer = Writer {
        bytes: Vec::new(),
        start: checked.span.start,
        end: checked.span.end,
        spans: Spans::Require(authored),
    };
    checked.params.put(&mut writer)?;
    checked.return_type.put(&mut writer)?;
    checked.body.put(&mut writer)?;
    checked.reactive_upgrades.put(&mut writer)?;
    Ok(writer.bytes)
}

/// Install a stored delta on `function`. `None`, with `function` untouched,
/// when the bytes do not decode completely.
pub(super) fn apply(function: &mut Func, bytes: &[u8]) -> Option<()> {
    let mut reader = Reader {
        bytes,
        at: 0,
        start: function.span.start,
    };
    let params = Vec::<Param>::get(&mut reader)?;
    let return_type = Option::<Type>::get(&mut reader)?;
    let body = Vec::<Stmt>::get(&mut reader)?;
    let reactive_upgrades = Vec::<String>::get(&mut reader)?;
    if reader.at != bytes.len() {
        return None;
    }
    function.params = params;
    function.return_type = return_type;
    function.body = body;
    function.reactive_upgrades = reactive_upgrades;
    Some(())
}

/// A compile-time value, or why it cannot be stored. A value holding a
/// source span (a map type's key span) is refused: spans are positions the
/// value's reuse key does not cover. Closures and told reports are refused.
pub(super) fn encode_value(value: &CtValue) -> Result<Vec<u8>, Refusal> {
    // No span passes the bounds `1..=1` but the empty `(1, 1)`, and no
    // span is authored, so only the position-free empty span is stored.
    let mut writer = Writer {
        bytes: Vec::new(),
        start: 1,
        end: 1,
        spans: Spans::Require(HashSet::new()),
    };
    value.put(&mut writer)?;
    Ok(writer.bytes)
}

/// The value `encode_value` stored, or `None` when the bytes do not decode
/// completely.
pub(super) fn decode_value(bytes: &[u8]) -> Option<CtValue> {
    let mut reader = Reader {
        bytes,
        at: 0,
        start: 1,
    };
    let value = CtValue::get(&mut reader)?;
    (reader.at == bytes.len()).then_some(value)
}

struct Writer {
    bytes: Vec<u8>,
    /// The function's span: stored spans are relative to `start`.
    start: usize,
    end: usize,
    spans: Spans,
}

/// Item spans the parser authored, as `(start, end)`.
enum Spans {
    /// Encoding the parsed function: collect its spans.
    Collect(HashSet<(usize, usize)>),
    /// Encoding the checked function: every span must be one of these.
    Require(HashSet<(usize, usize)>),
}

impl Writer {
    fn byte(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn uint(&mut self, mut value: u64) {
        loop {
            let low = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                self.bytes.push(low);
                return;
            }
            self.bytes.push(low | 0x80);
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    /// Start of the function the spans are installed on.
    start: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let value = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(value)
    }

    fn take(&mut self, count: usize) -> Option<&[u8]> {
        let end = self.at.checked_add(count)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn uint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = self.byte()?;
            if shift >= 64 {
                return None;
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
            shift += 7;
        }
    }

    fn size(&mut self) -> Option<usize> {
        usize::try_from(self.uint()?).ok()
    }

    /// An element count. Every element takes at least one byte, so a count
    /// beyond the remaining bytes is corrupt, never a huge allocation.
    fn count(&mut self) -> Option<usize> {
        let count = self.size()?;
        (count <= self.bytes.len() - self.at).then_some(count)
    }
}

trait Codec: Sized {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal>;
    fn get(reader: &mut Reader<'_>) -> Option<Self>;
}

/// A sema-filled field the delta does not carry: it must be empty in the
/// checked body and is empty after decoding.
trait Absent {
    fn is_absent(&self) -> bool;
    fn absent() -> Self;
}

impl<T> Absent for Option<T> {
    fn is_absent(&self) -> bool {
        self.is_none()
    }

    fn absent() -> Self {
        None
    }
}

impl<T> Absent for Vec<T> {
    fn is_absent(&self) -> bool {
        self.is_empty()
    }

    fn absent() -> Self {
        Vec::new()
    }
}

impl Codec for bool {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        writer.byte(u8::from(*self));
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        match reader.byte()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

impl Codec for u8 {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        writer.byte(*self);
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        reader.byte()
    }
}

impl Codec for u32 {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        writer.uint(u64::from(*self));
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        u32::try_from(reader.uint()?).ok()
    }
}

impl Codec for usize {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        writer.uint(*self as u64);
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        reader.size()
    }
}

impl Codec for i64 {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        writer.uint(((*self << 1) ^ (*self >> 63)) as u64);
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        let value = reader.uint()?;
        Some(((value >> 1) as i64) ^ -((value & 1) as i64))
    }
}

impl Codec for f64 {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        writer.bytes.extend_from_slice(&self.to_bits().to_le_bytes());
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        let bytes: [u8; 8] = reader.take(8)?.try_into().ok()?;
        Some(f64::from_bits(u64::from_le_bytes(bytes)))
    }
}

impl Codec for char {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        writer.uint(u64::from(u32::from(*self)));
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        char::from_u32(u32::try_from(reader.uint()?).ok()?)
    }
}

impl Codec for String {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        writer.uint(self.len() as u64);
        writer.bytes.extend_from_slice(self.as_bytes());
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        let len = reader.size()?;
        String::from_utf8(reader.take(len)?.to_vec()).ok()
    }
}

impl Codec for Span {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        if self.start == 0 && self.end == 0 {
            // At offset 0 a synthesized empty span and an empty span at the
            // item's start cannot be told apart.
            if writer.start == 0 {
                return Err("ambiguous empty span");
            }
            writer.byte(0);
            return Ok(());
        }
        if self.start < writer.start || self.end < self.start || self.end > writer.end {
            return Err("span outside the item");
        }
        match &mut writer.spans {
            Spans::Collect(spans) => {
                spans.insert((self.start, self.end));
            }
            Spans::Require(spans) => {
                if !spans.contains(&(self.start, self.end)) {
                    return Err("span not authored by the item");
                }
            }
        }
        writer.byte(1);
        writer.uint((self.start - writer.start) as u64);
        writer.uint((self.end - self.start) as u64);
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        match reader.byte()? {
            0 => Some(Span::new(0, 0)),
            1 => {
                let start = reader.start.checked_add(reader.size()?)?;
                let end = start.checked_add(reader.size()?)?;
                Some(Span::new(start, end))
            }
            _ => None,
        }
    }
}

impl<T: Codec> Codec for Box<T> {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        (**self).put(writer)
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        T::get(reader).map(Box::new)
    }
}

impl<T: Codec> Codec for Option<T> {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        match self {
            None => {
                writer.byte(0);
                Ok(())
            }
            Some(value) => {
                writer.byte(1);
                value.put(writer)
            }
        }
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        match reader.byte()? {
            0 => Some(None),
            1 => T::get(reader).map(Some),
            _ => None,
        }
    }
}

impl<T: Codec> Codec for Vec<T> {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        writer.uint(self.len() as u64);
        for item in self {
            item.put(writer)?;
        }
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        let count = reader.count()?;
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(T::get(reader)?);
        }
        Some(items)
    }
}

impl<A: Codec, B: Codec> Codec for (A, B) {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        self.0.put(writer)?;
        self.1.put(writer)
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        let first = A::get(reader)?;
        Some((first, B::get(reader)?))
    }
}

impl<A: Codec, B: Codec, C: Codec> Codec for (A, B, C) {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        self.0.put(writer)?;
        self.1.put(writer)?;
        self.2.put(writer)
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        let first = A::get(reader)?;
        let second = B::get(reader)?;
        Some((first, second, C::get(reader)?))
    }
}

impl<K: Codec + Ord, V: Codec> Codec for BTreeMap<K, V> {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        writer.uint(self.len() as u64);
        for (key, value) in self {
            key.put(writer)?;
            value.put(writer)?;
        }
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        let count = reader.count()?;
        let mut map = BTreeMap::new();
        for _ in 0..count {
            let key = K::get(reader)?;
            if map.insert(key, V::get(reader)?).is_some() {
                return None;
            }
        }
        Some(map)
    }
}

/// Encode one field, or require an uncarried (`= absent`) field to be empty.
macro_rules! put_field {
    ($writer:ident, $value:ident) => {
        Codec::put($value, $writer)?
    };
    ($writer:ident, $value:ident, absent) => {
        if !Absent::is_absent($value) {
            return Err(concat!("sema-filled `", stringify!($value), "`"));
        }
    };
}

macro_rules! get_field {
    ($reader:ident, $value:ident) => {
        Codec::get($reader)?
    };
    ($reader:ident, $value:ident, absent) => {
        Absent::absent()
    };
}

/// A struct codec. Destructuring names every field, so a field added to
/// the AST fails to compile here until it is carried or marked `= absent`.
macro_rules! codec_struct {
    ($ty:ident { $($field:ident $(= $mark:ident)?),* $(,)? }) => {
        impl Codec for $ty {
            fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
                let $ty { $($field),* } = self;
                $( put_field!(writer, $field $(, $mark)?); )*
                Ok(())
            }

            fn get(reader: &mut Reader<'_>) -> Option<Self> {
                Some($ty { $( $field: get_field!(reader, $field $(, $mark)?) ),* })
            }
        }
    };
}

/// An enum codec over the listed variants; any other variant is refused.
/// A listed variant names every field, as `codec_struct` does.
macro_rules! codec_enum {
    ($ty:ident {
        $( $tag:literal => $variant:ident
            $( ( $($item:ident),* ) )?
            $( { $($field:ident $(= $mark:ident)?),* } )?
        ),* $(,)?
    }) => {
        impl Codec for $ty {
            #[allow(unreachable_patterns)]
            fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
                match self {
                    $(
                        $ty::$variant $( ( $($item),* ) )? $( { $($field),* } )? => {
                            writer.byte($tag);
                            $( $( put_field!(writer, $item); )* )?
                            $( $( put_field!(writer, $field $(, $mark)?); )* )?
                        }
                    )*
                    _ => return Err(concat!("unsupported ", stringify!($ty), " node")),
                }
                Ok(())
            }

            fn get(reader: &mut Reader<'_>) -> Option<Self> {
                Some(match reader.byte()? {
                    $(
                        $tag => $ty::$variant
                            $( ( $( get_field!(reader, $item) ),* ) )?
                            $( { $( $field: get_field!(reader, $field $(, $mark)?) ),* } )?,
                    )*
                    _ => return None,
                })
            }
        }
    };
}

codec_struct!(Param {
    convention,
    root,
    name,
    name_span,
    public_label,
    zone,
    ty,
    ty_span,
    default,
    variadic,
    variadic_bound_list,
    declared_view_from_names,
});

codec_enum!(ParamZone {
    0 => PositionalOnly,
    1 => Either,
    2 => LabelOnly,
});

codec_enum!(AccessConvention {
    0 => Read,
    1 => Write,
    2 => Move,
});

codec_enum!(PlaceAccess {
    0 => Read,
    1 => Write,
    2 => Take,
});

codec_enum!(Type {
    0 => Int,
    1 => Float,
    2 => Bool,
    3 => String,
    4 => Char,
    5 => List(elem),
    6 => Map { key, key_span, value },
    7 => Shared(inner),
    8 => Option(inner),
    9 => Result { ok, err },
    10 => Fn {
        params,
        ret,
        effect_bound,
        param_contract,
        call_metadata = absent,
        return_view_provenance = absent
    },
    11 => Named(name),
    12 => Apply { name, args },
    13 => TraitObject(names),
    14 => Tuple(fields),
    15 => IntN { signed, bits },
    16 => InlineRange { base, lo, hi },
    17 => Float32,
    18 => Tagged { marker, inner },
    19 => Union(members),
});

codec_enum!(TagMarker {
    0 => User(name),
    1 => Internal(tag),
});

codec_enum!(InternalTag {
    0 => CoreCryptoNominal,
    1 => DeterministicClock,
    2 => SystemClock,
    3 => ExpiringSecretLoan,
    4 => SharedGuardRead,
    5 => SharedGuardEdit,
    6 => TerminalFactSet,
    7 => CppCallbackAbi,
    8 => AllocatorView,
});

codec_enum!(Stmt {
    0 => Expr(value),
    1 => Val(binding),
    2 => Assign { target, op, op_span, value },
    3 => Return(value, span),
    4 => While { cond, body, span, arrow_body, label },
    5 => For {
        var,
        var_span,
        var2,
        kind,
        body,
        span,
        arrow_body,
        label,
        auto_vectorization = absent
    },
    6 => Switch { subject, arms, else_body, span },
    7 => Break(span),
    8 => BreakValue(value, span),
    9 => Continue(span),
    10 => BreakLabel(label, span),
    11 => BreakLabelValue(label, label_span, value, span),
    12 => ContinueLabel(label, span),
    13 => Loop { body, span, arrow_body, label },
    14 => CountedLoop { init, cond, step, body, span, arrow_body, label },
    15 => Unsafe { audit, audit_expr, body, span },
    16 => Impure { reason, reason_expr, body, span },
    17 => Reactive { body, span },
    18 => Shield { body, span },
    19 => Region { name, name_span, body, span },
    20 => TaskGroup { name, name_span, limit, body, span },
    21 => Layout { name, name_span, body, span },
    22 => AuthorityScope { caps, caps_span, binding, binding_span, body, span },
    23 => ComptimeIf { cond, cond_span, then_body, else_body, span, selected_then },
    24 => ComptimeSwitch { subject, arms, else_body, span },
    25 => ComptimeBlock { body, is_template_loop, span },
    26 => ContextBlock { fields, body, span },
    27 => Live { body, span },
    28 => AssumeDet { reason, reason_expr, body, span },
    29 => Transact { name, name_span, body, implicit, span },
    30 => Yield(value, span),
    31 => ScopeMember { name, name_span, args, args_span, body, dsl, dot_span, span },
    32 => DeferClose { close, span },
});

codec_struct!(SwitchArm { cond, body, span });

codec_struct!(Binding {
    mutable,
    markers = absent,
    reactive_upgrade,
    meta = absent,
    name,
    name_span,
    sigil_span,
    pattern,
    ty,
    ty_span,
    init,
    is_comptime,
    ct = absent,
    uninit,
    arena_view,
    string_view,
    gc_promotion = absent,
    gc_transferred,
});

codec_enum!(LValue {
    0 => Local { name, name_span },
    1 => Index { base, index, span, kind },
    2 => Field { base, field, span },
});

codec_enum!(IndexKind {
    0 => Unknown,
    1 => List,
    2 => Range,
    3 => FixedListProof,
    4 => Map,
    5 => Lane(name),
    6 => User(name),
    7 => LayoutField(name),
    8 => Pool,
});

codec_enum!(ForKind {
    0 => Range { start, end, step, exclusive },
    1 => In { collection, step },
});

codec_enum!(BindPattern {
    0 => Struct { type_name, type_span, fields, rest, span },
    1 => List { elems, span },
    2 => Tuple { elems, span },
    3 => Refutable { pattern, fallback, names, span, synthesized },
});

codec_struct!(BindName { name, span, rename });

codec_enum!(BinOp {
    0 => Add,
    1 => Sub,
    2 => Mul,
    3 => Div,
    4 => FloorDiv,
    5 => Mod,
    6 => Rem,
    7 => Pow,
    8 => BitAnd,
    9 => BitOr,
    10 => BitXor,
    11 => Shl,
    12 => Shr,
    13 => Eq,
    14 => Ne,
    15 => Lt,
    16 => Gt,
    17 => Le,
    18 => Ge,
    19 => Compare,
    20 => And,
    21 => Or,
});

codec_enum!(UnOp {
    0 => Neg,
    1 => Not,
});

codec_enum!(Expr {
    0 => Str(parts, span),
    1 => Int(value, span, width, suffix),
    2 => Float(value, span, narrow, suffix),
    3 => Bool(value, span),
    4 => Unit(span),
    5 => Char(value, span),
    6 => ListLit(items, span),
    7 => MemberSpread { base, members, span },
    8 => Spread(value, span),
    9 => MapLit(entries, span),
    10 => Index { base, index, span, kind },
    11 => Slice { base, start, end, range, span },
    12 => Range { start, end, exclusive, span },
    13 => Ident(name, span),
    14 => Call(call),
    15 => Unary(op, value, span),
    16 => Binary(op, left, right, span),
    17 => CompareChain { operands, ops, hooks, span },
    18 => UnitLit { raw, int, float, suffix, suffix_span, span },
    19 => Deref(value, span),
    20 => RawOf(value, span),
    21 => Copy(value, span),
    22 => Place(value, access, span),
    23 => Field(base, name, span),
    24 => OptField { base, member, member_span, flatten, span },
    25 => MethodCall {
        receiver,
        method,
        method_span,
        owner_type_args,
        type_args,
        args,
        recv_type,
        resolved_ret,
        operator_rhs,
        checked_widen
    },
    26 => StructLit { type_name, type_args, import_ns, as_trait, fields, inferred, span },
    27 => TypedLit { head, body, span },
    28 => EnumLit { type_name, variant, variant_span, args, leading_dot, span },
    29 => Tainted(value, tag, span),
    30 => Present(value, span),
    31 => Absent(span),
    32 => Todo { span, expected_type },
    33 => NoElse(span),
    34 => ReduceMarker(name, span),
    35 => PatternTest { subject, pattern, span },
    36 => Ok(value, span),
    37 => Err(value, span),
    38 => Try(value, span, convert, context),
    39 => OrFallback { value, fallback, is_option, span },
    40 => If { cond, then_body, then_value, else_body, else_value, span },
    41 => TupleLit(fields, span, ty),
    42 => CallValue { callee, args, span },
    43 => PtrFromAddr { alias, alias_span, elem, addr, span },
    44 => Paren(value, span),
});

codec_enum!(StrPart {
    0 => Lit(text),
    1 => Interp(value, format),
});

codec_enum!(StrFormat {
    0 => Display,
    1 => Debug,
    2 => Pretty,
    3 => Fixed(digits),
    4 => Grouped(digits),
    5 => Hex(digits),
    6 => Pad { width, fill },
    7 => PadLeft { width, fill },
    8 => Sci(digits),
    9 => Percent(digits),
    10 => Bin,
    11 => Oct,
    12 => Unit(unit),
});

codec_enum!(UnitFormat {
    0 => Symbol,
    1 => Name,
    2 => Bare,
});

codec_enum!(TypedLitBody {
    0 => Fields(fields),
    1 => Elements(items),
    2 => Entries(entries),
    3 => Value(value),
    4 => ByteText(parts),
    5 => Empty,
});

codec_enum!(ByteTextPart {
    0 => Lit(text),
    1 => Byte(value),
});

codec_struct!(Call {
    name,
    name_span,
    type_args,
    args,
    resolved_ret,
    range_checked,
    widen_approx,
});

codec_struct!(CallArg { convention, expr, span, flags, label, spread });

codec_struct!(CallArgFlags {
    implicit_clone,
    shared_auto_clone,
    owned_last_use,
    shared_access_desugar,
    is_trailing_block,
    template_items = absent,
    c_callback_symbol,
    c_callback_function_key,
    c_callback_managed,
    c_callback_plan_digest,
    c_callback_identity,
    trusted_html,
    source_index,
    binder_slot,
    binder_label,
    binder_refs,
    binder_site,
    callable_policy = absent,
    authority_boundary,
    arithmetic_policy,
});

codec_struct!(ArithmeticPolicyFact { mode, scope_span, operation_span, operation });

codec_enum!(ArithmeticMode {
    0 => Checked,
    1 => Wrapping,
    2 => Saturating,
});

codec_enum!(ArithmeticOperation {
    0 => Add,
    1 => Sub,
    2 => Mul,
    3 => Neg,
    4 => Pow,
});

codec_enum!(EnumLitArg {
    0 => Positional(expr),
    1 => Named { label, expr },
});

codec_enum!(Pattern {
    0 => Variant { variant, bindings, leading_dot, span },
    1 => Present { binding, binding_span, inner, span },
    2 => Absent(span),
    3 => Ok { binding, binding_span, inner, span },
    4 => Err { binding, binding_span, inner, span },
    5 => Range { lo, hi, span },
    6 => Or(alternatives, span),
    7 => Struct { fields, rest, span },
});

codec_enum!(PatSlot {
    0 => Wildcard,
    1 => Bind { name, span },
    2 => Range { lo, hi },
    3 => Nested(pattern),
    4 => Named { field, field_span, slot },
    5 => Rest(span),
});

codec_enum!(StructPatField {
    0 => Bind { field, field_span, local, local_span },
    1 => Value { field, field_span, value },
});

codec_enum!(OrFallback {
    0 => Value(value),
    1 => Block { body, value, span },
    2 => Return(value, span),
    3 => Panic { name_span, args },
    4 => Break(span),
    5 => Continue(span),
    6 => BreakLabel(label, span),
    7 => ContinueLabel(label, span),
});

codec_enum!(TryConvert {
    0 => None,
    1 => Never,
    2 => DefaultErr,
    3 => Typed { fn_name, source, target },
    4 => WidenUnion { enum_name, tag },
});

// Compile-time values (`encode_value`).

impl Codec for CtFloat {
    fn put(&self, writer: &mut Writer) -> Result<(), Refusal> {
        match self {
            CtFloat::F32(value) => {
                writer.byte(0);
                writer.bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            CtFloat::F64(value) => {
                writer.byte(1);
                value.put(writer)?;
            }
        }
        Ok(())
    }

    fn get(reader: &mut Reader<'_>) -> Option<Self> {
        match reader.byte()? {
            0 => {
                let bytes: [u8; 4] = reader.take(4)?.try_into().ok()?;
                Some(CtFloat::F32(f32::from_bits(u32::from_le_bytes(bytes))))
            }
            1 => Some(CtFloat::F64(f64::get(reader)?)),
            _ => None,
        }
    }
}

codec_struct!(CtBigInt { negative, limbs });

codec_enum!(CtKey {
    0 => Int(value),
    1 => Str(value),
    2 => Bool(value),
    3 => Char(value),
    4 => Tuple(fields),
    5 => Struct { type_name, fields },
    6 => Enum { type_name, variant },
});

// A told report is refused: an error value can carry the source line of
// its context frames.
codec_enum!(CtReport {
    0 => Clean(ty),
});

codec_enum!(CtValue {
    0 => Int(value),
    1 => Float(value),
    2 => Bool(value),
    3 => Char(value),
    4 => Str(value),
    5 => BigInt(value),
    6 => Bytes(value),
    7 => List(values),
    8 => Map(entries),
    9 => Struct { type_name, fields },
    10 => Enum { type_name, variant, args },
    11 => Present(value),
    12 => Failed(report),
    13 => Unit,
});
