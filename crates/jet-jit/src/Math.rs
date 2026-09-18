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

/// One resident copy of the fixed-lane kernel; `Compute`'s parallel kernel
/// include reaches its scalar trait here instead of compiling a third copy.
pub(crate) mod simd_lanes {
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
        MathVal::F32(x) => {
            simd_type_name(jet_foundation::Syntax::SimdLaneKind::F32, x.len as usize)
                .expect("known F32 lane layout")
        }
        MathVal::F64(x) => {
            simd_type_name(jet_foundation::Syntax::SimdLaneKind::F64, x.len as usize)
                .expect("known F64 lane layout")
        }
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
#[derive(Clone, Copy)]
enum MathLaneValue {
    F32(f32),
    F64(f64),
    Int(i64),
}

fn lane_count(v: MathVal) -> usize {
    match v {
        MathVal::F32(x) => x.len as usize,
        MathVal::F64(x) => x.len as usize,
        MathVal::Int(x) => x.len as usize,
        MathVal::Vec2(_) => 2,
        MathVal::Vec3(_) => 3,
        MathVal::Vec4(_) => 4,
        MathVal::Mat3(_) => 9,
        MathVal::Mat4(_) => 16,
    }
}

fn math_lane_value(value: i64, index: i64) -> Result<MathLaneValue, String> {
    let value = take_val(value).ok_or_else(|| "lane: bad recv".to_string())?;
    let index =
        simd_lanes::jet_simd_lane_index(index, type_name_of(value), lane_count(value))?;
    Ok(match value {
        MathVal::F32(x) => MathLaneValue::F32(x.lanes[index]),
        MathVal::F64(x) => MathLaneValue::F64(x.lanes[index]),
        MathVal::Int(x) => MathLaneValue::Int(x.lanes[index]),
        MathVal::Vec2(x) => MathLaneValue::F64(x.0[index]),
        MathVal::Vec3(x) => MathLaneValue::F64(x.0[index]),
        MathVal::Vec4(x) => MathLaneValue::F64(x.0[index]),
        MathVal::Mat3(x) => MathLaneValue::F64(x.0[index]),
        MathVal::Mat4(x) => MathLaneValue::F64(x.0[index]),
    })
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
    Some(MathVal::F32(F32Lanes {
        lanes: out,
        len: left.len,
    }))
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
    Some(MathVal::F64(F64Lanes {
        lanes: out,
        len: left.len,
    }))
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
            if left.len == right.len && left.signed == right.signed && left.bits == right.bits =>
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
        (MathVal::Mat3(left), MathVal::Mat3(right))
            if op == simd_lanes::JetSimdBinaryOp::Mul =>
        {
            let out = mat_mul(3, &left.0, &right.0);
            from_lanes("Mat3", &out)
        }
        (MathVal::Mat4(left), MathVal::Mat4(right))
            if op == simd_lanes::JetSimdBinaryOp::Mul =>
        {
            let out = mat_mul(4, &left.0, &right.0);
            from_lanes("Mat4", &out)
        }
        (MathVal::Mat3(matrix), MathVal::Vec3(vector))
            if op == simd_lanes::JetSimdBinaryOp::Mul =>
        {
            let out = mat_vec(3, &matrix.0, &vector.0);
            from_lanes("Vec3", &out)
        }
        (MathVal::Mat4(matrix), MathVal::Vec4(vector))
            if op == simd_lanes::JetSimdBinaryOp::Mul =>
        {
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
        MathVal::F32(value) => {
            simd_lanes::jet_simd_reduce_slice(&value.lanes[..value.len as usize], op)
                .map(|value| pack_float(f64::from(value)))
                .unwrap_or_else(|| {
                    trap("math reduce: empty lanes");
                    0
                })
        }
        MathVal::F64(value) => {
            simd_lanes::jet_simd_reduce_slice(&value.lanes[..value.len as usize], op)
                .map(pack_float)
                .unwrap_or_else(|| {
                    trap("math reduce: empty lanes");
                    0
                })
        }
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

fn zip_int_binop(op: &str, a: &[i64], b: &[i64], signed: bool, bits: u8) -> Option<Vec<i64>> {
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
fn mat_transpose(n: usize, a: &[f64]) -> Vec<f64> {
    let mut r = vec![0.0f64; n * n];
    for c in 0..n {
        for row in 0..n {
            r[c * n + row] = a[row * n + c];
        }
    }
    r
}

fn vec3_scalar_op(
    value: MathVal,
    scalar: f64,
    op: simd_lanes::JetSimdBinaryOp,
) -> Option<MathVal> {
    let MathVal::Vec3(Vec3(mut lanes)) = value else {
        return None;
    };
    for lane in &mut lanes {
        *lane = match op {
            simd_lanes::JetSimdBinaryOp::Mul => *lane * scalar,
            simd_lanes::JetSimdBinaryOp::Div => *lane / scalar,
            _ => return None,
        };
    }
    Some(MathVal::Vec3(Vec3(lanes)))
}

fn scalar_vec3_div(scalar: f64, value: MathVal) -> Option<MathVal> {
    let MathVal::Vec3(Vec3(lanes)) = value else {
        return None;
    };
    Some(MathVal::Vec3(Vec3([
        scalar / lanes[0],
        scalar / lanes[1],
        scalar / lanes[2],
    ])))
}

fn reduce_op(lanes: &[f64], op: &str, f32_lanes: bool) -> Option<f64> {
    let op = simd_reduce_op(op)?;
    if f32_lanes {
        let lanes = lanes.iter().map(|value| *value as f32).collect::<Vec<_>>();
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

fn trap_at(file: i64, line: i64, message: &str) {
    let file = clone_string(file);
    let line = u32::try_from(line.max(0)).unwrap_or(u32::MAX);
    Concurrency::with_runtime_mut(|rt| {
        rt.set_runtime_stop_at("E3001", &file, line, message);
    });
}

fn jet_jit_math_lane_f32(value: i64, index: i64, file: i64, line: i64) -> f32 {
    match math_lane_value(value, index) {
        Ok(MathLaneValue::F32(value)) => value,
        Ok(_) => {
            trap("lane: F32 carrier mismatch");
            0.0
        }
        Err(message) => {
            trap_at(file, line, &message);
            0.0
        }
    }
}

fn jet_jit_math_lane_f64(value: i64, index: i64, file: i64, line: i64) -> f64 {
    match math_lane_value(value, index) {
        Ok(MathLaneValue::F64(value)) => value,
        Ok(_) => {
            trap("lane: F64 carrier mismatch");
            0.0
        }
        Err(message) => {
            trap_at(file, line, &message);
            0.0
        }
    }
}

fn jet_jit_math_lane_i64(value: i64, index: i64, file: i64, line: i64) -> i64 {
    match math_lane_value(value, index) {
        Ok(MathLaneValue::Int(value)) => value,
        Ok(_) => {
            trap("lane: integer carrier mismatch");
            0
        }
        Err(message) => {
            trap_at(file, line, &message);
            0
        }
    }
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
    let f32_lanes =
        simd_layout.is_some_and(|(kind, _)| kind == jet_foundation::Syntax::SimdLaneKind::F32);
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
                    "Mat3" => 9,
                    "Mat4" => 16,
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
        ("Vec3", "mul" | "div") if argv.len() == 2 && is_float_pack(argv[1]) => {
            let Some(value) = take_val(argv[0]) else {
                trap("math scalar binary: bad receiver");
                return 0;
            };
            let op = if func == "mul" {
                simd_lanes::JetSimdBinaryOp::Mul
            } else {
                simd_lanes::JetSimdBinaryOp::Div
            };
            vec3_scalar_op(value, unpack_float(argv[1]), op)
                .map(|value| pack_handle(push_val(value)))
                .or_else(|| {
                    trap("math scalar binary: expected Vec3");
                    None
                })
        }
        ("Float", "div_Vec3") if argv.len() == 2 && is_float_pack(argv[0]) => {
            let Some(value) = take_val(argv[1]) else {
                trap("math scalar binary: bad receiver");
                return 0;
            };
            scalar_vec3_div(unpack_float(argv[0]), value)
                .map(|value| pack_handle(push_val(value)))
                .or_else(|| {
                    trap("math scalar binary: expected Vec3");
                    None
                })
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
        (
            _,
            "sum"
            | "product"
            | "min"
            | "max"
            | "reduce_add"
            | "reduce_mul"
            | "reduce_min"
            | "reduce_max"
            | "reduce_avg"
            | "length",
        ) if argv.len() == 1 => {
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
            let op = match func.as_str() {
                "reduce_add" => "Add",
                "reduce_mul" => "Mul",
                "reduce_min" => "Min",
                "reduce_max" => "Max",
                "reduce_avg" => "Avg",
                other => other,
            };
            if let Some((signed, bits)) = int_layout {
                let Some(lanes) = int_lanes_of(v) else {
                    trap("integer reduction failed");
                    return 0;
                };
                let Some(n) = reduce_int_op(&lanes, op, signed, bits) else {
                    trap("integer reduction failed");
                    return 0;
                };
                Some(pack_int(n))
            } else {
                let Some(n) = reduce_op(&lanes_of(v), op, f32_lanes) else {
                    trap("math reduction failed");
                    return 0;
                };
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
        (_, "transform") if argv.len() == 2 => {
            let Some(a) = take_val(argv[0]) else {
                trap("transform: bad recv");
                return 0;
            };
            let Some(b) = take_val(argv[1]) else {
                trap("transform: bad arg");
                return 0;
            };
            let result = match (a, b) {
                (MathVal::Mat3(matrix), MathVal::Vec3(vector)) => {
                    from_lanes("Vec3", &mat_vec(3, &matrix.0, &vector.0))
                }
                (MathVal::Mat4(matrix), MathVal::Vec4(vector)) => {
                    from_lanes("Vec4", &mat_vec(4, &matrix.0, &vector.0))
                }
                _ => None,
            };
            result
                .map(|value| pack_handle(push_val(value)))
                .or_else(|| {
                    trap("transform expects a matching matrix and vector");
                    None
                })
        }
        (_, "transpose") if argv.len() == 1 => {
            let Some(value) = take_val(argv[0]) else {
                trap("transpose: bad recv");
                return 0;
            };
            let result = match value {
                MathVal::Mat3(matrix) => {
                    from_lanes("Mat3", &mat_transpose(3, &matrix.0))
                }
                MathVal::Mat4(matrix) => {
                    from_lanes("Mat4", &mat_transpose(4, &matrix.0))
                }
                _ => None,
            };
            result
                .map(|value| pack_handle(push_val(value)))
                .or_else(|| {
                    trap("transpose expects a matrix");
                    None
                })
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
            match math_lane_value(argv[0], argv[1]) {
                Ok(MathLaneValue::Int(value)) if int_layout.is_some() => Some(pack_int(value)),
                Ok(MathLaneValue::F32(value)) if int_layout.is_none() => {
                    Some(pack_float(f64::from(value)))
                }
                Ok(MathLaneValue::F64(value)) if int_layout.is_none() => Some(pack_float(value)),
                Ok(_) => {
                    trap("lane: scalar carrier mismatch");
                    None
                }
                Err(message) => {
                    trap(&message);
                    None
                }
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

fn typed_math_call(type_name: &str, func: &str, args: &[i64]) -> i64 {
    let type_name = alloc_string(type_name.to_owned());
    let func = alloc_string(func.to_owned());
    let args = alloc_i64_list(args);
    jet_jit_math_call(type_name, func, args)
}

fn typed_math_new_f64_value(type_name: &str, lanes: &[f64]) -> i64 {
    match from_lanes(type_name, lanes) {
        Some(value) => pack_handle(push_val(value)),
        None => {
            trap(&format!(
                "jit math constructor {} expects {} lanes",
                type_name,
                lanes.len(),
            ));
            0
        }
    }
}

fn typed_math_new_int_value(type_name: &str, lanes: &[i64]) -> i64 {
    match from_int_lanes(type_name, lanes) {
        Some(value) => pack_handle(push_val(value)),
        None => {
            trap(&format!(
                "jit math constructor {} expects {} integer lanes",
                type_name,
                lanes.len(),
            ));
            0
        }
    }
}
fn math_lane_count(type_name: &str) -> Option<usize> {
    jet_foundation::Syntax::simd_lane_layout(type_name)
        .map(|(_, len)| len)
        .or_else(|| match type_name {
            "Vec2" => Some(2),
            "Vec3" => Some(3),
            "Vec4" => Some(4),
            "Mat3" => Some(9),
            "Mat4" => Some(16),
            _ => None,
        })
}

fn typed_math_splat_f64_value(type_name: &str, value: f64) -> i64 {
    let Some(len) = math_lane_count(type_name) else {
        trap(&format!("jit math splat has unknown type {type_name}"));
        return 0;
    };
    let lanes = [value; 32];
    typed_math_new_f64_value(type_name, &lanes[..len])
}

fn typed_math_splat_int_value(type_name: &str, value: i64) -> i64 {
    let Some(len) = math_lane_count(type_name) else {
        trap(&format!("jit math splat has unknown type {type_name}"));
        return 0;
    };
    let lanes = [value; 32];
    typed_math_new_int_value(type_name, &lanes[..len])
}


fn typed_math_float(type_name: &str, func: &str, args: &[i64]) -> f64 {
    unpack_float(typed_math_call(type_name, func, args))
}

fn typed_math_f32(type_name: &str, func: &str, args: &[i64]) -> f32 {
    typed_math_float(type_name, func, args) as f32
}

fn typed_math_int(type_name: &str, func: &str, args: &[i64]) -> i64 {
    unpack_int(typed_math_call(type_name, func, args))
}

macro_rules! typed_math_new_f32 {
    ($name:ident, $type_name:literal, $( $arg:ident ),+ $(,)?) => {
        fn $name($( $arg: f32 ),+) -> i64 {
            let lanes = [$( f64::from($arg) ),+];
            typed_math_new_f64_value($type_name, &lanes)
        }
    };
}

macro_rules! typed_math_new_f64 {
    ($name:ident, $type_name:literal, $( $arg:ident ),+ $(,)?) => {
        fn $name($( $arg: f64 ),+) -> i64 {
            let lanes = [$( $arg ),+];
            typed_math_new_f64_value($type_name, &lanes)
        }
    };
}
macro_rules! typed_math_new_int {
    ($name:ident, $type_name:literal, $( $arg:ident ),+ $(,)?) => {
        fn $name($( $arg: i64 ),+) -> i64 {
            let lanes = [$( $arg ),+];
            typed_math_new_int_value($type_name, &lanes)
        }
    };
}


macro_rules! typed_math_splat_f32 {
    ($name:ident, $type_name:literal) => {
        fn $name(value: f32) -> i64 {
            typed_math_splat_f64_value($type_name, f64::from(value))
        }
    };
}

macro_rules! typed_math_splat_f64 {
    ($name:ident, $type_name:literal) => {
        fn $name(value: f64) -> i64 {
            typed_math_splat_f64_value($type_name, value)
        }
    };
}

macro_rules! typed_math_splat_int {
    ($name:ident, $type_name:literal) => {
        fn $name(value: i64) -> i64 {
            typed_math_splat_int_value($type_name, value)
        }
    };
}

macro_rules! typed_math_array {
    ($name:ident, $type_name:literal, $func:literal) => {
        fn $name(value: i64) -> i64 {
            typed_math_call($type_name, $func, &[value])
        }
    };
}

typed_math_new_f32!(jet_jit_math_f32x4_new, "F32x4", a, b, c, d);
typed_math_new_f32!(
    jet_jit_math_f32x8_new,
    "F32x8",
    a,
    b,
    c,
    d,
    e,
    f,
    g,
    h
);
typed_math_new_f64!(jet_jit_math_f64x2_new, "F64x2", a, b);
typed_math_new_f64!(jet_jit_math_f64x4_new, "F64x4", a, b, c, d);
typed_math_new_int!(
    jet_jit_math_i8x16_new,
    "I8x16",
    a,
    b,
    c,
    d,
    e,
    f,
    g,
    h,
    i,
    j,
    k,
    l,
    m,
    n,
    o,
    p
);
typed_math_new_int!(jet_jit_math_i16x8_new, "I16x8", a, b, c, d, e, f, g, h);
typed_math_new_int!(jet_jit_math_i32x4_new, "I32x4", a, b, c, d);
typed_math_new_int!(jet_jit_math_i64x2_new, "I64x2", a, b);
typed_math_new_int!(
    jet_jit_math_u8x16_new,
    "U8x16",
    a,
    b,
    c,
    d,
    e,
    f,
    g,
    h,
    i,
    j,
    k,
    l,
    m,
    n,
    o,
    p
);
typed_math_new_int!(jet_jit_math_u16x8_new, "U16x8", a, b, c, d, e, f, g, h);
typed_math_new_int!(jet_jit_math_u32x4_new, "U32x4", a, b, c, d);
typed_math_new_int!(jet_jit_math_u64x2_new, "U64x2", a, b);
typed_math_new_int!(
    jet_jit_math_i8x32_new,
    "I8x32",
    a,
    b,
    c,
    d,
    e,
    f,
    g,
    h,
    i,
    j,
    k,
    l,
    m,
    n,
    o,
    p,
    q,
    r,
    s,
    t,
    u,
    v,
    w,
    x,
    y,
    z,
    aa,
    ab,
    ac,
    ad,
    ae,
    af
);
typed_math_new_int!(
    jet_jit_math_i16x16_new,
    "I16x16",
    a,
    b,
    c,
    d,
    e,
    f,
    g,
    h,
    i,
    j,
    k,
    l,
    m,
    n,
    o,
    p
);
typed_math_new_int!(
    jet_jit_math_i32x8_new,
    "I32x8",
    a,
    b,
    c,
    d,
    e,
    f,
    g,
    h
);
typed_math_new_int!(jet_jit_math_i64x4_new, "I64x4", a, b, c, d);
typed_math_new_int!(
    jet_jit_math_u8x32_new,
    "U8x32",
    a,
    b,
    c,
    d,
    e,
    f,
    g,
    h,
    i,
    j,
    k,
    l,
    m,
    n,
    o,
    p,
    q,
    r,
    s,
    t,
    u,
    v,
    w,
    x,
    y,
    z,
    aa,
    ab,
    ac,
    ad,
    ae,
    af
);
typed_math_new_int!(
    jet_jit_math_u16x16_new,
    "U16x16",
    a,
    b,
    c,
    d,
    e,
    f,
    g,
    h,
    i,
    j,
    k,
    l,
    m,
    n,
    o,
    p
);
typed_math_new_int!(
    jet_jit_math_u32x8_new,
    "U32x8",
    a,
    b,
    c,
    d,
    e,
    f,
    g,
    h
);
typed_math_new_int!(jet_jit_math_u64x4_new, "U64x4", a, b, c, d);
typed_math_new_f64!(jet_jit_math_vec2_new, "Vec2", x, y);
typed_math_new_f64!(jet_jit_math_vec3_new, "Vec3", x, y, z);
typed_math_new_f64!(jet_jit_math_vec4_new, "Vec4", x, y, z, w);
typed_math_new_f64!(
    jet_jit_math_mat3_new,
    "Mat3",
    m0,
    m1,
    m2,
    m3,
    m4,
    m5,
    m6,
    m7,
    m8
);
typed_math_new_f64!(
    jet_jit_math_mat4_new,
    "Mat4",
    m0,
    m1,
    m2,
    m3,
    m4,
    m5,
    m6,
    m7,
    m8,
    m9,
    m10,
    m11,
    m12,
    m13,
    m14,
    m15
);

typed_math_splat_f32!(jet_jit_math_f32x4_splat, "F32x4");
typed_math_splat_f32!(jet_jit_math_f32x8_splat, "F32x8");
typed_math_splat_f64!(jet_jit_math_f64x2_splat, "F64x2");
typed_math_splat_f64!(jet_jit_math_f64x4_splat, "F64x4");
typed_math_splat_int!(jet_jit_math_i8x16_splat, "I8x16");
typed_math_splat_int!(jet_jit_math_i16x8_splat, "I16x8");
typed_math_splat_int!(jet_jit_math_i32x4_splat, "I32x4");
typed_math_splat_int!(jet_jit_math_i64x2_splat, "I64x2");
typed_math_splat_int!(jet_jit_math_u8x16_splat, "U8x16");
typed_math_splat_int!(jet_jit_math_u16x8_splat, "U16x8");
typed_math_splat_int!(jet_jit_math_u32x4_splat, "U32x4");
typed_math_splat_int!(jet_jit_math_u64x2_splat, "U64x2");
typed_math_splat_int!(jet_jit_math_i8x32_splat, "I8x32");
typed_math_splat_int!(jet_jit_math_i16x16_splat, "I16x16");
typed_math_splat_int!(jet_jit_math_i32x8_splat, "I32x8");
typed_math_splat_int!(jet_jit_math_i64x4_splat, "I64x4");
typed_math_splat_int!(jet_jit_math_u8x32_splat, "U8x32");
typed_math_splat_int!(jet_jit_math_u16x16_splat, "U16x16");
typed_math_splat_int!(jet_jit_math_u32x8_splat, "U32x8");
typed_math_splat_int!(jet_jit_math_u64x4_splat, "U64x4");
typed_math_splat_f64!(jet_jit_math_vec2_splat, "Vec2");
typed_math_splat_f64!(jet_jit_math_vec3_splat, "Vec3");
typed_math_splat_f64!(jet_jit_math_vec4_splat, "Vec4");
typed_math_splat_f64!(jet_jit_math_mat3_splat, "Mat3");
typed_math_splat_f64!(jet_jit_math_mat4_splat, "Mat4");

typed_math_array!(jet_jit_math_f32x4_from_array, "F32x4", "from_array");
typed_math_array!(jet_jit_math_f64x2_from_array, "F64x2", "from_array");
typed_math_array!(jet_jit_math_f32x8_from_array, "F32x8", "from_array");
typed_math_array!(jet_jit_math_f64x4_from_array, "F64x4", "from_array");
typed_math_array!(jet_jit_math_i8x16_from_array, "I8x16", "from_array");
typed_math_array!(jet_jit_math_i16x8_from_array, "I16x8", "from_array");
typed_math_array!(jet_jit_math_i32x4_from_array, "I32x4", "from_array");
typed_math_array!(jet_jit_math_i64x2_from_array, "I64x2", "from_array");
typed_math_array!(jet_jit_math_u8x16_from_array, "U8x16", "from_array");
typed_math_array!(jet_jit_math_u16x8_from_array, "U16x8", "from_array");
typed_math_array!(jet_jit_math_u32x4_from_array, "U32x4", "from_array");
typed_math_array!(jet_jit_math_u64x2_from_array, "U64x2", "from_array");
typed_math_array!(jet_jit_math_i8x32_from_array, "I8x32", "from_array");
typed_math_array!(jet_jit_math_i16x16_from_array, "I16x16", "from_array");
typed_math_array!(jet_jit_math_i32x8_from_array, "I32x8", "from_array");
typed_math_array!(jet_jit_math_i64x4_from_array, "I64x4", "from_array");
typed_math_array!(jet_jit_math_u8x32_from_array, "U8x32", "from_array");
typed_math_array!(jet_jit_math_u16x16_from_array, "U16x16", "from_array");
typed_math_array!(jet_jit_math_u32x8_from_array, "U32x8", "from_array");
typed_math_array!(jet_jit_math_u64x4_from_array, "U64x4", "from_array");
typed_math_array!(jet_jit_math_vec2_from_array, "Vec2", "from_array");
typed_math_array!(jet_jit_math_vec3_from_array, "Vec3", "from_array");
typed_math_array!(jet_jit_math_vec4_from_array, "Vec4", "from_array");
typed_math_array!(jet_jit_math_mat3_from_array, "Mat3", "from_array");
typed_math_array!(jet_jit_math_mat4_from_array, "Mat4", "from_array");

fn jet_jit_math_typed_to_array(value: i64) -> i64 {
    let Some(value) = take_val(value) else {
        trap("to_array: bad recv");
        return 0;
    };
    if matches!(value, MathVal::Int(_)) {
        pack_handle(alloc_i64_list(&int_lanes_of(value).unwrap()))
    } else {
        pack_handle(alloc_f64_list(&lanes_of(value)))
    }
}

fn typed_reduce_f32(value: i64, op: i64) -> f32 {
    unpack_float(jet_jit_math_reduce(value, op)) as f32
}

fn typed_reduce_f64(value: i64, op: i64) -> f64 {
    unpack_float(jet_jit_math_reduce(value, op))
}

fn typed_reduce_int(value: i64, op: i64) -> i64 {
    unpack_int(jet_jit_math_reduce(value, op))
}

macro_rules! typed_math_reduce_family {
    (
        $sum:ident,
        $product:ident,
        $min:ident,
        $max:ident,
        $reduce_add:ident,
        $reduce_mul:ident,
        $reduce_min:ident,
        $reduce_max:ident,
        $reduce_avg:ident,
        $call:ident,
        $return:ty
    ) => {
        fn $sum(value: i64) -> $return {
            $call(value, 0)
        }
        fn $product(value: i64) -> $return {
            $call(value, 1)
        }
        fn $min(value: i64) -> $return {
            $call(value, 2)
        }
        fn $max(value: i64) -> $return {
            $call(value, 3)
        }
        fn $reduce_add(value: i64) -> $return {
            $call(value, 0)
        }
        fn $reduce_mul(value: i64) -> $return {
            $call(value, 1)
        }
        fn $reduce_min(value: i64) -> $return {
            $call(value, 2)
        }
        fn $reduce_max(value: i64) -> $return {
            $call(value, 3)
        }
        fn $reduce_avg(value: i64) -> $return {
            $call(value, 4)
        }
    };
}

typed_math_reduce_family!(
    jet_jit_math_f32_sum,
    jet_jit_math_f32_product,
    jet_jit_math_f32_min,
    jet_jit_math_f32_max,
    jet_jit_math_f32_reduce_add,
    jet_jit_math_f32_reduce_mul,
    jet_jit_math_f32_reduce_min,
    jet_jit_math_f32_reduce_max,
    jet_jit_math_f32_reduce_avg,
    typed_reduce_f32,
    f32
);
typed_math_reduce_family!(
    jet_jit_math_f64_sum,
    jet_jit_math_f64_product,
    jet_jit_math_f64_min,
    jet_jit_math_f64_max,
    jet_jit_math_f64_reduce_add,
    jet_jit_math_f64_reduce_mul,
    jet_jit_math_f64_reduce_min,
    jet_jit_math_f64_reduce_max,
    jet_jit_math_f64_reduce_avg,
    typed_reduce_f64,
    f64
);
typed_math_reduce_family!(
    jet_jit_math_int_sum,
    jet_jit_math_int_product,
    jet_jit_math_int_min,
    jet_jit_math_int_max,
    jet_jit_math_int_reduce_add,
    jet_jit_math_int_reduce_mul,
    jet_jit_math_int_reduce_min,
    jet_jit_math_int_reduce_max,
    jet_jit_math_int_reduce_avg,
    typed_reduce_int,
    i64
);

fn jet_jit_math_typed_add(left: i64, right: i64) -> i64 {
    jet_jit_math_binary(left, right, 0)
}

fn jet_jit_math_typed_sub(left: i64, right: i64) -> i64 {
    jet_jit_math_binary(left, right, 1)
}

fn jet_jit_math_typed_mul(left: i64, right: i64) -> i64 {
    jet_jit_math_binary(left, right, 2)
}

fn jet_jit_math_typed_div(left: i64, right: i64) -> i64 {
    jet_jit_math_binary(left, right, 3)
}

macro_rules! typed_math_float_method {
    ($name:ident, $type_name:literal, $func:literal) => {
        fn $name(left: i64, right: i64) -> f64 {
            typed_math_float($type_name, $func, &[left, right])
        }
    };
}

macro_rules! typed_math_unary_float_method {
    ($name:ident, $type_name:literal, $func:literal) => {
        fn $name(value: i64) -> f64 {
            typed_math_float($type_name, $func, &[value])
        }
    };
}

macro_rules! typed_math_handle_method {
    ($name:ident, $type_name:literal, $func:literal) => {
        fn $name(value: i64) -> i64 {
            typed_math_call($type_name, $func, &[value])
        }
    };
}

macro_rules! typed_math_binary_handle_method {
    ($name:ident, $type_name:literal, $func:literal) => {
        fn $name(left: i64, right: i64) -> i64 {
            typed_math_call($type_name, $func, &[left, right])
        }
    };
}

typed_math_float_method!(jet_jit_math_vec2_dot, "Vec2", "dot");
typed_math_float_method!(jet_jit_math_vec3_dot, "Vec3", "dot");
typed_math_float_method!(jet_jit_math_vec4_dot, "Vec4", "dot");
typed_math_unary_float_method!(jet_jit_math_vec2_length, "Vec2", "length");
typed_math_unary_float_method!(jet_jit_math_vec3_length, "Vec3", "length");
typed_math_unary_float_method!(jet_jit_math_vec4_length, "Vec4", "length");
typed_math_handle_method!(jet_jit_math_vec2_normalize, "Vec2", "normalize");
typed_math_handle_method!(jet_jit_math_vec3_normalize, "Vec3", "normalize");
typed_math_handle_method!(jet_jit_math_vec4_normalize, "Vec4", "normalize");
typed_math_binary_handle_method!(jet_jit_math_vec3_cross, "Vec3", "cross");
typed_math_binary_handle_method!(jet_jit_math_mat3_matmul, "Mat3", "matmul");
typed_math_binary_handle_method!(jet_jit_math_mat4_matmul, "Mat4", "matmul");
typed_math_binary_handle_method!(jet_jit_math_mat3_transform, "Mat3", "transform");
typed_math_binary_handle_method!(jet_jit_math_mat4_transform, "Mat4", "transform");
typed_math_handle_method!(jet_jit_math_mat3_transpose, "Mat3", "transpose");
typed_math_handle_method!(jet_jit_math_mat4_transpose, "Mat4", "transpose");

fn jet_jit_math_vec3_scalar_op(
    value: i64,
    scalar: f64,
    op: simd_lanes::JetSimdBinaryOp,
) -> i64 {
    let Some(value) = take_val(value) else {
        trap("math scalar binary: bad receiver");
        return 0;
    };
    let Some(value) = vec3_scalar_op(value, scalar, op) else {
        trap("math scalar binary: expected Vec3");
        return 0;
    };
    pack_handle(push_val(value))
}

fn jet_jit_math_vec3_mul_scalar(value: i64, scalar: f64) -> i64 {
    jet_jit_math_vec3_scalar_op(value, scalar, simd_lanes::JetSimdBinaryOp::Mul)
}

fn jet_jit_math_vec3_div_scalar(value: i64, scalar: f64) -> i64 {
    jet_jit_math_vec3_scalar_op(value, scalar, simd_lanes::JetSimdBinaryOp::Div)
}

fn jet_jit_math_float_div_vec3(scalar: f64, value: i64) -> i64 {
    let Some(value) = take_val(value) else {
        trap("math scalar binary: bad receiver");
        return 0;
    };
    let Some(value) = scalar_vec3_div(scalar, value) else {
        trap("math scalar binary: expected Vec3");
        return 0;
    };
    pack_handle(push_val(value))
}

fn jet_jit_math_float_mul_vec3(scalar: f64, value: i64) -> i64 {
    jet_jit_math_vec3_scalar_op(value, scalar, simd_lanes::JetSimdBinaryOp::Mul)
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

fn require_bool_list(list: i64) -> Option<Vec<bool>> {
    let values: Option<Vec<bool>> = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list)?;
        (0..len)
            .map(|index| rt.heap.list_get_int(list, index).map(|value| value != 0))
            .collect()
    });
    if values.is_none() {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("typed HTML trust list contains a non-boolean value")
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

/// The canonical DateTime typed-head route carries a parsed civil-time handle,
/// not the interpolated source text. AOT and the interpreter both preserve the
/// DateTime value; the JIT adapter must do the same before civil dispatch.
fn jet_jit_typed_datetime_literal(literals: i64, holes: i64) -> i64 {
    let Some(text) = typed_datetime_interpolate(literals, holes) else {
        return 0;
    };
    match crate::Time::time_rt::jet_time_parse_rfc3339(&text) {
        Ok(value) => crate::Time::push(crate::Time::TimeValue::DateTime(value)),
        Err(error) => Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault(&format!(
                "invalid DateTime typed-head value reached the JIT: {error}"
            ));
            0
        }),
    }
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

fn jet_jit_typed_sql_raw(s: i64) -> i64 {
    super::DB::alloc_sql_value(typed_text_semantics::jet_typed_sql_raw(clone_string(s)))
}

fn jet_jit_typed_sql_interpolate(literals: i64, holes: i64) -> i64 {
    let Some(literals) = require_string_list(literals) else {
        return 0;
    };
    let Some(holes) = super::DB::values_from_list_checked(holes) else {
        trap("typed-text SQL bindings are malformed");
        return 0;
    };
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    super::DB::alloc_sql_value(typed_text_semantics::jet_typed_sql_interpolate(
        &literal_refs,
        holes,
    ))
}

fn jet_jit_typed_sql_template(value: i64) -> i64 {
    let Some(value) = super::DB::clone_sql_value(value) else {
        trap("typed-text SQL value is malformed");
        return 0;
    };
    alloc_string(typed_text_semantics::jet_typed_sql_template(&value))
}

fn jet_jit_typed_sql_params(value: i64) -> i64 {
    let Some(value) = super::DB::clone_sql_value(value) else {
        trap("typed-text SQL value is malformed");
        return 0;
    };
    super::DB::alloc_dbvalue_list(typed_text_semantics::jet_typed_sql_params(&value))
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

fn jet_jit_typed_html_interpolate(literals: i64, holes: i64, trusted_html: i64) -> i64 {
    let Some(literals) = require_string_list(literals) else {
        return 0;
    };
    let Some(holes) = require_string_list(holes) else {
        return 0;
    };
    let Some(trusted_html) = require_bool_list(trusted_html) else {
        return 0;
    };
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    alloc_string(typed_text_semantics::jet_typed_html_interpolate(
        &literal_refs,
        holes,
        &trusted_html,
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

#[derive(Clone, Copy)]
struct JitGeometryTransform {
    matrix: [f64; 6],
    from_frame: i64,
    to_frame: i64,
}

fn geometry_coord(rt: &crate::runtime_host::JitRuntime, handle: i64) -> Option<(f64, f64, i64)> {
    Some((
        rt.heap.record_get_float(handle, 0)?,
        rt.heap.record_get_float(handle, 1)?,
        rt.heap.record_get_int(handle, 2)?,
    ))
}

fn geometry_transform(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<JitGeometryTransform> {
    Some(JitGeometryTransform {
        matrix: [
            rt.heap.record_get_float(handle, 0)?,
            rt.heap.record_get_float(handle, 1)?,
            rt.heap.record_get_float(handle, 2)?,
            rt.heap.record_get_float(handle, 3)?,
            rt.heap.record_get_float(handle, 4)?,
            rt.heap.record_get_float(handle, 5)?,
        ],
        from_frame: rt.heap.record_get_int(handle, 6)?,
        to_frame: rt.heap.record_get_int(handle, 7)?,
    })
}

fn geometry_coord_record(rt: &mut crate::runtime_host::JitRuntime, x: f64, y: f64, frame: i64) -> i64 {
    let record = rt.heap.alloc_record(3);
    let _ = rt.heap.record_set_float(record, 0, x);
    let _ = rt.heap.record_set_float(record, 1, y);
    let _ = rt.heap.record_set_int(record, 2, frame);
    record
}

fn geometry_transform_record(
    rt: &mut crate::runtime_host::JitRuntime,
    matrix: [f64; 6],
    from_frame: i64,
    to_frame: i64,
) -> i64 {
    let record = rt.heap.alloc_record(8);
    for (index, value) in matrix.into_iter().enumerate() {
        let _ = rt.heap.record_set_float(record, index as i64, value);
    }
    let _ = rt.heap.record_set_int(record, 6, from_frame);
    let _ = rt.heap.record_set_int(record, 7, to_frame);
    record
}

fn geometry_ray_record(
    rt: &mut crate::runtime_host::JitRuntime,
    origin: i64,
    direction: i64,
) -> i64 {
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_record(record, 0, origin);
    let _ = rt.heap.record_set_record(record, 1, direction);
    record
}

fn geometry_error_result(rt: &mut crate::runtime_host::JitRuntime, message: &'static str) -> i64 {
    rt.errors
        .push(jet_foundation::Outcome::jet_err_from_message(message.to_string()));
    let error_handle = rt.errors.len() as u64;
    crate::runtime_host::alloc_jit_result(rt, false, error_handle)
}

fn geometry_ok_result(rt: &mut crate::runtime_host::JitRuntime, value: i64) -> i64 {
    crate::runtime_host::alloc_jit_result(rt, true, value as u64)
}

fn jet_jit_geometry_coord_new(x: f64, y: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| geometry_coord_record(rt, x, y, 0))
}
fn jet_jit_geometry_coord_new_frame(x: f64, y: f64, frame: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| geometry_coord_record(rt, x, y, frame))
}

fn jet_jit_geometry_coord_checked_op(a: i64, b: i64, subtract: bool) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((ax, ay, a_frame)) = geometry_coord(rt, a) else {
            rt.set_host_fault("coordinate add received an invalid left carrier");
            return 0;
        };
        let Some((bx, by, b_frame)) = geometry_coord(rt, b) else {
            rt.set_host_fault("coordinate add received an invalid right carrier");
            return 0;
        };
        if a_frame != 0 && b_frame != 0 && a_frame != b_frame {
            return geometry_error_result(rt, "coordinate values belong to different frames");
        }
        let frame = if a_frame != 0 { a_frame } else { b_frame };
        let record = geometry_coord_record(
            rt,
            if subtract { ax - bx } else { ax + bx },
            if subtract { ay - by } else { ay + by },
            frame,
        );
        geometry_ok_result(rt, record)
    })
}

fn jet_jit_geometry_coord_plain_op(a: i64, b: i64, subtract: bool) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((ax, ay, frame)) = geometry_coord(rt, a) else {
            rt.set_host_fault("coordinate add received an invalid left carrier");
            return 0;
        };
        let Some((bx, by, _)) = geometry_coord(rt, b) else {
            rt.set_host_fault("coordinate add received an invalid right carrier");
            return 0;
        };
        geometry_coord_record(
            rt,
            if subtract { ax - bx } else { ax + bx },
            if subtract { ay - by } else { ay + by },
            frame,
        )
    })
}

fn jet_jit_geometry_coord_add(a: i64, b: i64) -> i64 {
    jet_jit_geometry_coord_checked_op(a, b, false)
}

fn jet_jit_geometry_coord_sub(a: i64, b: i64) -> i64 {
    jet_jit_geometry_coord_checked_op(a, b, true)
}

fn jet_jit_geometry_coord_plain_add(a: i64, b: i64) -> i64 {
    jet_jit_geometry_coord_plain_op(a, b, false)
}

fn jet_jit_geometry_coord_plain_sub(a: i64, b: i64) -> i64 {
    jet_jit_geometry_coord_plain_op(a, b, true)
}

fn jet_jit_geometry_transform_new(
    m00: f64,
    m01: f64,
    m10: f64,
    m11: f64,
    tx: f64,
    ty: f64,
    from_frame: i64,
    to_frame: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        geometry_transform_record(rt, [m00, m01, m10, m11, tx, ty], from_frame, to_frame)
    })
}

fn jet_jit_geometry_transform_then(first: i64, next: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(first) = geometry_transform(rt, first) else {
            rt.set_host_fault("transform composition received an invalid left carrier");
            return 0;
        };
        let Some(next) = geometry_transform(rt, next) else {
            rt.set_host_fault("transform composition received an invalid right carrier");
            return 0;
        };
        if first.to_frame != 0
            && next.from_frame != 0
            && first.to_frame != next.from_frame
        {
            return geometry_error_result(rt, "transform composition has mismatched frame identity");
        }
        let a = first.matrix;
        let b = next.matrix;
        let matrix = [
            b[0] * a[0] + b[1] * a[2],
            b[0] * a[1] + b[1] * a[3],
            b[2] * a[0] + b[3] * a[2],
            b[2] * a[1] + b[3] * a[3],
            b[0] * a[4] + b[1] * a[5] + b[4],
            b[2] * a[4] + b[3] * a[5] + b[5],
        ];
        let record = geometry_transform_record(rt, matrix, first.from_frame, next.to_frame);
        geometry_ok_result(rt, record)
    })
}

fn jet_jit_geometry_transform_inverse(transform: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(transform) = geometry_transform(rt, transform) else {
            rt.set_host_fault("transform inverse received an invalid carrier");
            return 0;
        };
        let m = transform.matrix;
        let det = m[0] * m[3] - m[1] * m[2];
        if !det.is_finite() || det.abs() <= f64::EPSILON {
            return geometry_error_result(rt, "transform is singular and has no inverse");
        }
        let inv_det = 1.0 / det;
        let matrix = [
            m[3] * inv_det,
            -m[1] * inv_det,
            -m[2] * inv_det,
            m[0] * inv_det,
            (m[1] * m[5] - m[3] * m[4]) * inv_det,
            (m[2] * m[4] - m[0] * m[5]) * inv_det,
        ];
        let record = geometry_transform_record(
            rt,
            matrix,
            transform.to_frame,
            transform.from_frame,
        );
        geometry_ok_result(rt, record)
    })
}

fn jet_jit_geometry_transform_point(transform: i64, point: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(transform) = geometry_transform(rt, transform) else {
            rt.set_host_fault("transform point received an invalid transform carrier");
            return 0;
        };
        let Some((x, y, frame)) = geometry_coord(rt, point) else {
            rt.set_host_fault("transform point received an invalid point carrier");
            return 0;
        };
        if frame != 0 && frame != transform.from_frame {
            return geometry_error_result(rt, "coordinate point belongs to a stale frame");
        }
        let m = transform.matrix;
        let record = geometry_coord_record(
            rt,
            m[0] * x + m[1] * y + m[4],
            m[2] * x + m[3] * y + m[5],
            transform.to_frame,
        );
        geometry_ok_result(rt, record)
    })
}

fn jet_jit_geometry_transform_point_at_depth(transform: i64, point: i64, depth: f64) -> i64 {
    if !depth.is_finite() {
        return Concurrency::with_runtime_mut(|rt| {
            geometry_error_result(rt, "perspective depth must be finite")
        });
    }
    Concurrency::with_runtime_mut(|rt| {
        let Some(transform) = geometry_transform(rt, transform) else {
            rt.set_host_fault("transform point received an invalid transform carrier");
            return 0;
        };
        let Some((x, y, frame)) = geometry_coord(rt, point) else {
            rt.set_host_fault("transform point received an invalid point carrier");
            return 0;
        };
        if frame != 0 && frame != transform.from_frame {
            return geometry_error_result(rt, "coordinate point belongs to a stale frame");
        }
        let m = transform.matrix;
        let origin_x = m[0] * x + m[1] * y + m[4];
        let origin_y = m[2] * x + m[3] * y + m[5];
        let record = geometry_coord_record(
            rt,
            origin_x + m[0] * depth,
            origin_y + m[2] * depth,
            transform.to_frame,
        );
        geometry_ok_result(rt, record)
    })
}

fn jet_jit_geometry_transform_ray(transform: i64, point: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(transform) = geometry_transform(rt, transform) else {
            rt.set_host_fault("transform ray received an invalid transform carrier");
            return 0;
        };
        let Some((x, y, frame)) = geometry_coord(rt, point) else {
            rt.set_host_fault("transform ray received an invalid point carrier");
            return 0;
        };
        if frame != 0 && frame != transform.from_frame {
            return geometry_error_result(rt, "coordinate point belongs to a stale frame");
        }
        let m = transform.matrix;
        let origin = geometry_coord_record(
            rt,
            m[0] * x + m[1] * y + m[4],
            m[2] * x + m[3] * y + m[5],
            transform.to_frame,
        );
        let direction = geometry_coord_record(rt, m[0], m[2], transform.to_frame);
        let ray = geometry_ray_record(rt, origin, direction);
        geometry_ok_result(rt, ray)
    })
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
        let mut sig_lane_f32 = Signature::new(cc);
        for _ in 0..4 {
            sig_lane_f32.params.push(AbiParam::new(types::I64));
        }
        sig_lane_f32.returns.push(AbiParam::new(types::F32));
        let mut sig_lane_f64 = sig_lane_f32.clone();
        sig_lane_f64.returns[0] = AbiParam::new(types::F64);
        let mut sig_lane_i64 = sig_lane_f32.clone();
        sig_lane_i64.returns[0] = AbiParam::new(types::I64);
        let mut sig_i64_i8 = Signature::new(cc);
        sig_i64_i8.params.push(AbiParam::new(types::I64));
        sig_i64_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_i64_f64 = Signature::new(cc);
        sig_i64_f64.params.push(AbiParam::new(types::I64));
        sig_i64_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_float_binary = Signature::new(cc);
        sig_float_binary.params.push(AbiParam::new(types::F64));
        sig_float_binary.params.push(AbiParam::new(types::F64));
        sig_float_binary.returns.push(AbiParam::new(types::I64));
        let mut sig_float_float_int = Signature::new(cc);
        sig_float_float_int.params.push(AbiParam::new(types::F64));
        sig_float_float_int.params.push(AbiParam::new(types::F64));
        sig_float_float_int.params.push(AbiParam::new(types::I64));
        sig_float_float_int.returns.push(AbiParam::new(types::I64));
        let mut sig_transform_new = Signature::new(cc);
        for _ in 0..6 {
            sig_transform_new.params.push(AbiParam::new(types::F64));
        }
        sig_transform_new.params.push(AbiParam::new(types::I64));
        sig_transform_new.params.push(AbiParam::new(types::I64));
        sig_transform_new.returns.push(AbiParam::new(types::I64));
        let mut sig_handle_handle_float = Signature::new(cc);
        sig_handle_handle_float.params.push(AbiParam::new(types::I64));
        sig_handle_handle_float.params.push(AbiParam::new(types::I64));
        sig_handle_handle_float.params.push(AbiParam::new(types::F64));
        sig_handle_handle_float.returns.push(AbiParam::new(types::I64));
        let mut sig_unary = Signature::new(cc);
        sig_unary.params.push(AbiParam::new(types::I64));
        sig_unary.returns.push(AbiParam::new(types::I64));
        let mut sig_binary = Signature::new(cc);
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.returns.push(AbiParam::new(types::I64));
        let mut sig_unary_f64 = Signature::new(cc);
        sig_unary_f64.params.push(AbiParam::new(types::I64));
        sig_unary_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_binary_f64 = Signature::new(cc);
        sig_binary_f64.params.push(AbiParam::new(types::I64));
        sig_binary_f64.params.push(AbiParam::new(types::I64));
        sig_binary_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_ternary = sig_binary.clone();
        sig_ternary.params.push(AbiParam::new(types::I64));
        let mut sig_i64_f32 = Signature::new(cc);
        sig_i64_f32.params.push(AbiParam::new(types::I64));
        sig_i64_f32.returns.push(AbiParam::new(types::F32));
        let mut sig_f32_i64 = Signature::new(cc);
        sig_f32_i64.params.push(AbiParam::new(types::F32));
        sig_f32_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_f64_i64 = Signature::new(cc);
        sig_f64_i64.params.push(AbiParam::new(types::F64));
        sig_f64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_handle_float = Signature::new(cc);
        sig_handle_float.params.push(AbiParam::new(types::I64));
        sig_handle_float.params.push(AbiParam::new(types::F64));
        sig_handle_float.returns.push(AbiParam::new(types::I64));
        let mut sig_float_handle = Signature::new(cc);
        sig_float_handle.params.push(AbiParam::new(types::F64));
        sig_float_handle.params.push(AbiParam::new(types::I64));
        sig_float_handle.returns.push(AbiParam::new(types::I64));
        macro_rules! repeated_sig {
            ($name:ident, $param:expr, $count:expr, $ret:expr) => {
                let mut $name = Signature::new(cc);
                for _ in 0..$count {
                    $name.params.push(AbiParam::new($param));
                }
                $name.returns.push(AbiParam::new($ret));
            };
        }
        repeated_sig!(sig_f32x4, types::F32, 4, types::I64);
        repeated_sig!(sig_f32x8, types::F32, 8, types::I64);
        repeated_sig!(sig_f64x2, types::F64, 2, types::I64);
        repeated_sig!(sig_f64x3, types::F64, 3, types::I64);
        repeated_sig!(sig_f64x4, types::F64, 4, types::I64);
        repeated_sig!(sig_f64x9, types::F64, 9, types::I64);
        repeated_sig!(sig_f64x16, types::F64, 16, types::I64);
        repeated_sig!(sig_i64x2, types::I64, 2, types::I64);
        repeated_sig!(sig_i64x4, types::I64, 4, types::I64);
        repeated_sig!(sig_i64x8, types::I64, 8, types::I64);
        repeated_sig!(sig_i64x16, types::I64, 16, types::I64);
        repeated_sig!(sig_i64x32, types::I64, 32, types::I64);
    }
    call: "jet_jit_math_call" => jet_jit_math_call: sig_call;
    binary: "jet_jit_math_binary" => jet_jit_math_binary: sig_call;
    splat: "jet_jit_math_splat" => jet_jit_math_splat: sig_call;
    simd_f32x4_new: "jet_math_F32x4_new" => jet_jit_math_f32x4_new: sig_f32x4;
    simd_f32x4_splat: "jet_math_F32x4_splat" => jet_jit_math_f32x4_splat: sig_f32_i64;
    simd_f32x4_from_array: "jet_math_F32x4_from_array" => jet_jit_math_f32x4_from_array: sig_unary;
    simd_f32x4_to_array: "jet_math_F32x4_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_f32x4_sum: "jet_math_F32x4_sum" => jet_jit_math_f32_sum: sig_i64_f32;
    simd_f32x4_product: "jet_math_F32x4_product" => jet_jit_math_f32_product: sig_i64_f32;
    simd_f32x4_min: "jet_math_F32x4_min" => jet_jit_math_f32_min: sig_i64_f32;
    simd_f32x4_max: "jet_math_F32x4_max" => jet_jit_math_f32_max: sig_i64_f32;
    simd_f32x4_reduce_add: "jet_math_F32x4_reduce_add" => jet_jit_math_f32_reduce_add: sig_i64_f32;
    simd_f32x4_reduce_mul: "jet_math_F32x4_reduce_mul" => jet_jit_math_f32_reduce_mul: sig_i64_f32;
    simd_f32x4_reduce_min: "jet_math_F32x4_reduce_min" => jet_jit_math_f32_reduce_min: sig_i64_f32;
    simd_f32x4_reduce_max: "jet_math_F32x4_reduce_max" => jet_jit_math_f32_reduce_max: sig_i64_f32;
    simd_f32x4_reduce_avg: "jet_math_F32x4_reduce_avg" => jet_jit_math_f32_reduce_avg: sig_i64_f32;
    simd_f32x4_add: "jet_math_F32x4_add" => jet_jit_math_typed_add: sig_binary;
    simd_f32x4_sub: "jet_math_F32x4_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_f32x4_mul: "jet_math_F32x4_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_f32x4_div: "jet_math_F32x4_div" => jet_jit_math_typed_div: sig_binary;
    simd_f64x2_new: "jet_math_F64x2_new" => jet_jit_math_f64x2_new: sig_f64x2;
    simd_f64x2_splat: "jet_math_F64x2_splat" => jet_jit_math_f64x2_splat: sig_f64_i64;
    simd_f64x2_from_array: "jet_math_F64x2_from_array" => jet_jit_math_f64x2_from_array: sig_unary;
    simd_f64x2_to_array: "jet_math_F64x2_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_f64x2_sum: "jet_math_F64x2_sum" => jet_jit_math_f64_sum: sig_i64_f64;
    simd_f64x2_product: "jet_math_F64x2_product" => jet_jit_math_f64_product: sig_i64_f64;
    simd_f64x2_min: "jet_math_F64x2_min" => jet_jit_math_f64_min: sig_i64_f64;
    simd_f64x2_max: "jet_math_F64x2_max" => jet_jit_math_f64_max: sig_i64_f64;
    simd_f64x2_reduce_add: "jet_math_F64x2_reduce_add" => jet_jit_math_f64_reduce_add: sig_i64_f64;
    simd_f64x2_reduce_mul: "jet_math_F64x2_reduce_mul" => jet_jit_math_f64_reduce_mul: sig_i64_f64;
    simd_f64x2_reduce_min: "jet_math_F64x2_reduce_min" => jet_jit_math_f64_reduce_min: sig_i64_f64;
    simd_f64x2_reduce_max: "jet_math_F64x2_reduce_max" => jet_jit_math_f64_reduce_max: sig_i64_f64;
    simd_f64x2_reduce_avg: "jet_math_F64x2_reduce_avg" => jet_jit_math_f64_reduce_avg: sig_i64_f64;
    simd_f64x2_add: "jet_math_F64x2_add" => jet_jit_math_typed_add: sig_binary;
    simd_f64x2_sub: "jet_math_F64x2_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_f64x2_mul: "jet_math_F64x2_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_f64x2_div: "jet_math_F64x2_div" => jet_jit_math_typed_div: sig_binary;
    simd_f32x8_new: "jet_math_F32x8_new" => jet_jit_math_f32x8_new: sig_f32x8;
    simd_f32x8_splat: "jet_math_F32x8_splat" => jet_jit_math_f32x8_splat: sig_f32_i64;
    simd_f32x8_from_array: "jet_math_F32x8_from_array" => jet_jit_math_f32x8_from_array: sig_unary;
    simd_f32x8_to_array: "jet_math_F32x8_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_f32x8_sum: "jet_math_F32x8_sum" => jet_jit_math_f32_sum: sig_i64_f32;
    simd_f32x8_product: "jet_math_F32x8_product" => jet_jit_math_f32_product: sig_i64_f32;
    simd_f32x8_min: "jet_math_F32x8_min" => jet_jit_math_f32_min: sig_i64_f32;
    simd_f32x8_max: "jet_math_F32x8_max" => jet_jit_math_f32_max: sig_i64_f32;
    simd_f32x8_reduce_add: "jet_math_F32x8_reduce_add" => jet_jit_math_f32_reduce_add: sig_i64_f32;
    simd_f32x8_reduce_mul: "jet_math_F32x8_reduce_mul" => jet_jit_math_f32_reduce_mul: sig_i64_f32;
    simd_f32x8_reduce_min: "jet_math_F32x8_reduce_min" => jet_jit_math_f32_reduce_min: sig_i64_f32;
    simd_f32x8_reduce_max: "jet_math_F32x8_reduce_max" => jet_jit_math_f32_reduce_max: sig_i64_f32;
    simd_f32x8_reduce_avg: "jet_math_F32x8_reduce_avg" => jet_jit_math_f32_reduce_avg: sig_i64_f32;
    simd_f32x8_add: "jet_math_F32x8_add" => jet_jit_math_typed_add: sig_binary;
    simd_f32x8_sub: "jet_math_F32x8_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_f32x8_mul: "jet_math_F32x8_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_f32x8_div: "jet_math_F32x8_div" => jet_jit_math_typed_div: sig_binary;
    simd_f64x4_new: "jet_math_F64x4_new" => jet_jit_math_f64x4_new: sig_f64x4;
    simd_f64x4_splat: "jet_math_F64x4_splat" => jet_jit_math_f64x4_splat: sig_f64_i64;
    simd_f64x4_from_array: "jet_math_F64x4_from_array" => jet_jit_math_f64x4_from_array: sig_unary;
    simd_f64x4_to_array: "jet_math_F64x4_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_f64x4_sum: "jet_math_F64x4_sum" => jet_jit_math_f64_sum: sig_i64_f64;
    simd_f64x4_product: "jet_math_F64x4_product" => jet_jit_math_f64_product: sig_i64_f64;
    simd_f64x4_min: "jet_math_F64x4_min" => jet_jit_math_f64_min: sig_i64_f64;
    simd_f64x4_max: "jet_math_F64x4_max" => jet_jit_math_f64_max: sig_i64_f64;
    simd_f64x4_reduce_add: "jet_math_F64x4_reduce_add" => jet_jit_math_f64_reduce_add: sig_i64_f64;
    simd_f64x4_reduce_mul: "jet_math_F64x4_reduce_mul" => jet_jit_math_f64_reduce_mul: sig_i64_f64;
    simd_f64x4_reduce_min: "jet_math_F64x4_reduce_min" => jet_jit_math_f64_reduce_min: sig_i64_f64;
    simd_f64x4_reduce_max: "jet_math_F64x4_reduce_max" => jet_jit_math_f64_reduce_max: sig_i64_f64;
    simd_f64x4_reduce_avg: "jet_math_F64x4_reduce_avg" => jet_jit_math_f64_reduce_avg: sig_i64_f64;
    simd_f64x4_add: "jet_math_F64x4_add" => jet_jit_math_typed_add: sig_binary;
    simd_f64x4_sub: "jet_math_F64x4_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_f64x4_mul: "jet_math_F64x4_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_f64x4_div: "jet_math_F64x4_div" => jet_jit_math_typed_div: sig_binary;
    simd_i8x16_new: "jet_math_I8x16_new" => jet_jit_math_i8x16_new: sig_i64x16;
    simd_i8x16_splat: "jet_math_I8x16_splat" => jet_jit_math_i8x16_splat: sig_unary;
    simd_i8x16_from_array: "jet_math_I8x16_from_array" => jet_jit_math_i8x16_from_array: sig_unary;
    simd_i8x16_to_array: "jet_math_I8x16_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_i8x16_sum: "jet_math_I8x16_sum" => jet_jit_math_int_sum: sig_unary;
    simd_i8x16_product: "jet_math_I8x16_product" => jet_jit_math_int_product: sig_unary;
    simd_i8x16_min: "jet_math_I8x16_min" => jet_jit_math_int_min: sig_unary;
    simd_i8x16_max: "jet_math_I8x16_max" => jet_jit_math_int_max: sig_unary;
    simd_i8x16_reduce_add: "jet_math_I8x16_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_i8x16_reduce_mul: "jet_math_I8x16_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_i8x16_reduce_min: "jet_math_I8x16_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_i8x16_reduce_max: "jet_math_I8x16_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_i8x16_reduce_avg: "jet_math_I8x16_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_i8x16_add: "jet_math_I8x16_add" => jet_jit_math_typed_add: sig_binary;
    simd_i8x16_sub: "jet_math_I8x16_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_i8x16_mul: "jet_math_I8x16_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_i8x16_div: "jet_math_I8x16_div" => jet_jit_math_typed_div: sig_binary;
    simd_i16x8_new: "jet_math_I16x8_new" => jet_jit_math_i16x8_new: sig_i64x8;
    simd_i16x8_splat: "jet_math_I16x8_splat" => jet_jit_math_i16x8_splat: sig_unary;
    simd_i16x8_from_array: "jet_math_I16x8_from_array" => jet_jit_math_i16x8_from_array: sig_unary;
    simd_i16x8_to_array: "jet_math_I16x8_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_i16x8_sum: "jet_math_I16x8_sum" => jet_jit_math_int_sum: sig_unary;
    simd_i16x8_product: "jet_math_I16x8_product" => jet_jit_math_int_product: sig_unary;
    simd_i16x8_min: "jet_math_I16x8_min" => jet_jit_math_int_min: sig_unary;
    simd_i16x8_max: "jet_math_I16x8_max" => jet_jit_math_int_max: sig_unary;
    simd_i16x8_reduce_add: "jet_math_I16x8_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_i16x8_reduce_mul: "jet_math_I16x8_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_i16x8_reduce_min: "jet_math_I16x8_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_i16x8_reduce_max: "jet_math_I16x8_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_i16x8_reduce_avg: "jet_math_I16x8_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_i16x8_add: "jet_math_I16x8_add" => jet_jit_math_typed_add: sig_binary;
    simd_i16x8_sub: "jet_math_I16x8_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_i16x8_mul: "jet_math_I16x8_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_i16x8_div: "jet_math_I16x8_div" => jet_jit_math_typed_div: sig_binary;
    simd_i32x4_new: "jet_math_I32x4_new" => jet_jit_math_i32x4_new: sig_i64x4;
    simd_i32x4_splat: "jet_math_I32x4_splat" => jet_jit_math_i32x4_splat: sig_unary;
    simd_i32x4_from_array: "jet_math_I32x4_from_array" => jet_jit_math_i32x4_from_array: sig_unary;
    simd_i32x4_to_array: "jet_math_I32x4_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_i32x4_sum: "jet_math_I32x4_sum" => jet_jit_math_int_sum: sig_unary;
    simd_i32x4_product: "jet_math_I32x4_product" => jet_jit_math_int_product: sig_unary;
    simd_i32x4_min: "jet_math_I32x4_min" => jet_jit_math_int_min: sig_unary;
    simd_i32x4_max: "jet_math_I32x4_max" => jet_jit_math_int_max: sig_unary;
    simd_i32x4_reduce_add: "jet_math_I32x4_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_i32x4_reduce_mul: "jet_math_I32x4_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_i32x4_reduce_min: "jet_math_I32x4_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_i32x4_reduce_max: "jet_math_I32x4_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_i32x4_reduce_avg: "jet_math_I32x4_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_i32x4_add: "jet_math_I32x4_add" => jet_jit_math_typed_add: sig_binary;
    simd_i32x4_sub: "jet_math_I32x4_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_i32x4_mul: "jet_math_I32x4_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_i32x4_div: "jet_math_I32x4_div" => jet_jit_math_typed_div: sig_binary;
    simd_i64x2_new: "jet_math_I64x2_new" => jet_jit_math_i64x2_new: sig_i64x2;
    simd_i64x2_splat: "jet_math_I64x2_splat" => jet_jit_math_i64x2_splat: sig_unary;
    simd_i64x2_from_array: "jet_math_I64x2_from_array" => jet_jit_math_i64x2_from_array: sig_unary;
    simd_i64x2_to_array: "jet_math_I64x2_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_i64x2_sum: "jet_math_I64x2_sum" => jet_jit_math_int_sum: sig_unary;
    simd_i64x2_product: "jet_math_I64x2_product" => jet_jit_math_int_product: sig_unary;
    simd_i64x2_min: "jet_math_I64x2_min" => jet_jit_math_int_min: sig_unary;
    simd_i64x2_max: "jet_math_I64x2_max" => jet_jit_math_int_max: sig_unary;
    simd_i64x2_reduce_add: "jet_math_I64x2_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_i64x2_reduce_mul: "jet_math_I64x2_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_i64x2_reduce_min: "jet_math_I64x2_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_i64x2_reduce_max: "jet_math_I64x2_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_i64x2_reduce_avg: "jet_math_I64x2_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_i64x2_add: "jet_math_I64x2_add" => jet_jit_math_typed_add: sig_binary;
    simd_i64x2_sub: "jet_math_I64x2_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_i64x2_mul: "jet_math_I64x2_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_i64x2_div: "jet_math_I64x2_div" => jet_jit_math_typed_div: sig_binary;
    simd_u8x16_new: "jet_math_U8x16_new" => jet_jit_math_u8x16_new: sig_i64x16;
    simd_u8x16_splat: "jet_math_U8x16_splat" => jet_jit_math_u8x16_splat: sig_unary;
    simd_u8x16_from_array: "jet_math_U8x16_from_array" => jet_jit_math_u8x16_from_array: sig_unary;
    simd_u8x16_to_array: "jet_math_U8x16_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_u8x16_sum: "jet_math_U8x16_sum" => jet_jit_math_int_sum: sig_unary;
    simd_u8x16_product: "jet_math_U8x16_product" => jet_jit_math_int_product: sig_unary;
    simd_u8x16_min: "jet_math_U8x16_min" => jet_jit_math_int_min: sig_unary;
    simd_u8x16_max: "jet_math_U8x16_max" => jet_jit_math_int_max: sig_unary;
    simd_u8x16_reduce_add: "jet_math_U8x16_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_u8x16_reduce_mul: "jet_math_U8x16_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_u8x16_reduce_min: "jet_math_U8x16_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_u8x16_reduce_max: "jet_math_U8x16_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_u8x16_reduce_avg: "jet_math_U8x16_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_u8x16_add: "jet_math_U8x16_add" => jet_jit_math_typed_add: sig_binary;
    simd_u8x16_sub: "jet_math_U8x16_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_u8x16_mul: "jet_math_U8x16_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_u8x16_div: "jet_math_U8x16_div" => jet_jit_math_typed_div: sig_binary;
    simd_u16x8_new: "jet_math_U16x8_new" => jet_jit_math_u16x8_new: sig_i64x8;
    simd_u16x8_splat: "jet_math_U16x8_splat" => jet_jit_math_u16x8_splat: sig_unary;
    simd_u16x8_from_array: "jet_math_U16x8_from_array" => jet_jit_math_u16x8_from_array: sig_unary;
    simd_u16x8_to_array: "jet_math_U16x8_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_u16x8_sum: "jet_math_U16x8_sum" => jet_jit_math_int_sum: sig_unary;
    simd_u16x8_product: "jet_math_U16x8_product" => jet_jit_math_int_product: sig_unary;
    simd_u16x8_min: "jet_math_U16x8_min" => jet_jit_math_int_min: sig_unary;
    simd_u16x8_max: "jet_math_U16x8_max" => jet_jit_math_int_max: sig_unary;
    simd_u16x8_reduce_add: "jet_math_U16x8_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_u16x8_reduce_mul: "jet_math_U16x8_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_u16x8_reduce_min: "jet_math_U16x8_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_u16x8_reduce_max: "jet_math_U16x8_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_u16x8_reduce_avg: "jet_math_U16x8_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_u16x8_add: "jet_math_U16x8_add" => jet_jit_math_typed_add: sig_binary;
    simd_u16x8_sub: "jet_math_U16x8_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_u16x8_mul: "jet_math_U16x8_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_u16x8_div: "jet_math_U16x8_div" => jet_jit_math_typed_div: sig_binary;
    simd_u32x4_new: "jet_math_U32x4_new" => jet_jit_math_u32x4_new: sig_i64x4;
    simd_u32x4_splat: "jet_math_U32x4_splat" => jet_jit_math_u32x4_splat: sig_unary;
    simd_u32x4_from_array: "jet_math_U32x4_from_array" => jet_jit_math_u32x4_from_array: sig_unary;
    simd_u32x4_to_array: "jet_math_U32x4_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_u32x4_sum: "jet_math_U32x4_sum" => jet_jit_math_int_sum: sig_unary;
    simd_u32x4_product: "jet_math_U32x4_product" => jet_jit_math_int_product: sig_unary;
    simd_u32x4_min: "jet_math_U32x4_min" => jet_jit_math_int_min: sig_unary;
    simd_u32x4_max: "jet_math_U32x4_max" => jet_jit_math_int_max: sig_unary;
    simd_u32x4_reduce_add: "jet_math_U32x4_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_u32x4_reduce_mul: "jet_math_U32x4_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_u32x4_reduce_min: "jet_math_U32x4_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_u32x4_reduce_max: "jet_math_U32x4_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_u32x4_reduce_avg: "jet_math_U32x4_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_u32x4_add: "jet_math_U32x4_add" => jet_jit_math_typed_add: sig_binary;
    simd_u32x4_sub: "jet_math_U32x4_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_u32x4_mul: "jet_math_U32x4_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_u32x4_div: "jet_math_U32x4_div" => jet_jit_math_typed_div: sig_binary;
    simd_u64x2_new: "jet_math_U64x2_new" => jet_jit_math_u64x2_new: sig_i64x2;
    simd_u64x2_splat: "jet_math_U64x2_splat" => jet_jit_math_u64x2_splat: sig_unary;
    simd_u64x2_from_array: "jet_math_U64x2_from_array" => jet_jit_math_u64x2_from_array: sig_unary;
    simd_u64x2_to_array: "jet_math_U64x2_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_u64x2_sum: "jet_math_U64x2_sum" => jet_jit_math_int_sum: sig_unary;
    simd_u64x2_product: "jet_math_U64x2_product" => jet_jit_math_int_product: sig_unary;
    simd_u64x2_min: "jet_math_U64x2_min" => jet_jit_math_int_min: sig_unary;
    simd_u64x2_max: "jet_math_U64x2_max" => jet_jit_math_int_max: sig_unary;
    simd_u64x2_reduce_add: "jet_math_U64x2_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_u64x2_reduce_mul: "jet_math_U64x2_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_u64x2_reduce_min: "jet_math_U64x2_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_u64x2_reduce_max: "jet_math_U64x2_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_u64x2_reduce_avg: "jet_math_U64x2_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_u64x2_add: "jet_math_U64x2_add" => jet_jit_math_typed_add: sig_binary;
    simd_u64x2_sub: "jet_math_U64x2_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_u64x2_mul: "jet_math_U64x2_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_u64x2_div: "jet_math_U64x2_div" => jet_jit_math_typed_div: sig_binary;
    simd_i8x32_new: "jet_math_I8x32_new" => jet_jit_math_i8x32_new: sig_i64x32;
    simd_i8x32_splat: "jet_math_I8x32_splat" => jet_jit_math_i8x32_splat: sig_unary;
    simd_i8x32_from_array: "jet_math_I8x32_from_array" => jet_jit_math_i8x32_from_array: sig_unary;
    simd_i8x32_to_array: "jet_math_I8x32_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_i8x32_sum: "jet_math_I8x32_sum" => jet_jit_math_int_sum: sig_unary;
    simd_i8x32_product: "jet_math_I8x32_product" => jet_jit_math_int_product: sig_unary;
    simd_i8x32_min: "jet_math_I8x32_min" => jet_jit_math_int_min: sig_unary;
    simd_i8x32_max: "jet_math_I8x32_max" => jet_jit_math_int_max: sig_unary;
    simd_i8x32_reduce_add: "jet_math_I8x32_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_i8x32_reduce_mul: "jet_math_I8x32_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_i8x32_reduce_min: "jet_math_I8x32_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_i8x32_reduce_max: "jet_math_I8x32_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_i8x32_reduce_avg: "jet_math_I8x32_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_i8x32_add: "jet_math_I8x32_add" => jet_jit_math_typed_add: sig_binary;
    simd_i8x32_sub: "jet_math_I8x32_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_i8x32_mul: "jet_math_I8x32_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_i8x32_div: "jet_math_I8x32_div" => jet_jit_math_typed_div: sig_binary;
    simd_i16x16_new: "jet_math_I16x16_new" => jet_jit_math_i16x16_new: sig_i64x16;
    simd_i16x16_splat: "jet_math_I16x16_splat" => jet_jit_math_i16x16_splat: sig_unary;
    simd_i16x16_from_array: "jet_math_I16x16_from_array" => jet_jit_math_i16x16_from_array: sig_unary;
    simd_i16x16_to_array: "jet_math_I16x16_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_i16x16_sum: "jet_math_I16x16_sum" => jet_jit_math_int_sum: sig_unary;
    simd_i16x16_product: "jet_math_I16x16_product" => jet_jit_math_int_product: sig_unary;
    simd_i16x16_min: "jet_math_I16x16_min" => jet_jit_math_int_min: sig_unary;
    simd_i16x16_max: "jet_math_I16x16_max" => jet_jit_math_int_max: sig_unary;
    simd_i16x16_reduce_add: "jet_math_I16x16_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_i16x16_reduce_mul: "jet_math_I16x16_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_i16x16_reduce_min: "jet_math_I16x16_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_i16x16_reduce_max: "jet_math_I16x16_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_i16x16_reduce_avg: "jet_math_I16x16_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_i16x16_add: "jet_math_I16x16_add" => jet_jit_math_typed_add: sig_binary;
    simd_i16x16_sub: "jet_math_I16x16_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_i16x16_mul: "jet_math_I16x16_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_i16x16_div: "jet_math_I16x16_div" => jet_jit_math_typed_div: sig_binary;
    simd_i32x8_new: "jet_math_I32x8_new" => jet_jit_math_i32x8_new: sig_i64x8;
    simd_i32x8_splat: "jet_math_I32x8_splat" => jet_jit_math_i32x8_splat: sig_unary;
    simd_i32x8_from_array: "jet_math_I32x8_from_array" => jet_jit_math_i32x8_from_array: sig_unary;
    simd_i32x8_to_array: "jet_math_I32x8_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_i32x8_sum: "jet_math_I32x8_sum" => jet_jit_math_int_sum: sig_unary;
    simd_i32x8_product: "jet_math_I32x8_product" => jet_jit_math_int_product: sig_unary;
    simd_i32x8_min: "jet_math_I32x8_min" => jet_jit_math_int_min: sig_unary;
    simd_i32x8_max: "jet_math_I32x8_max" => jet_jit_math_int_max: sig_unary;
    simd_i32x8_reduce_add: "jet_math_I32x8_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_i32x8_reduce_mul: "jet_math_I32x8_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_i32x8_reduce_min: "jet_math_I32x8_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_i32x8_reduce_max: "jet_math_I32x8_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_i32x8_reduce_avg: "jet_math_I32x8_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_i32x8_add: "jet_math_I32x8_add" => jet_jit_math_typed_add: sig_binary;
    simd_i32x8_sub: "jet_math_I32x8_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_i32x8_mul: "jet_math_I32x8_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_i32x8_div: "jet_math_I32x8_div" => jet_jit_math_typed_div: sig_binary;
    simd_i64x4_new: "jet_math_I64x4_new" => jet_jit_math_i64x4_new: sig_i64x4;
    simd_i64x4_splat: "jet_math_I64x4_splat" => jet_jit_math_i64x4_splat: sig_unary;
    simd_i64x4_from_array: "jet_math_I64x4_from_array" => jet_jit_math_i64x4_from_array: sig_unary;
    simd_i64x4_to_array: "jet_math_I64x4_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_i64x4_sum: "jet_math_I64x4_sum" => jet_jit_math_int_sum: sig_unary;
    simd_i64x4_product: "jet_math_I64x4_product" => jet_jit_math_int_product: sig_unary;
    simd_i64x4_min: "jet_math_I64x4_min" => jet_jit_math_int_min: sig_unary;
    simd_i64x4_max: "jet_math_I64x4_max" => jet_jit_math_int_max: sig_unary;
    simd_i64x4_reduce_add: "jet_math_I64x4_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_i64x4_reduce_mul: "jet_math_I64x4_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_i64x4_reduce_min: "jet_math_I64x4_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_i64x4_reduce_max: "jet_math_I64x4_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_i64x4_reduce_avg: "jet_math_I64x4_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_i64x4_add: "jet_math_I64x4_add" => jet_jit_math_typed_add: sig_binary;
    simd_i64x4_sub: "jet_math_I64x4_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_i64x4_mul: "jet_math_I64x4_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_i64x4_div: "jet_math_I64x4_div" => jet_jit_math_typed_div: sig_binary;
    simd_u8x32_new: "jet_math_U8x32_new" => jet_jit_math_u8x32_new: sig_i64x32;
    simd_u8x32_splat: "jet_math_U8x32_splat" => jet_jit_math_u8x32_splat: sig_unary;
    simd_u8x32_from_array: "jet_math_U8x32_from_array" => jet_jit_math_u8x32_from_array: sig_unary;
    simd_u8x32_to_array: "jet_math_U8x32_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_u8x32_sum: "jet_math_U8x32_sum" => jet_jit_math_int_sum: sig_unary;
    simd_u8x32_product: "jet_math_U8x32_product" => jet_jit_math_int_product: sig_unary;
    simd_u8x32_min: "jet_math_U8x32_min" => jet_jit_math_int_min: sig_unary;
    simd_u8x32_max: "jet_math_U8x32_max" => jet_jit_math_int_max: sig_unary;
    simd_u8x32_reduce_add: "jet_math_U8x32_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_u8x32_reduce_mul: "jet_math_U8x32_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_u8x32_reduce_min: "jet_math_U8x32_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_u8x32_reduce_max: "jet_math_U8x32_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_u8x32_reduce_avg: "jet_math_U8x32_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_u8x32_add: "jet_math_U8x32_add" => jet_jit_math_typed_add: sig_binary;
    simd_u8x32_sub: "jet_math_U8x32_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_u8x32_mul: "jet_math_U8x32_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_u8x32_div: "jet_math_U8x32_div" => jet_jit_math_typed_div: sig_binary;
    simd_u16x16_new: "jet_math_U16x16_new" => jet_jit_math_u16x16_new: sig_i64x16;
    simd_u16x16_splat: "jet_math_U16x16_splat" => jet_jit_math_u16x16_splat: sig_unary;
    simd_u16x16_from_array: "jet_math_U16x16_from_array" => jet_jit_math_u16x16_from_array: sig_unary;
    simd_u16x16_to_array: "jet_math_U16x16_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_u16x16_sum: "jet_math_U16x16_sum" => jet_jit_math_int_sum: sig_unary;
    simd_u16x16_product: "jet_math_U16x16_product" => jet_jit_math_int_product: sig_unary;
    simd_u16x16_min: "jet_math_U16x16_min" => jet_jit_math_int_min: sig_unary;
    simd_u16x16_max: "jet_math_U16x16_max" => jet_jit_math_int_max: sig_unary;
    simd_u16x16_reduce_add: "jet_math_U16x16_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_u16x16_reduce_mul: "jet_math_U16x16_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_u16x16_reduce_min: "jet_math_U16x16_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_u16x16_reduce_max: "jet_math_U16x16_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_u16x16_reduce_avg: "jet_math_U16x16_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_u16x16_add: "jet_math_U16x16_add" => jet_jit_math_typed_add: sig_binary;
    simd_u16x16_sub: "jet_math_U16x16_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_u16x16_mul: "jet_math_U16x16_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_u16x16_div: "jet_math_U16x16_div" => jet_jit_math_typed_div: sig_binary;
    simd_u32x8_new: "jet_math_U32x8_new" => jet_jit_math_u32x8_new: sig_i64x8;
    simd_u32x8_splat: "jet_math_U32x8_splat" => jet_jit_math_u32x8_splat: sig_unary;
    simd_u32x8_from_array: "jet_math_U32x8_from_array" => jet_jit_math_u32x8_from_array: sig_unary;
    simd_u32x8_to_array: "jet_math_U32x8_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_u32x8_sum: "jet_math_U32x8_sum" => jet_jit_math_int_sum: sig_unary;
    simd_u32x8_product: "jet_math_U32x8_product" => jet_jit_math_int_product: sig_unary;
    simd_u32x8_min: "jet_math_U32x8_min" => jet_jit_math_int_min: sig_unary;
    simd_u32x8_max: "jet_math_U32x8_max" => jet_jit_math_int_max: sig_unary;
    simd_u32x8_reduce_add: "jet_math_U32x8_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_u32x8_reduce_mul: "jet_math_U32x8_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_u32x8_reduce_min: "jet_math_U32x8_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_u32x8_reduce_max: "jet_math_U32x8_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_u32x8_reduce_avg: "jet_math_U32x8_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_u32x8_add: "jet_math_U32x8_add" => jet_jit_math_typed_add: sig_binary;
    simd_u32x8_sub: "jet_math_U32x8_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_u32x8_mul: "jet_math_U32x8_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_u32x8_div: "jet_math_U32x8_div" => jet_jit_math_typed_div: sig_binary;
    simd_u64x4_new: "jet_math_U64x4_new" => jet_jit_math_u64x4_new: sig_i64x4;
    simd_u64x4_splat: "jet_math_U64x4_splat" => jet_jit_math_u64x4_splat: sig_unary;
    simd_u64x4_from_array: "jet_math_U64x4_from_array" => jet_jit_math_u64x4_from_array: sig_unary;
    simd_u64x4_to_array: "jet_math_U64x4_to_array" => jet_jit_math_typed_to_array: sig_unary;
    simd_u64x4_sum: "jet_math_U64x4_sum" => jet_jit_math_int_sum: sig_unary;
    simd_u64x4_product: "jet_math_U64x4_product" => jet_jit_math_int_product: sig_unary;
    simd_u64x4_min: "jet_math_U64x4_min" => jet_jit_math_int_min: sig_unary;
    simd_u64x4_max: "jet_math_U64x4_max" => jet_jit_math_int_max: sig_unary;
    simd_u64x4_reduce_add: "jet_math_U64x4_reduce_add" => jet_jit_math_int_reduce_add: sig_unary;
    simd_u64x4_reduce_mul: "jet_math_U64x4_reduce_mul" => jet_jit_math_int_reduce_mul: sig_unary;
    simd_u64x4_reduce_min: "jet_math_U64x4_reduce_min" => jet_jit_math_int_reduce_min: sig_unary;
    simd_u64x4_reduce_max: "jet_math_U64x4_reduce_max" => jet_jit_math_int_reduce_max: sig_unary;
    simd_u64x4_reduce_avg: "jet_math_U64x4_reduce_avg" => jet_jit_math_int_reduce_avg: sig_unary;
    simd_u64x4_add: "jet_math_U64x4_add" => jet_jit_math_typed_add: sig_binary;
    simd_u64x4_sub: "jet_math_U64x4_sub" => jet_jit_math_typed_sub: sig_binary;
    simd_u64x4_mul: "jet_math_U64x4_mul" => jet_jit_math_typed_mul: sig_binary;
    simd_u64x4_div: "jet_math_U64x4_div" => jet_jit_math_typed_div: sig_binary;
    vec2_new: "jet_math_Vec2_new" => jet_jit_math_vec2_new: sig_f64x2;
    vec2_splat: "jet_math_Vec2_splat" => jet_jit_math_vec2_splat: sig_f64_i64;
    vec2_from_array: "jet_math_Vec2_from_array" => jet_jit_math_vec2_from_array: sig_unary;
    vec2_to_array: "jet_math_Vec2_to_array" => jet_jit_math_typed_to_array: sig_unary;
    vec2_dot: "jet_math_Vec2_dot" => jet_jit_math_vec2_dot: sig_binary_f64;
    vec2_length: "jet_math_Vec2_length" => jet_jit_math_vec2_length: sig_unary_f64;
    vec2_normalize: "jet_math_Vec2_normalize" => jet_jit_math_vec2_normalize: sig_unary;
    vec2_add: "jet_math_Vec2_add" => jet_jit_math_typed_add: sig_binary;
    vec2_sub: "jet_math_Vec2_sub" => jet_jit_math_typed_sub: sig_binary;
    vec2_mul: "jet_math_Vec2_mul" => jet_jit_math_typed_mul: sig_binary;
    vec2_div: "jet_math_Vec2_div" => jet_jit_math_typed_div: sig_binary;
    vec3_new: "jet_math_Vec3_new" => jet_jit_math_vec3_new: sig_f64x3;
    vec3_splat: "jet_math_Vec3_splat" => jet_jit_math_vec3_splat: sig_f64_i64;
    vec3_from_array: "jet_math_Vec3_from_array" => jet_jit_math_vec3_from_array: sig_unary;
    vec3_to_array: "jet_math_Vec3_to_array" => jet_jit_math_typed_to_array: sig_unary;
    vec3_dot: "jet_math_Vec3_dot" => jet_jit_math_vec3_dot: sig_binary_f64;
    vec3_length: "jet_math_Vec3_length" => jet_jit_math_vec3_length: sig_unary_f64;
    vec3_normalize: "jet_math_Vec3_normalize" => jet_jit_math_vec3_normalize: sig_unary;
    vec3_cross: "jet_math_Vec3_cross" => jet_jit_math_vec3_cross: sig_binary;
    vec3_add: "jet_math_Vec3_add" => jet_jit_math_typed_add: sig_binary;
    vec3_sub: "jet_math_Vec3_sub" => jet_jit_math_typed_sub: sig_binary;
    vec3_hadamard_mul: "jet_math_Vec3_hadamard_mul" => jet_jit_math_typed_mul: sig_binary;
    vec3_mul_scalar: "jet_math_Vec3_mul" => jet_jit_math_vec3_mul_scalar: sig_handle_float;
    vec3_div_scalar: "jet_math_Vec3_div" => jet_jit_math_vec3_div_scalar: sig_handle_float;
    float_div_vec3: "jet_math_Float_div_Vec3" => jet_jit_math_float_div_vec3: sig_float_handle;
    float_mul_vec3: "jet_math_Float_mul_Vec3" => jet_jit_math_float_mul_vec3: sig_float_handle;
    vec4_new: "jet_math_Vec4_new" => jet_jit_math_vec4_new: sig_f64x4;
    vec4_splat: "jet_math_Vec4_splat" => jet_jit_math_vec4_splat: sig_f64_i64;
    vec4_from_array: "jet_math_Vec4_from_array" => jet_jit_math_vec4_from_array: sig_unary;
    vec4_to_array: "jet_math_Vec4_to_array" => jet_jit_math_typed_to_array: sig_unary;
    vec4_dot: "jet_math_Vec4_dot" => jet_jit_math_vec4_dot: sig_binary_f64;
    vec4_length: "jet_math_Vec4_length" => jet_jit_math_vec4_length: sig_unary_f64;
    vec4_normalize: "jet_math_Vec4_normalize" => jet_jit_math_vec4_normalize: sig_unary;
    vec4_add: "jet_math_Vec4_add" => jet_jit_math_typed_add: sig_binary;
    vec4_sub: "jet_math_Vec4_sub" => jet_jit_math_typed_sub: sig_binary;
    vec4_mul: "jet_math_Vec4_mul" => jet_jit_math_typed_mul: sig_binary;
    vec4_div: "jet_math_Vec4_div" => jet_jit_math_typed_div: sig_binary;
    mat3_new: "jet_math_Mat3_new" => jet_jit_math_mat3_new: sig_f64x9;
    mat3_splat: "jet_math_Mat3_splat" => jet_jit_math_mat3_splat: sig_f64_i64;
    mat3_from_array: "jet_math_Mat3_from_array" => jet_jit_math_mat3_from_array: sig_unary;
    mat3_to_array: "jet_math_Mat3_to_array" => jet_jit_math_typed_to_array: sig_unary;
    mat3_matmul: "jet_math_Mat3_matmul" => jet_jit_math_mat3_matmul: sig_binary;
    mat3_transform: "jet_math_Mat3_transform" => jet_jit_math_mat3_transform: sig_binary;
    mat3_transpose: "jet_math_Mat3_transpose" => jet_jit_math_mat3_transpose: sig_unary;
    mat3_add: "jet_math_Mat3_add" => jet_jit_math_typed_add: sig_binary;
    mat3_sub: "jet_math_Mat3_sub" => jet_jit_math_typed_sub: sig_binary;
    mat3_mul: "jet_math_Mat3_mul" => jet_jit_math_typed_mul: sig_binary;
    mat4_new: "jet_math_Mat4_new" => jet_jit_math_mat4_new: sig_f64x16;
    mat4_splat: "jet_math_Mat4_splat" => jet_jit_math_mat4_splat: sig_f64_i64;
    mat4_from_array: "jet_math_Mat4_from_array" => jet_jit_math_mat4_from_array: sig_unary;
    mat4_to_array: "jet_math_Mat4_to_array" => jet_jit_math_typed_to_array: sig_unary;
    mat4_matmul: "jet_math_Mat4_matmul" => jet_jit_math_mat4_matmul: sig_binary;
    mat4_transform: "jet_math_Mat4_transform" => jet_jit_math_mat4_transform: sig_binary;
    mat4_transpose: "jet_math_Mat4_transpose" => jet_jit_math_mat4_transpose: sig_unary;
    mat4_add: "jet_math_Mat4_add" => jet_jit_math_typed_add: sig_binary;
    mat4_sub: "jet_math_Mat4_sub" => jet_jit_math_typed_sub: sig_binary;
    mat4_mul: "jet_math_Mat4_mul" => jet_jit_math_typed_mul: sig_binary;
    lane_f32x4: "jet_math_F32x4_lane" => jet_jit_math_lane_f32: sig_lane_f32;
    lane_f64x2: "jet_math_F64x2_lane" => jet_jit_math_lane_f64: sig_lane_f64;
    lane_f32x8: "jet_math_F32x8_lane" => jet_jit_math_lane_f32: sig_lane_f32;
    lane_f64x4: "jet_math_F64x4_lane" => jet_jit_math_lane_f64: sig_lane_f64;
    lane_i8x16: "jet_math_I8x16_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_i16x8: "jet_math_I16x8_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_i32x4: "jet_math_I32x4_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_i64x2: "jet_math_I64x2_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_u8x16: "jet_math_U8x16_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_u16x8: "jet_math_U16x8_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_u32x4: "jet_math_U32x4_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_u64x2: "jet_math_U64x2_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_i8x32: "jet_math_I8x32_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_i16x16: "jet_math_I16x16_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_i32x8: "jet_math_I32x8_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_i64x4: "jet_math_I64x4_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_u8x32: "jet_math_U8x32_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_u16x16: "jet_math_U16x16_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_u32x8: "jet_math_U32x8_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    lane_u64x4: "jet_math_U64x4_lane" => jet_jit_math_lane_i64: sig_lane_i64;
    reduce: "jet_jit_math_reduce" => jet_jit_math_reduce: sig_binary;
    dot: "jet_jit_math_dot" => jet_jit_math_dot: sig_binary;
    length: "jet_jit_math_length" => jet_jit_math_length: sig_unary;
    result_is_float: "jet_jit_math_result_is_float" => jet_jit_math_result_is_float: sig_i64_i8;
    result_float: "jet_jit_math_result_float" => jet_jit_math_result_float: sig_i64_f64;
    geometry_screen_point_new: "jet_math_ScreenPoint_new" => jet_jit_geometry_coord_new: sig_float_binary;
    geometry_world_point_new: "jet_math_WorldPoint_new" => jet_jit_geometry_coord_new: sig_float_binary;
    geometry_view_point_new: "jet_math_ViewPoint_new" => jet_jit_geometry_coord_new: sig_float_binary;
    geometry_camera_point_new: "jet_math_CameraPoint_new" => jet_jit_geometry_coord_new: sig_float_binary;
    geometry_device_point_new: "jet_math_DevicePoint_new" => jet_jit_geometry_coord_new: sig_float_binary;
    geometry_screen_delta_new: "jet_math_ScreenDelta_new" => jet_jit_geometry_coord_new: sig_float_binary;
    geometry_world_delta_new: "jet_math_WorldDelta_new" => jet_jit_geometry_coord_new: sig_float_binary;
    geometry_view_delta_new: "jet_math_ViewDelta_new" => jet_jit_geometry_coord_new: sig_float_binary;
    geometry_camera_delta_new: "jet_math_CameraDelta_new" => jet_jit_geometry_coord_new: sig_float_binary;
    geometry_device_delta_new: "jet_math_DeviceDelta_new" => jet_jit_geometry_coord_new: sig_float_binary;
    geometry_point2_new: "jet_math_Point2_new" => jet_jit_geometry_coord_new_frame: sig_float_float_int;
    geometry_delta2_new: "jet_math_Delta2_new" => jet_jit_geometry_coord_new_frame: sig_float_float_int;
    geometry_screen_point_add: "jet_math_ScreenPoint_add" => jet_jit_geometry_coord_plain_add: sig_binary;
    geometry_screen_point_sub: "jet_math_ScreenPoint_sub" => jet_jit_geometry_coord_plain_sub: sig_binary;
    geometry_world_point_add: "jet_math_WorldPoint_add" => jet_jit_geometry_coord_plain_add: sig_binary;
    geometry_world_point_sub: "jet_math_WorldPoint_sub" => jet_jit_geometry_coord_plain_sub: sig_binary;
    geometry_view_point_add: "jet_math_ViewPoint_add" => jet_jit_geometry_coord_plain_add: sig_binary;
    geometry_view_point_sub: "jet_math_ViewPoint_sub" => jet_jit_geometry_coord_plain_sub: sig_binary;
    geometry_camera_point_add: "jet_math_CameraPoint_add" => jet_jit_geometry_coord_plain_add: sig_binary;
    geometry_camera_point_sub: "jet_math_CameraPoint_sub" => jet_jit_geometry_coord_plain_sub: sig_binary;
    geometry_device_point_add: "jet_math_DevicePoint_add" => jet_jit_geometry_coord_plain_add: sig_binary;
    geometry_device_point_sub: "jet_math_DevicePoint_sub" => jet_jit_geometry_coord_plain_sub: sig_binary;
    geometry_screen_delta_add: "jet_math_ScreenDelta_add" => jet_jit_geometry_coord_plain_add: sig_binary;
    geometry_screen_delta_sub: "jet_math_ScreenDelta_sub" => jet_jit_geometry_coord_plain_sub: sig_binary;
    geometry_world_delta_add: "jet_math_WorldDelta_add" => jet_jit_geometry_coord_plain_add: sig_binary;
    geometry_world_delta_sub: "jet_math_WorldDelta_sub" => jet_jit_geometry_coord_plain_sub: sig_binary;
    geometry_view_delta_add: "jet_math_ViewDelta_add" => jet_jit_geometry_coord_plain_add: sig_binary;
    geometry_view_delta_sub: "jet_math_ViewDelta_sub" => jet_jit_geometry_coord_plain_sub: sig_binary;
    geometry_camera_delta_add: "jet_math_CameraDelta_add" => jet_jit_geometry_coord_plain_add: sig_binary;
    geometry_camera_delta_sub: "jet_math_CameraDelta_sub" => jet_jit_geometry_coord_plain_sub: sig_binary;
    geometry_device_delta_add: "jet_math_DeviceDelta_add" => jet_jit_geometry_coord_plain_add: sig_binary;
    geometry_device_delta_sub: "jet_math_DeviceDelta_sub" => jet_jit_geometry_coord_plain_sub: sig_binary;
    geometry_point2_add: "jet_math_Point2_add" => jet_jit_geometry_coord_add: sig_binary;
    geometry_point2_sub: "jet_math_Point2_sub" => jet_jit_geometry_coord_sub: sig_binary;
    geometry_delta2_add: "jet_math_Delta2_add" => jet_jit_geometry_coord_add: sig_binary;
    geometry_delta2_sub: "jet_math_Delta2_sub" => jet_jit_geometry_coord_sub: sig_binary;
    geometry_transform_affine: "jet_math_Transform_affine" => jet_jit_geometry_transform_new: sig_transform_new;
    geometry_transform2_affine: "jet_math_Transform2_affine" => jet_jit_geometry_transform_new: sig_transform_new;
    geometry_transform_then: "jet_math_Transform_then" => jet_jit_geometry_transform_then: sig_binary;
    geometry_transform2_then: "jet_math_Transform2_then" => jet_jit_geometry_transform_then: sig_binary;
    geometry_transform_inverse: "jet_math_Transform_inverse" => jet_jit_geometry_transform_inverse: sig_unary;
    geometry_transform2_inverse: "jet_math_Transform2_inverse" => jet_jit_geometry_transform_inverse: sig_unary;
    geometry_transform_point: "jet_math_Transform_point" => jet_jit_geometry_transform_point: sig_binary;
    geometry_transform2_point: "jet_math_Transform2_point" => jet_jit_geometry_transform_point: sig_binary;
    geometry_transform_point_at_depth: "jet_math_Transform_point_at_depth" => jet_jit_geometry_transform_point_at_depth: sig_handle_handle_float;
    geometry_transform2_point_at_depth: "jet_math_Transform2_point_at_depth" => jet_jit_geometry_transform_point_at_depth: sig_handle_handle_float;
    geometry_transform_ray: "jet_math_Transform_ray" => jet_jit_geometry_transform_ray: sig_binary;
    geometry_transform2_ray: "jet_math_Transform2_ray" => jet_jit_geometry_transform_ray: sig_binary;
    result_int: "jet_jit_math_result_int" => jet_jit_math_result_int: sig_unary;
    result_handle: "jet_jit_math_result_handle" => jet_jit_math_result_handle: sig_unary;
    html_escape: "jet_jit_html_escape" => jet_jit_html_escape: sig_unary;
    str_concat: "jet_jit_str_concat" => jet_jit_str_concat: sig_binary;
    typed_sql_raw: "jet_jit_typed_sql_raw" => jet_jit_typed_sql_raw: sig_unary;
    typed_sql_interp: "jet_jit_typed_sql_interpolate" => jet_jit_typed_sql_interpolate: sig_binary;
    typed_sql_interp_canonical: "jet_typed_sql_interpolate" => jet_jit_typed_sql_interpolate: sig_binary;
    typed_sql_template: "jet_jit_typed_sql_template" => jet_jit_typed_sql_template: sig_unary;
    typed_sql_params: "jet_jit_typed_sql_params" => jet_jit_typed_sql_params: sig_unary;
    typed_sh_raw: "jet_jit_typed_sh_raw" => jet_jit_typed_sh_raw: sig_unary;
    typed_sh_interp: "jet_jit_typed_sh_interpolate" => jet_jit_typed_sh_interpolate: sig_binary;
    typed_sh_interp_canonical: "jet_typed_sh_interpolate" => jet_jit_typed_sh_interpolate: sig_binary;
    typed_html_raw: "jet_jit_typed_html_raw" => jet_jit_typed_html_raw: sig_unary;
    typed_html_text: "jet_jit_typed_html_text" => jet_jit_typed_html_text: sig_unary;
    typed_html_interp: "jet_jit_typed_html_interpolate" => jet_jit_typed_html_interpolate: sig_ternary;
    typed_html_interp_canonical: "jet_typed_html_interpolate" => jet_jit_typed_html_interpolate: sig_ternary;
    typed_path_interp: "jet_jit_typed_path_interpolate" => jet_jit_typed_path_interpolate: sig_binary;
    typed_path_interp_canonical: "jet_typed_path_literal" => jet_jit_typed_path_interpolate: sig_binary;
    typed_datetime_interp: "jet_jit_typed_datetime_interpolate" => jet_jit_typed_datetime_interpolate: sig_binary;
    typed_datetime_interp_canonical: "jet_typed_datetime_literal" => jet_jit_typed_datetime_literal: sig_binary;
}

#[cfg(test)]
mod shared_simd_tests {
    use super::simd_lanes;

    #[test]
    fn f64_binary_slice_handles_vec2_lanes() {
        let result = simd_lanes::jet_simd_f64_binary_slice(
            &[1.0, 2.0],
            &[3.0, 4.0],
            simd_lanes::JetSimdBinaryOp::Add,
        );
        assert_eq!(result, Some(vec![4.0, 6.0]));
    }

    #[test]
    fn f64_binary_slice_handles_vec3_lanes() {
        let result = simd_lanes::jet_simd_f64_binary_slice(
            &[1.0, 2.0, 3.0],
            &[4.0, 5.0, 6.0],
            simd_lanes::JetSimdBinaryOp::Add,
        );
        assert_eq!(result, Some(vec![5.0, 7.0, 9.0]));
    }

    #[test]
    fn f32_binary_slice_handles_vec3_lanes() {
        let result = simd_lanes::jet_simd_f32_binary_slice(
            &[1.0, 2.0, 3.0],
            &[4.0, 5.0, 6.0],
            simd_lanes::JetSimdBinaryOp::Add,
        );
        assert_eq!(result, Some(vec![5.0, 7.0, 9.0]));
    }
}
