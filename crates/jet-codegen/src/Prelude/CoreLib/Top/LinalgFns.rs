// ── D-SIMD2 / D-LINALG1: math value-type free functions ───────────────────────
// Constructors (`_new`), statics (`splat`/`from_array`), instance methods, lane
// reads, and reductions. Codegen names these `jet_math_<Type>_<fn>` and always
// passes the receiver as `&recv` (value types — every op returns a fresh value).
// Plain std math; no intrinsics, no `un`+`safe`.

fn jet_math_F32x4_new(a: f32, b: f32, c: f32, d: f32) -> jet_std::F32x4 {
    jet_std::F32x4([a, b, c, d])
}
fn jet_math_F64x2_new(a: f64, b: f64) -> jet_std::F64x2 {
    jet_std::F64x2([a, b])
}
fn jet_math_F32x4_splat(x: f32) -> jet_std::F32x4 {
    jet_std::F32x4([x; 4])
}
fn jet_math_F64x2_splat(x: f64) -> jet_std::F64x2 {
    jet_std::F64x2([x; 2])
}
fn jet_math_F32x4_from_array(a: [f32; 4]) -> jet_std::F32x4 {
    jet_std::F32x4(a)
}
fn jet_math_F64x2_from_array(a: [f64; 2]) -> jet_std::F64x2 {
    jet_std::F64x2(a)
}
fn jet_math_F32x4_to_array(v: &jet_std::F32x4) -> [f32; 4] {
    v.0
}
fn jet_math_F64x2_to_array(v: &jet_std::F64x2) -> [f64; 2] {
    v.0
}

fn jet_math_F32x4_lane(v: &jet_std::F32x4, i: i64, file: &str, line: u32) -> f32 {
    if i < 0 || i as usize >= 4 {
        jet_panic(
            file,
            line,
            &format!("lane index {} out of range for F32x4 (4 lanes)", i),
        );
    }
    v.0[i as usize]
}
fn jet_math_F64x2_lane(v: &jet_std::F64x2, i: i64, file: &str, line: u32) -> f64 {
    if i < 0 || i as usize >= 2 {
        jet_panic(
            file,
            line,
            &format!("lane index {} out of range for F64x2 (2 lanes)", i),
        );
    }
    v.0[i as usize]
}

fn jet_math_F32x4_sum(v: &jet_std::F32x4) -> f32 {
    v.0.iter().sum()
}
fn jet_math_F32x4_product(v: &jet_std::F32x4) -> f32 {
    v.0.iter().product()
}
fn jet_math_F32x4_min(v: &jet_std::F32x4) -> f32 {
    v.0.iter().copied().fold(f32::INFINITY, f32::min)
}
fn jet_math_F32x4_max(v: &jet_std::F32x4) -> f32 {
    v.0.iter().copied().fold(f32::NEG_INFINITY, f32::max)
}
fn jet_math_F32x4_reduce_add(v: &jet_std::F32x4) -> f32 {
    jet_math_F32x4_sum(v)
}
fn jet_math_F32x4_reduce_mul(v: &jet_std::F32x4) -> f32 {
    jet_math_F32x4_product(v)
}
fn jet_math_F32x4_reduce_min(v: &jet_std::F32x4) -> f32 {
    jet_math_F32x4_min(v)
}
fn jet_math_F32x4_reduce_max(v: &jet_std::F32x4) -> f32 {
    jet_math_F32x4_max(v)
}
fn jet_math_F32x4_reduce_avg(v: &jet_std::F32x4) -> f32 {
    jet_math_F32x4_sum(v) / 4.0
}

fn jet_math_F64x2_sum(v: &jet_std::F64x2) -> f64 {
    v.0.iter().sum()
}
fn jet_math_F64x2_product(v: &jet_std::F64x2) -> f64 {
    v.0.iter().product()
}
fn jet_math_F64x2_min(v: &jet_std::F64x2) -> f64 {
    v.0.iter().copied().fold(f64::INFINITY, f64::min)
}
fn jet_math_F64x2_max(v: &jet_std::F64x2) -> f64 {
    v.0.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}
fn jet_math_F64x2_reduce_add(v: &jet_std::F64x2) -> f64 {
    jet_math_F64x2_sum(v)
}
fn jet_math_F64x2_reduce_mul(v: &jet_std::F64x2) -> f64 {
    jet_math_F64x2_product(v)
}
fn jet_math_F64x2_reduce_min(v: &jet_std::F64x2) -> f64 {
    jet_math_F64x2_min(v)
}
fn jet_math_F64x2_reduce_max(v: &jet_std::F64x2) -> f64 {
    jet_math_F64x2_max(v)
}
fn jet_math_F64x2_reduce_avg(v: &jet_std::F64x2) -> f64 {
    jet_math_F64x2_sum(v) / 2.0
}

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
