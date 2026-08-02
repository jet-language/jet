//! D-SIMD2 / D-LINALG1: math-value host shims for the Cranelift JIT.
//! Lane/matrix layouts match `MathTaskMem` (`[f32;4]` / column-major F64). Host
//! ops live here so the include fragment's `JetShow`/`Shared` deps stay out.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::cell::RefCell;

mod typed_text_semantics {
    include!("../../jet-codegen/src/Prelude/TypedText.rs");
}

#[derive(Clone, Copy)]
struct F32x4([f32; 4]);
#[derive(Clone, Copy)]
struct F64x2([f64; 2]);
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
    F32x4(F32x4),
    F64x2(F64x2),
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

fn clone_string(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}

fn alloc_string(s: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
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

fn lanes_of(v: MathVal) -> Vec<f64> {
    match v {
        MathVal::F32x4(x) => x.0.iter().map(|n| f64::from(*n)).collect(),
        MathVal::F64x2(x) => x.0.to_vec(),
        MathVal::Vec2(x) => x.0.to_vec(),
        MathVal::Vec3(x) => x.0.to_vec(),
        MathVal::Vec4(x) => x.0.to_vec(),
        MathVal::Mat3(x) => x.0.to_vec(),
        MathVal::Mat4(x) => x.0.to_vec(),
    }
}

fn from_lanes(type_name: &str, lanes: &[f64]) -> Option<MathVal> {
    match type_name {
        "F32x4" if lanes.len() == 4 => Some(MathVal::F32x4(F32x4([
            lanes[0] as f32,
            lanes[1] as f32,
            lanes[2] as f32,
            lanes[3] as f32,
        ]))),
        "F64x2" if lanes.len() == 2 => Some(MathVal::F64x2(F64x2([lanes[0], lanes[1]]))),
        "Vec2" if lanes.len() == 2 => Some(MathVal::Vec2(Vec2([lanes[0], lanes[1]]))),
        "Vec3" if lanes.len() == 3 => Some(MathVal::Vec3(Vec3([lanes[0], lanes[1], lanes[2]]))),
        "Vec4" if lanes.len() == 4 => {
            Some(MathVal::Vec4(Vec4([lanes[0], lanes[1], lanes[2], lanes[3]])))
        }
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

fn type_name_of(v: MathVal) -> &'static str {
    match v {
        MathVal::F32x4(_) => "F32x4",
        MathVal::F64x2(_) => "F64x2",
        MathVal::Vec2(_) => "Vec2",
        MathVal::Vec3(_) => "Vec3",
        MathVal::Vec4(_) => "Vec4",
        MathVal::Mat3(_) => "Mat3",
        MathVal::Mat4(_) => "Mat4",
    }
}

/// Pack: bit0 = is_float_result; remaining bits = f64 bits or math handle.
fn pack_float(x: f64) -> i64 {
    (1i64 << 63) | (f64_bits(x) & !(1i64 << 63))
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

fn unpack_handle(p: i64) -> i64 {
    p & !(1i64 << 63)
}

fn zip_binop(op: &str, a: &[f64], b: &[f64], f32_lanes: bool) -> Option<Vec<f64>> {
    if a.len() != b.len() {
        return None;
    }
    let mut out = Vec::with_capacity(a.len());
    for (l, r) in a.iter().zip(b.iter()) {
        let n = match op {
            "add" => l + r,
            "sub" => l - r,
            "mul" => l * r,
            "div" => l / r,
            _ => return None,
        };
        out.push(if f32_lanes { (n as f32) as f64 } else { n });
    }
    Some(out)
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
    if lanes.is_empty() {
        return None;
    }
    let acc = match op {
        "Add" | "sum" => lanes.iter().sum(),
        "Mul" | "product" => lanes.iter().copied().product(),
        "Min" => lanes.iter().copied().fold(f64::INFINITY, f64::min),
        "Max" => lanes.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        "Avg" => lanes.iter().sum::<f64>() / lanes.len() as f64,
        _ => return None,
    };
    Some(if f32_lanes { (acc as f32) as f64 } else { acc })
}

fn trap(msg: &str) {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_trap(msg);
    });
}

/// `type_name`/`func` are string handles. `args` is a list of i64:
/// - for scalar float args: f64 bits
/// - for math-value args: math handles
/// - for array args (`from_array`): list handle of f64
/// Returns packed float or math handle.
extern "C" fn jet_jit_math_call(type_name: i64, func: i64, args: i64) -> i64 {
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

    let f32_lanes = ty == "F32x4";
    let result = match (ty.as_str(), func.as_str()) {
        (_, "new") => {
            let lanes: Vec<f64> = argv.iter().map(|b| bits_f64(*b)).collect();
            from_lanes(&ty, &lanes).map(|v| pack_handle(push_val(v)))
        }
        (_, "splat") if argv.len() == 1 => {
            let v = bits_f64(argv[0]);
            let n = match ty.as_str() {
                "F32x4" => 4,
                "F64x2" | "Vec2" => 2,
                "Vec3" => 3,
                "Vec4" => 4,
                _ => 0,
            };
            let lanes = vec![if f32_lanes { (v as f32) as f64 } else { v }; n];
            from_lanes(&ty, &lanes).map(|v| pack_handle(push_val(v)))
        }
        (_, "from_array") if argv.len() == 1 => {
            let lanes = list_f64s(argv[0]);
            let lanes = if f32_lanes {
                lanes.into_iter().map(|v| (v as f32) as f64).collect()
            } else {
                lanes
            };
            from_lanes(&ty, &lanes).map(|v| pack_handle(push_val(v)))
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
            // MatN * VecN
            if func == "mul" {
                match (a, b) {
                    (MathVal::Mat3(m), MathVal::Vec3(v)) => {
                        let out = mat_vec(3, &m.0, &v.0);
                        return pack_handle(push_val(from_lanes("Vec3", &out).unwrap()));
                    }
                    (MathVal::Mat4(m), MathVal::Vec4(v)) => {
                        let out = mat_vec(4, &m.0, &v.0);
                        return pack_handle(push_val(from_lanes("Vec4", &out).unwrap()));
                    }
                    _ => {}
                }
            }
            let la = lanes_of(a);
            let lb = lanes_of(b);
            let f32 = matches!(a, MathVal::F32x4(_));
            let Some(out) = zip_binop(&func, &la, &lb, f32) else {
                trap("math binary size mismatch");
                return 0;
            };
            from_lanes(type_name_of(a), &out).map(|v| pack_handle(push_val(v)))
        }
        (_, "to_array") if argv.len() == 1 => {
            let Some(v) = take_val(argv[0]) else {
                trap("to_array: bad recv");
                return 0;
            };
            Some(pack_handle(alloc_f64_list(&lanes_of(v))))
        }
        (_, "sum" | "product" | "min" | "max" | "length") if argv.len() == 1 => {
            let Some(v) = take_val(argv[0]) else {
                trap("math unary: bad recv");
                return 0;
            };
            let lanes = lanes_of(v);
            let f32 = matches!(v, MathVal::F32x4(_));
            let n = match func.as_str() {
                "length" => {
                    let acc: f64 = lanes.iter().map(|n| n * n).sum();
                    acc.sqrt()
                }
                other => reduce_op(&lanes, other, f32).unwrap_or(0.0),
            };
            Some(pack_float(n))
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
            if la.len() != lb.len() {
                trap("dot size mismatch");
                return 0;
            }
            let mut acc = 0.0f64;
            for (x, y) in la.iter().zip(lb.iter()) {
                acc += x * y;
            }
            Some(pack_float(acc))
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
            let f32 = matches!(v, MathVal::F32x4(_));
            let Some(n) = reduce_op(&lanes_of(v), &op, f32) else {
                trap(&format!("reduce({op})"));
                return 0;
            };
            Some(pack_float(n))
        }
        (_, "lane") if argv.len() == 2 => {
            let Some(v) = take_val(argv[0]) else {
                trap("lane: bad recv");
                return 0;
            };
            let idx = argv[1];
            let lanes = lanes_of(v);
            if idx < 0 || idx as usize >= lanes.len() {
                trap(&format!(
                    "lane index {idx} out of range for {} ({} lanes)",
                    type_name_of(v),
                    lanes.len()
                ));
                return 0;
            }
            Some(pack_float(lanes[idx as usize]))
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
                if f32_lanes && ty != "F32x4" {
                    // F32x4 → VecN promotes to f64
                }
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
                let result_ty = if (ty == "F32x4" && out.len() == 4)
                    || (ty == "F64x2" && out.len() == 2)
                {
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
                cur[lane] = if matches!(base, MathVal::F32x4(_)) {
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
                    if matches!(base, MathVal::F32x4(_)) {
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

extern "C" fn jet_jit_math_result_is_float(packed: i64) -> i8 {
    i8::from(is_float_pack(packed))
}

extern "C" fn jet_jit_math_result_float(packed: i64) -> f64 {
    unpack_float(packed)
}

extern "C" fn jet_jit_math_result_handle(packed: i64) -> i64 {
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
        Concurrency::with_runtime_mut(|rt| rt.set_trap("typed-text list contains a non-string value"));
    }
    values
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

extern "C" fn jet_jit_typed_sql_raw(s: i64) -> i64 {
    alloc_sql_value(typed_text_semantics::jet_typed_sql_raw(clone_string(s)))
}

extern "C" fn jet_jit_typed_sql_interpolate(literals: i64, holes: i64) -> i64 {
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

extern "C" fn jet_jit_typed_sql_template(value: i64) -> i64 {
    let Some(value) = clone_sql_value(value) else {
        trap("typed-text SQL value is malformed");
        return 0;
    };
    alloc_string(typed_text_semantics::jet_typed_sql_template(&value))
}

extern "C" fn jet_jit_typed_sql_params(value: i64) -> i64 {
    let Some(value) = clone_sql_value(value) else {
        trap("typed-text SQL value is malformed");
        return 0;
    };
    alloc_string_list(typed_text_semantics::jet_typed_sql_params(&value))
}

extern "C" fn jet_jit_typed_sh_raw(s: i64) -> i64 {
    alloc_string_list(typed_text_semantics::jet_typed_sh_raw(clone_string(s)))
}

extern "C" fn jet_jit_typed_sh_interpolate(literals: i64, holes: i64) -> i64 {
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

extern "C" fn jet_jit_typed_html_interpolate(literals: i64, holes: i64) -> i64 {
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

extern "C" fn jet_jit_typed_html_raw(value: i64) -> i64 {
    alloc_string(typed_text_semantics::jet_typed_html_raw(clone_string(value)))
}

extern "C" fn jet_jit_typed_html_text(value: i64) -> i64 {
    alloc_string(typed_text_semantics::jet_typed_html_text(clone_string(value)))
}

extern "C" fn jet_jit_html_escape(s: i64) -> i64 {
    alloc_string(typed_text_semantics::jet_typed_html_escape(&clone_string(s)))
}

extern "C" fn jet_jit_str_concat(a: i64, b: i64) -> i64 {
    let mut s = clone_string(a);
    s.push_str(&clone_string(b));
    alloc_string(s)
}

pub(crate) fn clear_math_values() {
    MATH_VALUES.with(|slot| slot.borrow_mut().clear());
}

pub(crate) struct MathHostFns {
    pub call: FuncId,
    pub result_is_float: FuncId,
    pub result_float: FuncId,
    pub result_handle: FuncId,
    pub html_escape: FuncId,
    pub str_concat: FuncId,
    pub typed_sql_raw: FuncId,
    pub typed_sql_interp: FuncId,
    pub typed_sql_template: FuncId,
    pub typed_sql_params: FuncId,
    pub typed_sh_raw: FuncId,
    pub typed_sh_interp: FuncId,
    pub typed_html_raw: FuncId,
    pub typed_html_text: FuncId,
    pub typed_html_interp: FuncId,
}

pub(crate) fn register_math_host_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_math_call", jet_jit_math_call as *const u8);
    builder.symbol(
        "jet_jit_math_result_is_float",
        jet_jit_math_result_is_float as *const u8,
    );
    builder.symbol(
        "jet_jit_math_result_float",
        jet_jit_math_result_float as *const u8,
    );
    builder.symbol(
        "jet_jit_math_result_handle",
        jet_jit_math_result_handle as *const u8,
    );
    builder.symbol("jet_jit_html_escape", jet_jit_html_escape as *const u8);
    builder.symbol("jet_jit_str_concat", jet_jit_str_concat as *const u8);
    builder.symbol("jet_jit_typed_sql_raw", jet_jit_typed_sql_raw as *const u8);
    builder.symbol(
        "jet_jit_typed_sql_interpolate",
        jet_jit_typed_sql_interpolate as *const u8,
    );
    builder.symbol(
        "jet_jit_typed_sql_template",
        jet_jit_typed_sql_template as *const u8,
    );
    builder.symbol(
        "jet_jit_typed_sql_params",
        jet_jit_typed_sql_params as *const u8,
    );
    builder.symbol("jet_jit_typed_sh_raw", jet_jit_typed_sh_raw as *const u8);
    builder.symbol(
        "jet_jit_typed_sh_interpolate",
        jet_jit_typed_sh_interpolate as *const u8,
    );
    builder.symbol(
        "jet_jit_typed_html_raw",
        jet_jit_typed_html_raw as *const u8,
    );
    builder.symbol(
        "jet_jit_typed_html_text",
        jet_jit_typed_html_text as *const u8,
    );
    builder.symbol(
        "jet_jit_typed_html_interpolate",
        jet_jit_typed_html_interpolate as *const u8,
    );
}

pub(crate) fn declare_math_host_fns(module: &mut JITModule) -> Result<MathHostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut import = |name: &str, sig: &Signature| -> Result<FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
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
    Ok(MathHostFns {
        call: import("jet_jit_math_call", &sig_call)?,
        result_is_float: import("jet_jit_math_result_is_float", &sig_i64_i8)?,
        result_float: import("jet_jit_math_result_float", &sig_i64_f64)?,
        result_handle: import("jet_jit_math_result_handle", &sig_unary)?,
        html_escape: import("jet_jit_html_escape", &sig_unary)?,
        str_concat: import("jet_jit_str_concat", &sig_binary)?,
        typed_sql_raw: import("jet_jit_typed_sql_raw", &sig_unary)?,
        typed_sql_interp: import("jet_jit_typed_sql_interpolate", &sig_binary)?,
        typed_sql_template: import("jet_jit_typed_sql_template", &sig_unary)?,
        typed_sql_params: import("jet_jit_typed_sql_params", &sig_unary)?,
        typed_sh_raw: import("jet_jit_typed_sh_raw", &sig_unary)?,
        typed_sh_interp: import("jet_jit_typed_sh_interpolate", &sig_binary)?,
        typed_html_raw: import("jet_jit_typed_html_raw", &sig_unary)?,
        typed_html_text: import("jet_jit_typed_html_text", &sig_unary)?,
        typed_html_interp: import("jet_jit_typed_html_interpolate", &sig_binary)?,
    })
}
