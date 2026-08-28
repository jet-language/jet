// ── D-SIMD2 / D-SIMD3 / D-LINALG1: math value-type free functions ────────────
// Constructors (`_new`), statics (`splat`/`from_array`), instance methods, lane
// reads, and reductions. Codegen names these `jet_math_<Type>_<fn>` and always
// passes the receiver as `&recv` (value types — every op returns a fresh value).
// Fixed arrays plus aggressive inlining give native AOT LLVM a vectorizable
// shape while keeping the same safe Prelude semantics on every tier.

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
jet_simd_lane_fns!(
    F64x4,
    jet_math_F64x4_new, jet_math_F64x4_splat, jet_math_F64x4_from_array,
    jet_math_F64x4_to_array, jet_math_F64x4_lane, jet_math_F64x4_sum,
    jet_math_F64x4_product, jet_math_F64x4_min, jet_math_F64x4_max,
    jet_math_F64x4_reduce_add, jet_math_F64x4_reduce_mul,
    jet_math_F64x4_reduce_min, jet_math_F64x4_reduce_max,
    jet_math_F64x4_reduce_avg, f64, 4, a, b, c, d
);

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
