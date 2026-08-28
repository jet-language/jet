// D-SIMD1/D-SIMD2/D-SIMD3 / I9: one portable lane kernel for every tier.
//
// AOT uses the fixed-array entry points. The JIT and TIR/comptime adapters use
// the slice entry points after they marshal their resident value carriers.
// Keep lane order, scalar narrowing, and reduction order here; those are
// language semantics, not engine behavior.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JetSimdBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JetSimdReduceOp {
    Add,
    Mul,
    Min,
    Max,
    Avg,
}

pub(crate) trait JetSimdScalar: Copy {
    fn simd_zero() -> Self;
    fn simd_one() -> Self;
    fn simd_min_identity() -> Self;
    fn simd_max_identity() -> Self;
    fn simd_from_len(len: usize) -> Self;
    fn simd_add(self, other: Self) -> Self;
    fn simd_sub(self, other: Self) -> Self;
    fn simd_mul(self, other: Self) -> Self;
    fn simd_div(self, other: Self) -> Self;
    fn simd_min(self, other: Self) -> Self;
    fn simd_max(self, other: Self) -> Self;
}

macro_rules! jet_simd_float_scalar {
    ($scalar:ty, $zero:expr, $one:expr, $min:expr, $max:expr) => {
        impl JetSimdScalar for $scalar {
            #[inline(always)]
            fn simd_zero() -> Self {
                $zero
            }
            #[inline(always)]
            fn simd_one() -> Self {
                $one
            }
            #[inline(always)]
            fn simd_min_identity() -> Self {
                $min
            }
            #[inline(always)]
            fn simd_max_identity() -> Self {
                $max
            }
            #[inline(always)]
            fn simd_from_len(len: usize) -> Self {
                len as $scalar
            }
            #[inline(always)]
            fn simd_add(self, other: Self) -> Self {
                self + other
            }
            #[inline(always)]
            fn simd_sub(self, other: Self) -> Self {
                self - other
            }
            #[inline(always)]
            fn simd_mul(self, other: Self) -> Self {
                self * other
            }
            #[inline(always)]
            fn simd_div(self, other: Self) -> Self {
                self / other
            }
            #[inline(always)]
            fn simd_min(self, other: Self) -> Self {
                self.min(other)
            }
            #[inline(always)]
            fn simd_max(self, other: Self) -> Self {
                self.max(other)
            }
        }
    };
}

macro_rules! jet_simd_int_scalar {
    ($($scalar:ty),+ $(,)?) => {
        $(
            impl JetSimdScalar for $scalar {
                #[inline(always)]
                fn simd_zero() -> Self { 0 }
                #[inline(always)]
                fn simd_one() -> Self { 1 }
                #[inline(always)]
                fn simd_min_identity() -> Self { <$scalar>::MIN }
                #[inline(always)]
                fn simd_max_identity() -> Self { <$scalar>::MAX }
                #[inline(always)]
                fn simd_from_len(len: usize) -> Self { len as Self }
                #[inline(always)]
                fn simd_add(self, other: Self) -> Self { self.wrapping_add(other) }
                #[inline(always)]
                fn simd_sub(self, other: Self) -> Self { self.wrapping_sub(other) }
                #[inline(always)]
                fn simd_mul(self, other: Self) -> Self { self.wrapping_mul(other) }
                #[inline(always)]
                fn simd_div(self, other: Self) -> Self { self / other }
                #[inline(always)]
                fn simd_min(self, other: Self) -> Self { self.min(other) }
                #[inline(always)]
                fn simd_max(self, other: Self) -> Self { self.max(other) }
            }
        )+
    };
}

jet_simd_float_scalar!(f32, 0.0, 1.0, f32::INFINITY, f32::NEG_INFINITY);
jet_simd_float_scalar!(f64, 0.0, 1.0, f64::INFINITY, f64::NEG_INFINITY);
jet_simd_int_scalar!(i8, i16, i32, i64, u8, u16, u32, u64);

#[inline(always)]
fn jet_simd_apply<T: JetSimdScalar>(left: T, right: T, op: JetSimdBinaryOp) -> T {
    match op {
        JetSimdBinaryOp::Add => left.simd_add(right),
        JetSimdBinaryOp::Sub => left.simd_sub(right),
        JetSimdBinaryOp::Mul => left.simd_mul(right),
        JetSimdBinaryOp::Div => left.simd_div(right),
    }
}

#[inline(always)]
fn jet_simd_reduce_apply<T: JetSimdScalar>(left: T, right: T, op: JetSimdReduceOp) -> T {
    match op {
        JetSimdReduceOp::Add | JetSimdReduceOp::Avg => left.simd_add(right),
        JetSimdReduceOp::Mul => left.simd_mul(right),
        JetSimdReduceOp::Min => left.simd_min(right),
        JetSimdReduceOp::Max => left.simd_max(right),
    }
}

#[inline(always)]
pub(crate) fn jet_simd_binary_array<T: JetSimdScalar, const N: usize>(
    left: &[T; N],
    right: &[T; N],
    op: JetSimdBinaryOp,
) -> [T; N] {
    let mut out = *left;
    for index in 0..N {
        out[index] = jet_simd_apply(left[index], right[index], op);
    }
    out
}

#[inline(always)]
pub(crate) fn jet_simd_add_array<T: JetSimdScalar, const N: usize>(
    left: &[T; N],
    right: &[T; N],
) -> [T; N] {
    jet_simd_binary_array(left, right, JetSimdBinaryOp::Add)
}

#[inline(always)]
pub(crate) fn jet_simd_sub_array<T: JetSimdScalar, const N: usize>(
    left: &[T; N],
    right: &[T; N],
) -> [T; N] {
    jet_simd_binary_array(left, right, JetSimdBinaryOp::Sub)
}

#[inline(always)]
pub(crate) fn jet_simd_mul_array<T: JetSimdScalar, const N: usize>(
    left: &[T; N],
    right: &[T; N],
) -> [T; N] {
    jet_simd_binary_array(left, right, JetSimdBinaryOp::Mul)
}

#[inline(always)]
pub(crate) fn jet_simd_div_array<T: JetSimdScalar, const N: usize>(
    left: &[T; N],
    right: &[T; N],
) -> [T; N] {
    jet_simd_binary_array(left, right, JetSimdBinaryOp::Div)
}

#[inline(always)]
pub(crate) fn jet_simd_binary_slice<T: JetSimdScalar>(
    left: &[T],
    right: &[T],
    op: JetSimdBinaryOp,
) -> Option<Vec<T>> {
    if left.len() != right.len() {
        return None;
    }
    Some(
        left.iter()
            .copied()
            .zip(right.iter().copied())
            .map(|(left, right)| jet_simd_apply(left, right, op))
            .collect(),
    )
}

#[inline(always)]
pub(crate) fn jet_simd_splat_array<T: JetSimdScalar, const N: usize>(value: T) -> [T; N] {
    [value; N]
}

#[inline(always)]
pub(crate) fn jet_simd_splat_slice<T: JetSimdScalar>(value: T, len: usize) -> Vec<T> {
    vec![value; len]
}

#[inline(always)]
pub(crate) fn jet_simd_reduce_array<T: JetSimdScalar, const N: usize>(
    lanes: &[T; N],
    op: JetSimdReduceOp,
) -> Option<T> {
    jet_simd_reduce_slice(lanes, op)
}

#[inline(always)]
pub(crate) fn jet_simd_reduce_slice<T: JetSimdScalar>(
    lanes: &[T],
    op: JetSimdReduceOp,
) -> Option<T> {
    lanes.first()?;
    let mut value = match op {
        JetSimdReduceOp::Add | JetSimdReduceOp::Avg => T::simd_zero(),
        JetSimdReduceOp::Mul => T::simd_one(),
        JetSimdReduceOp::Min => T::simd_min_identity(),
        JetSimdReduceOp::Max => T::simd_max_identity(),
    };
    for &lane in lanes {
        value = jet_simd_reduce_apply(value, lane, op);
    }
    if op == JetSimdReduceOp::Avg {
        value = value.simd_div(T::simd_from_len(lanes.len()));
    }
    Some(value)
}

#[inline(always)]
pub(crate) fn jet_simd_sum_array<T: JetSimdScalar, const N: usize>(lanes: &[T; N]) -> T {
    jet_simd_reduce_array(lanes, JetSimdReduceOp::Add).expect("non-empty SIMD lane family")
}

#[inline(always)]
pub(crate) fn jet_simd_product_array<T: JetSimdScalar, const N: usize>(lanes: &[T; N]) -> T {
    jet_simd_reduce_array(lanes, JetSimdReduceOp::Mul).expect("non-empty SIMD lane family")
}

#[inline(always)]
pub(crate) fn jet_simd_min_array<T: JetSimdScalar, const N: usize>(lanes: &[T; N]) -> T {
    jet_simd_reduce_array(lanes, JetSimdReduceOp::Min).expect("non-empty SIMD lane family")
}

#[inline(always)]
pub(crate) fn jet_simd_max_array<T: JetSimdScalar, const N: usize>(lanes: &[T; N]) -> T {
    jet_simd_reduce_array(lanes, JetSimdReduceOp::Max).expect("non-empty SIMD lane family")
}

#[inline(always)]
pub(crate) fn jet_simd_avg_array<T: JetSimdScalar, const N: usize>(lanes: &[T; N]) -> T {
    jet_simd_reduce_array(lanes, JetSimdReduceOp::Avg).expect("non-empty SIMD lane family")
}

fn jet_simd_integer_widen(value: i64, signed: bool) -> i128 {
    if signed {
        value as i128
    } else {
        value as u64 as i128
    }
}

fn jet_simd_integer_narrow(value: i128, signed: bool, bits: u8) -> i64 {
    if bits == 64 {
        return if signed {
            value as i64
        } else {
            value as u64 as i64
        };
    }
    let mask = (1_i128 << bits) - 1;
    let value = value & mask;
    if signed && value & (1_i128 << (bits - 1)) != 0 {
        (value | !mask) as i64
    } else {
        value as i64
    }
}

#[inline(always)]
pub(crate) fn jet_simd_integer_binary(
    left: &[i64],
    right: &[i64],
    op: JetSimdBinaryOp,
    signed: bool,
    bits: u8,
) -> Option<Vec<i64>> {
    if left.len() != right.len() || !(1..=64).contains(&bits) {
        return None;
    }
    left.iter()
        .copied()
        .zip(right.iter().copied())
        .map(|(left, right)| {
            let value = if signed {
                let left = jet_simd_integer_widen(left, true);
                let right = jet_simd_integer_widen(right, true);
                match op {
                    JetSimdBinaryOp::Add => left.wrapping_add(right),
                    JetSimdBinaryOp::Sub => left.wrapping_sub(right),
                    JetSimdBinaryOp::Mul => left.wrapping_mul(right),
                    JetSimdBinaryOp::Div if right != 0 => left / right,
                    JetSimdBinaryOp::Div => return None,
                }
            } else {
                let left = jet_simd_integer_widen(left, false) as u128;
                let right = jet_simd_integer_widen(right, false) as u128;
                match op {
                    JetSimdBinaryOp::Add => left.wrapping_add(right) as i128,
                    JetSimdBinaryOp::Sub => left.wrapping_sub(right) as i128,
                    JetSimdBinaryOp::Mul => left.wrapping_mul(right) as i128,
                    JetSimdBinaryOp::Div if right != 0 => (left / right) as i128,
                    JetSimdBinaryOp::Div => return None,
                }
            };
            Some(jet_simd_integer_narrow(value, signed, bits))
        })
        .collect()
}

#[inline(always)]
pub(crate) fn jet_simd_integer_reduce(
    lanes: &[i64],
    op: JetSimdReduceOp,
    signed: bool,
    bits: u8,
) -> Option<i64> {
    if lanes.is_empty() || !(1..=64).contains(&bits) {
        return None;
    }
    let values = lanes
        .iter()
        .map(|&lane| {
            jet_simd_integer_widen(jet_simd_integer_narrow(lane as i128, signed, bits), signed)
        })
        .collect::<Vec<_>>();
    let value = match op {
        JetSimdReduceOp::Add => values
            .iter()
            .copied()
            .fold(0_i128, |left, right| {
                jet_simd_integer_widen(
                    jet_simd_integer_narrow(left + right, signed, bits),
                    signed,
                )
            }),
        JetSimdReduceOp::Mul => values
            .iter()
            .copied()
            .fold(1_i128, |left, right| {
                jet_simd_integer_widen(
                    jet_simd_integer_narrow(left * right, signed, bits),
                    signed,
                )
            }),
        JetSimdReduceOp::Min if signed => values.iter().copied().min()?,
        JetSimdReduceOp::Max if signed => values.iter().copied().max()?,
        JetSimdReduceOp::Min => values
            .iter()
            .map(|&value| value as u128)
            .min()? as i128,
        JetSimdReduceOp::Max => values
            .iter()
            .map(|&value| value as u128)
            .max()? as i128,
        JetSimdReduceOp::Avg => {
            values.iter().copied().sum::<i128>() / values.len() as i128
        }
    };
    Some(jet_simd_integer_narrow(value, signed, bits))
}

#[inline(always)]
pub(crate) fn jet_simd_lane_index(
    index: i64,
    type_name: &str,
    lane_count: usize,
) -> Result<usize, String> {
    if index < 0 || index as usize >= lane_count {
        Err(format!(
            "lane index {} out of range for {} ({} lanes)",
            index, type_name, lane_count
        ))
    } else {
        Ok(index as usize)
    }
}
