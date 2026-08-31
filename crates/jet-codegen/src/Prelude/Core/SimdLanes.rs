// D-SIMD1/D-SIMD2/D-SIMD3 / I9: one portable lane kernel for every tier.
//
// AOT uses fixed-array entry points for portable lane families and a private
// native carrier for host F64x4. The JIT and TIR/comptime adapters use the
// fixed-array or slice entry points after they marshal their resident values.
// Keep lane order, scalar narrowing, and reduction order here; those are
// language semantics, not engine behavior.

/// D-SIMD3=B: an authoritative scalar-loop boundary. `black_box` keeps the
/// iteration opaque to LLVM and the compiler fence prevents loop motion or
/// vector packing across the boundary. The value is unchanged, so this is a
/// codegen control point rather than a Jet semantic operation.
#[inline(never)]
fn jet_scalar_loop_barrier<T>(value: T) -> T {
    let value = std::hint::black_box(value);
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    value
}

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

// I1: the native lane backend is a vetted Prelude implementation. Runtime
// dispatch keeps cross-target binaries on the portable array path when a
// width's feature is unavailable; host-native F64x4 value entries additionally
// select their AVX kernel at compile time. The public lane surface exposes no
// target-dependent carrier ABI. The scalar reduction functions below remain
// the only implementation for ordering-sensitive folds.
// JET_VETTED_UNSAFE_BEGIN: jet_simd_x86
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod jet_simd_x86 {
    use super::JetSimdBinaryOp;

    #[cfg(target_arch = "x86")]
    use std::arch::x86 as arch;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64 as arch;

    #[target_feature(enable = "sse")]
    pub(super) unsafe fn f32x4_binary(
        left: &[f32; 4],
        right: &[f32; 4],
        op: JetSimdBinaryOp,
    ) -> [f32; 4] {
        let left = arch::_mm_loadu_ps(left.as_ptr());
        let right = arch::_mm_loadu_ps(right.as_ptr());
        let value = match op {
            JetSimdBinaryOp::Add => arch::_mm_add_ps(left, right),
            JetSimdBinaryOp::Sub => arch::_mm_sub_ps(left, right),
            JetSimdBinaryOp::Mul => arch::_mm_mul_ps(left, right),
            JetSimdBinaryOp::Div => arch::_mm_div_ps(left, right),
        };
        let mut out = [0.0; 4];
        arch::_mm_storeu_ps(out.as_mut_ptr(), value);
        out
    }

    #[target_feature(enable = "sse2")]
    pub(super) unsafe fn f64x2_binary(
        left: &[f64; 2],
        right: &[f64; 2],
        op: JetSimdBinaryOp,
    ) -> [f64; 2] {
        let left = arch::_mm_loadu_pd(left.as_ptr());
        let right = arch::_mm_loadu_pd(right.as_ptr());
        let value = match op {
            JetSimdBinaryOp::Add => arch::_mm_add_pd(left, right),
            JetSimdBinaryOp::Sub => arch::_mm_sub_pd(left, right),
            JetSimdBinaryOp::Mul => arch::_mm_mul_pd(left, right),
            JetSimdBinaryOp::Div => arch::_mm_div_pd(left, right),
        };
        let mut out = [0.0; 2];
        arch::_mm_storeu_pd(out.as_mut_ptr(), value);
        out
    }

    #[target_feature(enable = "avx")]
    pub(super) unsafe fn f32x8_binary(
        left: &[f32; 8],
        right: &[f32; 8],
        op: JetSimdBinaryOp,
    ) -> [f32; 8] {
        let left = arch::_mm256_loadu_ps(left.as_ptr());
        let right = arch::_mm256_loadu_ps(right.as_ptr());
        let value = match op {
            JetSimdBinaryOp::Add => arch::_mm256_add_ps(left, right),
            JetSimdBinaryOp::Sub => arch::_mm256_sub_ps(left, right),
            JetSimdBinaryOp::Mul => arch::_mm256_mul_ps(left, right),
            JetSimdBinaryOp::Div => arch::_mm256_div_ps(left, right),
        };
        let mut out = [0.0; 8];
        arch::_mm256_storeu_ps(out.as_mut_ptr(), value);
        out
    }

    #[target_feature(enable = "avx")]
    pub(super) unsafe fn f64x4_binary(
        left: &[f64; 4],
        right: &[f64; 4],
        op: JetSimdBinaryOp,
    ) -> [f64; 4] {
        let left = arch::_mm256_loadu_pd(left.as_ptr());
        let right = arch::_mm256_loadu_pd(right.as_ptr());
        let value = match op {
            JetSimdBinaryOp::Add => arch::_mm256_add_pd(left, right),
            JetSimdBinaryOp::Sub => arch::_mm256_sub_pd(left, right),
            JetSimdBinaryOp::Mul => arch::_mm256_mul_pd(left, right),
            JetSimdBinaryOp::Div => arch::_mm256_div_pd(left, right),
        };
        let mut out = [0.0; 4];
        arch::_mm256_storeu_pd(out.as_mut_ptr(), value);
        out
    }

    #[target_feature(enable = "sse")]
    pub(super) unsafe fn f32x4_neg(left: &[f32; 4]) -> [f32; 4] {
        let value = arch::_mm_loadu_ps(left.as_ptr());
        let sign = arch::_mm_set1_ps(-0.0);
        let value = arch::_mm_xor_ps(value, sign);
        let mut out = [0.0; 4];
        arch::_mm_storeu_ps(out.as_mut_ptr(), value);
        out
    }

    #[target_feature(enable = "sse2")]
    pub(super) unsafe fn f64x2_neg(left: &[f64; 2]) -> [f64; 2] {
        let value = arch::_mm_loadu_pd(left.as_ptr());
        let sign = arch::_mm_set1_pd(-0.0);
        let value = arch::_mm_xor_pd(value, sign);
        let mut out = [0.0; 2];
        arch::_mm_storeu_pd(out.as_mut_ptr(), value);
        out
    }

    #[target_feature(enable = "avx")]
    pub(super) unsafe fn f32x8_neg(left: &[f32; 8]) -> [f32; 8] {
        let value = arch::_mm256_loadu_ps(left.as_ptr());
        let sign = arch::_mm256_set1_ps(-0.0);
        let value = arch::_mm256_xor_ps(value, sign);
        let mut out = [0.0; 8];
        arch::_mm256_storeu_ps(out.as_mut_ptr(), value);
        out
    }

    #[target_feature(enable = "avx")]
    pub(super) unsafe fn f64x4_neg(left: &[f64; 4]) -> [f64; 4] {
        let value = arch::_mm256_loadu_pd(left.as_ptr());
        let sign = arch::_mm256_set1_pd(-0.0);
        let value = arch::_mm256_xor_pd(value, sign);
        let mut out = [0.0; 4];
        arch::_mm256_storeu_pd(out.as_mut_ptr(), value);
        out
    }

    pub(super) fn f32x4_binary_if_available(
        left: &[f32; 4],
        right: &[f32; 4],
        op: JetSimdBinaryOp,
    ) -> Option<[f32; 4]> {
        if !is_x86_feature_detected!("sse") {
            return None;
        }
        Some(unsafe { f32x4_binary(left, right, op) })
    }

    pub(super) fn f64x2_binary_if_available(
        left: &[f64; 2],
        right: &[f64; 2],
        op: JetSimdBinaryOp,
    ) -> Option<[f64; 2]> {
        if !is_x86_feature_detected!("sse2") {
            return None;
        }
        Some(unsafe { f64x2_binary(left, right, op) })
    }

    pub(super) fn f32x8_binary_if_available(
        left: &[f32; 8],
        right: &[f32; 8],
        op: JetSimdBinaryOp,
    ) -> Option<[f32; 8]> {
        if !is_x86_feature_detected!("avx") {
            return None;
        }
        Some(unsafe { f32x8_binary(left, right, op) })
    }

    pub(super) fn f64x4_binary_if_available(
        left: &[f64; 4],
        right: &[f64; 4],
        op: JetSimdBinaryOp,
    ) -> Option<[f64; 4]> {
        if !is_x86_feature_detected!("avx") {
            return None;
        }
        Some(unsafe { f64x4_binary(left, right, op) })
    }
    pub(super) fn f32x4_neg_if_available(left: &[f32; 4]) -> Option<[f32; 4]> {
        if !is_x86_feature_detected!("sse") {
            return None;
        }
        Some(unsafe { f32x4_neg(left) })
    }

    pub(super) fn f64x2_neg_if_available(left: &[f64; 2]) -> Option<[f64; 2]> {
        if !is_x86_feature_detected!("sse2") {
            return None;
        }
        Some(unsafe { f64x2_neg(left) })
    }

    pub(super) fn f32x8_neg_if_available(left: &[f32; 8]) -> Option<[f32; 8]> {
        if !is_x86_feature_detected!("avx") {
            return None;
        }
        Some(unsafe { f32x8_neg(left) })
    }

    pub(super) fn f64x4_neg_if_available(left: &[f64; 4]) -> Option<[f64; 4]> {
        if !is_x86_feature_detected!("avx") {
            return None;
        }
        Some(unsafe { f64x4_neg(left) })
    }
}
// JET_VETTED_UNSAFE_END: jet_simd_x86

// D-SIMD3: native AOT lane operators use a private AVX F64x4 carrier when the
// generated crate enables AVX. This keeps chained lane expressions in vector
// registers instead of storing every operator result through an array. The
// array helpers below remain the portable and runtime-dispatched bridge for
// every other tier.
// JET_VETTED_UNSAFE_BEGIN: jet_simd_f64x4_avx
#[cfg(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
))]
mod jet_simd_f64x4_avx {
    #[cfg(target_arch = "x86")]
    use std::arch::x86 as arch;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64 as arch;

    pub(crate) type Native = arch::__m256d;

    #[inline(always)]
    pub(super) fn from_array(value: [f64; 4]) -> Native {
        unsafe { arch::_mm256_loadu_pd(value.as_ptr()) }
    }

    #[inline(always)]
    pub(super) fn to_array(value: Native) -> [f64; 4] {
        unsafe {
            let mut out = [0.0; 4];
            arch::_mm256_storeu_pd(out.as_mut_ptr(), value);
            out
        }
    }

    #[inline(always)]
    pub(super) fn splat(value: f64) -> Native {
        unsafe { arch::_mm256_set1_pd(value) }
    }

    #[inline(always)]
    pub(super) fn lane(value: Native, index: usize) -> f64 {
        unsafe {
            let low = arch::_mm256_castpd256_pd128(value);
            let high = arch::_mm256_extractf128_pd::<1>(value);
            match index {
                0 => arch::_mm_cvtsd_f64(low),
                1 => arch::_mm_cvtsd_f64(arch::_mm_unpackhi_pd(low, low)),
                2 => arch::_mm_cvtsd_f64(high),
                3 => arch::_mm_cvtsd_f64(arch::_mm_unpackhi_pd(high, high)),
                _ => unreachable!("F64x4 lane index validated before native access"),
            }
        }
    }

    #[inline(always)]
    pub(super) fn lane_const<const INDEX: usize>(value: Native) -> f64 {
        unsafe {
            let low = arch::_mm256_castpd256_pd128(value);
            let high = arch::_mm256_extractf128_pd::<1>(value);
            match INDEX {
                0 => arch::_mm_cvtsd_f64(low),
                1 => arch::_mm_cvtsd_f64(arch::_mm_unpackhi_pd(low, low)),
                2 => arch::_mm_cvtsd_f64(high),
                3 => arch::_mm_cvtsd_f64(arch::_mm_unpackhi_pd(high, high)),
                _ => unreachable!("F64x4 lane index validated before native access"),
            }
        }
    }

    /// Transpose one lane from four resident vectors. The AVX unpack/permute
    /// sequence keeps the four source carriers in registers; scalar extraction
    /// would otherwise force four lane reads and a fresh vector load.
    #[inline(always)]
    pub(super) fn gather_lane<const INDEX: usize>(
        first: Native,
        second: Native,
        third: Native,
        fourth: Native,
    ) -> Native {
        unsafe {
            let (first_pair, second_pair) = match INDEX {
                0 | 2 => (
                    arch::_mm256_unpacklo_pd(first, second),
                    arch::_mm256_unpacklo_pd(third, fourth),
                ),
                1 | 3 => (
                    arch::_mm256_unpackhi_pd(first, second),
                    arch::_mm256_unpackhi_pd(third, fourth),
                ),
                _ => unreachable!("F64x4 lane index validated before native access"),
            };
            match INDEX {
                0 | 1 => arch::_mm256_permute2f128_pd::<0x20>(first_pair, second_pair),
                2 | 3 => arch::_mm256_permute2f128_pd::<0x31>(first_pair, second_pair),
                _ => unreachable!("F64x4 lane index validated before native access"),
            }
        }
    }

    macro_rules! binary {
        ($name:ident, $op:path) => {
            #[inline(always)]
            pub(super) fn $name(left: Native, right: Native) -> Native {
                unsafe { $op(left, right) }
            }
        };
    }

    #[inline(always)]
    pub(super) fn sqrt(value: Native) -> Native {
        unsafe { arch::_mm256_sqrt_pd(value) }
    }

    #[inline(always)]
    pub(super) fn neg(value: Native) -> Native {
        unsafe {
            let sign = arch::_mm256_set1_pd(-0.0);
            arch::_mm256_xor_pd(value, sign)
        }
    }

    #[inline(always)]
    pub(super) fn add_array(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
        to_array(add(from_array(left), from_array(right)))
    }

    #[inline(always)]
    pub(super) fn sub_array(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
        to_array(sub(from_array(left), from_array(right)))
    }

    #[inline(always)]
    pub(super) fn mul_array(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
        to_array(mul(from_array(left), from_array(right)))
    }

    #[inline(always)]
    pub(super) fn div_array(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
        to_array(div(from_array(left), from_array(right)))
    }

    #[inline(always)]
    pub(super) fn sqrt_array(value: [f64; 4]) -> [f64; 4] {
        to_array(sqrt(from_array(value)))
    }

    binary!(add, arch::_mm256_add_pd);
    binary!(sub, arch::_mm256_sub_pd);
    binary!(mul, arch::_mm256_mul_pd);
    binary!(div, arch::_mm256_div_pd);
}
// JET_VETTED_UNSAFE_END: jet_simd_f64x4_avx

#[cfg(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
))]
pub(crate) type JetF64x4 = jet_simd_f64x4_avx::Native;

#[cfg(not(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
)))]
pub(crate) type JetF64x4 = [f64; 4];

// These native entries carry the opaque F64x4 value. On a portable target the
// same names collapse to the fixed-array implementation, so the lane API and
// its arithmetic remain one Prelude path across execution tiers.
#[inline(always)]
pub(crate) fn jet_simd_f64x4_new_native(value: [f64; 4]) -> JetF64x4 {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::from_array(value);
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        value
    }
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_splat_native(value: f64) -> JetF64x4 {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::splat(value);
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        [value; 4]
    }
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_to_array_native(value: JetF64x4) -> [f64; 4] {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::to_array(value);
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        value
    }
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_lane_native(value: JetF64x4, index: usize) -> f64 {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::lane(value, index);
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        value[index]
    }
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_lane_const_native<const INDEX: usize>(value: JetF64x4) -> f64 {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::lane_const::<INDEX>(value);
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        value[INDEX]
    }
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_gather_lane_native<const INDEX: usize>(
    first: JetF64x4,
    second: JetF64x4,
    third: JetF64x4,
    fourth: JetF64x4,
) -> JetF64x4 {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::gather_lane::<INDEX>(first, second, third, fourth);
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        [first[INDEX], second[INDEX], third[INDEX], fourth[INDEX]]
    }
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_add_native(left: JetF64x4, right: JetF64x4) -> JetF64x4 {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::add(left, right);
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        jet_simd_f64x4_add_array(&left, &right)
    }
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_sub_native(left: JetF64x4, right: JetF64x4) -> JetF64x4 {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::sub(left, right);
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        jet_simd_f64x4_sub_array(&left, &right)
    }
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_mul_native(left: JetF64x4, right: JetF64x4) -> JetF64x4 {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::mul(left, right);
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        jet_simd_f64x4_mul_array(&left, &right)
    }
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_div_native(left: JetF64x4, right: JetF64x4) -> JetF64x4 {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::div(left, right);
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        jet_simd_f64x4_div_array(&left, &right)
    }
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_sqrt_native(value: [f64; 4]) -> JetF64x4 {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::sqrt(jet_simd_f64x4_avx::from_array(value));
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        [value[0].sqrt(), value[1].sqrt(), value[2].sqrt(), value[3].sqrt()]
    }
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_sqrt_native_carrier(value: JetF64x4) -> JetF64x4 {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        return jet_simd_f64x4_avx::sqrt(value);
    }
    #[cfg(not(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    )))]
    {
        [value[0].sqrt(), value[1].sqrt(), value[2].sqrt(), value[3].sqrt()]
    }
}

#[inline(always)]
fn jet_simd_f32x4_binary(
    left: &[f32; 4],
    right: &[f32; 4],
    op: JetSimdBinaryOp,
) -> [f32; 4] {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if let Some(value) = jet_simd_x86::f32x4_binary_if_available(left, right, op) {
        return value;
    }
    jet_simd_binary_array(left, right, op)
}

#[inline(always)]
fn jet_simd_f64x2_binary(
    left: &[f64; 2],
    right: &[f64; 2],
    op: JetSimdBinaryOp,
) -> [f64; 2] {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if let Some(value) = jet_simd_x86::f64x2_binary_if_available(left, right, op) {
        return value;
    }
    jet_simd_binary_array(left, right, op)
}

#[inline(always)]
fn jet_simd_f32x8_binary(
    left: &[f32; 8],
    right: &[f32; 8],
    op: JetSimdBinaryOp,
) -> [f32; 8] {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if let Some(value) = jet_simd_x86::f32x8_binary_if_available(left, right, op) {
        return value;
    }
    jet_simd_binary_array(left, right, op)
}

#[inline(always)]
fn jet_simd_f64x4_binary(
    left: &[f64; 4],
    right: &[f64; 4],
    op: JetSimdBinaryOp,
) -> [f64; 4] {
    // A target compiled with AVX already has a proved-safe native carrier.
    // Keep this branch ahead of runtime dispatch so every inlined AOT lane
    // operation stays in the AVX register rail; portable binaries retain the
    // runtime feature check below. Both branches use the same lane operation.
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        let left = jet_simd_f64x4_avx::from_array(*left);
        let right = jet_simd_f64x4_avx::from_array(*right);
        let value = match op {
            JetSimdBinaryOp::Add => jet_simd_f64x4_avx::add(left, right),
            JetSimdBinaryOp::Sub => jet_simd_f64x4_avx::sub(left, right),
            JetSimdBinaryOp::Mul => jet_simd_f64x4_avx::mul(left, right),
            JetSimdBinaryOp::Div => jet_simd_f64x4_avx::div(left, right),
        };
        return jet_simd_f64x4_avx::to_array(value);
    }
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if let Some(value) = jet_simd_x86::f64x4_binary_if_available(left, right, op) {
        return value;
    }
    jet_simd_binary_array(left, right, op)
}

#[inline(always)]
fn jet_simd_f32x4_neg(left: &[f32; 4]) -> [f32; 4] {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if let Some(value) = jet_simd_x86::f32x4_neg_if_available(left) {
        return value;
    }
    let mut out = *left;
    for value in &mut out {
        *value = -*value;
    }
    out
}

#[inline(always)]
fn jet_simd_f64x2_neg(left: &[f64; 2]) -> [f64; 2] {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if let Some(value) = jet_simd_x86::f64x2_neg_if_available(left) {
        return value;
    }
    let mut out = *left;
    for value in &mut out {
        *value = -*value;
    }
    out
}

#[inline(always)]
fn jet_simd_f32x8_neg(left: &[f32; 8]) -> [f32; 8] {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if let Some(value) = jet_simd_x86::f32x8_neg_if_available(left) {
        return value;
    }
    let mut out = *left;
    for value in &mut out {
        *value = -*value;
    }
    out
}

#[inline(always)]
fn jet_simd_f64x4_neg(left: &[f64; 4]) -> [f64; 4] {
    #[cfg(all(
        any(target_arch = "x86", target_arch = "x86_64"),
        target_feature = "avx"
    ))]
    {
        let value = jet_simd_f64x4_avx::from_array(*left);
        return jet_simd_f64x4_avx::to_array(jet_simd_f64x4_avx::neg(value));
    }
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if let Some(value) = jet_simd_x86::f64x4_neg_if_available(left) {
        return value;
    }
    let mut out = *left;
    for value in &mut out {
        *value = -*value;
    }
    out
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_add_array(
    left: &[f64; 4],
    right: &[f64; 4],
) -> [f64; 4] {
    jet_simd_f64x4_binary(left, right, JetSimdBinaryOp::Add)
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_sub_array(
    left: &[f64; 4],
    right: &[f64; 4],
) -> [f64; 4] {
    jet_simd_f64x4_binary(left, right, JetSimdBinaryOp::Sub)
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_mul_array(
    left: &[f64; 4],
    right: &[f64; 4],
) -> [f64; 4] {
    jet_simd_f64x4_binary(left, right, JetSimdBinaryOp::Mul)
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_div_array(
    left: &[f64; 4],
    right: &[f64; 4],
) -> [f64; 4] {
    jet_simd_f64x4_binary(left, right, JetSimdBinaryOp::Div)
}


// These value-entry points are selected at compile time for host-native AOT
// artifacts. Non-native builds keep the existing runtime-dispatched array
// entry points, so portable/interpreter/JIT carriers do not inherit an AVX ABI.
#[cfg(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
))]
#[inline(always)]
pub(crate) fn jet_simd_f64x4_add_value(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    jet_simd_f64x4_avx::add_array(left, right)
}

#[cfg(not(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
)))]
#[inline(always)]
pub(crate) fn jet_simd_f64x4_add_value(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    jet_simd_f64x4_add_array(&left, &right)
}

#[cfg(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
))]
#[inline(always)]
pub(crate) fn jet_simd_f64x4_sub_value(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    jet_simd_f64x4_avx::sub_array(left, right)
}

#[cfg(not(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
)))]
#[inline(always)]
pub(crate) fn jet_simd_f64x4_sub_value(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    jet_simd_f64x4_sub_array(&left, &right)
}

#[cfg(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
))]
#[inline(always)]
pub(crate) fn jet_simd_f64x4_mul_value(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    jet_simd_f64x4_avx::mul_array(left, right)
}

#[cfg(not(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
)))]
#[inline(always)]
pub(crate) fn jet_simd_f64x4_mul_value(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    jet_simd_f64x4_mul_array(&left, &right)
}

#[cfg(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
))]
#[inline(always)]
pub(crate) fn jet_simd_f64x4_div_value(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    jet_simd_f64x4_avx::div_array(left, right)
}

#[cfg(not(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
)))]
#[inline(always)]
pub(crate) fn jet_simd_f64x4_div_value(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    jet_simd_f64x4_div_array(&left, &right)
}

// D-SIMD3=B: AOT may fuse four independent scalar `sqrt` calls when the
// frontend has already arranged them as one F64x4 constructor. The fallback
// preserves the scalar operation order on targets without AVX.
#[cfg(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
))]
#[inline(always)]
pub(crate) fn jet_simd_f64x4_sqrt_value(value: [f64; 4]) -> [f64; 4] {
    jet_simd_f64x4_avx::sqrt_array(value)
}

#[cfg(not(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx"
)))]
#[inline(always)]
pub(crate) fn jet_simd_f64x4_sqrt_value(value: [f64; 4]) -> [f64; 4] {
    [value[0].sqrt(), value[1].sqrt(), value[2].sqrt(), value[3].sqrt()]
}


#[inline(always)]
pub(crate) fn jet_simd_f64x4_splat_array(value: f64) -> [f64; 4] {
    [value; 4]
}

#[inline(always)]
pub(crate) fn jet_simd_f32x4_add_array(
    left: &[f32; 4],
    right: &[f32; 4],
) -> [f32; 4] {
    jet_simd_f32x4_binary(left, right, JetSimdBinaryOp::Add)
}

#[inline(always)]
pub(crate) fn jet_simd_f32x4_sub_array(
    left: &[f32; 4],
    right: &[f32; 4],
) -> [f32; 4] {
    jet_simd_f32x4_binary(left, right, JetSimdBinaryOp::Sub)
}

#[inline(always)]
pub(crate) fn jet_simd_f32x4_mul_array(
    left: &[f32; 4],
    right: &[f32; 4],
) -> [f32; 4] {
    jet_simd_f32x4_binary(left, right, JetSimdBinaryOp::Mul)
}

#[inline(always)]
pub(crate) fn jet_simd_f32x4_div_array(
    left: &[f32; 4],
    right: &[f32; 4],
) -> [f32; 4] {
    jet_simd_f32x4_binary(left, right, JetSimdBinaryOp::Div)
}

#[inline(always)]
pub(crate) fn jet_simd_f32x4_splat_array(value: f32) -> [f32; 4] {
    [value; 4]
}

#[inline(always)]
pub(crate) fn jet_simd_f64x2_add_array(
    left: &[f64; 2],
    right: &[f64; 2],
) -> [f64; 2] {
    jet_simd_f64x2_binary(left, right, JetSimdBinaryOp::Add)
}

#[inline(always)]
pub(crate) fn jet_simd_f64x2_sub_array(
    left: &[f64; 2],
    right: &[f64; 2],
) -> [f64; 2] {
    jet_simd_f64x2_binary(left, right, JetSimdBinaryOp::Sub)
}

#[inline(always)]
pub(crate) fn jet_simd_f64x2_mul_array(
    left: &[f64; 2],
    right: &[f64; 2],
) -> [f64; 2] {
    jet_simd_f64x2_binary(left, right, JetSimdBinaryOp::Mul)
}

#[inline(always)]
pub(crate) fn jet_simd_f64x2_div_array(
    left: &[f64; 2],
    right: &[f64; 2],
) -> [f64; 2] {
    jet_simd_f64x2_binary(left, right, JetSimdBinaryOp::Div)
}

#[inline(always)]
pub(crate) fn jet_simd_f64x2_splat_array(value: f64) -> [f64; 2] {
    [value; 2]
}

#[inline(always)]
pub(crate) fn jet_simd_f32x8_add_array(
    left: &[f32; 8],
    right: &[f32; 8],
) -> [f32; 8] {
    jet_simd_f32x8_binary(left, right, JetSimdBinaryOp::Add)
}

#[inline(always)]
pub(crate) fn jet_simd_f32x8_sub_array(
    left: &[f32; 8],
    right: &[f32; 8],
) -> [f32; 8] {
    jet_simd_f32x8_binary(left, right, JetSimdBinaryOp::Sub)
}

#[inline(always)]
pub(crate) fn jet_simd_f32x8_mul_array(
    left: &[f32; 8],
    right: &[f32; 8],
) -> [f32; 8] {
    jet_simd_f32x8_binary(left, right, JetSimdBinaryOp::Mul)
}

#[inline(always)]
pub(crate) fn jet_simd_f32x8_div_array(
    left: &[f32; 8],
    right: &[f32; 8],
) -> [f32; 8] {
    jet_simd_f32x8_binary(left, right, JetSimdBinaryOp::Div)
}

#[inline(always)]
pub(crate) fn jet_simd_f32x8_splat_array(value: f32) -> [f32; 8] {
    [value; 8]
}

#[inline(always)]
pub(crate) fn jet_simd_f32x4_neg_array(left: &[f32; 4]) -> [f32; 4] {
    jet_simd_f32x4_neg(left)
}

#[inline(always)]
pub(crate) fn jet_simd_f64x2_neg_array(left: &[f64; 2]) -> [f64; 2] {
    jet_simd_f64x2_neg(left)
}

#[inline(always)]
pub(crate) fn jet_simd_f32x8_neg_array(left: &[f32; 8]) -> [f32; 8] {
    jet_simd_f32x8_neg(left)
}

#[inline(always)]
pub(crate) fn jet_simd_f64x4_neg_array(left: &[f64; 4]) -> [f64; 4] {
    jet_simd_f64x4_neg(left)
}

/// Apply a lane-wise float operation to a resident slice. This is the shared
/// carrier path used by non-AOT engines: fixed-width chunks use the same
/// runtime-dispatched kernels as AOT, and the tail uses the scalar definition.
/// No reduction or reassociation happens here.
#[inline(always)]
pub(crate) fn jet_simd_f32_binary_slice(
    left: &[f32],
    right: &[f32],
    op: JetSimdBinaryOp,
) -> Option<Vec<f32>> {
    if left.len() != right.len() {
        return None;
    }
    let mut out = Vec::with_capacity(left.len());
    let mut index = 0;
    while index <= left.len().saturating_sub(8) {
        let mut left_chunk = [0.0; 8];
        let mut right_chunk = [0.0; 8];
        left_chunk.copy_from_slice(&left[index..index + 8]);
        right_chunk.copy_from_slice(&right[index..index + 8]);
        out.extend_from_slice(&jet_simd_f32x8_binary(&left_chunk, &right_chunk, op));
        index += 8;
    }
    while index <= left.len().saturating_sub(4) {
        let mut left_chunk = [0.0; 4];
        let mut right_chunk = [0.0; 4];
        left_chunk.copy_from_slice(&left[index..index + 4]);
        right_chunk.copy_from_slice(&right[index..index + 4]);
        out.extend_from_slice(&jet_simd_f32x4_binary(&left_chunk, &right_chunk, op));
        index += 4;
    }
    while index < left.len() {
        out.push(jet_simd_apply(left[index], right[index], op));
        index += 1;
    }
    Some(out)
}

/// The F64 sibling of `jet_simd_f32_binary_slice`; four-wide AVX chunks,
/// two-wide SSE2 chunks, then the exact scalar tail.
#[inline(always)]
pub(crate) fn jet_simd_f64_binary_slice(
    left: &[f64],
    right: &[f64],
    op: JetSimdBinaryOp,
) -> Option<Vec<f64>> {
    if left.len() != right.len() {
        return None;
    }
    let mut out = Vec::with_capacity(left.len());
    let mut index = 0;
    while index <= left.len().saturating_sub(4) {
        let mut left_chunk = [0.0; 4];
        let mut right_chunk = [0.0; 4];
        left_chunk.copy_from_slice(&left[index..index + 4]);
        right_chunk.copy_from_slice(&right[index..index + 4]);
        out.extend_from_slice(&jet_simd_f64x4_binary(&left_chunk, &right_chunk, op));
        index += 4;
    }
    while index <= left.len().saturating_sub(2) {
        let mut left_chunk = [0.0; 2];
        let mut right_chunk = [0.0; 2];
        left_chunk.copy_from_slice(&left[index..index + 2]);
        right_chunk.copy_from_slice(&right[index..index + 2]);
        out.extend_from_slice(&jet_simd_f64x2_binary(&left_chunk, &right_chunk, op));
        index += 2;
    }
    while index < left.len() {
        out.push(jet_simd_apply(left[index], right[index], op));
        index += 1;
    }
    Some(out)
}

/// Shared left-to-right dot product for the resident math carriers. Keeping
/// the fold here makes the JIT adapter use the same scalar reduction order as
/// the AOT Prelude implementation.
#[inline(always)]
pub(crate) fn jet_simd_dot_f64_slice(left: &[f64], right: &[f64]) -> Option<f64> {
    if left.len() != right.len() {
        return None;
    }
    let mut value = 0.0;
    for (&left, &right) in left.iter().zip(right) {
        value += left * right;
    }
    Some(value)
}

#[inline(always)]
pub(crate) fn jet_simd_length_f64_slice(lanes: &[f64]) -> Option<f64> {
    jet_simd_dot_f64_slice(lanes, lanes).map(f64::sqrt)
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
