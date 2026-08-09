//! `core.fmt` host shims (#729). The formatting rules live in the shared
//! Prelude kernel; this file only marshals JIT strings.

use crate::Marshal::{alloc_string, clone_string};

mod fmt_rt {
    include!("../../jet-codegen/src/Prelude/Core/Fmt.rs");
}

extern "C" fn jet_jit_fmt_number(value: i64) -> i64 {
    alloc_string(fmt_rt::jet_fmt_number(value))
}

extern "C" fn jet_jit_fmt_decimal(value: f64, precision: i64) -> i64 {
    alloc_string(fmt_rt::jet_fmt_decimal(value, precision))
}

extern "C" fn jet_jit_fmt_percent(value: f64, precision: i64) -> i64 {
    alloc_string(fmt_rt::jet_fmt_percent(value, precision))
}

extern "C" fn jet_jit_fmt_bytes(value: i64) -> i64 {
    alloc_string(fmt_rt::jet_fmt_bytes(value))
}

extern "C" fn jet_jit_fmt_duration(ms: i64) -> i64 {
    alloc_string(fmt_rt::jet_fmt_duration(ms))
}

extern "C" fn jet_jit_fmt_ordinal(value: i64) -> i64 {
    alloc_string(fmt_rt::jet_fmt_ordinal(value))
}

extern "C" fn jet_jit_fmt_plural(count: i64, singular: i64, plural: i64) -> i64 {
    let singular = clone_string(singular);
    let plural = clone_string(plural);
    alloc_string(fmt_rt::jet_fmt_plural(count, &singular, &plural))
}

extern "C" fn jet_jit_fmt_pad_left(text: i64, width: i64, fill: i64) -> i64 {
    let text = clone_string(text);
    let fill = clone_string(fill);
    alloc_string(fmt_rt::jet_fmt_pad_left(&text, width, &fill))
}

extern "C" fn jet_jit_fmt_pad_right(text: i64, width: i64, fill: i64) -> i64 {
    let text = clone_string(text);
    let fill = clone_string(fill);
    alloc_string(fmt_rt::jet_fmt_pad_right(&text, width, &fill))
}

extern "C" fn jet_jit_fmt_pad_center(text: i64, width: i64, fill: i64) -> i64 {
    let text = clone_string(text);
    let fill = clone_string(fill);
    alloc_string(fmt_rt::jet_fmt_pad_center(&text, width, &fill))
}

host_fns! {
    struct FmtHostFns;
    register: register_fmt_symbols;
    declare: declare_fmt_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;
        let mut sig_i64 = Signature::new(cc);
        sig_i64.params.push(AbiParam::new(types::I64));
        sig_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_f64_i64 = Signature::new(cc);
        sig_f64_i64.params.push(AbiParam::new(types::F64));
        sig_f64_i64.params.push(AbiParam::new(types::I64));
        sig_f64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_i64x3 = Signature::new(cc);
        sig_i64x3.params.push(AbiParam::new(types::I64));
        sig_i64x3.params.push(AbiParam::new(types::I64));
        sig_i64x3.params.push(AbiParam::new(types::I64));
        sig_i64x3.returns.push(AbiParam::new(types::I64));
    }
    number: "jet_jit_fmt_number" => jet_jit_fmt_number: sig_i64;
    decimal: "jet_jit_fmt_decimal" => jet_jit_fmt_decimal: sig_f64_i64;
    percent: "jet_jit_fmt_percent" => jet_jit_fmt_percent: sig_f64_i64;
    bytes: "jet_jit_fmt_bytes" => jet_jit_fmt_bytes: sig_i64;
    duration: "jet_jit_fmt_duration" => jet_jit_fmt_duration: sig_i64;
    ordinal: "jet_jit_fmt_ordinal" => jet_jit_fmt_ordinal: sig_i64;
    plural: "jet_jit_fmt_plural" => jet_jit_fmt_plural: sig_i64x3;
    pad_left: "jet_jit_fmt_pad_left" => jet_jit_fmt_pad_left: sig_i64x3;
    pad_right: "jet_jit_fmt_pad_right" => jet_jit_fmt_pad_right: sig_i64x3;
    pad_center: "jet_jit_fmt_pad_center" => jet_jit_fmt_pad_center: sig_i64x3;
}
