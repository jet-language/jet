use crate::AST::Type;

/// D-SIMD2 / D-LINALG1: is `name` a built-in math value type (lane or linalg)?
pub fn is_math_type(name: &str) -> bool {
    is_simd_lane_type(name) || is_linalg_type(name)
}

/// D-SIMD1/D-SIMD2/D-SIMD3: a named portable SIMD lane type.
pub fn is_simd_lane_type(name: &str) -> bool {
    crate::Syntax::is_simd_lane_type(name)
}

/// D-LINALG1: a linear-algebra value type (vectors + square matrices).
pub(crate) fn is_linalg_type(name: &str) -> bool {
    matches!(name, "Vec2" | "Vec3" | "Vec4" | "Mat3" | "Mat4")
}

/// The scalar component type of a math value type. SIMD lanes carry their named
/// D-SIMD3: the scalar component type of a lane. Linalg types are all
/// `F64`/`Float`.
pub fn math_scalar_ty(name: &str) -> Type {
    match crate::Syntax::simd_lane_layout(name).map(|(kind, _)| kind) {
        Some(crate::Syntax::SimdLaneKind::F32) => Type::Float32,
        Some(crate::Syntax::SimdLaneKind::I8) => Type::IntN {
            signed: true,
            bits: 8,
        },
        Some(crate::Syntax::SimdLaneKind::I16) => Type::IntN {
            signed: true,
            bits: 16,
        },
        Some(crate::Syntax::SimdLaneKind::I32) => Type::IntN {
            signed: true,
            bits: 32,
        },
        Some(crate::Syntax::SimdLaneKind::I64) => Type::IntN {
            signed: true,
            bits: 64,
        },
        Some(crate::Syntax::SimdLaneKind::U8) => Type::IntN {
            signed: false,
            bits: 8,
        },
        Some(crate::Syntax::SimdLaneKind::U16) => Type::IntN {
            signed: false,
            bits: 16,
        },
        Some(crate::Syntax::SimdLaneKind::U32) => Type::IntN {
            signed: false,
            bits: 32,
        },
        Some(crate::Syntax::SimdLaneKind::U64) => Type::IntN {
            signed: false,
            bits: 64,
        },
        Some(crate::Syntax::SimdLaneKind::F64) | None => Type::Float,
    }
}

/// Read a named SIMD type's lane measure from the shared registry projection.
/// The name remains the surface discriminator; the numeric payload comes from
/// the same Measure plane used by fixed lists, shapes, and dimensions.
fn registered_lane_arity(name: &str) -> Option<usize> {
    Type::Named(name.to_string())
        .knowledge_vector()
        .facts(crate::Registry::type_plane("Measure"))
        .find_map(|fact| match fact {
            crate::AST::KnowledgeFact::Measure(measure) if measure.kind() == "lane" => measure
                .literal_value()
                .and_then(|value| usize::try_from(value).ok()),
            _ => None,
        })
}

/// The number of scalar slots in the positional constructor / `from_array` bridge.
/// Lanes: the registered measure. Vectors: dimension. Matrices: N*N (column-major flat).
pub(crate) fn math_arity(name: &str) -> usize {
    if let Some(arity) = crate::Syntax::simd_lane_arity(name) {
        return arity;
    }
    if let Some(arity) = registered_lane_arity(name) {
        return arity;
    }
    match name {
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
        len: crate::AST::Measure::literal("length", math_arity(name) as u64),
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
        // `reduce(.Op)` takes a ReduceOp value, checked specially by the caller.
        _ => None,
    }
}

/// D-REDUCE-VALUE1=A: Core ReduceOp values accepted by `v.reduce(.Op)`.
pub(crate) fn simd_reduce_markers() -> &'static [&'static str] {
    crate::Syntax::REDUCE_OP_VALUES
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

fn math_scalar_operand(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int | Type::Float | Type::Float32 | Type::InlineRange { .. }
    ) || matches!(ty, Type::Named(name) if name == "Decimal" || name == "F64")
}

/// D-LINALG1 / D-SIMD2: a lane or vector times a scalar splat.
pub fn math_scalar_binop_result(op: crate::AST::BinOp, math_name: &str, scalar: &Type) -> Option<Type> {
    use crate::AST::BinOp;
    if !is_math_type(math_name) {
        return None;
    }
    if !matches!(op, BinOp::Mul | BinOp::Div) {
        return None;
    }
    if !math_scalar_operand(scalar) {
        return None;
    }
    Some(Type::Named(math_name.to_string()))
}

/// D-LAYOUT1 / D-LAYOUT-GATES1: is `name` an axis-typed layout variable
/// (`HVar`/`VVar`/`LengthVar`)? `LengthVar` is axis-neutral: it combines with
/// either `HVar` or `VVar` without a mismatch, and is what a bare numeric
/// literal elaborates to in a layout-value position.
pub fn is_layout_axis_type(name: &str) -> bool {
    matches!(name, "HVar" | "VVar" | "LengthVar")
}

/// D-LAYOUT1: the full closed layout-value family (axis types + the
/// `Constraint`/`Layout` handles).
pub fn is_layout_type(name: &str) -> bool {
    is_layout_axis_type(name) || matches!(name, "Constraint" | "Layout")
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
        Type::Int | Type::InlineRange { .. } | Type::Float => Some(LayoutAxis::Neutral),
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

/// D-LAYOUT1: type-check an instance method on `Layout`/`Constraint`.
/// Mirrors `math_method_return`'s pattern (a plain match table, not a
/// HashMap — this family is tiny).
pub fn layout_method_return(name: &str, method: &str, n_args: usize) -> Option<Type> {
    match name {
        "Layout" => match (method, n_args) {
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

/// D-LAYOUT1: the fixed argument type a `Layout` method expects, by
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

/// D-TYPE2-DEFAULT1 / D-NUMTYPE1: binary ops on exact numeric values.
pub fn precise_binop_result(op: crate::AST::BinOp, lt: &str, rt: &str) -> Option<Type> {
    use crate::Numeric::is_decimal_type_name;
    use crate::AST::BinOp;
    let same = lt == rt;
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul if same && is_decimal_type_name(lt) => {
            Some(Type::Named(crate::Syntax::TYPE_DECIMAL.to_string()))
        }
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div
            if same && lt == crate::Syntax::TYPE_FRACTION =>
        {
            Some(Type::Named(crate::Syntax::TYPE_FRACTION.to_string()))
        }
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div
            if same && lt == crate::Syntax::TYPE_COMPLEX =>
        {
            Some(Type::Named(crate::Syntax::TYPE_COMPLEX.to_string()))
        }
        BinOp::Eq | BinOp::Ne if same && is_decimal_type_name(lt) => Some(Type::Bool),
        BinOp::Eq | BinOp::Ne if same && lt == crate::Syntax::TYPE_FRACTION => Some(Type::Bool),
        _ => None,
    }
}

/// D-DECIMAL1: decimal values do not mix implicitly with other numeric kinds.
pub fn precise_mix_error(lt: &Type, rt: &Type) -> Option<(&'static str, String, String)> {
    use crate::Numeric::type_is_decimal;
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
    None
}
/// D-SPACE-GEOMETRY1=A: coordinate-space geometry stays in the ordinary type
/// algebra.  The scalar payload is still the existing `Float`; only the point
/// or displacement carrier and its nominal space are checked here.
pub fn is_geometry_type(name: &str) -> bool {
    matches!(
        name,
        "Point2"
            | "Delta2"
            | "Transform"
            | "Transform2"
            | "Ray2"
            | "ScreenPoint"
            | "WorldPoint"
            | "ViewPoint"
            | "CameraPoint"
            | "DevicePoint"
            | "ScreenDelta"
            | "WorldDelta"
            | "ViewDelta"
            | "CameraDelta"
            | "DeviceDelta"
    )
}

pub fn geometry_point_space(name: &str) -> Option<&'static str> {
    match name {
        "ScreenPoint" | "ScreenDelta" => Some("Screen"),
        "WorldPoint" | "WorldDelta" => Some("World"),
        "ViewPoint" | "ViewDelta" => Some("View"),
        "CameraPoint" | "CameraDelta" => Some("Camera"),
        "DevicePoint" | "DeviceDelta" => Some("Device"),
        _ => None,
    }
}

pub fn geometry_is_point(ty: &Type) -> bool {
    match ty {
        Type::Named(name) => matches!(
            name.as_str(),
            "ScreenPoint" | "WorldPoint" | "ViewPoint" | "CameraPoint" | "DevicePoint"
        ),
        Type::Apply { name, args } => name == "Point2" && args.len() == 2,
        _ => false,
    }
}

pub fn geometry_is_delta(ty: &Type) -> bool {
    match ty {
        Type::Named(name) => matches!(
            name.as_str(),
            "ScreenDelta" | "WorldDelta" | "ViewDelta" | "CameraDelta" | "DeviceDelta"
        ),
        Type::Apply { name, args } => name == "Delta2" && args.len() == 2,
        _ => false,
    }
}

pub fn geometry_space(ty: &Type) -> Option<Type> {
    match ty {
        Type::Named(name) => geometry_point_space(name).map(|space| {
            Type::Named(
                space
                    .strip_suffix("Point")
                    .or_else(|| space.strip_suffix("Delta"))
                    .unwrap_or(space)
                    .to_string(),
            )
        }),
        Type::Apply { name, args } if matches!(name.as_str(), "Point2" | "Delta2") => {
            args.get(1).cloned()
        }
        _ => None,
    }
}

fn geometry_scalar(ty: &Type) -> Type {
    match ty {
        Type::Apply { args, .. } => args.first().cloned().unwrap_or(Type::Float),
        _ => Type::Float,
    }
}

fn geometry_same_space(left: &Type, right: &Type) -> bool {
    geometry_space(left).is_some_and(|space| geometry_space(right) == Some(space))
}

fn geometry_generic(name: &str, scalar: Type, space: Type) -> Type {
    Type::Apply {
        name: name.to_string(),
        args: vec![scalar, space],
    }
}
fn geometry_point_type(ty: &Type, dynamic: bool) -> Type {
    if dynamic {
        return geometry_generic("Point2", geometry_scalar(ty), geometry_space(ty).expect("point has a space"));
    }
    ty.clone()
}

fn geometry_delta_type(ty: &Type, dynamic: bool) -> Type {
    if dynamic {
        return geometry_generic("Delta2", geometry_scalar(ty), geometry_space(ty).expect("delta has a space"));
    }
    match ty {
        Type::Named(name) => {
            let stock = match name.as_str() {
                "ScreenPoint" => Some("ScreenDelta"),
                "WorldPoint" => Some("WorldDelta"),
                "ViewPoint" => Some("ViewDelta"),
                "CameraPoint" => Some("CameraDelta"),
                "DevicePoint" => Some("DeviceDelta"),
                _ => None,
            };
            stock.map_or_else(|| ty.clone(), |name| Type::Named(name.to_string()))
        }
        _ => geometry_generic(
            "Delta2",
            geometry_scalar(ty),
            geometry_space(ty).expect("delta has a space"),
        ),
    }
}


fn geometry_dynamic(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Apply { name, .. } if matches!(name.as_str(), "Point2" | "Delta2")
    )
}

fn geometry_result(ok: Type) -> Type {
    Type::Result {
        ok: Box::new(ok),
        err: Box::new(Type::Named("TransformError".to_string())),
    }
}

pub fn geometry_binop_result(
    op: crate::AST::BinOp,
    left: &Type,
    right: &Type,
) -> Option<Result<Type, ()>> {
    let has_geometry = geometry_is_point(left)
        || geometry_is_point(right)
        || geometry_is_delta(left)
        || geometry_is_delta(right);
    if !has_geometry {
        return None;
    }
    use crate::AST::BinOp;
    let same = geometry_same_space(left, right);
    let dynamic = geometry_dynamic(left) || geometry_dynamic(right);
    let result = match op {
        BinOp::Add if same && geometry_is_point(left) && geometry_is_delta(right) => {
            Some(Ok(if dynamic {
                geometry_result(geometry_point_type(left, true))
            } else {
                left.clone()
            }))
        }
        BinOp::Add if same && geometry_is_delta(left) && geometry_is_point(right) => {
            Some(Ok(if dynamic {
                geometry_result(geometry_point_type(right, true))
            } else {
                right.clone()
            }))
        }
        BinOp::Add if same && geometry_is_delta(left) && geometry_is_delta(right) => {
            Some(Ok(if dynamic {
                geometry_result(geometry_delta_type(left, true))
            } else {
                left.clone()
            }))
        }
        BinOp::Sub if same && geometry_is_point(left) && geometry_is_delta(right) => {
            Some(Ok(if dynamic {
                geometry_result(geometry_point_type(left, true))
            } else {
                left.clone()
            }))
        }
        BinOp::Sub if same && geometry_is_point(left) && geometry_is_point(right) => {
            Some(Ok(if dynamic {
                geometry_result(geometry_delta_type(left, true))
            } else {
                geometry_delta_type(left, false)
            }))
        }
        BinOp::Sub if same && geometry_is_delta(left) && geometry_is_delta(right) => {
            Some(Ok(if dynamic {
                geometry_result(geometry_delta_type(left, true))
            } else {
                left.clone()
            }))
        }
        BinOp::Eq | BinOp::Ne
            if same
                && ((geometry_is_point(left) && geometry_is_point(right))
                    || (geometry_is_delta(left) && geometry_is_delta(right))) =>
        {
            Some(Ok(Type::Bool))
        }
        _ => None,
    };
    result.or_else(|| Some(Err(())))
}

fn geometry_transform_parts(ty: &Type) -> Option<(Type, Type, Type)> {
    let Type::Apply { name, args } = ty else {
        return None;
    };
    match (name.as_str(), args.as_slice()) {
        ("Transform", [from, to]) => Some((Type::Float, from.clone(), to.clone())),
        ("Transform2", [scalar, from, to]) => Some((scalar.clone(), from.clone(), to.clone())),
        _ => None,
    }
}

pub fn geometry_method_return(
    recv: &Type,
    method: &str,
    args: &[Type],
) -> Option<Result<Type, ()>> {
    if let Some((scalar, from, to)) = geometry_transform_parts(recv) {
        return Some(match method {
            "then" if args.len() == 1 => {
                match args.first().and_then(geometry_transform_parts) {
                    Some((next_scalar, next_from, next_to))
                        if next_scalar == scalar && next_from == to =>
                    {
                        let composed = Type::Apply {
                            name: if scalar == Type::Float {
                                "Transform".to_string()
                            } else {
                                "Transform2".to_string()
                            },
                            args: if scalar == Type::Float {
                                vec![from, next_to]
                            } else {
                                vec![scalar, from, next_to]
                            },
                        };
                        Ok(Type::Result {
                            ok: Box::new(composed),
                            err: Box::new(Type::Named("TransformError".to_string())),
                        })
                    }
                    _ => Err(()),
                }
            }
            "inverse" if args.is_empty() => Ok(Type::Result {
                ok: Box::new(Type::Apply {
                    name: if scalar == Type::Float {
                        "Transform".to_string()
                    } else {
                        "Transform2".to_string()
                    },
                    args: if scalar == Type::Float {
                        vec![to, from]
                    } else {
                        vec![scalar, to, from]
                    },
                }),
                err: Box::new(Type::Named("TransformError".to_string())),
            }),
            "point" if args.len() == 1
                && args.first().is_some_and(geometry_is_point)
                && args.first().and_then(geometry_space) == Some(from.clone()) =>
            {
                Ok(Type::Result {
                    ok: Box::new(geometry_generic("Point2", scalar, to)),
                    err: Box::new(Type::Named("TransformError".to_string())),
                })
            }
            "point_at_depth" if args.len() == 2
                && args.first().is_some_and(geometry_is_point)
                && args.first().and_then(geometry_space) == Some(from.clone()) =>
            {
                Ok(Type::Result {
                    ok: Box::new(geometry_generic("Point2", scalar, to)),
                    err: Box::new(Type::Named("TransformError".to_string())),
                })
            }
            "ray" if args.len() == 1
                && args.first().is_some_and(geometry_is_point)
                && args.first().and_then(geometry_space) == Some(from.clone()) =>
            {
                Ok(Type::Result {
                    ok: Box::new(Type::Apply {
                        name: "Ray2".to_string(),
                        args: vec![scalar, from, to],
                    }),
                    err: Box::new(Type::Named("TransformError".to_string())),
                })
            }
            _ => Err(()),
        });
    }
    if geometry_is_point(recv) {
        if args.len() != 1 {
            return Some(Err(()));
        }
        let arg = &args[0];
        let dynamic = geometry_dynamic(recv) || geometry_dynamic(arg);
        let output = match method {
            "add" if geometry_is_delta(arg) => Some(geometry_point_type(recv, dynamic)),
            "sub" if geometry_is_point(arg) => Some(geometry_delta_type(recv, dynamic)),
            _ => None,
        };
        let valid = output.is_some() && geometry_space(recv) == geometry_space(arg);
        return Some(if valid {
            let output = output.expect("validated geometry method result");
            Ok(if dynamic {
                geometry_result(output)
            } else {
                output
            })
        } else {
            Err(())
        });
    }
    if geometry_is_delta(recv) {
        if args.len() != 1 {
            return Some(Err(()));
        }
        let arg = &args[0];
        let dynamic = geometry_dynamic(recv) || geometry_dynamic(arg);
        let valid = matches!(method, "add" | "sub")
            && geometry_is_delta(arg)
            && geometry_space(recv) == geometry_space(arg);
        return Some(if valid {
            let output = geometry_delta_type(recv, dynamic);
            Ok(if dynamic {
                geometry_result(output)
            } else {
                output
            })
        } else {
            Err(())
        });
    }
    None
}

/// Return the source-facing values used by the registered E2520 row.
///
/// The operation table above intentionally keeps only the result type.  The
/// checker still needs stable hole values when the table rejects an operation.
/// Keep this helper beside the table so diagnostics cannot drift from the
/// same-space rule.
pub fn geometry_binop_diagnostic(
    op: crate::AST::BinOp,
    left: &Type,
    right: &Type,
) -> Option<(String, String, String)> {
    if !(geometry_is_point(left)
        || geometry_is_delta(left)
        || geometry_is_point(right)
        || geometry_is_delta(right))
    {
        return None;
    }
    let operation = op.spell().to_string();
    let mut expected = geometry_space(left)
        .map(|space| space.show())
        .unwrap_or_else(|| left.show());
    let mut actual = geometry_space(right)
        .map(|space| space.show())
        .unwrap_or_else(|| right.show());
    if geometry_is_point(left) && geometry_is_point(right) {
        expected = format!("a displacement in {expected}");
        actual = format!("a point in {actual}");
    }
    Some((operation, expected, actual))
}

/// Return the source-facing values used by the registered E2521 row when a
/// transform chain has incompatible middle spaces.
pub fn geometry_transform_diagnostic(
    recv: &Type,
    method: &str,
    args: &[Type],
) -> Option<(String, String, String, String)> {
    if method != "then" || args.len() != 1 {
        return None;
    }
    let (
        Type::Apply {
            name,
            args: recv_args,
        },
        Type::Apply {
            name: next_name,
            args: next_args,
        },
    ) = (recv, &args[0])
    else {
        return None;
    };
    let (_, middle_a) = match (name.as_str(), recv_args.as_slice()) {
        ("Transform", [_, to]) => ((), to),
        ("Transform2", [_, _, to]) => ((), to),
        _ => return None,
    };
    let middle_b = match (next_name.as_str(), next_args.as_slice()) {
        ("Transform", [next_from, _]) => next_from,
        ("Transform2", [_, next_from, _]) => next_from,
        _ => return None,
    };
    if middle_a == middle_b {
        return None;
    }
    Some((
        recv.show(),
        args[0].show(),
        middle_a.show(),
        middle_b.show(),
    ))
}

/// Expected value arguments for a valid geometry constructor.  Dynamic frame
/// identities use the existing integer carrier; the generic type parameter
/// remains the static space proof and is never inferred from a value.
pub fn geometry_static_arg_types(
    name: &str,
    method: &str,
    owner_args: &[Type],
) -> Option<Vec<Type>> {
    match (name, method) {
        (
            "ScreenPoint"
            | "WorldPoint"
            | "ViewPoint"
            | "CameraPoint"
            | "DevicePoint"
            | "ScreenDelta"
            | "WorldDelta"
            | "ViewDelta"
            | "CameraDelta"
            | "DeviceDelta",
            "new",
        ) => Some(vec![Type::Float, Type::Float]),
        ("Point2" | "Delta2", "new") if owner_args.len() == 2 => {
            Some(vec![Type::Float, Type::Float, Type::Int])
        }
        ("Transform", "affine") if owner_args.len() == 2 => {
            Some(vec![
                Type::Float,
                Type::Float,
                Type::Float,
                Type::Float,
                Type::Float,
                Type::Float,
                Type::Int,
                Type::Int,
            ])
        }
        ("Transform2", "affine") if owner_args.len() == 3 => {
            let scalar = owner_args[0].clone();
            Some(vec![
                scalar.clone(),
                scalar.clone(),
                scalar.clone(),
                scalar.clone(),
                scalar.clone(),
                scalar,
                Type::Int,
                Type::Int,
            ])
        }
        _ => None,
    }
}


pub fn geometry_static_return(name: &str, method: &str, n_args: usize) -> Option<Type> {
    if !is_geometry_type(name) {
        return None;
    }
    match (name, method, n_args) {
        (name, "new", 2) if geometry_point_space(name).is_some() => {
            Some(Type::Named(name.to_string()))
        }
        ("Point2" | "Delta2", "new", 3) => None,
        _ => None,
    }
}
/// Named affine construction keeps the matrix slots explicit at the call site.
/// The owner type supplies the static `From`/`To` spaces (and optional scalar
/// for `Transform2`); frame identities remain dynamic carrier data.
pub fn geometry_static_return_with_owner(
    name: &str,
    method: &str,
    n_args: usize,
    owner_args: &[Type],
) -> Option<Type> {
    if matches!(name, "Point2" | "Delta2")
        && method == "new"
        && n_args == 3
        && owner_args.len() == 2
    {
        return Some(Type::Apply {
            name: name.to_string(),
            args: owner_args.to_vec(),
        });
    }
    if matches!(name, "Transform" | "Transform2")
        && method == "affine"
        && n_args == 8
        && ((name == "Transform" && owner_args.len() == 2)
            || (name == "Transform2" && owner_args.len() == 3))
    {
        return Some(Type::Apply {
            name: name.to_string(),
            args: owner_args.to_vec(),
        });
    }
    geometry_static_return(name, method, n_args)
}
