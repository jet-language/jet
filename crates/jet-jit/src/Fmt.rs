//! `core.fmt` host shims (#729). Mirrors `jet_fmt_*` in
//! `jet-codegen/.../DataFmt.rs` (prelude is string-embedded; same algorithm).

use super::Concurrency;

fn comma_int(value: i64) -> String {
    let raw = value.abs().to_string();
    let mut out = String::new();
    for (i, ch) in raw.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    let mut text: String = out.chars().rev().collect();
    if value < 0 {
        text.insert(0, '-');
    }
    text
}

fn comma_decimal(raw: String) -> String {
    let (sign, rest) = raw
        .strip_prefix('-')
        .map_or(("", raw.as_str()), |s| ("-", s));
    let mut split = rest.splitn(2, '.');
    let whole = split.next().unwrap_or("0");
    let frac = split.next();
    let whole_value = whole.parse::<i64>().unwrap_or(0);
    let whole_text = comma_int(whole_value);
    match frac {
        Some(frac) => format!("{sign}{whole_text}.{frac}"),
        None => format!("{sign}{whole_text}"),
    }
}

fn pad_need(text: &str, width: i64) -> usize {
    let width = width.max(0) as usize;
    width.saturating_sub(text.chars().count())
}

fn pad_fill(fill: &str, len: usize) -> String {
    if len == 0 {
        return String::new();
    }
    let fill = if fill.is_empty() { " " } else { fill };
    let mut out = String::new();
    while out.chars().count() < len {
        out.push_str(fill);
    }
    out.chars().take(len).collect()
}

fn fmt_number(value: i64) -> String {
    comma_int(value)
}

fn fmt_decimal(value: f64, precision: i64) -> String {
    let precision = precision.clamp(0, 9) as usize;
    comma_decimal(format!("{:.*}", precision, value))
}

fn fmt_percent(value: f64, precision: i64) -> String {
    format!("{}%", fmt_decimal(value * 100.0, precision))
}

fn fmt_bytes(value: i64) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let mut size = (value as f64).abs();
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut unit = 0usize;
    while size >= 1000.0 && unit + 1 < units.len() {
        size /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}{} {}", sign, size as i64, units[unit])
    } else if size >= 10.0 {
        format!("{}{} {}", sign, size.round() as i64, units[unit])
    } else {
        let shown = format!("{:.1}", size);
        format!("{}{} {}", sign, shown.trim_end_matches(".0"), units[unit])
    }
}

fn fmt_duration(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "" };
    let mut rest = ms.abs();
    if rest < 1000 {
        return format!("{sign}{rest}ms");
    }
    let days = rest / 86_400_000;
    rest %= 86_400_000;
    let hours = rest / 3_600_000;
    rest %= 3_600_000;
    let minutes = rest / 60_000;
    rest %= 60_000;
    let seconds = rest / 1000;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{seconds}s"));
    }
    format!(
        "{}{}",
        sign,
        parts.into_iter().take(3).collect::<Vec<_>>().join(" ")
    )
}

fn fmt_ordinal(value: i64) -> String {
    let n = value.abs();
    let suffix = if (11..=13).contains(&(n % 100)) {
        "th"
    } else {
        match n % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{}{}", comma_int(value), suffix)
}

fn clone_str(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}

fn alloc_str(s: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
}

extern "C" fn jet_jit_fmt_number(value: i64) -> i64 {
    alloc_str(fmt_number(value))
}

extern "C" fn jet_jit_fmt_decimal(value: f64, precision: i64) -> i64 {
    alloc_str(fmt_decimal(value, precision))
}

extern "C" fn jet_jit_fmt_percent(value: f64, precision: i64) -> i64 {
    alloc_str(fmt_percent(value, precision))
}

extern "C" fn jet_jit_fmt_bytes(value: i64) -> i64 {
    alloc_str(fmt_bytes(value))
}

extern "C" fn jet_jit_fmt_duration(ms: i64) -> i64 {
    alloc_str(fmt_duration(ms))
}

extern "C" fn jet_jit_fmt_ordinal(value: i64) -> i64 {
    alloc_str(fmt_ordinal(value))
}

extern "C" fn jet_jit_fmt_plural(count: i64, singular: i64, plural: i64) -> i64 {
    let singular = clone_str(singular);
    let plural = clone_str(plural);
    let word = if count.abs() == 1 {
        &singular
    } else {
        &plural
    };
    alloc_str(format!("{} {}", comma_int(count), word))
}

extern "C" fn jet_jit_fmt_pad_left(text: i64, width: i64, fill: i64) -> i64 {
    let text = clone_str(text);
    let fill = clone_str(fill);
    let need = pad_need(&text, width);
    alloc_str(format!("{}{}", pad_fill(&fill, need), text))
}

extern "C" fn jet_jit_fmt_pad_right(text: i64, width: i64, fill: i64) -> i64 {
    let text = clone_str(text);
    let fill = clone_str(fill);
    let need = pad_need(&text, width);
    alloc_str(format!("{}{}", text, pad_fill(&fill, need)))
}

extern "C" fn jet_jit_fmt_pad_center(text: i64, width: i64, fill: i64) -> i64 {
    let text = clone_str(text);
    let fill = clone_str(fill);
    let need = pad_need(&text, width);
    let left = need / 2;
    let right = need - left;
    alloc_str(format!(
        "{}{}{}",
        pad_fill(&fill, left),
        text,
        pad_fill(&fill, right)
    ))
}

pub(crate) struct FmtHostFns {
    pub number: cranelift_module::FuncId,
    pub decimal: cranelift_module::FuncId,
    pub percent: cranelift_module::FuncId,
    pub bytes: cranelift_module::FuncId,
    pub duration: cranelift_module::FuncId,
    pub ordinal: cranelift_module::FuncId,
    pub plural: cranelift_module::FuncId,
    pub pad_left: cranelift_module::FuncId,
    pub pad_right: cranelift_module::FuncId,
    pub pad_center: cranelift_module::FuncId,
}

pub(crate) fn register_fmt_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_fmt_number", jet_jit_fmt_number as *const u8);
    builder.symbol("jet_jit_fmt_decimal", jet_jit_fmt_decimal as *const u8);
    builder.symbol("jet_jit_fmt_percent", jet_jit_fmt_percent as *const u8);
    builder.symbol("jet_jit_fmt_bytes", jet_jit_fmt_bytes as *const u8);
    builder.symbol("jet_jit_fmt_duration", jet_jit_fmt_duration as *const u8);
    builder.symbol("jet_jit_fmt_ordinal", jet_jit_fmt_ordinal as *const u8);
    builder.symbol("jet_jit_fmt_plural", jet_jit_fmt_plural as *const u8);
    builder.symbol("jet_jit_fmt_pad_left", jet_jit_fmt_pad_left as *const u8);
    builder.symbol("jet_jit_fmt_pad_right", jet_jit_fmt_pad_right as *const u8);
    builder.symbol("jet_jit_fmt_pad_center", jet_jit_fmt_pad_center as *const u8);
}

pub(crate) fn declare_fmt_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<FmtHostFns, String> {
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
    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(FmtHostFns {
        number: import("jet_jit_fmt_number", &sig_i64)?,
        decimal: import("jet_jit_fmt_decimal", &sig_f64_i64)?,
        percent: import("jet_jit_fmt_percent", &sig_f64_i64)?,
        bytes: import("jet_jit_fmt_bytes", &sig_i64)?,
        duration: import("jet_jit_fmt_duration", &sig_i64)?,
        ordinal: import("jet_jit_fmt_ordinal", &sig_i64)?,
        plural: import("jet_jit_fmt_plural", &sig_i64x3)?,
        pad_left: import("jet_jit_fmt_pad_left", &sig_i64x3)?,
        pad_right: import("jet_jit_fmt_pad_right", &sig_i64x3)?,
        pad_center: import("jet_jit_fmt_pad_center", &sig_i64x3)?,
    })
}
