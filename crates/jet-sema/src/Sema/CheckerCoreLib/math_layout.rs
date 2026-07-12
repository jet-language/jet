use crate::AST::Type;

/// D-SIMD2 / D-LINALG1: is `name` a built-in math value type (lane or linalg)?
pub fn is_math_type(name: &str) -> bool {
    is_simd_lane_type(name) || is_linalg_type(name)
}

/// D-SIMD2: a portable SIMD lane type (`F32x4`/`F64x2`).
pub fn is_simd_lane_type(name: &str) -> bool {
    matches!(name, "F32x4" | "F64x2")
}

/// D-LINALG1: a linear-algebra value type (vectors + square matrices).
pub(crate) fn is_linalg_type(name: &str) -> bool {
    matches!(name, "Vec2" | "Vec3" | "Vec4" | "Mat3" | "Mat4")
}

/// The scalar component type of a math value type. SIMD lanes carry their named
/// float width (`F32x4` → `F32`/`Float32`, `F64x2` → `Float`); linalg types are
/// all `F64`/`Float`.
pub fn math_scalar_ty(name: &str) -> Type {
    match name {
        "F32x4" => Type::Float32,
        _ => Type::Float,
    }
}

/// The number of scalar slots in the positional constructor / `from_array` bridge.
/// Lanes: lane count. Vectors: dimension. Matrices: N*N (column-major flat).
pub(crate) fn math_arity(name: &str) -> usize {
    match name {
        "F32x4" => 4,
        "F64x2" => 2,
        "Vec2" => 2,
        "Vec3" => 3,
        "Vec4" => 4,
        "Mat3" => 9,
        "Mat4" => 16,
        _ => 0,
    }
}

/// D-SWIZZLE1: built-in vector/SIMD lane types that support `.xyz` member swizzles.
/// Matrices are not swizzleable.
pub fn is_swizzleable_math_type(name: &str) -> bool {
    matches!(name, "F32x4" | "F64x2" | "Vec2" | "Vec3" | "Vec4")
}

/// Outcome of parsing a swizzle member name (`xy`, `wzyx`, …) on a swizzleable type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwizzleParse {
    /// Valid lane indices in write order (x=0, y=1, z=2, w=3).
    Ok(Vec<usize>),
    /// A lane letter is out of range for this type (e.g. `.z` on `Vec2`).
    InvalidLane { lane: char },
    /// Not a swizzle pattern (wrong chars or length) — fall through to field lookup.
    NotSwizzle,
}

/// D-SWIZZLE1: parse `member` as a swizzle on `type_name`. Up to four `x`/`y`/`z`/`w`
/// letters; each must be in range for the type's lane count.
pub fn parse_swizzle_member(member: &str, type_name: &str) -> SwizzleParse {
    if !is_swizzleable_math_type(type_name) || member.is_empty() || member.len() > 4 {
        return SwizzleParse::NotSwizzle;
    }
    let max = math_arity(type_name);
    let mut lanes = Vec::with_capacity(member.len());
    for c in member.chars() {
        let idx = match c {
            'x' => 0,
            'y' => 1,
            'z' => 2,
            'w' => 3,
            _ => return SwizzleParse::NotSwizzle,
        };
        if idx >= max {
            return SwizzleParse::InvalidLane { lane: c };
        }
        lanes.push(idx);
    }
    SwizzleParse::Ok(lanes)
}

/// D-SWIZZLE1: the type of a read swizzle — one lane → scalar, N lanes → `VecN`
/// (or the same SIMD lane type when all lanes are selected).
pub fn swizzle_read_type(type_name: &str, lane_count: usize) -> Type {
    if lane_count == 1 {
        return math_scalar_ty(type_name);
    }
    if is_simd_lane_type(type_name) && lane_count == math_arity(type_name) {
        return Type::Named(type_name.to_string());
    }
    Type::Named(match lane_count {
        2 => "Vec2".to_string(),
        3 => "Vec3".to_string(),
        4 => "Vec4".to_string(),
        _ => unreachable!("swizzle lane count 2..=4"),
    })
}

/// D-SWIZZLE1: true when a write swizzle names the same source lane twice (`v.xx`).
pub fn swizzle_write_overlaps(lanes: &[usize]) -> bool {
    let mut seen = [false; 4];
    for &lane in lanes {
        if seen[lane] {
            return true;
        }
        seen[lane] = true;
    }
    false
}

/// D-SIMD2 / D-LINALG1: type-check a positional constructor `T(a, b, …)` for a
/// built-in math type. The arg types are bound by `expected` so a literal `1.0`
/// elaborates to the component type (`F32` for `F32x4`). Returns the field types
/// the caller must check each argument against; arity is `math_arity(name)`.
pub(crate) fn math_constructor_arg_types(name: &str) -> Option<Vec<Type>> {
    if !is_math_type(name) {
        return None;
    }
    let scalar = math_scalar_ty(name);
    // `from_array`-style construction of a matrix takes its N*N components in
    // column-major order; vectors/lanes take one scalar per slot.
    Some(vec![scalar; math_arity(name)])
}

/// D-SIMD2 / D-LINALG1: the `[T#N]` fixed-list bridge type for a math value type,
/// used by `T.from_array([..])` / `v.to_array()`. `None` for non-math types.
pub(crate) fn math_array_bridge_ty(name: &str) -> Option<Type> {
    if !is_math_type(name) {
        return None;
    }
    Some(Type::FixedList {
        elem: Box::new(math_scalar_ty(name)),
        len: math_arity(name) as u64,
    })
}

/// D-SIMD2 / D-LINALG1: type-check an INSTANCE method `recv.method(args)` on a
/// built-in math type. `Some(Some(t))` → returns `t`; `Some(None)` → not a method
/// (caller falls through to its normal "no such method" diagnostic).
pub fn math_method_return(name: &str, method: &str, n_args: usize) -> Option<Type> {
    let float = Type::Float;
    let scalar = math_scalar_ty(name);
    let self_ty = Type::Named(name.to_string());
    if is_simd_lane_type(name) {
        return match (method, n_args) {
            // Reductions collapse the lanes to a single scalar of the lane width.
            ("sum" | "product" | "min" | "max", 0) => Some(scalar),
            ("reduce", 1) => Some(scalar),
            // `[F32#4]` round-trip out.
            ("to_array", 0) => math_array_bridge_ty(name),
            _ => None,
        };
    }
    // linalg
    match name {
        "Vec2" | "Vec3" | "Vec4" => match (method, n_args) {
            ("dot", 1) => Some(float),
            // cross product is only defined for 3-vectors.
            ("cross", 1) if name == "Vec3" => Some(self_ty),
            ("length", 0) => Some(float),
            ("normalize", 0) => Some(self_ty),
            ("to_array", 0) => math_array_bridge_ty(name),
            _ => None,
        },
        "Mat3" | "Mat4" => match (method, n_args) {
            ("matmul", 1) => Some(self_ty.clone()),
            ("transpose", 0) => Some(self_ty),
            // `m * v` is the operator path; `transform` is the named method form.
            ("transform", 1) => Some(Type::Named(
                if name == "Mat3" { "Vec3" } else { "Vec4" }.to_string(),
            )),
            ("to_array", 0) => math_array_bridge_ty(name),
            _ => None,
        },
        _ => None,
    }
}

/// D-SIMD2 / D-LINALG1: type-check a STATIC method `T.method(args)` on a math
/// type. Only `splat` (lanes/vectors) and `from_array` are provided.
pub fn math_static_return(name: &str, method: &str, n_args: usize) -> Option<Type> {
    if !is_math_type(name) {
        return None;
    }
    let self_ty = Type::Named(name.to_string());
    match (method, n_args) {
        ("splat", 1) => Some(self_ty),
        ("from_array", 1) => Some(self_ty),
        _ => None,
    }
}

/// D-SIMD2 / D-LINALG1: the argument type a static method expects.
pub fn math_static_arg_ty(name: &str, method: &str) -> Option<Type> {
    match method {
        "splat" => Some(math_scalar_ty(name)),
        "from_array" => math_array_bridge_ty(name),
        _ => None,
    }
}

/// D-SIMD2 / D-LINALG1: the argument type an instance method expects (for the
/// single-arg methods). `None` means "no fixed arg type" (e.g. nullary methods).
pub(crate) fn math_method_arg_ty(name: &str, method: &str) -> Option<Type> {
    let self_ty = Type::Named(name.to_string());
    match (name, method) {
        (_, "dot") | (_, "cross") => Some(self_ty),
        (_, "matmul") => Some(self_ty),
        ("Mat3", "transform") => Some(Type::Named("Vec3".to_string())),
        ("Mat4", "transform") => Some(Type::Named("Vec4".to_string())),
        // `reduce(#Op)` takes a reduce-op marker, checked specially by the caller.
        _ => None,
    }
}

/// D-SIMD2: the closed set of reduce-op markers accepted by `v.reduce(#Op)`.
pub(crate) fn simd_reduce_markers() -> &'static [&'static str] {
    &["Add", "Mul", "Min", "Max"]
}

/// D-SIMD2 / D-LINALG1: type-check a binary operator between two math values.
/// Returns the result type, or `None` if the op isn't defined for these operands.
/// Operator overloading is blessed on this closed built-in family ONLY.
pub fn math_binop_result(op: crate::AST::BinOp, lt: &str, rt: &str) -> Option<Type> {
    use crate::AST::BinOp;
    let same = lt == rt;
    match op {
        // Element-wise add/sub require identical types.
        BinOp::Add | BinOp::Sub if same && is_math_type(lt) => Some(Type::Named(lt.to_string())),
        // Multiplication: lane×lane / vec×vec element-wise; matrix×vector transform.
        BinOp::Mul => match (lt, rt) {
            (a, b) if a == b && is_math_type(a) => Some(Type::Named(a.to_string())),
            ("Mat3", "Vec3") => Some(Type::Named("Vec3".to_string())),
            ("Mat4", "Vec4") => Some(Type::Named("Vec4".to_string())),
            _ => None,
        },
        // Division: lane÷lane element-wise (linalg has no `/`).
        BinOp::Div if same && is_simd_lane_type(lt) => Some(Type::Named(lt.to_string())),
        BinOp::Eq | BinOp::Ne if same && is_math_type(lt) => Some(Type::Bool),
        _ => None,
    }
}

/// D-LAYOUT1 / D-LAYOUT-GATES1: is `name` an axis-typed layout variable
/// (`HVar`/`VVar`/`LengthVar`)? `LengthVar` is axis-neutral: it combines with
/// either `HVar` or `VVar` without a mismatch, and is what a bare numeric
/// literal elaborates to in a layout-value position.
pub fn is_layout_axis_type(name: &str) -> bool {
    matches!(name, "HVar" | "VVar" | "LengthVar")
}

/// D-LAYOUT1: the full closed layout-value family (axis types + the
/// `Constraint`/`LayoutHandle` handles).
pub fn is_layout_type(name: &str) -> bool {
    is_layout_axis_type(name) || matches!(name, "Constraint" | "LayoutHandle")
}

/// D-LAYOUT1: the axis a value belongs to, for cross-axis checking. Plain
/// `Int`/`Float` are axis-neutral too (a bare numeric literal is allowed
/// anywhere a `LengthVar` is — same neutrality as `LengthVar` itself, so
/// `label.width >= 80.0` never needs an explicit `LengthVar(80.0)` wrapper).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutAxis {
    H,
    V,
    Neutral,
}

pub fn layout_axis_of(ty: &Type) -> Option<LayoutAxis> {
    match ty {
        Type::Named(n) if n == "HVar" => Some(LayoutAxis::H),
        Type::Named(n) if n == "VVar" => Some(LayoutAxis::V),
        Type::Named(n) if n == "LengthVar" => Some(LayoutAxis::Neutral),
        Type::Int | Type::Float => Some(LayoutAxis::Neutral),
        _ => None,
    }
}

/// D-LAYOUT1: combine two axes for `+`/`-` (same-axis closure) or a
/// comparison (`>=`/`<=`/`==`, GATE 1). `None` = cross-axis mismatch
/// (E2932, `E-LAYOUT-AXIS-MISMATCH`).
fn layout_axis_combine(a: LayoutAxis, b: LayoutAxis) -> Option<LayoutAxis> {
    use LayoutAxis::*;
    match (a, b) {
        (H, V) | (V, H) => None,
        (H, _) | (_, H) => Some(H),
        (V, _) | (_, V) => Some(V),
        (Neutral, Neutral) => Some(Neutral),
    }
}

fn layout_axis_type_name(axis: LayoutAxis) -> &'static str {
    match axis {
        LayoutAxis::H => "HVar",
        LayoutAxis::V => "VVar",
        LayoutAxis::Neutral => "LengthVar",
    }
}

/// D-LAYOUT1 / D-LAYOUT-GATES1: type-check a binary operator between two
/// layout values — mirrors `math_binop_result`'s closed-operator pattern
/// exactly (GATE 1 is what lets the comparison arms return `Constraint`
/// instead of `Bool`; this is the ONLY place that blessing is wired, no
/// parallel mechanism). `Some(Ok(ty))` = success; `Some(Err(()))` = axis
/// mismatch (caller emits E2932 naming both axes); `None` = not a layout
/// combination at all (caller falls through to normal operator checking).
pub fn layout_binop_result(
    op: crate::AST::BinOp,
    lt: &Type,
    rt: &Type,
) -> Option<Result<Type, ()>> {
    use crate::AST::BinOp;
    // At least one side must be an actual layout axis type — `Int + Float`
    // (both merely "neutral") is not our concern.
    let l_is_layout = matches!(lt, Type::Named(n) if is_layout_axis_type(n));
    let r_is_layout = matches!(rt, Type::Named(n) if is_layout_axis_type(n));
    if !l_is_layout && !r_is_layout {
        return None;
    }
    let (Some(la), Some(ra)) = (layout_axis_of(lt), layout_axis_of(rt)) else {
        return None;
    };
    match op {
        BinOp::Add | BinOp::Sub => match layout_axis_combine(la, ra) {
            Some(axis) => Some(Ok(Type::Named(layout_axis_type_name(axis).to_string()))),
            None => Some(Err(())),
        },
        BinOp::Ge | BinOp::Le | BinOp::Eq => match layout_axis_combine(la, ra) {
            Some(_) => Some(Ok(Type::Named("Constraint".to_string()))),
            None => Some(Err(())),
        },
        _ => None,
    }
}

/// D-LAYOUT1: type-check an instance method on `LayoutHandle`/`Constraint`.
/// Mirrors `math_method_return`'s pattern (a plain match table, not a
/// HashMap — this family is tiny).
pub fn layout_method_return(name: &str, method: &str, n_args: usize) -> Option<Type> {
    match name {
        "LayoutHandle" => match (method, n_args) {
            ("h", 2) => Some(Type::Named("HVar".to_string())),
            ("v", 2) => Some(Type::Named("VVar".to_string())),
            ("value", 1) => Some(Type::Float),
            ("suggest", 2) => Some(Type::Named("Unit".to_string())),
            ("is_feasible", 0) => Some(Type::Bool),
            ("conflict", 0) => Some(Type::List(Box::new(Type::String))),
            _ => None,
        },
        "Constraint" => match (method, n_args) {
            ("required" | "strong" | "medium" | "weak", 0) => {
                Some(Type::Named("Constraint".to_string()))
            }
            _ => None,
        },
        _ => None,
    }
}

/// D-LAYOUT1: the fixed argument type a `LayoutHandle` method expects, by
/// position. `None` means "no plain fixed type" — `.value(v)`/`.suggest(v, _)`'s
/// first argument accepts ANY of `HVar`/`VVar`/`LengthVar` (checked by the
/// caller via `is_layout_axis_type`, not a single `Type`).
pub fn layout_method_arg_ty(method: &str, arg_index: usize) -> Option<Type> {
    match (method, arg_index) {
        ("h", 0) | ("h", 1) | ("v", 0) | ("v", 1) => Some(Type::String),
        ("suggest", 1) => Some(Type::Float),
        _ => None,
    }
}

/// D-BIGINT1 / D-DECIMAL1: binary ops on precise numeric types (no Int promotion).
pub fn precise_binop_result(op: crate::AST::BinOp, lt: &str, rt: &str) -> Option<Type> {
    use crate::Numeric::{is_bigint_type_name, is_decimal_type_name};
    use crate::AST::BinOp;
    let same = lt == rt;
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul if same && is_bigint_type_name(lt) => {
            Some(Type::Named(crate::Syntax::TYPE_BIGINT.to_string()))
        }
        BinOp::Add | BinOp::Sub | BinOp::Mul if same && is_decimal_type_name(lt) => {
            Some(Type::Named(crate::Syntax::TYPE_DECIMAL.to_string()))
        }
        BinOp::Eq | BinOp::Ne if same && (is_bigint_type_name(lt) || is_decimal_type_name(lt)) => {
            Some(Type::Bool)
        }
        _ => None,
    }
}

/// D-BIGINT1: mixing fixed `Int` with `BigInt` is rejected — no silent promotion.
pub fn precise_mix_error(lt: &Type, rt: &Type) -> Option<(&'static str, String, String)> {
    use crate::Numeric::{type_is_bigint, type_is_decimal};
    let li = lt.is_integer();
    let ri = rt.is_integer();
    if (type_is_bigint(lt) && ri) || (type_is_bigint(rt) && li) {
        return Some((
            "E0130",
            format!(
                "`Int` and `BigInt` can't be mixed — got `{}` and `{}`",
                lt.show(),
                rt.show()
            ),
            "fixed-width `Int` never promotes to `BigInt`; construct a `BigInt` explicitly with `BigInt(…)` or `BigInt(\"…\")`".to_string(),
        ));
    }
    if (type_is_decimal(lt) && rt.is_float()) || (type_is_decimal(rt) && lt.is_float()) {
        return Some((
            "E0131",
            format!(
                "`Float` and `Decimal` can't be mixed — got `{}` and `{}`",
                lt.show(),
                rt.show()
            ),
            "use `Decimal(\"…\")` for exact money arithmetic; `Float` is for approximate science"
                .to_string(),
        ));
    }
    if (type_is_bigint(lt) && type_is_decimal(rt)) || (type_is_bigint(rt) && type_is_decimal(lt)) {
        return Some((
            "E0132",
            format!(
                "`BigInt` and `Decimal` can't be mixed — got `{}` and `{}`",
                lt.show(),
                rt.show()
            ),
            "convert explicitly with `to_string()` / `Decimal(\"…\")` at a boundary".to_string(),
        ));
    }
    None
}
