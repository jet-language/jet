//! D-SIMD2 / D-SIMD3 / D-LINALG1: math-value host shims for the Cranelift JIT.
//! Lane/matrix layouts match `MathTaskMem` (fixed arrays / column-major F64). Host
//! ops live here so the include fragment's `JetShow`/`Shared` deps stay out.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use crate::Marshal::{alloc_string, clone_string};
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;
use std::cell::RefCell;

mod typed_text_semantics {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/TypedText.rs");
}

mod string_concat_semantics {
    include!("../../jet-codegen/src/Prelude/Core/StringConcat.rs");
}

mod simd_lanes {
    include!("../../jet-codegen/src/Prelude/Core/SimdLanes.rs");
}

#[derive(Clone, Copy)]
struct F32Lanes {
    lanes: [f32; 8],
    len: u8,
}
#[derive(Clone, Copy)]
struct F64Lanes {
    lanes: [f64; 4],
    len: u8,
}
#[derive(Clone, Copy)]
struct IntLanes {
    lanes: [i64; 32],
    len: u8,
    signed: bool,
    bits: u8,
}
#[derive(Clone, Copy)]
struct Vec2([f64; 2]);
#[derive(Clone, Copy)]
struct Vec3([f64; 3]);
#[derive(Clone, Copy)]
struct Vec4([f64; 4]);
#[derive(Clone, Copy)]
struct Mat3([f64; 9]);
#[derive(Clone, Copy)]
struct Mat4([f64; 16]);

#[derive(Clone, Copy)]
enum MathVal {
    F32(F32Lanes),
    F64(F64Lanes),
    Int(IntLanes),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    Mat3(Mat3),
    Mat4(Mat4),
}

thread_local! {
    static MATH_VALUES: RefCell<Vec<Option<MathVal>>> = RefCell::new(Vec::new());
}

fn push_val(v: MathVal) -> i64 {
    MATH_VALUES.with(|slot| {
        let mut vals = slot.borrow_mut();
        vals.push(Some(v));
        vals.len() as i64
    })
}

fn take_val(handle: i64) -> Option<MathVal> {
    MATH_VALUES.with(|slot| {
        let idx = handle.saturating_sub(1) as usize;
        slot.borrow().get(idx).and_then(|s| s.as_ref()).copied()
    })
}

fn store_val(handle: i64, v: MathVal) {
    MATH_VALUES.with(|slot| {
        let idx = handle.saturating_sub(1) as usize;
        if let Some(entry) = slot.borrow_mut().get_mut(idx) {
            *entry = Some(v);
        }
    });
}

fn f64_bits(x: f64) -> i64 {
    x.to_bits() as i64
}

fn bits_f64(bits: i64) -> f64 {
    f64::from_bits(bits as u64)
}

fn list_f64s(list: i64) -> Vec<f64> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_float(list, i).unwrap_or(0.0));
        }
        out
    })
}

fn alloc_f64_list(vals: &[f64]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for &v in vals {
            let _ = rt.heap.list_push_float(list, v);
        }
        list
    })
}

fn list_i64s(list: i64) -> Vec<i64> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(list, i).unwrap_or(0));
        }
        out
    })
}

fn alloc_i64_list(vals: &[i64]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for &v in vals {
            let _ = rt.heap.list_push_int(list, v);
        }
        list
    })
}

fn integer_lane_info(kind: jet_foundation::Syntax::SimdLaneKind) -> Option<(bool, u8)> {
    use jet_foundation::Syntax::SimdLaneKind;
    match kind {
        SimdLaneKind::I8 => Some((true, 8)),
        SimdLaneKind::I16 => Some((true, 16)),
        SimdLaneKind::I32 => Some((true, 32)),
        SimdLaneKind::I64 => Some((true, 64)),
        SimdLaneKind::U8 => Some((false, 8)),
        SimdLaneKind::U16 => Some((false, 16)),
        SimdLaneKind::U32 => Some((false, 32)),
        SimdLaneKind::U64 => Some((false, 64)),
        SimdLaneKind::F32 | SimdLaneKind::F64 => None,
    }
}

fn simd_type_name(kind: jet_foundation::Syntax::SimdLaneKind, len: usize) -> Option<&'static str> {
    jet_foundation::Syntax::SIMD_LANE_TYPE_NAMES
        .iter()
        .copied()
        .find(|name| jet_foundation::Syntax::simd_lane_layout(name) == Some((kind, len)))
}

fn narrow_int(value: i128, signed: bool, bits: u8) -> i64 {
    if bits == 64 {
        return if signed {
            value as i64
        } else {
            value as u64 as i64
        };
    }
    let mask = (1i128 << bits) - 1;
    let value = value & mask;
    if signed && (value & (1i128 << (bits - 1))) != 0 {
        (value | !mask) as i64
    } else {
        value as i64
    }
}

fn int_lanes_of(v: MathVal) -> Option<Vec<i64>> {
    let MathVal::Int(x) = v else { return None };
    Some(x.lanes[..x.len as usize].to_vec())
}

fn lanes_of(v: MathVal) -> Vec<f64> {
    match v {
        MathVal::F32(x) => x.lanes[..x.len as usize]
            .iter()
            .map(|n| f64::from(*n))
            .collect(),
        MathVal::F64(x) => x.lanes[..x.len as usize].to_vec(),
        MathVal::Int(x) => x.lanes[..x.len as usize]
            .iter()
            .map(|n| *n as f64)
            .collect(),
        MathVal::Vec2(x) => x.0.to_vec(),
        MathVal::Vec3(x) => x.0.to_vec(),
        MathVal::Vec4(x) => x.0.to_vec(),
        MathVal::Mat3(x) => x.0.to_vec(),
        MathVal::Mat4(x) => x.0.to_vec(),
    }
}

fn from_lanes(type_name: &str, lanes: &[f64]) -> Option<MathVal> {
    if let Some((kind, len)) = jet_foundation::Syntax::simd_lane_layout(type_name) {
        if lanes.len() != len {
            return None;
        }
        match kind {
            jet_foundation::Syntax::SimdLaneKind::F32 => {
                let mut out = [0.0f32; 8];
                for (dst, src) in out.iter_mut().zip(lanes) {
                    *dst = *src as f32;
                }
                return Some(MathVal::F32(F32Lanes {
                    lanes: out,
                    len: len as u8,
                }));
            }
            jet_foundation::Syntax::SimdLaneKind::F64 => {
                let mut out = [0.0f64; 4];
                out[..len].copy_from_slice(lanes);
                return Some(MathVal::F64(F64Lanes {
                    lanes: out,
                    len: len as u8,
                }));
            }
            kind => {
                let (signed, bits) = integer_lane_info(kind)?;
                let mut out = [0i64; 32];
                for (dst, src) in out.iter_mut().zip(lanes) {
                    *dst = narrow_int(*src as i128, signed, bits);
                }
                return Some(MathVal::Int(IntLanes {
                    lanes: out,
                    len: len as u8,
                    signed,
                    bits,
                }));
            }
        }
    }
    match type_name {
        "Vec2" if lanes.len() == 2 => Some(MathVal::Vec2(Vec2([lanes[0], lanes[1]]))),
        "Vec3" if lanes.len() == 3 => Some(MathVal::Vec3(Vec3([lanes[0], lanes[1], lanes[2]]))),
        "Vec4" if lanes.len() == 4 => Some(MathVal::Vec4(Vec4([
            lanes[0], lanes[1], lanes[2], lanes[3],
        ]))),
        "Mat3" if lanes.len() == 9 => {
            let mut a = [0.0f64; 9];
            a.copy_from_slice(lanes);
            Some(MathVal::Mat3(Mat3(a)))
        }
        "Mat4" if lanes.len() == 16 => {
            let mut a = [0.0f64; 16];
            a.copy_from_slice(lanes);
            Some(MathVal::Mat4(Mat4(a)))
        }
        _ => None,
    }
}

fn from_int_lanes(type_name: &str, lanes: &[i64]) -> Option<MathVal> {
    let (kind, len) = jet_foundation::Syntax::simd_lane_layout(type_name)?;
    let (signed, bits) = integer_lane_info(kind)?;
    if lanes.len() != len {
        return None;
    }
    let mut out = [0i64; 32];
    for (dst, src) in out.iter_mut().zip(lanes) {
        *dst = narrow_int(i128::from(*src), signed, bits);
    }
    Some(MathVal::Int(IntLanes {
        lanes: out,
        len: len as u8,
        signed,
        bits,
    }))
}

fn type_name_of(v: MathVal) -> &'static str {
    match v {
        MathVal::F32(x) => simd_type_name(jet_foundation::Syntax::SimdLaneKind::F32, x.len as usize)
            .expect("known F32 lane layout"),
        MathVal::F64(x) => simd_type_name(jet_foundation::Syntax::SimdLaneKind::F64, x.len as usize)
            .expect("known F64 lane layout"),
        MathVal::Int(x) => {
            let kind = match (x.signed, x.bits) {
                (true, 8) => jet_foundation::Syntax::SimdLaneKind::I8,
                (true, 16) => jet_foundation::Syntax::SimdLaneKind::I16,
                (true, 32) => jet_foundation::Syntax::SimdLaneKind::I32,
                (true, 64) => jet_foundation::Syntax::SimdLaneKind::I64,
                (false, 8) => jet_foundation::Syntax::SimdLaneKind::U8,
                (false, 16) => jet_foundation::Syntax::SimdLaneKind::U16,
                (false, 32) => jet_foundation::Syntax::SimdLaneKind::U32,
                (false, 64) => jet_foundation::Syntax::SimdLaneKind::U64,
                _ => unreachable!("known integer lane layout"),
            };
            simd_type_name(kind, x.len as usize).expect("known integer lane layout")
        }
        MathVal::Vec2(_) => "Vec2",
        MathVal::Vec3(_) => "Vec3",
        MathVal::Vec4(_) => "Vec4",
        MathVal::Mat3(_) => "Mat3",
        MathVal::Mat4(_) => "Mat4",
    }
}

/// Pack a scalar float with a negative tag; math handles remain non-negative.
fn pack_float(x: f64) -> i64 {
    (1i64 << 63) | (f64_bits(x) & !(1i64 << 63))
}

/// Integer reductions and lane reads already have an unboxed I64 carrier in
/// Cranelift, so they need no tag. The caller selects this unpacker from the
/// sema-proven return type.
fn pack_int(x: i64) -> i64 {
    x
}

fn pack_handle(h: i64) -> i64 {
    h & !(1i64 << 63)
}

fn is_float_pack(p: i64) -> bool {
    p < 0
}

fn unpack_float(p: i64) -> f64 {
    bits_f64(p & !(1i64 << 63))
}

fn unpack_int(p: i64) -> i64 {
    p
}

fn unpack_handle(p: i64) -> i64 {
    p & !(1i64 << 63)
}

fn simd_binary_op(op: &str) -> Option<simd_lanes::JetSimdBinaryOp> {
    Some(match op {
        "add" => simd_lanes::JetSimdBinaryOp::Add,
        "sub" => simd_lanes::JetSimdBinaryOp::Sub,
        "mul" => simd_lanes::JetSimdBinaryOp::Mul,
        "div" => simd_lanes::JetSimdBinaryOp::Div,
        _ => return None,
    })
}

fn simd_reduce_op(op: &str) -> Option<simd_lanes::JetSimdReduceOp> {
    Some(match op {
        "Add" | "sum" => simd_lanes::JetSimdReduceOp::Add,
        "Mul" | "product" => simd_lanes::JetSimdReduceOp::Mul,
        "Min" => simd_lanes::JetSimdReduceOp::Min,
        "Max" => simd_lanes::JetSimdReduceOp::Max,
        "Avg" => simd_lanes::JetSimdReduceOp::Avg,
        _ => return None,
    })
}

fn zip_binop(op: &str, a: &[f64], b: &[f64], f32_lanes: bool) -> Option<Vec<f64>> {
    let op = simd_binary_op(op)?;
    if f32_lanes {
        let left = a.iter().map(|value| *value as f32).collect::<Vec<_>>();
        let right = b.iter().map(|value| *value as f32).collect::<Vec<_>>();
        return simd_lanes::jet_simd_f32_binary_slice(&left, &right, op)
            .map(|values| values.into_iter().map(f64::from).collect());
    }
    simd_lanes::jet_simd_f64_binary_slice(a, b, op)
}

fn binary_op_name(op: simd_lanes::JetSimdBinaryOp) -> &'static str {
    match op {
        simd_lanes::JetSimdBinaryOp::Add => "add",
        simd_lanes::JetSimdBinaryOp::Sub => "sub",
        simd_lanes::JetSimdBinaryOp::Mul => "mul",
        simd_lanes::JetSimdBinaryOp::Div => "div",
    }
}

fn f32_lanes_binary(
    left: F32Lanes,
    right: F32Lanes,
    op: simd_lanes::JetSimdBinaryOp,
) -> Option<MathVal> {
    if left.len != right.len {
        return None;
    }
    let mut out = left.lanes;
    match left.len {
        4 => {
            let left = [left.lanes[0], left.lanes[1], left.lanes[2], left.lanes[3]];
            let right = [
                right.lanes[0],
                right.lanes[1],
                right.lanes[2],
                right.lanes[3],
            ];
            let value = match op {
                simd_lanes::JetSimdBinaryOp::Add => {
                    simd_lanes::jet_simd_f32x4_add_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Sub => {
                    simd_lanes::jet_simd_f32x4_sub_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Mul => {
                    simd_lanes::jet_simd_f32x4_mul_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Div => {
                    simd_lanes::jet_simd_f32x4_div_array(&left, &right)
                }
            };
            out[..4].copy_from_slice(&value);
        }
        8 => {
            let left = left.lanes;
            let right = right.lanes;
            let value = match op {
                simd_lanes::JetSimdBinaryOp::Add => {
                    simd_lanes::jet_simd_f32x8_add_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Sub => {
                    simd_lanes::jet_simd_f32x8_sub_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Mul => {
                    simd_lanes::jet_simd_f32x8_mul_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Div => {
                    simd_lanes::jet_simd_f32x8_div_array(&left, &right)
                }
            };
            out.copy_from_slice(&value);
        }
        _ => return None,
    }
    Some(MathVal::F32(F32Lanes { lanes: out, len: left.len }))
}

fn f64_lanes_binary(
    left: F64Lanes,
    right: F64Lanes,
    op: simd_lanes::JetSimdBinaryOp,
) -> Option<MathVal> {
    if left.len != right.len {
        return None;
    }
    let mut out = left.lanes;
    match left.len {
        2 => {
            let left = [left.lanes[0], left.lanes[1]];
            let right = [right.lanes[0], right.lanes[1]];
            let value = match op {
                simd_lanes::JetSimdBinaryOp::Add => {
                    simd_lanes::jet_simd_f64x2_add_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Sub => {
                    simd_lanes::jet_simd_f64x2_sub_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Mul => {
                    simd_lanes::jet_simd_f64x2_mul_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Div => {
                    simd_lanes::jet_simd_f64x2_div_array(&left, &right)
                }
            };
            out[..2].copy_from_slice(&value);
        }
        4 => {
            let left = left.lanes;
            let right = right.lanes;
            let value = match op {
                simd_lanes::JetSimdBinaryOp::Add => {
                    simd_lanes::jet_simd_f64x4_add_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Sub => {
                    simd_lanes::jet_simd_f64x4_sub_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Mul => {
                    simd_lanes::jet_simd_f64x4_mul_array(&left, &right)
                }
                simd_lanes::JetSimdBinaryOp::Div => {
                    simd_lanes::jet_simd_f64x4_div_array(&left, &right)
                }
            };
            out.copy_from_slice(&value);
        }
        _ => return None,
    }
    Some(MathVal::F64(F64Lanes { lanes: out, len: left.len }))
}

fn math_binary_value(
    left: MathVal,
    right: MathVal,
    op: simd_lanes::JetSimdBinaryOp,
) -> Option<MathVal> {
    match (left, right) {
        (MathVal::F32(left), MathVal::F32(right)) => f32_lanes_binary(left, right, op),
        (MathVal::F64(left), MathVal::F64(right)) => f64_lanes_binary(left, right, op),
        (MathVal::Int(left), MathVal::Int(right))
            if left.len == right.len
                && left.signed == right.signed
                && left.bits == right.bits =>
        {
            let name = type_name_of(MathVal::Int(left));
            let result = zip_int_binop(
                binary_op_name(op),
                &left.lanes[..left.len as usize],
                &right.lanes[..right.len as usize],
                left.signed,
                left.bits,
            )?;
            from_int_lanes(name, &result)
        }
        (MathVal::Mat3(matrix), MathVal::Vec3(vector))
            if op == simd_lanes::JetSimdBinaryOp::Mul => {
            let out = mat_vec(3, &matrix.0, &vector.0);
            from_lanes("Vec3", &out)
        }
        (MathVal::Mat4(matrix), MathVal::Vec4(vector))
            if op == simd_lanes::JetSimdBinaryOp::Mul => {
            let out = mat_vec(4, &matrix.0, &vector.0);
            from_lanes("Vec4", &out)
        }
        (left, right) => {
            let name = type_name_of(left);
            let left = lanes_of(left);
            let right = lanes_of(right);
            let result = simd_lanes::jet_simd_f64_binary_slice(&left, &right, op)?;
            from_lanes(name, &result)
        }
    }
}

fn simd_kind_code(kind: jet_foundation::Syntax::SimdLaneKind) -> i64 {
    use jet_foundation::Syntax::SimdLaneKind;
    match kind {
        SimdLaneKind::F32 => 0,
        SimdLaneKind::F64 => 1,
        SimdLaneKind::I8 => 2,
        SimdLaneKind::I16 => 3,
        SimdLaneKind::I32 => 4,
        SimdLaneKind::I64 => 5,
        SimdLaneKind::U8 => 6,
        SimdLaneKind::U16 => 7,
        SimdLaneKind::U32 => 8,
        SimdLaneKind::U64 => 9,
    }
}

fn simd_kind_from_code(code: i64) -> Option<jet_foundation::Syntax::SimdLaneKind> {
    use jet_foundation::Syntax::SimdLaneKind;
    Some(match code {
        0 => SimdLaneKind::F32,
        1 => SimdLaneKind::F64,
        2 => SimdLaneKind::I8,
        3 => SimdLaneKind::I16,
        4 => SimdLaneKind::I32,
        5 => SimdLaneKind::I64,
        6 => SimdLaneKind::U8,
        7 => SimdLaneKind::U16,
        8 => SimdLaneKind::U32,
        9 => SimdLaneKind::U64,
        _ => return None,
    })
}

pub(crate) fn simd_lane_type_code(type_name: &str) -> Option<(i64, usize)> {
    let (kind, len) = jet_foundation::Syntax::simd_lane_layout(type_name)?;
    Some((simd_kind_code(kind), len))
}

pub(crate) fn simd_reduce_op_code(op: &str) -> Option<i64> {
    Some(match op {
        "Add" | "sum" => 0,
        "Mul" | "product" => 1,
        "Min" => 2,
        "Max" => 3,
        "Avg" => 4,
        _ => return None,
    })
}

fn simd_reduce_op_from_code(code: i64) -> Option<simd_lanes::JetSimdReduceOp> {
    Some(match code {
        0 => simd_lanes::JetSimdReduceOp::Add,
        1 => simd_lanes::JetSimdReduceOp::Mul,
        2 => simd_lanes::JetSimdReduceOp::Min,
        3 => simd_lanes::JetSimdReduceOp::Max,
        4 => simd_lanes::JetSimdReduceOp::Avg,
        _ => return None,
    })
}

fn jet_jit_math_binary(left: i64, right: i64, op: i64) -> i64 {
    let Some(op) = simd_binary_op(match op {
        0 => "add",
        1 => "sub",
        2 => "mul",
        3 => "div",
        _ => {
            trap("math binary: bad operator");
            return 0;
        }
    }) else {
        trap("math binary: bad operator");
        return 0;
    };
    let Some(left) = take_val(left) else {
        trap("math binary: bad left");
        return 0;
    };
    let Some(right) = take_val(right) else {
        trap("math binary: bad right");
        return 0;
    };
    let Some(value) = math_binary_value(left, right, op) else {
        trap("math binary size mismatch or division by zero");
        return 0;
    };
    pack_handle(push_val(value))
}

fn jet_jit_math_splat(value: i64, kind_code: i64, len: i64) -> i64 {
    let Some(kind) = simd_kind_from_code(kind_code) else {
        trap("math splat: bad lane kind");
        return 0;
    };
    if !(1..=32).contains(&len) {
        trap("math splat: bad lane count");
        return 0;
    }
    let len = len as usize;
    let Some(type_name) = simd_type_name(kind, len) else {
        trap("math splat: unsupported lane layout");
        return 0;
    };
    let value = match kind {
        jet_foundation::Syntax::SimdLaneKind::F32 => match len {
            4 => {
                let value = simd_lanes::jet_simd_f32x4_splat_array(bits_f64(value) as f32);
                let mut lanes = [0.0f32; 8];
                lanes[..4].copy_from_slice(&value);
                Some(MathVal::F32(F32Lanes { lanes, len: 4 }))
            }
            8 => Some(MathVal::F32(F32Lanes {
                lanes: simd_lanes::jet_simd_f32x8_splat_array(bits_f64(value) as f32),
                len: 8,
            })),
            _ => None,
        },
        jet_foundation::Syntax::SimdLaneKind::F64 => match len {
            2 => {
                let value = simd_lanes::jet_simd_f64x2_splat_array(bits_f64(value));
                let mut lanes = [0.0f64; 4];
                lanes[..2].copy_from_slice(&value);
                Some(MathVal::F64(F64Lanes { lanes, len: 2 }))
            }
            4 => Some(MathVal::F64(F64Lanes {
                lanes: simd_lanes::jet_simd_f64x4_splat_array(bits_f64(value)),
                len: 4,
            })),
            _ => None,
        },
        _ => {
            let lanes = simd_lanes::jet_simd_splat_slice(value, len);
            from_int_lanes(type_name, &lanes)
        }
    };
    let Some(value) = value else {
        trap("math splat: unsupported lane value");
        return 0;
    };
    pack_handle(push_val(value))
}

fn jet_jit_math_reduce(value: i64, op: i64) -> i64 {
    let Some(op) = simd_reduce_op_from_code(op) else {
        trap("math reduce: bad operator");
        return 0;
    };
    let Some(value) = take_val(value) else {
        trap("math reduce: bad receiver");
        return 0;
    };
    match value {
        MathVal::F32(value) => simd_lanes::jet_simd_reduce_slice(
            &value.lanes[..value.len as usize],
            op,
        )
        .map(|value| pack_float(f64::from(value)))
        .unwrap_or_else(|| {
            trap("math reduce: empty lanes");
            0
        }),
        MathVal::F64(value) => simd_lanes::jet_simd_reduce_slice(
            &value.lanes[..value.len as usize],
            op,
        )
        .map(pack_float)
        .unwrap_or_else(|| {
            trap("math reduce: empty lanes");
            0
        }),
        MathVal::Int(value) => reduce_int_op(
            &value.lanes[..value.len as usize],
            match op {
                simd_lanes::JetSimdReduceOp::Add => "Add",
                simd_lanes::JetSimdReduceOp::Mul => "Mul",
                simd_lanes::JetSimdReduceOp::Min => "Min",
                simd_lanes::JetSimdReduceOp::Max => "Max",
                simd_lanes::JetSimdReduceOp::Avg => "Avg",
            },
            value.signed,
            value.bits,
        )
        .map(pack_int)
        .unwrap_or_else(|| {
            trap("math reduce: invalid integer lanes");
            0
        }),
        value => simd_lanes::jet_simd_reduce_slice(&lanes_of(value), op)
            .map(pack_float)
            .unwrap_or_else(|| {
                trap("math reduce: empty lanes");
                0
            }),
    }
}

fn jet_jit_math_dot(left: i64, right: i64) -> i64 {
    let Some(left) = take_val(left) else {
        trap("dot: bad receiver");
        return 0;
    };
    let Some(right) = take_val(right) else {
        trap("dot: bad argument");
        return 0;
    };
    let left = lanes_of(left);
    let right = lanes_of(right);
    let Some(value) = simd_lanes::jet_simd_dot_f64_slice(&left, &right) else {
        trap("dot size mismatch");
        return 0;
    };
    pack_float(value)
}

fn jet_jit_math_length(value: i64) -> i64 {
    let Some(value) = take_val(value) else {
        trap("length: bad receiver");
        return 0;
    };
    let lanes = lanes_of(value);
    let Some(value) = simd_lanes::jet_simd_length_f64_slice(&lanes) else {
        trap("length: empty lanes");
        return 0;
    };
    pack_float(value)
}

fn zip_int_binop(
    op: &str,
    a: &[i64],
    b: &[i64],
    signed: bool,
    bits: u8,
) -> Option<Vec<i64>> {
    simd_lanes::jet_simd_integer_binary(a, b, simd_binary_op(op)?, signed, bits)
}

fn mat_mul(n: usize, a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut r = vec![0.0f64; n * n];
    for c in 0..n {
        for row in 0..n {
            let mut acc = 0.0f64;
            for k in 0..n {
                acc += a[k * n + row] * b[c * n + k];
            }
            r[c * n + row] = acc;
        }
    }
    r
}

fn mat_vec(n: usize, m: &[f64], v: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0f64; n];
    for row in 0..n {
        let mut acc = 0.0f64;
        for c in 0..n {
            acc += m[c * n + row] * v[c];
        }
        out[row] = acc;
    }
    out
}

fn reduce_op(lanes: &[f64], op: &str, f32_lanes: bool) -> Option<f64> {
    let op = simd_reduce_op(op)?;
    if f32_lanes {
        let lanes = lanes
            .iter()
            .map(|value| *value as f32)
            .collect::<Vec<_>>();
        return simd_lanes::jet_simd_reduce_slice(&lanes, op).map(f64::from);
    }
    simd_lanes::jet_simd_reduce_slice(lanes, op)
}

fn reduce_int_op(lanes: &[i64], op: &str, signed: bool, bits: u8) -> Option<i64> {
    simd_lanes::jet_simd_integer_reduce(lanes, simd_reduce_op(op)?, signed, bits)
}

fn trap(msg: &str) {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_trap(msg);
    });
}

/// `type_name`/`func` are string handles. `args` is a list of i64:
/// - for scalar float args: f64 bits
/// - for math-value args: math handles
/// - for array args (`from_array`): list handle of f64 or integer values
/// Returns a packed float, integer, or math handle.
fn jet_jit_math_call(type_name: i64, func: i64, args: i64) -> i64 {
    let ty = clone_string(type_name);
    let func = clone_string(func);
    let argv = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(args).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(args, i).unwrap_or(0));
        }
        out
    });

    let simd_layout = jet_foundation::Syntax::simd_lane_layout(&ty);
    let f32_lanes = simd_layout
        .is_some_and(|(kind, _)| kind == jet_foundation::Syntax::SimdLaneKind::F32);
    let int_layout = simd_layout.and_then(|(kind, _)| integer_lane_info(kind));
    let result = match (ty.as_str(), func.as_str()) {
        (_, "new") => {
            if int_layout.is_some() {
                from_int_lanes(&ty, &argv).map(|v| pack_handle(push_val(v)))
            } else {
                let lanes: Vec<f64> = argv.iter().map(|b| bits_f64(*b)).collect();
                from_lanes(&ty, &lanes).map(|v| pack_handle(push_val(v)))
            }
        }
        (_, "splat") if argv.len() == 1 => {
            let n = simd_layout.map_or_else(
                || match ty.as_str() {
                    "Vec2" => 2,
                    "Vec3" => 3,
                    "Vec4" => 4,
                    _ => 0,
                },
                |(_, n)| n,
            );
            if int_layout.is_some() {
                let lanes = simd_lanes::jet_simd_splat_slice(argv[0], n);
                from_int_lanes(&ty, &lanes).map(|v| pack_handle(push_val(v)))
            } else {
                let v = bits_f64(argv[0]);
                let lanes = if f32_lanes {
                    simd_lanes::jet_simd_splat_slice(v as f32, n)
                        .into_iter()
                        .map(f64::from)
                        .collect()
                } else {
                    simd_lanes::jet_simd_splat_slice(v, n)
                };
                from_lanes(&ty, &lanes).map(|v| pack_handle(push_val(v)))
            }
        }
        (_, "from_array") if argv.len() == 1 => {
            if int_layout.is_some() {
                from_int_lanes(&ty, &list_i64s(argv[0])).map(|v| pack_handle(push_val(v)))
            } else {
                let lanes = list_f64s(argv[0]);
                let lanes = if f32_lanes {
                    lanes.into_iter().map(|v| (v as f32) as f64).collect()
                } else {
                    lanes
                };
                from_lanes(&ty, &lanes).map(|v| pack_handle(push_val(v)))
            }
        }
        (_, "add" | "sub" | "mul" | "div") if argv.len() == 2 => {
            let Some(a) = take_val(argv[0]) else {
                trap("math binary: bad left");
                return 0;
            };
            let Some(b) = take_val(argv[1]) else {
                trap("math binary: bad right");
                return 0;
            };
            let Some(op) = simd_binary_op(&func) else {
                trap("math binary: bad operator");
                return 0;
            };
            math_binary_value(a, b, op)
                .map(|value| pack_handle(push_val(value)))
                .or_else(|| {
                    trap("math binary size mismatch or division by zero");
                    None
                })
        }
        (_, "to_array") if argv.len() == 1 => {
            let Some(v) = take_val(argv[0]) else {
                trap("to_array: bad recv");
                return 0;
            };
            if matches!(v, MathVal::Int(_)) {
                Some(pack_handle(alloc_i64_list(&int_lanes_of(v).unwrap())))
            } else {
                Some(pack_handle(alloc_f64_list(&lanes_of(v))))
            }
        }
        (_, "sum" | "product" | "min" | "max" | "length") if argv.len() == 1 => {
            let Some(v) = take_val(argv[0]) else {
                trap("math unary: bad recv");
                return 0;
            };
            if func == "length" {
                let lanes = lanes_of(v);
                return simd_lanes::jet_simd_length_f64_slice(&lanes)
                    .map(pack_float)
                    .unwrap_or_else(|| {
                        trap("length: empty lanes");
                        0
                    });
            }
            if let Some((signed, bits)) = int_layout {
                let Some(lanes) = int_lanes_of(v) else {
                    trap("integer reduction failed");
                    return 0;
                };
                let Some(n) = reduce_int_op(&lanes, &func, signed, bits) else {
                    trap("integer reduction failed");
                    return 0;
                };
                Some(pack_int(n))
            } else {
                let n = reduce_op(&lanes_of(v), &func, f32_lanes).unwrap_or(0.0);
                Some(pack_float(n))
            }
        }
        (_, "normalize") if argv.len() == 1 => {
            let Some(v) = take_val(argv[0]) else {
                trap("normalize: bad recv");
                return 0;
            };
            let name = type_name_of(v);
            let vals = lanes_of(v);
            let len: f64 = vals.iter().map(|n| n * n).sum::<f64>().sqrt();
            let out = if len == 0.0 {
                vals
            } else {
                vals.iter().map(|n| n / len).collect()
            };
            from_lanes(name, &out).map(|v| pack_handle(push_val(v)))
        }
        (_, "dot") if argv.len() == 2 => {
            let Some(a) = take_val(argv[0]) else {
                trap("dot: bad recv");
                return 0;
            };
            let Some(b) = take_val(argv[1]) else {
                trap("dot: bad arg");
                return 0;
            };
            let la = lanes_of(a);
            let lb = lanes_of(b);
            let Some(value) = simd_lanes::jet_simd_dot_f64_slice(&la, &lb) else {
                trap("dot size mismatch");
                return 0;
            };
            Some(pack_float(value))
        }
        (_, "cross") if argv.len() == 2 => {
            let Some(a) = take_val(argv[0]) else {
                trap("cross: bad recv");
                return 0;
            };
            let Some(b) = take_val(argv[1]) else {
                trap("cross: bad arg");
                return 0;
            };
            let la = lanes_of(a);
            let lb = lanes_of(b);
            if la.len() != 3 || lb.len() != 3 {
                trap("cross needs Vec3");
                return 0;
            }
            let out = [
                la[1] * lb[2] - la[2] * lb[1],
                la[2] * lb[0] - la[0] * lb[2],
                la[0] * lb[1] - la[1] * lb[0],
            ];
            from_lanes("Vec3", &out).map(|v| pack_handle(push_val(v)))
        }
        (_, "matmul") if argv.len() == 2 => {
            let Some(a) = take_val(argv[0]) else {
                trap("matmul: bad recv");
                return 0;
            };
            let Some(b) = take_val(argv[1]) else {
                trap("matmul: bad arg");
                return 0;
            };
            let name = type_name_of(a);
            let n = match name {
                "Mat3" => 3,
                "Mat4" => 4,
                _ => {
                    trap("matmul on a matrix");
                    return 0;
                }
            };
            let out = mat_mul(n, &lanes_of(a), &lanes_of(b));
            from_lanes(name, &out).map(|v| pack_handle(push_val(v)))
        }
        (_, "reduce") if argv.len() == 2 => {
            let Some(v) = take_val(argv[0]) else {
                trap("reduce: bad recv");
                return 0;
            };
            let op = clone_string(argv[1]);
            if let Some((signed, bits)) = int_layout {
                let Some(n) = reduce_int_op(&int_lanes_of(v).unwrap(), &op, signed, bits) else {
                    trap(&format!("reduce({op})"));
                    return 0;
                };
                Some(pack_int(n))
            } else {
                let Some(n) = reduce_op(&lanes_of(v), &op, f32_lanes) else {
                    trap(&format!("reduce({op})"));
                    return 0;
                };
                Some(pack_float(n))
            }
        }
        (_, "lane") if argv.len() == 2 => {
            let Some(v) = take_val(argv[0]) else {
                trap("lane: bad recv");
                return 0;
            };
            let idx = argv[1];
            let lanes = lanes_of(v);
            let idx = match simd_lanes::jet_simd_lane_index(idx, type_name_of(v), lanes.len()) {
                Ok(index) => index,
                Err(message) => {
                    trap(&message);
                    return 0;
                }
            };
            if int_layout.is_some() {
                Some(pack_int(int_lanes_of(v).unwrap()[idx]))
            } else {
                Some(pack_float(lanes[idx]))
            }
        }
        (_, "swizzle_read") => {
            // args: recv, then lane indices as i64
            if argv.is_empty() {
                trap("swizzle_read: missing recv");
                return 0;
            }
            let Some(v) = take_val(argv[0]) else {
                trap("swizzle_read: bad recv");
                return 0;
            };
            let src = lanes_of(v);
            let mut out = Vec::with_capacity(argv.len() - 1);
            for &lane in &argv[1..] {
                if lane < 0 || lane as usize >= src.len() {
                    trap("swizzle lane out of range");
                    return 0;
                }
                let mut n = src[lane as usize];
                if ty == "F32x4" {
                    n = (n as f32) as f64;
                }
                out.push(n);
            }
            if out.len() == 1 {
                Some(pack_float(out[0]))
            } else {
                let result_ty = match out.len() {
                    2 if ty == "F32x4" || ty == "F64x2" => ty.as_str(),
                    2 => "Vec2",
                    3 => "Vec3",
                    4 if ty == "F32x4" => "F32x4",
                    4 => "Vec4",
                    _ => {
                        trap("swizzle lane count");
                        return 0;
                    }
                };
                // Same-type full permute on F32x4/F64x2 keeps type; else VecN.
                let result_ty =
                    if (ty == "F32x4" && out.len() == 4) || (ty == "F64x2" && out.len() == 2) {
                        ty.as_str()
                    } else if out.len() == 1 {
                        unreachable!()
                    } else {
                        match out.len() {
                            2 => "Vec2",
                            3 => "Vec3",
                            4 => "Vec4",
                            _ => result_ty,
                        }
                    };
                from_lanes(result_ty, &out).map(|v| pack_handle(push_val(v)))
            }
        }
        (_, "swizzle_assign") => {
            // args: base, value (scalar bits or math handle), then lane indices
            if argv.len() < 3 {
                trap("swizzle_assign arity");
                return 0;
            }
            let Some(mut base) = take_val(argv[0]) else {
                trap("swizzle_assign: bad base");
                return 0;
            };
            let lanes_idx = &argv[2..];
            let mut cur = lanes_of(base);
            if lanes_idx.len() == 1 {
                let lane = lanes_idx[0] as usize;
                let val = bits_f64(argv[1]);
                if lane >= cur.len() {
                    trap("swizzle assign lane out of range");
                    return 0;
                }
                cur[lane] = if matches!(base, MathVal::F32(F32Lanes { len: 4, .. })) {
                    (val as f32) as f64
                } else {
                    val
                };
            } else {
                let Some(rhs) = take_val(argv[1]) else {
                    trap("swizzle_assign: bad value");
                    return 0;
                };
                let rhs_lanes = lanes_of(rhs);
                if rhs_lanes.len() != lanes_idx.len() {
                    trap("swizzle_assign size mismatch");
                    return 0;
                }
                for (i, &lane) in lanes_idx.iter().enumerate() {
                    let lane = lane as usize;
                    if lane >= cur.len() {
                        trap("swizzle assign lane out of range");
                        return 0;
                    }
                    let mut n = rhs_lanes[i];
                    if matches!(base, MathVal::F32(F32Lanes { len: 4, .. })) {
                        n = (n as f32) as f64;
                    }
                    cur[lane] = n;
                }
            }
            let name = type_name_of(base);
            base = from_lanes(name, &cur).unwrap();
            let handle = argv[0];
            store_val(handle, base);
            Some(pack_handle(handle))
        }
        _ => {
            trap(&format!("jit math unsupported: {ty}.{func}"));
            None
        }
    };
    result.unwrap_or(0)
}

fn jet_jit_math_result_is_float(packed: i64) -> i8 {
    i8::from(is_float_pack(packed))
}

fn jet_jit_math_result_float(packed: i64) -> f64 {
    unpack_float(packed)
}

fn jet_jit_math_result_int(packed: i64) -> i64 {
    unpack_int(packed)
}

fn jet_jit_math_result_handle(packed: i64) -> i64 {
    unpack_handle(packed)
}

fn clone_string_list(list: i64) -> Option<Vec<String>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list)?;
        (0..len)
            .map(|index| rt.heap.list_get_string(list, index))
            .collect()
    })
}

fn require_string_list(list: i64) -> Option<Vec<String>> {
    let values = clone_string_list(list);
    if values.is_none() {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("typed-text list contains a non-string value")
        });
    }
    values
}

/// D-BOUND-HEAD1=A: these are marshalling adapters only. Encoding and hole
/// policy live in the same Prelude functions emitted by AOT and used by the
/// interpreter.
fn typed_path_interpolate(literals: i64, holes: i64) -> Option<String> {
    let literals = require_string_list(literals)?;
    let holes = require_string_list(holes)?;
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    Some(typed_text_semantics::jet_typed_path_interpolate(
        &literal_refs,
        &holes,
    ))
}

fn typed_datetime_interpolate(literals: i64, holes: i64) -> Option<String> {
    let literals = require_string_list(literals)?;
    let holes = require_string_list(holes)?;
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    Some(typed_text_semantics::jet_typed_datetime_interpolate(
        &literal_refs,
        &holes,
    ))
}

fn jet_jit_typed_path_interpolate(literals: i64, holes: i64) -> i64 {
    typed_path_interpolate(literals, holes)
        .map(alloc_string)
        .unwrap_or(0)
}

fn jet_jit_typed_datetime_interpolate(literals: i64, holes: i64) -> i64 {
    typed_datetime_interpolate(literals, holes)
        .map(alloc_string)
        .unwrap_or(0)
}

fn alloc_string_list(values: Vec<String>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for value in values {
            let string = rt.heap.alloc_string(value);
            let _ = rt.heap.list_push_int(list, string);
        }
        list
    })
}

fn alloc_sql_value(value: (String, Vec<String>)) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(2);
        let template = rt.heap.alloc_string(value.0);
        let params = rt.heap.alloc_empty_list();
        for param in value.1 {
            let value = rt.heap.alloc_string(param);
            let _ = rt.heap.list_push_int(params, value);
        }
        let _ = rt.heap.record_set_string(record, 0, template);
        let _ = rt.heap.record_set_int(record, 1, params);
        record
    })
}

fn clone_sql_value(value: i64) -> Option<(String, Vec<String>)> {
    Concurrency::with_runtime_mut(|rt| {
        let template_handle = rt.heap.record_get_string(value, 0)?;
        let template = rt.heap.clone_string(template_handle)?;
        let params_handle = rt.heap.record_get_int(value, 1)?;
        let len = rt.heap.list_len(params_handle)?;
        let mut params = Vec::with_capacity(len as usize);
        for index in 0..len {
            params.push(rt.heap.list_get_string(params_handle, index)?);
        }
        Some((template, params))
    })
}

fn jet_jit_typed_sql_raw(s: i64) -> i64 {
    alloc_sql_value(typed_text_semantics::jet_typed_sql_raw(clone_string(s)))
}

fn jet_jit_typed_sql_interpolate(literals: i64, holes: i64) -> i64 {
    let Some(literals) = require_string_list(literals) else {
        return 0;
    };
    let Some(holes) = require_string_list(holes) else {
        return 0;
    };
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    alloc_sql_value(typed_text_semantics::jet_typed_sql_interpolate(
        &literal_refs,
        holes,
    ))
}

fn jet_jit_typed_sql_template(value: i64) -> i64 {
    let Some(value) = clone_sql_value(value) else {
        trap("typed-text SQL value is malformed");
        return 0;
    };
    alloc_string(typed_text_semantics::jet_typed_sql_template(&value))
}

fn jet_jit_typed_sql_params(value: i64) -> i64 {
    let Some(value) = clone_sql_value(value) else {
        trap("typed-text SQL value is malformed");
        return 0;
    };
    alloc_string_list(typed_text_semantics::jet_typed_sql_params(&value))
}

fn jet_jit_typed_sh_raw(s: i64) -> i64 {
    alloc_string_list(typed_text_semantics::jet_typed_sh_raw(clone_string(s)))
}

fn jet_jit_typed_sh_interpolate(literals: i64, holes: i64) -> i64 {
    let Some(literals) = require_string_list(literals) else {
        return 0;
    };
    let Some(holes) = require_string_list(holes) else {
        return 0;
    };
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    alloc_string_list(typed_text_semantics::jet_typed_sh_interpolate(
        &literal_refs,
        holes,
    ))
}

fn jet_jit_typed_html_interpolate(literals: i64, holes: i64) -> i64 {
    let Some(literals) = require_string_list(literals) else {
        return 0;
    };
    let Some(holes) = require_string_list(holes) else {
        return 0;
    };
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    alloc_string(typed_text_semantics::jet_typed_html_interpolate(
        &literal_refs,
        holes,
    ))
}

fn jet_jit_typed_html_raw(value: i64) -> i64 {
    alloc_string(typed_text_semantics::jet_typed_html_raw(clone_string(
        value,
    )))
}

fn jet_jit_typed_html_text(value: i64) -> i64 {
    alloc_string(typed_text_semantics::jet_typed_html_text(clone_string(
        value,
    )))
}

fn jet_jit_html_escape(s: i64) -> i64 {
    alloc_string(typed_text_semantics::jet_typed_html_escape(&clone_string(
        s,
    )))
}

fn jet_jit_str_concat(a: i64, b: i64) -> i64 {
    let left = clone_string(a);
    let right = clone_string(b);
    alloc_string(string_concat_semantics::jet_string_concat(&left, &right))
}

pub(crate) fn clear_math_values() {
    MATH_VALUES.with(|slot| slot.borrow_mut().clear());
}

host_fns! {
    struct MathHostFns;
    register: register_math_host_symbols;
    declare: declare_math_host_fns(module) {
        let cc = module.target_config().default_call_conv;

        let mut sig_call = Signature::new(cc);
        sig_call.params.push(AbiParam::new(types::I64));
        sig_call.params.push(AbiParam::new(types::I64));
        sig_call.params.push(AbiParam::new(types::I64));
        sig_call.returns.push(AbiParam::new(types::I64));
        let mut sig_i64_i8 = Signature::new(cc);
        sig_i64_i8.params.push(AbiParam::new(types::I64));
        sig_i64_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_i64_f64 = Signature::new(cc);
        sig_i64_f64.params.push(AbiParam::new(types::I64));
        sig_i64_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_unary = Signature::new(cc);
        sig_unary.params.push(AbiParam::new(types::I64));
        sig_unary.returns.push(AbiParam::new(types::I64));
        let mut sig_binary = Signature::new(cc);
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.returns.push(AbiParam::new(types::I64));

    }
    call: "jet_jit_math_call" => jet_jit_math_call: sig_call;
    binary: "jet_jit_math_binary" => jet_jit_math_binary: sig_call;
    splat: "jet_jit_math_splat" => jet_jit_math_splat: sig_call;
    reduce: "jet_jit_math_reduce" => jet_jit_math_reduce: sig_binary;
    dot: "jet_jit_math_dot" => jet_jit_math_dot: sig_binary;
    length: "jet_jit_math_length" => jet_jit_math_length: sig_unary;
    result_is_float: "jet_jit_math_result_is_float" => jet_jit_math_result_is_float: sig_i64_i8;
    result_float: "jet_jit_math_result_float" => jet_jit_math_result_float: sig_i64_f64;
    result_int: "jet_jit_math_result_int" => jet_jit_math_result_int: sig_unary;
    result_handle: "jet_jit_math_result_handle" => jet_jit_math_result_handle: sig_unary;
    html_escape: "jet_jit_html_escape" => jet_jit_html_escape: sig_unary;
    str_concat: "jet_jit_str_concat" => jet_jit_str_concat: sig_binary;
    typed_sql_raw: "jet_jit_typed_sql_raw" => jet_jit_typed_sql_raw: sig_unary;
    typed_sql_interp: "jet_jit_typed_sql_interpolate" => jet_jit_typed_sql_interpolate: sig_binary;
    typed_sql_template: "jet_jit_typed_sql_template" => jet_jit_typed_sql_template: sig_unary;
    typed_sql_params: "jet_jit_typed_sql_params" => jet_jit_typed_sql_params: sig_unary;
    typed_sh_raw: "jet_jit_typed_sh_raw" => jet_jit_typed_sh_raw: sig_unary;
    typed_sh_interp: "jet_jit_typed_sh_interpolate" => jet_jit_typed_sh_interpolate: sig_binary;
    typed_html_raw: "jet_jit_typed_html_raw" => jet_jit_typed_html_raw: sig_unary;
    typed_html_text: "jet_jit_typed_html_text" => jet_jit_typed_html_text: sig_unary;
    typed_html_interp: "jet_jit_typed_html_interpolate" => jet_jit_typed_html_interpolate: sig_binary;
    typed_path_interp: "jet_jit_typed_path_interpolate" => jet_jit_typed_path_interpolate: sig_binary;
    typed_datetime_interp: "jet_jit_typed_datetime_interpolate" => jet_jit_typed_datetime_interpolate: sig_binary;
}
