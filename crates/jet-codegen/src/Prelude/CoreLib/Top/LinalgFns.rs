// ── D-SIMD2 / D-SIMD3 / D-LINALG1: math value-type free functions ────────────
// Constructors (`_new`), statics (`splat`/`from_array`), instance methods, lane
// reads, and reductions. Codegen names these `jet_math_<Type>_<fn>` and always
// passes the receiver as `&recv` (value types — every op returns a fresh value).
// Fixed arrays plus aggressive inlining give portable AOT LLVM a vectorizable
// shape. Host-native F64x4 uses the private AVX carrier from the shared
// Prelude; reductions still marshal through its scalar left-to-right fold so
// every tier keeps the same result.

macro_rules! jet_simd_lane_fns {
    (
        $T:ident,
        $new:ident, $splat:ident, $from_array:ident, $to_array:ident,
        $lane:ident, $sum:ident, $product:ident, $min:ident, $max:ident,
        $reduce_add:ident, $reduce_mul:ident, $reduce_min:ident,
        $reduce_max:ident, $reduce_avg:ident,
        $scalar:ty, $n:literal, $( $arg:ident ),+
    ) => {
        #[inline(always)]
        fn $new($( $arg: $scalar ),+) -> jet_std::$T {
            jet_std::$T([$( $arg ),+])
        }
        #[inline(always)]
        fn $splat(x: $scalar) -> jet_std::$T {
            jet_std::$T(crate::jet_simd_splat_array(x))
        }
        #[inline(always)]
        fn $from_array(a: [$scalar; $n]) -> jet_std::$T {
            jet_std::$T(a)
        }
        #[inline(always)]
        fn $to_array(v: &jet_std::$T) -> [$scalar; $n] {
            v.0
        }
        #[inline(always)]
        fn $lane(v: &jet_std::$T, i: i64, file: &str, line: u32) -> $scalar {
            let index = match crate::jet_simd_lane_index(i, stringify!($T), $n) {
                Ok(index) => index,
                Err(message) => jet_panic(file, line, &message),
            };
            v.0[index]
        }
        #[inline(always)]
        fn $sum(v: &jet_std::$T) -> $scalar {
            crate::jet_simd_sum_array(&v.0)
        }
        #[inline(always)]
        fn $product(v: &jet_std::$T) -> $scalar {
            crate::jet_simd_product_array(&v.0)
        }
        #[inline(always)]
        fn $min(v: &jet_std::$T) -> $scalar {
            crate::jet_simd_min_array(&v.0)
        }
        #[inline(always)]
        fn $max(v: &jet_std::$T) -> $scalar {
            crate::jet_simd_max_array(&v.0)
        }
        #[inline(always)]
        fn $reduce_add(v: &jet_std::$T) -> $scalar { $sum(v) }
        #[inline(always)]
        fn $reduce_mul(v: &jet_std::$T) -> $scalar { $product(v) }
        #[inline(always)]
        fn $reduce_min(v: &jet_std::$T) -> $scalar { $min(v) }
        #[inline(always)]
        fn $reduce_max(v: &jet_std::$T) -> $scalar { $max(v) }
        #[inline(always)]
        fn $reduce_avg(v: &jet_std::$T) -> $scalar {
            crate::jet_simd_avg_array(&v.0)
        }
    };
}

jet_simd_lane_fns!(
    F32x4,
    jet_math_F32x4_new, jet_math_F32x4_splat, jet_math_F32x4_from_array,
    jet_math_F32x4_to_array, jet_math_F32x4_lane, jet_math_F32x4_sum,
    jet_math_F32x4_product, jet_math_F32x4_min, jet_math_F32x4_max,
    jet_math_F32x4_reduce_add, jet_math_F32x4_reduce_mul,
    jet_math_F32x4_reduce_min, jet_math_F32x4_reduce_max,
    jet_math_F32x4_reduce_avg, f32, 4, a, b, c, d
);
jet_simd_lane_fns!(
    F64x2,
    jet_math_F64x2_new, jet_math_F64x2_splat, jet_math_F64x2_from_array,
    jet_math_F64x2_to_array, jet_math_F64x2_lane, jet_math_F64x2_sum,
    jet_math_F64x2_product, jet_math_F64x2_min, jet_math_F64x2_max,
    jet_math_F64x2_reduce_add, jet_math_F64x2_reduce_mul,
    jet_math_F64x2_reduce_min, jet_math_F64x2_reduce_max,
    jet_math_F64x2_reduce_avg, f64, 2, a, b
);
jet_simd_lane_fns!(
    F32x8,
    jet_math_F32x8_new, jet_math_F32x8_splat, jet_math_F32x8_from_array,
    jet_math_F32x8_to_array, jet_math_F32x8_lane, jet_math_F32x8_sum,
    jet_math_F32x8_product, jet_math_F32x8_min, jet_math_F32x8_max,
    jet_math_F32x8_reduce_add, jet_math_F32x8_reduce_mul,
    jet_math_F32x8_reduce_min, jet_math_F32x8_reduce_max,
    jet_math_F32x8_reduce_avg, f32, 8, a, b, c, d, e, f, g, h
);
#[inline(always)]
fn jet_math_F64x4_new(a: f64, b: f64, c: f64, d: f64) -> jet_std::F64x4 {
    jet_std::F64x4(crate::jet_simd_f64x4_new_native([a, b, c, d]))
}
#[inline(always)]
fn jet_math_F64x4_splat(x: f64) -> jet_std::F64x4 {
    jet_std::F64x4(crate::jet_simd_f64x4_splat_native(x))
}
#[inline(always)]
fn jet_math_F64x4_from_array(a: [f64; 4]) -> jet_std::F64x4 {
    jet_std::F64x4(crate::jet_simd_f64x4_new_native(a))
}
#[inline(always)]
fn jet_math_F64x4_to_array(v: &jet_std::F64x4) -> [f64; 4] {
    crate::jet_simd_f64x4_to_array_native(v.0)
}
#[inline(always)]
fn jet_math_F64x4_lane(v: &jet_std::F64x4, i: i64, file: &str, line: u32) -> f64 {
    let index = match crate::jet_simd_lane_index(i, "F64x4", 4) {
        Ok(index) => index,
        Err(message) => jet_panic(file, line, &message),
    };
    crate::jet_simd_f64x4_lane_native(v.0, index)
}

#[inline(always)]
fn jet_math_F64x4_lane_const<const INDEX: usize>(v: &jet_std::F64x4) -> f64 {
    crate::jet_simd_f64x4_lane_const_native::<INDEX>(v.0)
}
#[inline(always)]
fn jet_math_F64x4_mul_lane_scale<const INDEX: usize>(
    value: &jet_std::F64x4,
    scale: f64,
    lane_source: &jet_std::F64x4,
) -> jet_std::F64x4 {
    jet_std::F64x4(crate::jet_simd_f64x4_mul_lane_scale_native::<INDEX>(
        value.0,
        scale,
        lane_source.0,
    ))
}


#[inline(always)]
fn jet_math_F64x4_gather_lane<const INDEX: usize>(
    first: &jet_std::F64x4,
    second: &jet_std::F64x4,
    third: &jet_std::F64x4,
    fourth: &jet_std::F64x4,
) -> jet_std::F64x4 {
    jet_std::F64x4(crate::jet_simd_f64x4_gather_lane_native::<INDEX>(
        first.0, second.0, third.0, fourth.0,
    ))
}

#[inline(always)]
fn jet_math_F64x4_sqrt(v: &jet_std::F64x4) -> jet_std::F64x4 {
    jet_std::F64x4(crate::jet_simd_f64x4_sqrt_native_carrier(v.0))
}

#[inline(always)]
fn jet_math_F64x4_sum(v: &jet_std::F64x4) -> f64 {
    let values = jet_math_F64x4_to_array(v);
    crate::jet_simd_sum_array(&values)
}
#[inline(always)]
fn jet_math_F64x4_product(v: &jet_std::F64x4) -> f64 {
    let values = jet_math_F64x4_to_array(v);
    crate::jet_simd_product_array(&values)
}
#[inline(always)]
fn jet_math_F64x4_min(v: &jet_std::F64x4) -> f64 {
    let values = jet_math_F64x4_to_array(v);
    crate::jet_simd_min_array(&values)
}
#[inline(always)]
fn jet_math_F64x4_max(v: &jet_std::F64x4) -> f64 {
    let values = jet_math_F64x4_to_array(v);
    crate::jet_simd_max_array(&values)
}
#[inline(always)]
fn jet_math_F64x4_reduce_add(v: &jet_std::F64x4) -> f64 {
    jet_math_F64x4_sum(v)
}
#[inline(always)]
fn jet_math_F64x4_reduce_mul(v: &jet_std::F64x4) -> f64 {
    jet_math_F64x4_product(v)
}
#[inline(always)]
fn jet_math_F64x4_reduce_min(v: &jet_std::F64x4) -> f64 {
    jet_math_F64x4_min(v)
}
#[inline(always)]
fn jet_math_F64x4_reduce_max(v: &jet_std::F64x4) -> f64 {
    jet_math_F64x4_max(v)
}
#[inline(always)]
fn jet_math_F64x4_reduce_avg(v: &jet_std::F64x4) -> f64 {
    let values = jet_math_F64x4_to_array(v);
    crate::jet_simd_avg_array(&values)
}

jet_simd_lane_fns!(I8x16, jet_math_I8x16_new, jet_math_I8x16_splat, jet_math_I8x16_from_array, jet_math_I8x16_to_array, jet_math_I8x16_lane, jet_math_I8x16_sum, jet_math_I8x16_product, jet_math_I8x16_min, jet_math_I8x16_max, jet_math_I8x16_reduce_add, jet_math_I8x16_reduce_mul, jet_math_I8x16_reduce_min, jet_math_I8x16_reduce_max, jet_math_I8x16_reduce_avg, i8, 16, a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p);
jet_simd_lane_fns!(I16x8, jet_math_I16x8_new, jet_math_I16x8_splat, jet_math_I16x8_from_array, jet_math_I16x8_to_array, jet_math_I16x8_lane, jet_math_I16x8_sum, jet_math_I16x8_product, jet_math_I16x8_min, jet_math_I16x8_max, jet_math_I16x8_reduce_add, jet_math_I16x8_reduce_mul, jet_math_I16x8_reduce_min, jet_math_I16x8_reduce_max, jet_math_I16x8_reduce_avg, i16, 8, a,b,c,d,e,f,g,h);
jet_simd_lane_fns!(I32x4, jet_math_I32x4_new, jet_math_I32x4_splat, jet_math_I32x4_from_array, jet_math_I32x4_to_array, jet_math_I32x4_lane, jet_math_I32x4_sum, jet_math_I32x4_product, jet_math_I32x4_min, jet_math_I32x4_max, jet_math_I32x4_reduce_add, jet_math_I32x4_reduce_mul, jet_math_I32x4_reduce_min, jet_math_I32x4_reduce_max, jet_math_I32x4_reduce_avg, i32, 4, a,b,c,d);
jet_simd_lane_fns!(I64x2, jet_math_I64x2_new, jet_math_I64x2_splat, jet_math_I64x2_from_array, jet_math_I64x2_to_array, jet_math_I64x2_lane, jet_math_I64x2_sum, jet_math_I64x2_product, jet_math_I64x2_min, jet_math_I64x2_max, jet_math_I64x2_reduce_add, jet_math_I64x2_reduce_mul, jet_math_I64x2_reduce_min, jet_math_I64x2_reduce_max, jet_math_I64x2_reduce_avg, i64, 2, a,b);
jet_simd_lane_fns!(U8x16, jet_math_U8x16_new, jet_math_U8x16_splat, jet_math_U8x16_from_array, jet_math_U8x16_to_array, jet_math_U8x16_lane, jet_math_U8x16_sum, jet_math_U8x16_product, jet_math_U8x16_min, jet_math_U8x16_max, jet_math_U8x16_reduce_add, jet_math_U8x16_reduce_mul, jet_math_U8x16_reduce_min, jet_math_U8x16_reduce_max, jet_math_U8x16_reduce_avg, u8, 16, a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p);
jet_simd_lane_fns!(U16x8, jet_math_U16x8_new, jet_math_U16x8_splat, jet_math_U16x8_from_array, jet_math_U16x8_to_array, jet_math_U16x8_lane, jet_math_U16x8_sum, jet_math_U16x8_product, jet_math_U16x8_min, jet_math_U16x8_max, jet_math_U16x8_reduce_add, jet_math_U16x8_reduce_mul, jet_math_U16x8_reduce_min, jet_math_U16x8_reduce_max, jet_math_U16x8_reduce_avg, u16, 8, a,b,c,d,e,f,g,h);
jet_simd_lane_fns!(U32x4, jet_math_U32x4_new, jet_math_U32x4_splat, jet_math_U32x4_from_array, jet_math_U32x4_to_array, jet_math_U32x4_lane, jet_math_U32x4_sum, jet_math_U32x4_product, jet_math_U32x4_min, jet_math_U32x4_max, jet_math_U32x4_reduce_add, jet_math_U32x4_reduce_mul, jet_math_U32x4_reduce_min, jet_math_U32x4_reduce_max, jet_math_U32x4_reduce_avg, u32, 4, a,b,c,d);
jet_simd_lane_fns!(U64x2, jet_math_U64x2_new, jet_math_U64x2_splat, jet_math_U64x2_from_array, jet_math_U64x2_to_array, jet_math_U64x2_lane, jet_math_U64x2_sum, jet_math_U64x2_product, jet_math_U64x2_min, jet_math_U64x2_max, jet_math_U64x2_reduce_add, jet_math_U64x2_reduce_mul, jet_math_U64x2_reduce_min, jet_math_U64x2_reduce_max, jet_math_U64x2_reduce_avg, u64, 2, a,b);
jet_simd_lane_fns!(I8x32, jet_math_I8x32_new, jet_math_I8x32_splat, jet_math_I8x32_from_array, jet_math_I8x32_to_array, jet_math_I8x32_lane, jet_math_I8x32_sum, jet_math_I8x32_product, jet_math_I8x32_min, jet_math_I8x32_max, jet_math_I8x32_reduce_add, jet_math_I8x32_reduce_mul, jet_math_I8x32_reduce_min, jet_math_I8x32_reduce_max, jet_math_I8x32_reduce_avg, i8, 32, a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p,q,r,s,t,u,v,w,x,y,z,aa,ab,ac,ad,ae,af);
jet_simd_lane_fns!(I16x16, jet_math_I16x16_new, jet_math_I16x16_splat, jet_math_I16x16_from_array, jet_math_I16x16_to_array, jet_math_I16x16_lane, jet_math_I16x16_sum, jet_math_I16x16_product, jet_math_I16x16_min, jet_math_I16x16_max, jet_math_I16x16_reduce_add, jet_math_I16x16_reduce_mul, jet_math_I16x16_reduce_min, jet_math_I16x16_reduce_max, jet_math_I16x16_reduce_avg, i16, 16, a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p);
jet_simd_lane_fns!(I32x8, jet_math_I32x8_new, jet_math_I32x8_splat, jet_math_I32x8_from_array, jet_math_I32x8_to_array, jet_math_I32x8_lane, jet_math_I32x8_sum, jet_math_I32x8_product, jet_math_I32x8_min, jet_math_I32x8_max, jet_math_I32x8_reduce_add, jet_math_I32x8_reduce_mul, jet_math_I32x8_reduce_min, jet_math_I32x8_reduce_max, jet_math_I32x8_reduce_avg, i32, 8, a,b,c,d,e,f,g,h);
jet_simd_lane_fns!(I64x4, jet_math_I64x4_new, jet_math_I64x4_splat, jet_math_I64x4_from_array, jet_math_I64x4_to_array, jet_math_I64x4_lane, jet_math_I64x4_sum, jet_math_I64x4_product, jet_math_I64x4_min, jet_math_I64x4_max, jet_math_I64x4_reduce_add, jet_math_I64x4_reduce_mul, jet_math_I64x4_reduce_min, jet_math_I64x4_reduce_max, jet_math_I64x4_reduce_avg, i64, 4, a,b,c,d);
jet_simd_lane_fns!(U8x32, jet_math_U8x32_new, jet_math_U8x32_splat, jet_math_U8x32_from_array, jet_math_U8x32_to_array, jet_math_U8x32_lane, jet_math_U8x32_sum, jet_math_U8x32_product, jet_math_U8x32_min, jet_math_U8x32_max, jet_math_U8x32_reduce_add, jet_math_U8x32_reduce_mul, jet_math_U8x32_reduce_min, jet_math_U8x32_reduce_max, jet_math_U8x32_reduce_avg, u8, 32, a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p,q,r,s,t,u,v,w,x,y,z,aa,ab,ac,ad,ae,af);
jet_simd_lane_fns!(U16x16, jet_math_U16x16_new, jet_math_U16x16_splat, jet_math_U16x16_from_array, jet_math_U16x16_to_array, jet_math_U16x16_lane, jet_math_U16x16_sum, jet_math_U16x16_product, jet_math_U16x16_min, jet_math_U16x16_max, jet_math_U16x16_reduce_add, jet_math_U16x16_reduce_mul, jet_math_U16x16_reduce_min, jet_math_U16x16_reduce_max, jet_math_U16x16_reduce_avg, u16, 16, a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p);
jet_simd_lane_fns!(U32x8, jet_math_U32x8_new, jet_math_U32x8_splat, jet_math_U32x8_from_array, jet_math_U32x8_to_array, jet_math_U32x8_lane, jet_math_U32x8_sum, jet_math_U32x8_product, jet_math_U32x8_min, jet_math_U32x8_max, jet_math_U32x8_reduce_add, jet_math_U32x8_reduce_mul, jet_math_U32x8_reduce_min, jet_math_U32x8_reduce_max, jet_math_U32x8_reduce_avg, u32, 8, a,b,c,d,e,f,g,h);
jet_simd_lane_fns!(U64x4, jet_math_U64x4_new, jet_math_U64x4_splat, jet_math_U64x4_from_array, jet_math_U64x4_to_array, jet_math_U64x4_lane, jet_math_U64x4_sum, jet_math_U64x4_product, jet_math_U64x4_min, jet_math_U64x4_max, jet_math_U64x4_reduce_add, jet_math_U64x4_reduce_mul, jet_math_U64x4_reduce_min, jet_math_U64x4_reduce_max, jet_math_U64x4_reduce_avg, u64, 4, a,b,c,d);
macro_rules! jet_simd_binary_fns {
    ($T:ident, $add:ident, $sub:ident, $mul:ident, $div:ident) => {
        #[inline(always)]
        fn $add(left: &jet_std::$T, right: jet_std::$T) -> jet_std::$T {
            *left + right
        }
        #[inline(always)]
        fn $sub(left: &jet_std::$T, right: jet_std::$T) -> jet_std::$T {
            *left - right
        }
        #[inline(always)]
        fn $mul(left: &jet_std::$T, right: jet_std::$T) -> jet_std::$T {
            *left * right
        }
        #[inline(always)]
        fn $div(left: &jet_std::$T, right: jet_std::$T) -> jet_std::$T {
            *left / right
        }
    };
}

jet_simd_binary_fns!(F32x4, jet_math_F32x4_add, jet_math_F32x4_sub, jet_math_F32x4_mul, jet_math_F32x4_div);
jet_simd_binary_fns!(F64x2, jet_math_F64x2_add, jet_math_F64x2_sub, jet_math_F64x2_mul, jet_math_F64x2_div);
jet_simd_binary_fns!(F32x8, jet_math_F32x8_add, jet_math_F32x8_sub, jet_math_F32x8_mul, jet_math_F32x8_div);
jet_simd_binary_fns!(F64x4, jet_math_F64x4_add, jet_math_F64x4_sub, jet_math_F64x4_mul, jet_math_F64x4_div);
jet_simd_binary_fns!(I8x16, jet_math_I8x16_add, jet_math_I8x16_sub, jet_math_I8x16_mul, jet_math_I8x16_div);
jet_simd_binary_fns!(I16x8, jet_math_I16x8_add, jet_math_I16x8_sub, jet_math_I16x8_mul, jet_math_I16x8_div);
jet_simd_binary_fns!(I32x4, jet_math_I32x4_add, jet_math_I32x4_sub, jet_math_I32x4_mul, jet_math_I32x4_div);
jet_simd_binary_fns!(I64x2, jet_math_I64x2_add, jet_math_I64x2_sub, jet_math_I64x2_mul, jet_math_I64x2_div);
jet_simd_binary_fns!(U8x16, jet_math_U8x16_add, jet_math_U8x16_sub, jet_math_U8x16_mul, jet_math_U8x16_div);
jet_simd_binary_fns!(U16x8, jet_math_U16x8_add, jet_math_U16x8_sub, jet_math_U16x8_mul, jet_math_U16x8_div);
jet_simd_binary_fns!(U32x4, jet_math_U32x4_add, jet_math_U32x4_sub, jet_math_U32x4_mul, jet_math_U32x4_div);
jet_simd_binary_fns!(U64x2, jet_math_U64x2_add, jet_math_U64x2_sub, jet_math_U64x2_mul, jet_math_U64x2_div);
jet_simd_binary_fns!(I8x32, jet_math_I8x32_add, jet_math_I8x32_sub, jet_math_I8x32_mul, jet_math_I8x32_div);
jet_simd_binary_fns!(I16x16, jet_math_I16x16_add, jet_math_I16x16_sub, jet_math_I16x16_mul, jet_math_I16x16_div);
jet_simd_binary_fns!(I32x8, jet_math_I32x8_add, jet_math_I32x8_sub, jet_math_I32x8_mul, jet_math_I32x8_div);
jet_simd_binary_fns!(I64x4, jet_math_I64x4_add, jet_math_I64x4_sub, jet_math_I64x4_mul, jet_math_I64x4_div);
jet_simd_binary_fns!(U8x32, jet_math_U8x32_add, jet_math_U8x32_sub, jet_math_U8x32_mul, jet_math_U8x32_div);
jet_simd_binary_fns!(U16x16, jet_math_U16x16_add, jet_math_U16x16_sub, jet_math_U16x16_mul, jet_math_U16x16_div);
jet_simd_binary_fns!(U32x8, jet_math_U32x8_add, jet_math_U32x8_sub, jet_math_U32x8_mul, jet_math_U32x8_div);
jet_simd_binary_fns!(U64x4, jet_math_U64x4_add, jet_math_U64x4_sub, jet_math_U64x4_mul, jet_math_U64x4_div);

// Vectors.
fn jet_math_Vec2_new(x: f64, y: f64) -> jet_std::Vec2 {
    jet_std::Vec2([x, y])
}
fn jet_math_Vec3_new(x: f64, y: f64, z: f64) -> jet_std::Vec3 {
    jet_std::Vec3([x, y, z])
}
fn jet_math_Vec4_new(x: f64, y: f64, z: f64, w: f64) -> jet_std::Vec4 {
    jet_std::Vec4([x, y, z, w])
}
fn jet_math_Vec2_splat(x: f64) -> jet_std::Vec2 {
    jet_std::Vec2([x; 2])
}
fn jet_math_Vec3_splat(x: f64) -> jet_std::Vec3 {
    jet_std::Vec3([x; 3])
}
fn jet_math_Vec4_splat(x: f64) -> jet_std::Vec4 {
    jet_std::Vec4([x; 4])
}
fn jet_math_Vec2_from_array(a: [f64; 2]) -> jet_std::Vec2 {
    jet_std::Vec2(a)
}
fn jet_math_Vec3_from_array(a: [f64; 3]) -> jet_std::Vec3 {
    jet_std::Vec3(a)
}
fn jet_math_Vec4_from_array(a: [f64; 4]) -> jet_std::Vec4 {
    jet_std::Vec4(a)
}
fn jet_math_Vec2_to_array(v: &jet_std::Vec2) -> [f64; 2] {
    v.0
}
fn jet_math_Vec3_to_array(v: &jet_std::Vec3) -> [f64; 3] {
    v.0
}
fn jet_math_Vec4_to_array(v: &jet_std::Vec4) -> [f64; 4] {
    v.0
}
#[inline(always)]
fn jet_math_Vec2_add(left: &jet_std::Vec2, right: jet_std::Vec2) -> jet_std::Vec2 {
    *left + right
}
#[inline(always)]
fn jet_math_Vec2_sub(left: &jet_std::Vec2, right: jet_std::Vec2) -> jet_std::Vec2 {
    *left - right
}
#[inline(always)]
fn jet_math_Vec2_mul(left: &jet_std::Vec2, right: jet_std::Vec2) -> jet_std::Vec2 {
    *left * right
}
#[inline(always)]
fn jet_math_Vec3_add(left: &jet_std::Vec3, right: jet_std::Vec3) -> jet_std::Vec3 {
    *left + right
}
#[inline(always)]
fn jet_math_Vec3_sub(left: &jet_std::Vec3, right: jet_std::Vec3) -> jet_std::Vec3 {
    *left - right
}
#[inline(always)]
fn jet_math_Vec3_hadamard_mul(
    left: &jet_std::Vec3,
    right: jet_std::Vec3,
) -> jet_std::Vec3 {
    *left * right
}
#[inline(always)]
fn jet_math_Vec4_add(left: &jet_std::Vec4, right: jet_std::Vec4) -> jet_std::Vec4 {
    *left + right
}
#[inline(always)]
fn jet_math_Vec4_sub(left: &jet_std::Vec4, right: jet_std::Vec4) -> jet_std::Vec4 {
    *left - right
}
#[inline(always)]
fn jet_math_Vec4_mul(left: &jet_std::Vec4, right: jet_std::Vec4) -> jet_std::Vec4 {
    *left * right
}
#[inline(always)]
fn jet_math_Vec3_mul(v: &jet_std::Vec3, s: f64) -> jet_std::Vec3 {
    jet_std::Vec3([v.0[0] * s, v.0[1] * s, v.0[2] * s])
}
#[inline(always)]
fn jet_math_Vec3_div(v: &jet_std::Vec3, s: f64) -> jet_std::Vec3 {
    jet_std::Vec3([v.0[0] / s, v.0[1] / s, v.0[2] / s])
}
#[inline(always)]
fn jet_math_Float_div_Vec3(s: f64, v: &jet_std::Vec3) -> jet_std::Vec3 {
    jet_std::Vec3([s / v.0[0], s / v.0[1], s / v.0[2]])
}


fn jet_math_Vec2_dot(v: &jet_std::Vec2, o: jet_std::Vec2) -> f64 {
    v.0[0] * o.0[0] + v.0[1] * o.0[1]
}
fn jet_math_Vec3_dot(v: &jet_std::Vec3, o: jet_std::Vec3) -> f64 {
    v.0[0] * o.0[0] + v.0[1] * o.0[1] + v.0[2] * o.0[2]
}
fn jet_math_Vec4_dot(v: &jet_std::Vec4, o: jet_std::Vec4) -> f64 {
    (0..4).map(|i| v.0[i] * o.0[i]).sum()
}
fn jet_math_Vec3_cross(v: &jet_std::Vec3, o: jet_std::Vec3) -> jet_std::Vec3 {
    jet_std::Vec3([
        v.0[1] * o.0[2] - v.0[2] * o.0[1],
        v.0[2] * o.0[0] - v.0[0] * o.0[2],
        v.0[0] * o.0[1] - v.0[1] * o.0[0],
    ])
}
fn jet_math_Vec2_length(v: &jet_std::Vec2) -> f64 {
    jet_math_Vec2_dot(v, *v).sqrt()
}
fn jet_math_Vec3_length(v: &jet_std::Vec3) -> f64 {
    jet_math_Vec3_dot(v, *v).sqrt()
}
fn jet_math_Vec4_length(v: &jet_std::Vec4) -> f64 {
    jet_math_Vec4_dot(v, *v).sqrt()
}
fn jet_math_Vec2_normalize(v: &jet_std::Vec2) -> jet_std::Vec2 {
    let l = jet_math_Vec2_length(v);
    if l == 0.0 {
        *v
    } else {
        jet_std::Vec2([v.0[0] / l, v.0[1] / l])
    }
}
fn jet_math_Vec3_normalize(v: &jet_std::Vec3) -> jet_std::Vec3 {
    let l = jet_math_Vec3_length(v);
    if l == 0.0 {
        *v
    } else {
        jet_std::Vec3([v.0[0] / l, v.0[1] / l, v.0[2] / l])
    }
}
fn jet_math_Vec4_normalize(v: &jet_std::Vec4) -> jet_std::Vec4 {
    let l = jet_math_Vec4_length(v);
    if l == 0.0 {
        *v
    } else {
        let mut r = v.0;
        for i in 0..4 {
            r[i] /= l;
        }
        jet_std::Vec4(r)
    }
}

// Matrices (column-major). Constructors take N*N components in column-major order.
fn jet_math_Mat3_new(
    m0: f64,
    m1: f64,
    m2: f64,
    m3: f64,
    m4: f64,
    m5: f64,
    m6: f64,
    m7: f64,
    m8: f64,
) -> jet_std::Mat3 {
    jet_std::Mat3([m0, m1, m2, m3, m4, m5, m6, m7, m8])
}
fn jet_math_Mat4_new(
    m0: f64,
    m1: f64,
    m2: f64,
    m3: f64,
    m4: f64,
    m5: f64,
    m6: f64,
    m7: f64,
    m8: f64,
    m9: f64,
    m10: f64,
    m11: f64,
    m12: f64,
    m13: f64,
    m14: f64,
    m15: f64,
) -> jet_std::Mat4 {
    jet_std::Mat4([
        m0, m1, m2, m3, m4, m5, m6, m7, m8, m9, m10, m11, m12, m13, m14, m15,
    ])
}
fn jet_math_Mat3_from_array(a: [f64; 9]) -> jet_std::Mat3 {
    jet_std::Mat3(a)
}
fn jet_math_Mat4_from_array(a: [f64; 16]) -> jet_std::Mat4 {
    jet_std::Mat4(a)
}
fn jet_math_Mat3_to_array(m: &jet_std::Mat3) -> [f64; 9] {
    m.0
}
fn jet_math_Mat4_to_array(m: &jet_std::Mat4) -> [f64; 16] {
    m.0
}
#[inline(always)]
fn jet_math_Mat3_add(left: &jet_std::Mat3, right: jet_std::Mat3) -> jet_std::Mat3 {
    *left + right
}
#[inline(always)]
fn jet_math_Mat3_sub(left: &jet_std::Mat3, right: jet_std::Mat3) -> jet_std::Mat3 {
    *left - right
}
#[inline(always)]
fn jet_math_Mat3_mul(left: &jet_std::Mat3, right: jet_std::Mat3) -> jet_std::Mat3 {
    *left * right
}
#[inline(always)]
fn jet_math_Mat4_add(left: &jet_std::Mat4, right: jet_std::Mat4) -> jet_std::Mat4 {
    *left + right
}
#[inline(always)]
fn jet_math_Mat4_sub(left: &jet_std::Mat4, right: jet_std::Mat4) -> jet_std::Mat4 {
    *left - right
}
#[inline(always)]
fn jet_math_Mat4_mul(left: &jet_std::Mat4, right: jet_std::Mat4) -> jet_std::Mat4 {
    *left * right
}
fn jet_math_Mat3_matmul(m: &jet_std::Mat3, o: jet_std::Mat3) -> jet_std::Mat3 {
    *m * o
}
fn jet_math_Mat4_matmul(m: &jet_std::Mat4, o: jet_std::Mat4) -> jet_std::Mat4 {
    *m * o
}
fn jet_math_Mat3_transform(m: &jet_std::Mat3, v: jet_std::Vec3) -> jet_std::Vec3 {
    *m * v
}
fn jet_math_Mat4_transform(m: &jet_std::Mat4, v: jet_std::Vec4) -> jet_std::Vec4 {
    *m * v
}
fn jet_math_Mat3_transpose(m: &jet_std::Mat3) -> jet_std::Mat3 {
    let mut r = [0.0f64; 9];
    for c in 0..3 {
        for row in 0..3 {
            r[c * 3 + row] = m.0[row * 3 + c];
        }
    }
    jet_std::Mat3(r)
}
fn jet_math_Mat4_transpose(m: &jet_std::Mat4) -> jet_std::Mat4 {
    let mut r = [0.0f64; 16];
    for c in 0..4 {
        for row in 0..4 {
            r[c * 4 + row] = m.0[row * 4 + c];
        }
    }
    jet_std::Mat4(r)
}
// D-SPACE-GEOMETRY1=A: compact carriers for typed coordinate spaces.  Space
// parameters are erased by codegen; dynamic frame ids stay on values so a
// transform cannot silently consume a value from a different live frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JetCoord2 {
    pub x: f64,
    pub y: f64,
    pub frame_id: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JetTransform2 {
    pub m00: f64,
    pub m01: f64,
    pub m10: f64,
    pub m11: f64,
    pub tx: f64,
    pub ty: f64,
    pub from_frame: i64,
    pub to_frame: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JetRay2 {
    pub origin: JetCoord2,
    pub direction: JetCoord2,
}

#[inline]
fn geometry_error(message: &str) -> JetErr {
    jet_err_from_message(message.to_string())
}

#[inline]
fn geometry_transform_point(
    transform: &JetTransform2,
    point: JetCoord2,
) -> Result<JetCoord2, JetErr> {
    if point.frame_id != 0 && point.frame_id != transform.from_frame {
        return Err(geometry_error("coordinate point belongs to a stale frame"));
    }
    Ok(JetCoord2 {
        x: transform.m00 * point.x + transform.m01 * point.y + transform.tx,
        y: transform.m10 * point.x + transform.m11 * point.y + transform.ty,
        frame_id: transform.to_frame,
    })
}

#[inline]
fn geometry_transform_then(
    first: &JetTransform2,
    next: JetTransform2,
) -> Result<JetTransform2, JetErr> {
    if first.to_frame != 0 && next.from_frame != 0 && first.to_frame != next.from_frame {
        return Err(geometry_error("transform composition has mismatched frame identity"));
    }
    Ok(JetTransform2 {
        m00: next.m00 * first.m00 + next.m01 * first.m10,
        m01: next.m00 * first.m01 + next.m01 * first.m11,
        m10: next.m10 * first.m00 + next.m11 * first.m10,
        m11: next.m10 * first.m01 + next.m11 * first.m11,
        tx: next.m00 * first.tx + next.m01 * first.ty + next.tx,
        ty: next.m10 * first.tx + next.m11 * first.ty + next.ty,
        from_frame: first.from_frame,
        to_frame: next.to_frame,
    })
}

#[inline]
fn geometry_transform_inverse(transform: &JetTransform2) -> Result<JetTransform2, JetErr> {
    let det = transform.m00 * transform.m11 - transform.m01 * transform.m10;
    if !det.is_finite() || det.abs() <= f64::EPSILON {
        return Err(geometry_error("transform is singular and has no inverse"));
    }
    let inv_det = 1.0 / det;
    Ok(JetTransform2 {
        m00: transform.m11 * inv_det,
        m01: -transform.m01 * inv_det,
        m10: -transform.m10 * inv_det,
        m11: transform.m00 * inv_det,
        tx: (transform.m01 * transform.ty - transform.m11 * transform.tx) * inv_det,
        ty: (transform.m10 * transform.tx - transform.m00 * transform.ty) * inv_det,
        from_frame: transform.to_frame,
        to_frame: transform.from_frame,
    })
}
#[inline]
fn jet_math_Transform_affine(
    m00: f64,
    m01: f64,
    m10: f64,
    m11: f64,
    tx: f64,
    ty: f64,
    from_frame: i64,
    to_frame: i64,
) -> JetTransform2 {
    JetTransform2 {
        m00,
        m01,
        m10,
        m11,
        tx,
        ty,
        from_frame,
        to_frame,
    }
}

#[inline]
fn jet_math_Transform2_affine(
    m00: f64,
    m01: f64,
    m10: f64,
    m11: f64,
    tx: f64,
    ty: f64,
    from_frame: i64,
    to_frame: i64,
) -> JetTransform2 {
    jet_math_Transform_affine(m00, m01, m10, m11, tx, ty, from_frame, to_frame)
}

fn jet_math_Point2_new(x: f64, y: f64, frame_id: i64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id }
}

fn jet_math_Delta2_new(x: f64, y: f64, frame_id: i64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id }
}



fn jet_math_ScreenPoint_new(x: f64, y: f64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id: 0 }
}
fn jet_math_WorldPoint_new(x: f64, y: f64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id: 0 }
}
fn jet_math_ViewPoint_new(x: f64, y: f64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id: 0 }
}
fn jet_math_CameraPoint_new(x: f64, y: f64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id: 0 }
}
fn jet_math_DevicePoint_new(x: f64, y: f64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id: 0 }
}
fn jet_math_ScreenDelta_new(x: f64, y: f64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id: 0 }
}
fn jet_math_WorldDelta_new(x: f64, y: f64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id: 0 }
}
fn jet_math_ViewDelta_new(x: f64, y: f64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id: 0 }
}
fn jet_math_CameraDelta_new(x: f64, y: f64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id: 0 }
}
fn jet_math_DeviceDelta_new(x: f64, y: f64) -> JetCoord2 {
    JetCoord2 { x, y, frame_id: 0 }
}

#[inline]
fn geometry_coord_checked(
    left: &JetCoord2,
    right: JetCoord2,
    subtract: bool,
) -> Result<JetCoord2, JetErr> {
    if left.frame_id != 0 && right.frame_id != 0 && left.frame_id != right.frame_id {
        return Err(geometry_error("coordinate values belong to different frames"));
    }
    let frame_id = if left.frame_id != 0 {
        left.frame_id
    } else {
        right.frame_id
    };
    Ok(JetCoord2 {
        x: if subtract {
            left.x - right.x
        } else {
            left.x + right.x
        },
        y: if subtract {
            left.y - right.y
        } else {
            left.y + right.y
        },
        frame_id,
    })
}

#[inline]
fn geometry_coord_add(point: &JetCoord2, delta: JetCoord2) -> JetCoord2 {
    JetCoord2 {
        x: point.x + delta.x,
        y: point.y + delta.y,
        frame_id: point.frame_id,
    }
}

#[inline]
fn geometry_coord_sub(point: &JetCoord2, other: JetCoord2) -> JetCoord2 {
    JetCoord2 {
        x: point.x - other.x,
        y: point.y - other.y,
        frame_id: point.frame_id,
    }
}

fn jet_math_ScreenPoint_add(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_add(a, b) }
fn jet_math_ScreenPoint_sub(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_sub(a, b) }
fn jet_math_WorldPoint_add(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_add(a, b) }
fn jet_math_WorldPoint_sub(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_sub(a, b) }
fn jet_math_ViewPoint_add(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_add(a, b) }
fn jet_math_ViewPoint_sub(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_sub(a, b) }
fn jet_math_CameraPoint_add(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_add(a, b) }
fn jet_math_CameraPoint_sub(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_sub(a, b) }
fn jet_math_DevicePoint_add(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_add(a, b) }
fn jet_math_DevicePoint_sub(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_sub(a, b) }
fn jet_math_ScreenDelta_add(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_add(a, b) }
fn jet_math_ScreenDelta_sub(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_sub(a, b) }
fn jet_math_WorldDelta_add(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_add(a, b) }
fn jet_math_WorldDelta_sub(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_sub(a, b) }
fn jet_math_ViewDelta_add(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_add(a, b) }
fn jet_math_ViewDelta_sub(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_sub(a, b) }
fn jet_math_CameraDelta_add(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_add(a, b) }
fn jet_math_CameraDelta_sub(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_sub(a, b) }
fn jet_math_DeviceDelta_add(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_add(a, b) }
fn jet_math_DeviceDelta_sub(a: &JetCoord2, b: JetCoord2) -> JetCoord2 { geometry_coord_sub(a, b) }
fn jet_math_Point2_add(a: &JetCoord2, b: JetCoord2) -> Result<JetCoord2, JetErr> {
    geometry_coord_checked(a, b, false)
}
fn jet_math_Point2_sub(a: &JetCoord2, b: JetCoord2) -> Result<JetCoord2, JetErr> {
    geometry_coord_checked(a, b, true)
}
fn jet_math_Delta2_add(a: &JetCoord2, b: JetCoord2) -> Result<JetCoord2, JetErr> {
    geometry_coord_checked(a, b, false)
}
fn jet_math_Delta2_sub(a: &JetCoord2, b: JetCoord2) -> Result<JetCoord2, JetErr> {
    geometry_coord_checked(a, b, true)
}


#[inline]
fn jet_math_Transform_then(
    transform: &JetTransform2,
    next: JetTransform2,
) -> Result<JetTransform2, JetErr> {
    geometry_transform_then(transform, next)
}

#[inline]
fn jet_math_Transform_inverse(transform: &JetTransform2) -> Result<JetTransform2, JetErr> {
    geometry_transform_inverse(transform)
}

#[inline]
fn jet_math_Transform_point(
    transform: &JetTransform2,
    point: JetCoord2,
) -> Result<JetCoord2, JetErr> {
    geometry_transform_point(transform, point)
}

#[inline]
fn jet_math_Transform_point_at_depth(
    transform: &JetTransform2,
    point: JetCoord2,
    depth: f64,
) -> Result<JetCoord2, JetErr> {
    if !depth.is_finite() {
        return Err(geometry_error("perspective depth must be finite"));
    }
    let origin = geometry_transform_point(transform, point)?;
    Ok(JetCoord2 {
        x: origin.x + transform.m00 * depth,
        y: origin.y + transform.m10 * depth,
        frame_id: origin.frame_id,
    })
}

#[inline]
fn jet_math_Transform_ray(
    transform: &JetTransform2,
    point: JetCoord2,
) -> Result<JetRay2, JetErr> {
    let origin = geometry_transform_point(transform, point)?;
    let direction = JetCoord2 {
        x: transform.m00,
        y: transform.m10,
        frame_id: transform.to_frame,
    };
    Ok(JetRay2 { origin, direction })
}
#[inline]
fn jet_math_Transform2_then(
    transform: &JetTransform2,
    next: JetTransform2,
) -> Result<JetTransform2, JetErr> {
    geometry_transform_then(transform, next)
}

#[inline]
fn jet_math_Transform2_inverse(transform: &JetTransform2) -> Result<JetTransform2, JetErr> {
    geometry_transform_inverse(transform)
}

#[inline]
fn jet_math_Transform2_point(
    transform: &JetTransform2,
    point: JetCoord2,
) -> Result<JetCoord2, JetErr> {
    geometry_transform_point(transform, point)
}

#[inline]
fn jet_math_Transform2_point_at_depth(
    transform: &JetTransform2,
    point: JetCoord2,
    depth: f64,
) -> Result<JetCoord2, JetErr> {
    if !depth.is_finite() {
        return Err(geometry_error("perspective depth must be finite"));
    }
    let origin = geometry_transform_point(transform, point)?;
    Ok(JetCoord2 {
        x: origin.x + transform.m00 * depth,
        y: origin.y + transform.m10 * depth,
        frame_id: origin.frame_id,
    })
}

#[inline]
fn jet_math_Transform2_ray(
    transform: &JetTransform2,
    point: JetCoord2,
) -> Result<JetRay2, JetErr> {
    let origin = geometry_transform_point(transform, point)?;
    let direction = JetCoord2 {
        x: transform.m00,
        y: transform.m10,
        frame_id: transform.to_frame,
    };
    Ok(JetRay2 { origin, direction })
}
