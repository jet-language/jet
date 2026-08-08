//! Curated Core calls shared by comptime and REPL evaluation.

use std::cell::Cell;

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtFloat, Type};
use super::super::Builtins::as_int;
use super::super::Diagnostics::unsupported;
use crate::AST::{CtReport, CtValue};

use super::repl_process::run_repl_process;

#[path = "../CorePureParity.rs"]
mod core_pure_parity;
#[path = "core_calls/regex.rs"]
mod regex;
use self::regex::*;

mod progress_semantics {
    include!("../../../../jet-codegen/src/Prelude/Core/Progress.rs");
}

mod math_lib_pure {
    include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/MathLibPure.rs");
}

// #1657 / I9: the one `core.data` statistics, bar-plot and bridge-status
// kernel. This is the exact source AOT embeds and the Cranelift JIT host
// includes, so comptime and the interpreter run the same compensated
// arithmetic and report the same `DataError`. Only the `jet_std` value types
// are declared here; every rule lives in the included file.
#[allow(dead_code)]
mod data_kernel {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;

    pub(crate) mod jet_std {
        #[allow(unused_imports)]
        pub use jet_foundation::Outcome::*;

        #[allow(dead_code)]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub(crate) enum DataErrorKind {
            Decode,
            Limit,
            IO,
            Empty,
            InvalidArgument,
            NonFinite,
            Overflow,
            State,
            Bridge,
        }

        /// `DataError.cause` only ever carries an absence here: the kernel's
        /// encoding-backed errors live in `DataFlow.rs`, not in this file.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub(crate) struct EncodingError;

        #[derive(Clone, Debug)]
        pub(crate) struct DataError {
            pub(crate) kind: DataErrorKind,
            pub(crate) operation: String,
            pub(crate) row: JetOutcome<i64, JetAbsent>,
            pub(crate) column: JetOutcome<i64, JetAbsent>,
            pub(crate) index: JetOutcome<i64, JetAbsent>,
            pub(crate) reason: String,
            pub(crate) cause: JetOutcome<EncodingError, JetAbsent>,
        }

        #[derive(Clone, Debug)]
        pub(crate) struct DataGroup {
            pub(crate) key: String,
            pub(crate) count: i64,
            pub(crate) sum: f64,
            pub(crate) mean: f64,
        }

        #[derive(Clone, Debug)]
        pub(crate) struct DataSummary {
            pub(crate) count: i64,
            pub(crate) sum: f64,
            pub(crate) mean: f64,
            pub(crate) min: f64,
            pub(crate) max: f64,
            pub(crate) median: f64,
            pub(crate) variance: f64,
            pub(crate) stddev: f64,
        }

        /// D-DATA-PLOT1=A: shared options for the deterministic line renderers.
        #[derive(Clone, Debug)]
        pub(crate) struct DataLineOptions {
            pub(crate) title: String,
            pub(crate) x_label: String,
            pub(crate) y_label: String,
            pub(crate) markers: bool,
            pub(crate) reference: JetOutcome<f64, JetAbsent>,
            pub(crate) style: String,
            pub(crate) color: String,
            pub(crate) legend: String,
        }

        #[derive(Clone, Debug)]
        pub(crate) struct DataStatus {
            pub(crate) step: String,
            pub(crate) path: String,
            pub(crate) copy: String,
            pub(crate) ownership: String,
            pub(crate) trust: String,
            pub(crate) fallback: String,
            pub(crate) replacement: String,
        }
    }

    include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs");
}

// #1657: `core.data` line renderers share AOT's `DataPlot.rs` the same way.
mod data_plot_rt {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    pub(crate) use super::data_kernel::jet_std;

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/DataPlot.rs");
}

pub(in super::super) fn apply_core_pure_method(
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    core_pure_parity::evaluate_method(recv, method, args, span)
}

pub(in super::super) fn sketch_add(
    recv: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Option<Result<(CtValue, CtValue), Diagnostic>> {
    core_pure_parity::sketch_add(recv, args, span)
}

pub(in super::super) fn solver_require(
    recv: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Option<Result<(CtValue, CtValue), Diagnostic>> {
    core_pure_parity::solver_require(recv, args, span)
}

pub(in super::super) fn solver_new(
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    core_pure_parity::solver_new(args, span)
}

pub fn display_core_pure_value(value: &CtValue) -> Option<String> {
    core_pure_parity::display(value)
}

pub(in super::super) use regex::apply_regex_method;
pub use regex::eval_regex_replace_all_with;

const PERF_DEFAULT_FIDELITY_BITS: u32 = 1.0f32.to_bits();
// D-FIDELITY-API1=A: this signal must behave like the AOT binary's
// process-global static (fresh default per program run, persists across
// reads/writes within one run). This compiler process hosts many concurrent
// or sequential "runs" on separate threads (parallel test threads, distinct
// compiles) but a session (REPL turns, a dev watch loop) is always driven
// from a single thread start to finish — so thread-local scoping gives each
// concurrent run its own signal (fixing a real cross-thread race a
// process-wide static had) while preserving the existing single-thread
// persistence a REPL session or dev run relies on.
thread_local! {
    static PERF_FIDELITY: Cell<u32> = const { Cell::new(PERF_DEFAULT_FIDELITY_BITS) };
}

// ---------------------------------------------------------------------------
// D-CTCORE1 (ratified 2026-06-22): curated pure Core whitelist for comptime.
//
// Only deterministic, pure functions may run at comptime. I/O (`fs.read`,
// `env.get`, etc.) is rejected here with a teaching diagnostic; the user
// can get build-time I/O via the explicit `embed_file`/`embed_bytes` tier.
//
// The whitelist grows with tests; start with core.math and core.string.
// ---------------------------------------------------------------------------

pub(in super::super) fn as_float(v: &CtValue, span: Span) -> Result<f64, Diagnostic> {
    match v {
        CtValue::Float(value) => Ok(value.as_f64()),
        CtValue::Int(n) => Ok(*n as f64),
        _ => Err(unsupported(
            "non-numeric argument to comptime math call",
            span,
        )),
    }
}

fn as_ct_float(v: &CtValue, span: Span) -> Result<CtFloat, Diagnostic> {
    match v {
        CtValue::Float(value) => Ok(*value),
        _ => Err(unsupported(
            "non-float argument to comptime math call",
            span,
        )),
    }
}

fn named_tuple(fields: &[(&str, CtValue)]) -> CtValue {
    CtValue::Struct {
        type_name: format!(
            "({})",
            fields.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(",")
        ),
        fields: fields
            .iter()
            .map(|(n, v)| ((*n).to_string(), v.clone()))
            .collect(),
    }
}

pub(in super::super) fn as_string(v: &CtValue, span: Span) -> Result<&str, Diagnostic> {
    match v {
        CtValue::Str(s) => Ok(s.as_str()),
        _ => Err(unsupported(
            "non-string argument to comptime string call",
            span,
        )),
    }
}

/// D-UUIDENC1=A: a `[U8]` argument — either the literal `Bytes` shape
/// (`embed_bytes`'s output) or a `List` of `Int` elements (an ordinary `[U8]`
/// list literal), matching whichever the caller happens to be holding.
pub(super) fn as_bytes(v: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match v {
        CtValue::Bytes(bs) => Ok(bs.clone()),
        CtValue::List(xs) => xs
            .iter()
            .map(|x| match x {
                CtValue::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
                _ => Err(unsupported(
                    "a `[U8]` list with an out-of-range element",
                    span,
                )),
            })
            .collect(),
        _ => Err(unsupported(
            "non-`[U8]` argument to comptime encoding call",
            span,
        )),
    }
}

/// D-URL1=A: `Vec<Vec<String>>`-shaped arg (`[[String]]`) — used by
/// `core.url.from_parts`'s `query` param and `core.url.query`'s pairs param,
/// mirroring AOT's `&Vec<Vec<String>>` signature.
fn as_string_rows(v: &CtValue, span: Span) -> Result<Vec<Vec<String>>, Diagnostic> {
    match v {
        CtValue::List(rows) => rows
            .iter()
            .map(|row| match row {
                CtValue::List(cols) => cols
                    .iter()
                    .map(|c| Ok(as_string(c, span)?.to_string()))
                    .collect::<Result<Vec<_>, _>>(),
                _ => Err(unsupported("rows that are not `[[String]]`", span)),
            })
            .collect(),
        _ => Err(unsupported("rows that are not `[[String]]`", span)),
    }
}

/// Typed `csv.to_string([T])`: a list of records becomes a header row taken from
/// the first record's fields plus one row per record. Mirrors AOT
/// `jet_enc_csv_to_string` and the resident JIT's `csv_render_datatree` cell for
/// cell. Returns `None` when the value is not a list of records, so the caller
/// can fall back to the dynamic `[[String]]` rows shape.
fn csv_rows_from_records(v: &CtValue) -> Option<Vec<Vec<String>>> {
    let CtValue::List(items) = v else {
        return None;
    };
    let field_names = |value: &CtValue| match value {
        CtValue::Struct { fields, .. } => Some(
            fields
                .iter()
                .map(|(name, _)| name.strip_prefix("user_").unwrap_or(name).to_string())
                .collect::<Vec<_>>(),
        ),
        _ => None,
    };
    let header = field_names(items.first()?)?;
    if items.iter().any(|item| field_names(item).is_none()) {
        return None;
    }
    let cell = |value: &CtValue| match value {
        CtValue::Str(s) => s.clone(),
        CtValue::Int(n) => n.to_string(),
        CtValue::Float(f) => f.render(),
        CtValue::Bool(b) => b.to_string(),
        CtValue::Unit | CtValue::Failed(CtReport::Clean(_)) => String::new(),
        other => other.to_json(),
    };
    let mut rows = vec![header.clone()];
    for item in items {
        let CtValue::Struct { fields, .. } = item else {
            return None;
        };
        rows.push(
            header
                .iter()
                .map(|key| {
                    fields
                        .iter()
                        .find(|(name, _)| name.strip_prefix("user_").unwrap_or(name) == key)
                        .map(|(_, value)| cell(value))
                        .unwrap_or_default()
                })
                .collect(),
        );
    }
    Some(rows)
}

/// Mirrors AOT's `JetURL` field shape 1:1 so `.scheme`/`.host`/`.path`/
/// `.query`/`.fragment` struct-field reads (generic member access,
/// `Interpreter.rs`) work the same as any other `CtValue::Struct`.
fn url_parts_to_ct(u: &super::super::UrlLite::UrlParts) -> CtValue {
    CtValue::Struct {
        type_name: "Url".to_string(),
        fields: vec![
            ("scheme".to_string(), CtValue::Str(u.scheme.clone())),
            (
                "host".to_string(),
                match &u.host {
                    Some(h) if !h.is_empty() => CtValue::Present(Box::new(CtValue::Str(h.clone()))),
                    _ => CtValue::absent(Type::String),
                },
            ),
            (
                "port".to_string(),
                match u.port {
                    Some(p) => CtValue::Present(Box::new(CtValue::Int(p))),
                    None => CtValue::absent(Type::Int),
                },
            ),
            ("path".to_string(), CtValue::Str(u.path.clone())),
            (
                "query".to_string(),
                CtValue::List(
                    u.query
                        .iter()
                        .map(|(k, v)| {
                            CtValue::List(vec![CtValue::Str(k.clone()), CtValue::Str(v.clone())])
                        })
                        .collect(),
                ),
            ),
            (
                "fragment".to_string(),
                match &u.fragment {
                    Some(f) => CtValue::Present(Box::new(CtValue::Str(f.clone()))),
                    None => CtValue::absent(Type::String),
                },
            ),
        ],
    }
}

/// `[Float]` argument — `core.data`'s stats functions all take `&Vec<f64>`.
fn as_float_list(v: &CtValue, span: Span) -> Result<Vec<f64>, Diagnostic> {
    match v {
        CtValue::List(xs) => xs.iter().map(|x| as_float(x, span)).collect(),
        _ => Err(unsupported("core.data: argument must be `[Float]`", span)),
    }
}

/// `[DataGroup]` argument for the bar and line renderers. Every field the
/// kernel validates is read here, so comptime rejects exactly what AOT rejects.
fn as_data_groups(
    v: &CtValue,
    span: Span,
) -> Result<Vec<data_kernel::jet_std::DataGroup>, Diagnostic> {
    let CtValue::List(items) = v else {
        return Err(unsupported("core.data: argument must be `[DataGroup]`", span));
    };
    items
        .iter()
        .map(|item| {
            let CtValue::Struct { type_name, fields } = item else {
                return Err(unsupported("core.data: argument must be `[DataGroup]`", span));
            };
            if type_name != "DataGroup" {
                return Err(unsupported("core.data: argument must be `[DataGroup]`", span));
            }
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
            };
            let (Some(CtValue::Str(key)), Some(CtValue::Int(count))) =
                (field("key"), field("count"))
            else {
                return Err(unsupported(
                    "core.data: a `DataGroup` needs `key: String` and `count: Int`",
                    span,
                ));
            };
            let (Some(sum), Some(mean)) = (field("sum"), field("mean")) else {
                return Err(unsupported(
                    "core.data: a `DataGroup` needs `sum: Float` and `mean: Float`",
                    span,
                ));
            };
            Ok(data_kernel::jet_std::DataGroup {
                key: key.clone(),
                count: *count,
                sum: as_float(sum, span)?,
                mean: as_float(mean, span)?,
            })
        })
        .collect()
}

fn as_data_line_options(
    v: &CtValue,
    span: Span,
) -> Result<data_kernel::jet_std::DataLineOptions, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
        return Err(unsupported(
            "core.data line renderers need `DataLineOptions`",
            span,
        ));
    };
    if type_name != "DataLineOptions" {
        return Err(unsupported(
            "core.data line renderers need `DataLineOptions`",
            span,
        ));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| unsupported("DataLineOptions is missing a required field", span))
    };
    let string = |name: &str| match field(name)? {
        CtValue::Str(value) => Ok(value),
        _ => Err(unsupported("DataLineOptions string field has the wrong type", span)),
    };
    let markers = match field("markers")? {
        CtValue::Bool(value) => value,
        _ => return Err(unsupported("DataLineOptions `markers` must be Bool", span)),
    };
    let reference = match field("reference")? {
        CtValue::Present(value) => Ok(as_float(&value, span)?),
        CtValue::Failed(CtReport::Clean(_)) => Err(jet_foundation::Outcome::JetAbsent),
        _ => return Err(unsupported("DataLineOptions `reference` must be Float?", span)),
    };
    Ok(data_kernel::jet_std::DataLineOptions {
        title: string("title")?,
        x_label: string("x_label")?,
        y_label: string("y_label")?,
        markers,
        reference,
        style: string("style")?,
        color: string("color")?,
        legend: string("legend")?,
    })
}

/// #1657 / I9: the checked `core.data` surface is the edition-2027 default and
/// older editions type the same calls as plain values. Sema picks the return
/// type from this same question (`fixed_sigs.rs`), so comptime asks it too.
fn data_checked_surface() -> bool {
    jet_foundation::PackageEdition::package_edition_at_least("2027")
}

/// One `DataError` value for every `core.data` failure, built from the kernel's
/// own error — comptime never writes its own reason text.
fn data_error_value(error: &data_kernel::jet_std::DataError) -> CtValue {
    let index = |slot: &jet_foundation::Outcome::JetOutcome<i64, jet_foundation::Outcome::JetAbsent>| match slot {
        Ok(value) => CtValue::Present(Box::new(CtValue::Int(*value))),
        Err(_) => CtValue::absent(Type::Int),
    };
    CtValue::Struct {
        type_name: "DataError".to_string(),
        fields: vec![
            (
                "kind".to_string(),
                CtValue::Enum {
                    type_name: "DataErrorKind".to_string(),
                    variant: format!("{:?}", error.kind),
                    args: Vec::new(),
                },
            ),
            ("operation".to_string(), CtValue::Str(error.operation.clone())),
            ("row".to_string(), index(&error.row)),
            ("column".to_string(), index(&error.column)),
            ("index".to_string(), index(&error.index)),
            ("reason".to_string(), CtValue::Str(error.reason.clone())),
            (
                "cause".to_string(),
                CtValue::absent(Type::Named("EncodingError".to_string())),
            ),
        ],
    }
}

/// Marshal one kernel result onto the surface the current edition types.
fn data_result_value<T>(
    checked: Result<T, data_kernel::jet_std::DataError>,
    unchecked: impl FnOnce() -> T,
    to_value: impl Fn(T) -> CtValue,
) -> CtValue {
    if !data_checked_surface() {
        return to_value(unchecked());
    }
    match checked {
        Ok(value) => CtValue::Present(Box::new(to_value(value))),
        Err(error) => CtValue::failed(Box::new(data_error_value(&error))),
    }
}

fn data_float_value(value: f64) -> CtValue {
    CtValue::Float(CtFloat::f64(value))
}

/// D-DATA-STATUS1 / #708: the `data.status()` rows for `jet inspect dossier`,
/// read from the one kernel rather than a second table.
pub fn data_status_rows() -> Vec<(String, String, String, String, String, String, String)> {
    data_kernel::jet_data_status()
        .into_iter()
        .map(|row| {
            (
                row.step,
                row.path,
                row.copy,
                row.ownership,
                row.trust,
                row.fallback,
                row.replacement,
            )
        })
        .collect()
}

pub fn apply_data_line_call(
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let groups = as_data_groups(
        args.first()
            .ok_or_else(|| unsupported("core.data line renderers need groups", span))?,
        span,
    )?;
    let options = as_data_line_options(
        args.get(1)
            .ok_or_else(|| unsupported("core.data line renderers need options", span))?,
        span,
    )?;
    let plot_error = |error: data_plot_rt::DataPlotError| data_kernel::jet_std::DataError {
        kind: match error.kind {
            "NonFinite" => data_kernel::jet_std::DataErrorKind::NonFinite,
            _ => data_kernel::jet_std::DataErrorKind::InvalidArgument,
        },
        operation: error.operation.to_string(),
        row: Err(jet_foundation::Outcome::JetAbsent),
        column: Err(jet_foundation::Outcome::JetAbsent),
        index: match error.index {
            Some(index) => Ok(index),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
        reason: error.reason.to_string(),
        cause: Err(jet_foundation::Outcome::JetAbsent),
    };
    match method {
        "line_text" => Ok(data_result_value(
            data_plot_rt::jet_data_line_text_plot_checked(&groups, &options).map_err(plot_error),
            || data_plot_rt::jet_data_line_text(&groups, &options),
            CtValue::Str,
        )),
        "line_svg" => Ok(data_result_value(
            data_plot_rt::jet_data_line_svg_plot_checked(&groups, &options).map_err(plot_error),
            || data_plot_rt::jet_data_line_svg(&groups, &options),
            CtValue::Str,
        )),
        _ => Err(unsupported(
            &format!("unsupported core.data line renderer `{method}`"),
            span,
        )),
    }
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

fn hex_encode(bytes: Vec<u8>) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX_DIGITS[(b >> 4) as usize] as char);
        out.push(HEX_DIGITS[(b & 0xf) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::with_capacity(chars.len() / 2);
    for pair in chars.chunks(2) {
        let hi = pair[0].to_digit(16)?;
        let lo = pair[1].to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: Vec<u8>) -> String {
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied();
        let b2 = chunk.get(2).copied();
        out.push(BASE64_ALPHABET[(b0 >> 2) as usize] as char);
        out.push(BASE64_ALPHABET[(((b0 & 0x03) << 4) | (b1.unwrap_or(0) >> 4)) as usize] as char);
        out.push(match b1 {
            Some(b1) => {
                BASE64_ALPHABET[(((b1 & 0x0f) << 2) | (b2.unwrap_or(0) >> 6)) as usize] as char
            }
            None => '=',
        });
        out.push(match b2 {
            Some(b2) => BASE64_ALPHABET[(b2 & 0x3f) as usize] as char,
            None => '=',
        });
    }
    out
}

const BASE32_CHARS: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            let idx = ((buffer >> (bits - 5)) & 31) as usize;
            out.push(BASE32_CHARS[idx] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        let idx = ((buffer << (5 - bits)) & 31) as usize;
        out.push(BASE32_CHARS[idx] as char);
    }
    while out.len() % 8 != 0 {
        out.push('=');
    }
    out
}

/// Core modules the REPL interpreter cannot run (native FFI / threads / HTTP stack).
fn repl_native_only_module(module: &str) -> Option<&'static str> {
    match module {
        "core.http" | "core.http.client" | "core.http.server" => {
            Some("the HTTP client/server (`core.http`)")
        }
        "core.db" => Some("`core.db` (SQLite)"),
        "core.net" => Some("network sockets (`core.net`)"),
        "core.reactive" => Some("`core.reactive`"),
        "core.crypto" | "core.crypto.random" => Some("`core.crypto`"),
        "core.auth" => Some("`core.auth` token verification"),
        "core.tasks" | "core.channels" => Some("tasks/channels (`core.tasks`)"),
        "core.mem" | "core.mem.alloc" => Some("`core.mem` (low-level memory tier)"),
        "core.log" => Some("`core.log`"),
        _ => None,
    }
}

fn repl_native_module_diag(module: &str, method: &str, span: Span) -> Diagnostic {
    let feature = repl_native_only_module(module).unwrap_or("a native-only core module");
    Diagnostic::error(
        "E1802",
        format!("the REPL interpreter can't run `{}.{method}()`", module),
        format!(
            "the REPL is an interpreter for learning Jet; {feature} needs the real compiler \
             and native runtime"
        ),
        "run `jet run <file.jet>` or `jet build <file.jet>` to use the full compiler".to_string(),
        Some(span),
    )
}

pub(super) fn io_error_value(path: &str, e: std::io::Error) -> CtValue {
    let kind = match e.kind() {
        std::io::ErrorKind::NotFound => "NotFound",
        std::io::ErrorKind::PermissionDenied => "PermissionDenied",
        _ => "Other",
    };
    CtValue::Struct {
        type_name: "IOError".to_string(),
        fields: if kind == "Other" {
            vec![
                ("kind".to_string(), CtValue::Str(kind.to_string())),
                ("message".to_string(), CtValue::Str(e.to_string())),
            ]
        } else {
            vec![
                ("kind".to_string(), CtValue::Str(kind.to_string())),
                ("path".to_string(), CtValue::Str(path.to_string())),
            ]
        },
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 7;
    x ^= x >> 9;
    x = x.wrapping_mul(0x9e3779b97f4a7c15);
    *state = x;
    x
}

pub(in super::super) fn random_int(state: &mut u64, low: i64, high: i64) -> i64 {
    if high <= low {
        return low;
    }
    low + (splitmix64(state) % ((high - low + 1) as u64)) as i64
}

pub(in super::super) fn random_float(state: &mut u64) -> f64 {
    (splitmix64(state) as f64) / (u64::MAX as f64)
}

/// D-DET1 widened ambient draws. Mirrors AOT's `jet_std_random_*` (Process.rs)
/// byte-for-byte — same `jet_rng_next`-equivalent `splitmix64` stream, same
/// formulas — so an ambient `core.random.*` call at comptime and the same
/// call at AOT runtime draw the identical sequence from the identical seed
/// (R12 parity).
fn random_float_open(state: &mut u64) -> f64 {
    let x = random_float(state);
    if x <= 0.0 {
        f64::MIN_POSITIVE
    } else {
        x
    }
}

fn random_float_range(state: &mut u64, low: f64, high: f64) -> f64 {
    if !(high > low) {
        return low;
    }
    low + (high - low) * random_float(state)
}

fn random_bool_p(state: &mut u64, p: f64) -> bool {
    if p <= 0.0 || p.is_nan() {
        false
    } else if p >= 1.0 {
        true
    } else {
        random_float(state) < p
    }
}

fn random_normal(state: &mut u64, mean: f64, stddev: f64) -> f64 {
    let u1 = random_float_open(state);
    let u2 = random_float(state);
    let z0 = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    mean + z0 * stddev.max(0.0)
}

fn random_exponential(state: &mut u64, lambda: f64) -> f64 {
    if lambda <= 0.0 || lambda.is_nan() {
        return 0.0;
    }
    -random_float_open(state).ln() / lambda
}

fn random_bytes(state: &mut u64, n: i64) -> Vec<u8> {
    let n = n.max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(splitmix64(state) as u8);
    }
    out
}

fn random_pick_ct(state: &mut u64, xs: &[CtValue]) -> Option<CtValue> {
    if xs.is_empty() {
        None
    } else {
        Some(xs[random_int(state, 0, xs.len() as i64 - 1) as usize].clone())
    }
}

fn random_weighted_pick_ct(
    state: &mut u64,
    xs: &[CtValue],
    weights: &[f64],
) -> Option<CtValue> {
    if xs.is_empty() || xs.len() != weights.len() {
        return None;
    }
    let mut total = 0.0;
    for &w in weights {
        if w.is_finite() && w > 0.0 {
            total += w;
        }
    }
    if total <= 0.0 {
        return None;
    }
    let mut needle = random_float_range(state, 0.0, total);
    for (item, &weight) in xs.iter().zip(weights.iter()) {
        let w = if weight.is_finite() && weight > 0.0 { weight } else { 0.0 };
        if needle < w {
            return Some(item.clone());
        }
        needle -= w;
    }
    xs.last().cloned()
}

fn random_sample_ct(state: &mut u64, xs: &[CtValue], k: i64) -> Vec<CtValue> {
    let want = (k.max(0) as usize).min(xs.len());
    let mut pool = xs.to_vec();
    for i in 0..want {
        let j = random_int(state, i as i64, pool.len() as i64 - 1) as usize;
        pool.swap(i, j);
    }
    pool.truncate(want);
    pool
}

pub(super) fn shuffle_ct_list(state: &mut u64, xs: &mut [CtValue]) {
    let len = xs.len();
    for i in (1..len).rev() {
        let j = random_int(state, 0, i as i64) as usize;
        xs.swap(i, j);
    }
}

// ── core.fmt: pure text formatting, mirrors AOT's `jet_fmt_*` (DataFmt.rs)
// byte-for-byte (comma grouping, byte-size units, duration parts, ordinal
// suffix, pad fill) so a comptime call prints identically to the same call
// at AOT runtime (R12 parity). ────────────────────────────────────────────

fn comma_int_ct(value: i64) -> String {
    let raw = value.unsigned_abs().to_string();
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

fn comma_decimal_ct(raw: String) -> String {
    let (sign, rest) = raw.strip_prefix('-').map_or(("", raw.as_str()), |s| ("-", s));
    let mut split = rest.splitn(2, '.');
    let whole = split.next().unwrap_or("0");
    let frac = split.next();
    let whole_value = whole.parse::<i64>().unwrap_or(0);
    let whole_text = comma_int_ct(whole_value);
    match frac {
        Some(frac) => format!("{}{}.{}", sign, whole_text, frac),
        None => format!("{}{}", sign, whole_text),
    }
}

fn fmt_decimal_ct(value: f64, precision: i64) -> String {
    let precision = precision.clamp(0, 9) as usize;
    comma_decimal_ct(format!("{:.*}", precision, value))
}

fn fmt_bytes_ct(value: i64) -> String {
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

fn fmt_duration_ct(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "" };
    let mut rest = ms.abs();
    if rest < 1000 {
        return format!("{}{}ms", sign, rest);
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
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{}s", seconds));
    }
    format!(
        "{}{}",
        sign,
        parts.into_iter().take(3).collect::<Vec<_>>().join(" ")
    )
}

fn pad_need_ct(text: &str, width: i64) -> usize {
    let width = width.max(0) as usize;
    width.saturating_sub(text.chars().count())
}

fn pad_fill_ct(fill: &str, len: usize) -> String {
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

thread_local! {
    static JET_AMBIENT_RNG: std::cell::Cell<u64> = std::cell::Cell::new(0x4d595df4d0f33173);
}

pub(super) fn with_ambient_rng<R>(f: impl FnOnce(&mut u64) -> R) -> R {
    JET_AMBIENT_RNG.with(|cell| {
        let mut state = cell.get();
        let out = f(&mut state);
        cell.set(state);
        out
    })
}

/// D-TEXTWIDTH1=B: pull the two policy flags back out of a `TextWidth`
/// `CtValue::Struct` (`ambiguous: .Wide|.Narrow`, `controls: .Zero|.Reject`).
/// Missing/malformed fields fall back to the portable default (`Narrow`,
/// `Zero`) rather than erroring — sema already guarantees the shape.
fn text_width_policy_flags(policy: &CtValue) -> (bool, bool) {
    let CtValue::Struct { fields, .. } = policy else {
        return (false, false);
    };
    let is_var = |v: &CtValue, name: &str| match v {
        CtValue::Enum { variant, .. } => variant == name,
        // Legacy Struct-shaped enum lit (`Type.Variant`) — accept either.
        CtValue::Struct { type_name, .. } => {
            type_name == name || type_name.ends_with(&format!(".{name}"))
        }
        _ => false,
    };
    let ambiguous_wide = fields
        .iter()
        .find(|(n, _)| n == "ambiguous")
        .is_some_and(|(_, v)| is_var(v, "Wide"));
    let controls_reject = fields
        .iter()
        .find(|(n, _)| n == "controls")
        .is_some_and(|(_, v)| is_var(v, "Reject"));
    (ambiguous_wide, controls_reject)
}

/// Evaluate a whitelisted pure Core call at comptime / in the REPL.
/// `module` is the full path (e.g. `"core.math"`, `"core.regex"`).
pub fn apply_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    repl_mode: bool,
) -> Result<CtValue, Diagnostic> {
    if let Some(result) = core_pure_parity::evaluate(module, method, &args, span) {
        return result;
    }
    if let Some(result) = crate::Comptime::try_ambient_core_call(module, method, args.clone(), span)
    {
        return result;
    }

    if repl_mode {
        if let Some(_) = repl_native_only_module(module) {
            return Err(repl_native_module_diag(module, method, span));
        }
    }

    let one = |i: usize| {
        args.get(i).ok_or_else(|| {
            unsupported(&format!("{}.{}(): missing arg {}", module, method, i), span)
        })
    };
    let args_bool = |index: usize, default: bool| -> Result<bool, Diagnostic> {
        match args.get(index) {
            Some(CtValue::Bool(value)) => Ok(*value),
            Some(_) => Err(unsupported(
                &format!("{}.{}(): argument {} must be Bool", module, method, index + 1),
                span,
            )),
            None => Ok(default),
        }
    };

    match (module, method) {
        ("jet.unit", "magnitude") => {
            Ok(CtValue::Str(as_float(one(0)?, span)?.to_string()))
        }
        // D-CORE-COMPRESS1=A / card #392 C4: pure gzip stays inside
        // tier-0. No native bridge, Boundary classification, or AOT fallback.
        ("core.compress.gzip", "compress") => Ok(CtValue::Bytes(
            super::super::ArchiveLite::gzip_compress(&as_bytes(one(0)?, span)?),
        )),
        ("core.compress.gzip", "decompress") => {
            Ok(match super::super::ArchiveLite::gzip_decompress(&as_bytes(one(0)?, span)?) {
                Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            })
        }
        // The std-only resident codec accepts ordinary dictionaryless zstd
        // frames. The encoder deliberately chooses interoperable raw blocks.
        ("core.compress.zstd", "compress") => Ok(CtValue::Bytes(
            super::super::ArchiveLite::zstd_compress(&as_bytes(one(0)?, span)?),
        )),
        ("core.compress.zstd", "decompress") => {
            Ok(match super::super::ArchiveLite::zstd_decompress(&as_bytes(one(0)?, span)?) {
                Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            })
        }
        // D-CORE-COMPRESS1=A / card #392 C4: archive containers are pure byte
        // transforms. Keep them interpreter-resident; never route through the
        // native FFI bridge or an AOT fallback.
        ("core.archive", "zip_compress") => Ok(CtValue::Bytes(
            super::super::ArchiveLite::zip_compress(
                as_string(one(0)?, span)?,
                &as_bytes(one(1)?, span)?,
            ),
        )),
        ("core.archive", "zip_decompress") => Ok(CtValue::Bytes(
            super::super::ArchiveLite::zip_decompress(&as_bytes(one(0)?, span)?),
        )),
        ("core.archive", "tar_add") => Ok(CtValue::Bytes(
            super::super::ArchiveLite::tar_add(
                &as_bytes(one(0)?, span)?,
                as_string(one(1)?, span)?,
                &as_bytes(one(2)?, span)?,
            ),
        )),
        ("core.archive", "tar_get") => Ok(CtValue::Bytes(
            super::super::ArchiveLite::tar_get(
                &as_bytes(one(0)?, span)?,
                as_string(one(1)?, span)?,
            ),
        )),
        ("core.archive", "tar_names_json") => Ok(CtValue::Str(
            super::super::ArchiveLite::tar_names_json(&as_bytes(one(0)?, span)?),
        )),
        // D-PENDING1=B: the same four enum variants AOT lowers to JetLoadable.
        ("core.reactive.loadable", state @ ("idle" | "loading")) => Ok(CtValue::Enum {
            type_name: "Loadable".to_string(),
            variant: if state == "idle" { "Idle" } else { "Loading" }.to_string(),
            args: Vec::new(),
        }),
        ("core.reactive.loadable", state @ ("loaded" | "failed")) => Ok(CtValue::Enum {
            type_name: "Loadable".to_string(),
            variant: if state == "loaded" { "Loaded" } else { "Failed" }.to_string(),
            args: vec![(None, one(0)?.clone())],
        }),
        // D-FIDELITY-API1=A: explicit runtime-global signal. Interpreter owns
        // same f32-backed range and validation contract as AOT/JIT.
        ("core.perf", "fidelity") => Ok(CtValue::Float(CtFloat::f64(
            f32::from_bits(PERF_FIDELITY.with(Cell::get)) as f64,
        ))),
        ("core.perf", "default_fidelity") => Ok(CtValue::Float(CtFloat::f64(
            f32::from_bits(PERF_DEFAULT_FIDELITY_BITS) as f64,
        ))),
        ("core.perf", "override_fidelity") => {
            let value = as_float(one(0)?, span)?;
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Ok(CtValue::failed(Box::new(CtValue::Str(format!(
                    "core.perf.Perf.override_fidelity needs 0.0 through 1.0, got {}",
                    value
                )))));
            }
            PERF_FIDELITY.with(|c| c.set((value as f32).to_bits()));
            Ok(CtValue::Present(Box::new(CtValue::Unit)))
        }
        ("core.perf", "reset_fidelity") => {
            PERF_FIDELITY.with(|c| c.set(PERF_DEFAULT_FIDELITY_BITS));
            Ok(CtValue::Unit)
        }
        // --- core.math whitelist ---
        ("core.math", "sqrt") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.sqrt())),
        ("core.math", "floor") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.floor())),
        ("core.math", "ceil") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.ceil())),
        ("core.math", "round") => Ok(CtValue::Int(as_ct_float(one(0)?, span)?.round_i64())),
        ("core.math", "abs") => match one(0)? {
            CtValue::Int(n) => Ok(CtValue::Int(n.abs())),
            CtValue::Float(f) => Ok(CtValue::Float(f.abs())),
            _ => Err(unsupported("core.math.abs: non-numeric argument", span)),
        },
        ("core.math", "pow") => {
            let a = as_ct_float(one(0)?, span)?;
            let b = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(
                a.powf(b).ok_or_else(|| unsupported("mixing float widths", span))?,
            ))
        }
        ("core.math", "min") => {
            let a = as_ct_float(one(0)?, span)?;
            let b = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(
                a.min(b).ok_or_else(|| unsupported("mixing float widths", span))?,
            ))
        }
        ("core.math", "max") => {
            let a = as_ct_float(one(0)?, span)?;
            let b = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(
                a.max(b).ok_or_else(|| unsupported("mixing float widths", span))?,
            ))
        }
        ("core.math", "clamp") => {
            let value = as_ct_float(one(0)?, span)?;
            let low = as_ct_float(one(1)?, span)?;
            let high = as_ct_float(one(2)?, span)?;
            Ok(CtValue::Float(value.clamp(low, high).ok_or_else(|| {
                unsupported("mixing float widths", span)
            })?))
        }
        ("core.math", "log2") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.log2())),
        ("core.math", "log10") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.log10())),
        // card #392 gap fix: the rest of `core.math` — mechanical ports of
        // the same one-line Rust std calls AOT's codegen emits
        // (`Codegen/TIR/emit/core_calls.rs`), so results match exactly.
        ("core.math", "sin") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.sin())),
        ("core.math", "cos") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.cos())),
        ("core.math", "tan") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.tan())),
        ("core.math", "asin") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.asin())),
        ("core.math", "acos") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.acos())),
        ("core.math", "atan") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.atan())),
        ("core.math", "sinh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.sinh())),
        ("core.math", "cosh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.cosh())),
        ("core.math", "tanh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.tanh())),
        ("core.math", "exp") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.exp())),
        ("core.math", "ln") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.ln())),
        ("core.math", "acosh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.acosh())),
        ("core.math", "asinh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.asinh())),
        ("core.math", "atanh") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.atanh())),
        ("core.math", "cbrt") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.cbrt())),
        ("core.math", "exp2") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.exp2())),
        ("core.math", "exp_m1") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.exp_m1())),
        ("core.math", "ln_1p") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.ln_1p())),
        ("core.math", "signum") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.signum())),
        ("core.math", "trunc") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.trunc())),
        ("core.math", "fract") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.fract())),
        ("core.math", "degrees") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.to_degrees())),
        ("core.math", "radians") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?.to_radians())),
        ("core.math", "atan2") => {
            let left = as_ct_float(one(0)?, span)?;
            let right = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(left.atan2(right).ok_or_else(|| {
                unsupported("mixing float widths", span)
            })?))
        }
        ("core.math", "copysign") => {
            let left = as_ct_float(one(0)?, span)?;
            let right = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(left.copysign(right).ok_or_else(|| {
                unsupported("mixing float widths", span)
            })?))
        }
        ("core.math", "log") => {
            let left = as_ct_float(one(0)?, span)?;
            let right = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(left.log(right).ok_or_else(|| {
                unsupported("mixing float widths", span)
            })?))
        }
        ("core.math", "fma") => {
            let a = as_ct_float(one(0)?, span)?;
            let b = as_ct_float(one(1)?, span)?;
            let c = as_ct_float(one(2)?, span)?;
            Ok(CtValue::Float(a.mul_add(b, c).ok_or_else(|| {
                unsupported("mixing float widths", span)
            })?))
        }
        ("core.math", "isqrt" | "factorial" | "checked_abs" | "checked_neg") => {
            let value = match one(0)? {
                CtValue::Int(value) => *value,
                _ => return Err(unsupported("this operation on a value that is not a whole number", span)),
            };
            match method {
                "isqrt" => {
                    if value < 0 {
                        return Ok(CtValue::absent(Type::Int));
                    }
                    let mut root = (value as f64).sqrt() as i64;
                    while root > 0 && root.saturating_mul(root) > value { root -= 1; }
                    while (root + 1).saturating_mul(root + 1) <= value { root += 1; }
                    Ok(CtValue::Present(Box::new(CtValue::Int(root))))
                }
                "factorial" => {
                    if value < 0 {
                        return Ok(CtValue::absent(Type::Int));
                    }
                    let mut total: i64 = 1;
                    let mut step: i64 = 2;
                    while step <= value {
                        total = match total.checked_mul(step) {
                            Some(next) => next,
                            None => return Ok(CtValue::absent(Type::Int)),
                        };
                        step += 1;
                    }
                    Ok(CtValue::Present(Box::new(CtValue::Int(total))))
                }
                "checked_abs" => Ok(match value.checked_abs() {
                    Some(answer) => CtValue::Present(Box::new(CtValue::Int(answer))),
                    None => CtValue::absent(Type::Int),
                }),
                _ => Ok(match value.checked_neg() {
                    Some(answer) => CtValue::Present(Box::new(CtValue::Int(answer))),
                    None => CtValue::absent(Type::Int),
                }),
            }
        }
        ("core.math", "is_even" | "is_odd") => {
            let value = match one(0)? {
                CtValue::Int(value) => *value,
                _ => return Err(unsupported("parity of a value that is not a whole number", span)),
            };
            Ok(CtValue::Bool(if method == "is_even" {
                value % 2 == 0
            } else {
                value % 2 != 0
            }))
        }
        ("core.math", "is_normal") => {
            Ok(CtValue::Bool(as_ct_float(one(0)?, span)?.as_f64().is_normal()))
        }
        ("core.math", "is_subnormal") => {
            Ok(CtValue::Bool(as_ct_float(one(0)?, span)?.as_f64().is_subnormal()))
        }
        ("core.math", "is_canonical") => {
            let x = as_ct_float(one(0)?, span)?;
            Ok(CtValue::Bool(x.is_finite() || x.is_nan()))
        }
        ("core.math", "is_signed" | "sign_bit") => {
            Ok(CtValue::Bool(as_ct_float(one(0)?, span)?.as_f64().is_sign_negative()))
        }
        ("core.math", "is_zero") => {
            Ok(CtValue::Bool(as_ct_float(one(0)?, span)?.as_f64() == 0.0))
        }
        ("core.math", "is_integer") => {
            let x = as_ct_float(one(0)?, span)?;
            Ok(CtValue::Bool(x.is_finite() && x.as_f64().fract() == 0.0))
        }
        ("core.math", "next_up") => {
            Ok(CtValue::Float(CtFloat::f64(as_ct_float(one(0)?, span)?.as_f64().next_up())))
        }
        ("core.math", "next_down") => {
            Ok(CtValue::Float(CtFloat::f64(
                as_ct_float(one(0)?, span)?.as_f64().next_down(),
            )))
        }
        ("core.math", "copy") => Ok(CtValue::Float(as_ct_float(one(0)?, span)?)),
        ("core.math", "cot") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            Ok(CtValue::Float(CtFloat::f64(1.0 / x.tan())))
        }
        ("core.math", "inv") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            Ok(CtValue::Float(CtFloat::f64(1.0 / x)))
        }
        ("core.math", "zero") => Ok(CtValue::Float(CtFloat::f64(0.0))),
        ("core.math", "radix") => Ok(CtValue::Int(2)),
        ("core.math", "leading_ones") => {
            let value = match one(0)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("leading_ones needs a whole number", span)),
            };
            Ok(CtValue::Int((value as u64).leading_ones() as i64))
        }
        ("core.math", "trailing_ones") => {
            let value = match one(0)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("trailing_ones needs a whole number", span)),
            };
            Ok(CtValue::Int((value as u64).trailing_ones() as i64))
        }
        ("core.math", "digits") => {
            let value = match one(0)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("digits needs a whole number", span)),
            };
            Ok(CtValue::Int(if value == 0 {
                1
            } else {
                let mut n = value.unsigned_abs();
                let mut count = 0i64;
                while n > 0 {
                    count += 1;
                    n /= 10;
                }
                count
            }))
        }
        ("core.math", "binomial") => {
            let n = match one(0)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("binomial needs whole numbers", span)),
            };
            let k = match one(1)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("binomial needs whole numbers", span)),
            };
            Ok(match math_lib_pure::jet_std_math_binomial(n, k) {
                Some(v) => CtValue::Present(Box::new(CtValue::Int(v))),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", "cmp") => {
            let a = as_ct_float(one(0)?, span)?.as_f64();
            let b = as_ct_float(one(1)?, span)?.as_f64();
            Ok(CtValue::Int(math_lib_pure::jet_std_math_cmp(a, b)))
        }
        ("core.math", "next_after") => {
            let a = as_ct_float(one(0)?, span)?.as_f64();
            let b = as_ct_float(one(1)?, span)?.as_f64();
            Ok(CtValue::Float(CtFloat::f64(math_lib_pure::jet_std_math_next_after(a, b))))
        }
        ("core.math", "ldexp" | "scaleb") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            let exp = match one(1)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("ldexp needs a whole-number exponent", span)),
            };
            Ok(CtValue::Float(CtFloat::f64(math_lib_pure::jet_std_math_ldexp(x, exp))))
        }
        ("core.math", "ilogb") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            Ok(match math_lib_pure::jet_std_math_ilogb(x) {
                Some(e) => CtValue::Present(Box::new(CtValue::Int(e))),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", "logb") => {
            Ok(CtValue::Float(CtFloat::f64(math_lib_pure::jet_std_math_logb(
                as_ct_float(one(0)?, span)?.as_f64(),
            ))))
        }
        ("core.math", "significand") => {
            Ok(CtValue::Float(CtFloat::f64(math_lib_pure::jet_std_math_significand(
                as_ct_float(one(0)?, span)?.as_f64(),
            ))))
        }
        ("core.math", "ulp") => {
            Ok(CtValue::Float(CtFloat::f64(math_lib_pure::jet_std_math_ulp(
                as_ct_float(one(0)?, span)?.as_f64(),
            ))))
        }
        ("core.math", "erf") => {
            Ok(CtValue::Float(CtFloat::f64(math_lib_pure::jet_std_math_erf(
                as_ct_float(one(0)?, span)?.as_f64(),
            ))))
        }
        ("core.math", "erfc") => {
            Ok(CtValue::Float(CtFloat::f64(
                1.0 - math_lib_pure::jet_std_math_erf(as_ct_float(one(0)?, span)?.as_f64()),
            )))
        }
        ("core.math", "gamma") => {
            Ok(CtValue::Float(CtFloat::f64(math_lib_pure::jet_std_math_gamma(
                as_ct_float(one(0)?, span)?.as_f64(),
            ))))
        }
        ("core.math", "lgamma") => {
            let g = math_lib_pure::jet_std_math_gamma(as_ct_float(one(0)?, span)?.as_f64());
            Ok(CtValue::Float(CtFloat::f64(if g.is_nan() || g <= 0.0 {
                f64::NAN
            } else {
                g.ln()
            })))
        }
        ("core.math", "sin_cos") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            let (s, c) = x.sin_cos();
            Ok(named_tuple(&[
                ("sin", CtValue::Float(CtFloat::f64(s))),
                ("cos", CtValue::Float(CtFloat::f64(c))),
            ]))
        }
        ("core.math", "modf") => {
            let x = as_ct_float(one(0)?, span)?;
            Ok(named_tuple(&[
                ("fract", CtValue::Float(x.fract())),
                ("whole", CtValue::Float(x.trunc())),
            ]))
        }
        ("core.math", "frexp") => {
            let x = as_ct_float(one(0)?, span)?.as_f64();
            let exp = math_lib_pure::jet_std_math_ilogb(x).unwrap_or(0);
            let frac = if x == 0.0 || !x.is_finite() {
                x
            } else {
                math_lib_pure::jet_std_math_ldexp(x, -exp)
            };
            Ok(named_tuple(&[
                ("frac", CtValue::Float(CtFloat::f64(frac))),
                ("exp", CtValue::Int(exp)),
            ]))
        }
        ("core.math", "div_mod" | "div_rem") => {
            let a = match one(0)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("div_mod needs whole numbers", span)),
            };
            let b = match one(1)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("div_mod needs whole numbers", span)),
            };
            if b == 0 {
                return Err(unsupported("division by zero", span));
            }
            let (q, r) = if method == "div_mod" {
                (a.div_euclid(b), a.rem_euclid(b))
            } else {
                (a / b, a % b)
            };
            Ok(named_tuple(&[("quot", CtValue::Int(q)), ("rem", CtValue::Int(r))]))
        }
        ("core.math", "hypot") => {
            let left = as_ct_float(one(0)?, span)?;
            let right = as_ct_float(one(1)?, span)?;
            Ok(CtValue::Float(left.hypot(right).ok_or_else(|| {
                unsupported("mixing float widths", span)
            })?))
        }
        ("core.math", "lerp") => {
            let left = as_ct_float(one(0)?, span)?;
            let right = as_ct_float(one(1)?, span)?;
            let t = as_ct_float(one(2)?, span)?;
            Ok(CtValue::Float(left.lerp(right, t).ok_or_else(|| {
                unsupported("mixing float widths", span)
            })?))
        }
        ("core.math", "is_nan") => Ok(CtValue::Bool(as_ct_float(one(0)?, span)?.is_nan())),
        ("core.math", "is_inf") => Ok(CtValue::Bool(as_ct_float(one(0)?, span)?.is_infinite())),
        ("core.math", "is_finite") => Ok(CtValue::Bool(as_ct_float(one(0)?, span)?.is_finite())),
        ("core.math", "sign") => Ok(CtValue::Int(as_ct_float(one(0)?, span)?.sign())),
        ("core.math", "to_bits") => Ok(CtValue::Int(as_ct_float(one(0)?, span)?.to_bits_i64())),
        ("core.math", "from_bits") => Ok(CtValue::Float(CtFloat::f64(f64::from_bits(
            as_int(one(0)?, span)? as u64,
        )))),
        ("core.math", "checked_add") => {
            let a = as_int(one(0)?, span)?;
            let b = as_int(one(1)?, span)?;
            Ok(match a.checked_add(b) {
                Some(n) => CtValue::Present(Box::new(CtValue::Int(n))),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", "checked_div") => {
            let a = as_int(one(0)?, span)?;
            let b = as_int(one(1)?, span)?;
            Ok(match a.checked_div(b) {
                Some(n) => CtValue::Present(Box::new(CtValue::Int(n))),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", "checked_rem") => {
            let a = as_int(one(0)?, span)?;
            let b = as_int(one(1)?, span)?;
            Ok(match a.checked_rem(b) {
                Some(n) => CtValue::Present(Box::new(CtValue::Int(n))),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", "checked_sub") => {
            let a = as_int(one(0)?, span)?;
            let b = as_int(one(1)?, span)?;
            Ok(match a.checked_sub(b) {
                Some(n) => CtValue::Present(Box::new(CtValue::Int(n))),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", "checked_mul") => {
            let a = as_int(one(0)?, span)?;
            let b = as_int(one(1)?, span)?;
            Ok(match a.checked_mul(b) {
                Some(n) => CtValue::Present(Box::new(CtValue::Int(n))),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.math", "checked_pow") => {
            let base = as_int(one(0)?, span)?;
            let exp = as_int(one(1)?, span)?;
            Ok(if exp < 0 {
                CtValue::absent(Type::Int)
            } else {
                match base.checked_pow(exp as u32) {
                    Some(n) => CtValue::Present(Box::new(CtValue::Int(n))),
                    None => CtValue::absent(Type::Int),
                }
            })
        }
        ("core.math", "saturating_add") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.saturating_add(as_int(one(1)?, span)?),
        )),
        ("core.math", "saturating_sub") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.saturating_sub(as_int(one(1)?, span)?),
        )),
        ("core.math", "saturating_mul") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.saturating_mul(as_int(one(1)?, span)?),
        )),
        ("core.math", "wrapping_add") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.wrapping_add(as_int(one(1)?, span)?),
        )),
        ("core.math", "wrapping_sub") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.wrapping_sub(as_int(one(1)?, span)?),
        )),
        ("core.math", "wrapping_mul") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.wrapping_mul(as_int(one(1)?, span)?),
        )),
        ("core.math", "int_pow") => {
            let base = as_int(one(0)?, span)?;
            let exp = as_int(one(1)?, span)?;
            Ok(CtValue::Int(if exp < 0 {
                0
            } else {
                base.saturating_pow(exp as u32)
            }))
        }
        ("core.math", "gcd") => {
            let mut a = as_int(one(0)?, span)?.abs();
            let mut b = as_int(one(1)?, span)?.abs();
            while b != 0 {
                let r = a % b;
                a = b;
                b = r;
            }
            Ok(CtValue::Int(a))
        }
        ("core.math", "lcm") => {
            let a = as_int(one(0)?, span)?;
            let b = as_int(one(1)?, span)?;
            Ok(CtValue::Int(if a == 0 || b == 0 {
                0
            } else {
                let mut x = a.abs();
                let mut y = b.abs();
                while y != 0 {
                    let r = x % y;
                    x = y;
                    y = r;
                }
                (a / x).saturating_mul(b).abs()
            }))
        }
        // --- core.text module whitelist (card #392: `"core.string"` was a
        // dead key here — no import ever resolves to it, `core.text` is the
        // only ratified spelling (KNOWN_CORE_MODULES), so every arm below was
        // unreachable and every `use core.text as t; t.trim(s)`-style call
        // hit the E0956 fallback. Logic ported verbatim from AOT's
        // `jet_text_*` prelude fns via `TextLite` — R12 parity. ---
        ("core.text", "nfc") => Ok(CtValue::Str(super::super::TextLite::nfc(as_string(one(0)?, span)?))),
        ("core.text", "nfd") => Ok(CtValue::Str(super::super::TextLite::nfd(as_string(one(0)?, span)?))),
        ("core.text", "nfkc") => Ok(CtValue::Str(super::super::TextLite::nfkc(as_string(one(0)?, span)?))),
        ("core.text", "nfkd") => Ok(CtValue::Str(super::super::TextLite::nfkd(as_string(one(0)?, span)?))),
        ("core.text", "casefold") => Ok(CtValue::Str(super::super::TextLite::casefold(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "lower") => Ok(CtValue::Str(super::super::TextLite::lower(as_string(one(0)?, span)?))),
        ("core.text", "upper") => Ok(CtValue::Str(super::super::TextLite::upper(as_string(one(0)?, span)?))),
        ("core.text", "caseless_eq") => Ok(CtValue::Bool(super::super::TextLite::caseless_eq(
            as_string(one(0)?, span)?,
            as_string(one(1)?, span)?,
        ))),
        ("core.text", "graphemes") => Ok(CtValue::List(
            super::super::TextLite::graphemes(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        ("core.text", "words") => Ok(CtValue::List(
            super::super::TextLite::words(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        ("core.text", "sentences") => Ok(CtValue::List(
            super::super::TextLite::sentences(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        ("core.text", "scalars") => Ok(CtValue::List(
            as_string(one(0)?, span)?
                .chars()
                .map(|c| CtValue::Str(c.to_string()))
                .collect(),
        )),
        // D-TEXTWIDTH1=B: 1-arg call uses the portable default policy and
        // returns a bare `Int`; the 2-arg (`policy:`) call can reject a
        // control character under `.Reject`, so it returns `Int ? TextError`.
        // `TextWidth`'s two enum fields evaluate generically (`CtValue::Struct`/
        // `CtValue::Enum`, no per-type interpreter code needed) — this arm
        // just reads them back out.
        ("core.text", "display_width") => {
            let s = as_string(one(0)?, span)?;
            if let Some(policy) = args.get(1) {
                let (ambiguous_wide, controls_reject) = text_width_policy_flags(policy);
                match super::super::TextLite::display_width_policy(s, ambiguous_wide, controls_reject) {
                    Ok(n) => Ok(CtValue::Present(Box::new(CtValue::Int(n)))),
                    Err(message) => Ok(CtValue::failed(Box::new(CtValue::Struct {
                        type_name: "TextError".to_string(),
                        fields: vec![("message".to_string(), CtValue::Str(message))],
                    }))),
                }
            } else {
                Ok(CtValue::Int(super::super::TextLite::display_width_default(s)))
            }
        }
        ("core.text", "scalar_count") => {
            Ok(CtValue::Int(as_string(one(0)?, span)?.chars().count() as i64))
        }
        ("core.text", "byte_count") => Ok(CtValue::Int(as_string(one(0)?, span)?.len() as i64)),
        ("core.text", "is_alphabetic") => Ok(CtValue::Bool(super::super::TextLite::is_alphabetic(
            as_string(one(0)?, span)?,
        ))),
        ("core.text", "is_numeric") => Ok(CtValue::Bool(super::super::TextLite::is_numeric(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "is_whitespace") => Ok(CtValue::Bool(super::super::TextLite::is_whitespace(
            as_string(one(0)?, span)?,
        ))),
        ("core.text", "is_ascii") => Ok(CtValue::Bool(as_string(one(0)?, span)?.is_ascii())),
        ("core.text", "splitn") => {
            let s = as_string(one(0)?, span)?.to_string();
            let pat = as_string(one(1)?, span)?.to_string();
            let n = as_int(one(2)?, span)?;
            Ok(CtValue::List(
                super::super::TextLite::splitn(&s, &pat, n)
                    .into_iter()
                    .map(CtValue::Str)
                    .collect(),
            ))
        }
        ("core.text", "rsplitn") => {
            let s = as_string(one(0)?, span)?.to_string();
            let pat = as_string(one(1)?, span)?.to_string();
            let n = as_int(one(2)?, span)?;
            Ok(CtValue::List(
                super::super::TextLite::rsplitn(&s, &pat, n)
                    .into_iter()
                    .map(CtValue::Str)
                    .collect(),
            ))
        }
        ("core.text", "trim") => Ok(CtValue::Str(super::super::TextLite::trim(as_string(one(0)?, span)?))),
        ("core.text", "trim_start") => Ok(CtValue::Str(super::super::TextLite::trim_start(
            as_string(one(0)?, span)?,
        ))),
        ("core.text", "trim_end") => Ok(CtValue::Str(super::super::TextLite::trim_end(
            as_string(one(0)?, span)?,
        ))),
        ("core.text", "pad_start") => {
            let s = as_string(one(0)?, span)?.to_string();
            let w = as_int(one(1)?, span)?;
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(super::super::TextLite::pad_start(&s, w, &fill)))
        }
        ("core.text", "pad_end") => {
            let s = as_string(one(0)?, span)?.to_string();
            let w = as_int(one(1)?, span)?;
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(super::super::TextLite::pad_end(&s, w, &fill)))
        }
        ("core.text", "center") => {
            let s = as_string(one(0)?, span)?.to_string();
            let w = as_int(one(1)?, span)?;
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(super::super::TextLite::center(&s, w, &fill)))
        }
        ("core.text", "starts_any") => {
            let s = as_string(one(0)?, span)?.to_string();
            let prefixes = match one(1)? {
                CtValue::List(xs) => xs
                    .iter()
                    .map(|v| as_string(v, span).map(|s| s.to_string()))
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err(unsupported("core.text.starts_any: non-list argument", span)),
            };
            Ok(CtValue::Bool(super::super::TextLite::starts_any(&s, &prefixes)))
        }
        ("core.text", "ends_any") => {
            let s = as_string(one(0)?, span)?.to_string();
            let suffixes = match one(1)? {
                CtValue::List(xs) => xs
                    .iter()
                    .map(|v| as_string(v, span).map(|s| s.to_string()))
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err(unsupported("core.text.ends_any: non-list argument", span)),
            };
            Ok(CtValue::Bool(super::super::TextLite::ends_any(&s, &suffixes)))
        }
        ("core.text", "char_indices") => Ok(CtValue::List(
            super::super::TextLite::char_indices(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        // --- core.path (pure) ---
        ("core.path", "join") => {
            let a = as_string(one(0)?, span)?;
            let b = as_string(one(1)?, span)?;
            let joined = std::path::Path::new(a)
                .join(b)
                .to_string_lossy()
                .into_owned();
            Ok(CtValue::Str(joined))
        }
        // D-ARGS1 / runtime-tier: empty ArgsSpec builder (same as AOT jet_args_spec).
        ("core.args", "spec") => Ok(crate::Comptime::core_args_spec()),
        // --- core.event (shared TIR evaluator / deopt; mirrors AOT JetEvent*) ---
        ("core.event", "scope") => Ok(crate::Comptime::core_event_scope()),
        ("core.event", "policy_sync") => Ok(crate::Comptime::core_event_policy_sync()),
        ("core.event", "new") => Ok(crate::Comptime::core_event_new()),
        ("core.event", "with_policy") => Ok(crate::Comptime::core_event_with_policy(one(0)?.clone())),
        ("core.event", "hook") => Ok(crate::Comptime::core_event_hook(one(0)?.clone())),
        ("core.event", "decision_hook") => {
            Ok(crate::Comptime::core_event_decision_hook(one(0)?.clone()))
        }
        ("core.event", "async_result") => {
            crate::Comptime::core_event_async_result(one(0)?, one(1)?, span)
        }
        ("core.path", "parent") => {
            let p = as_string(one(0)?, span)?;
            Ok(CtValue::Str(
                std::path::Path::new(p)
                    .parent()
                    .map(|x| x.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        ("core.path", "extension") => {
            let p = as_string(one(0)?, span)?;
            Ok(CtValue::Str(
                std::path::Path::new(p)
                    .extension()
                    .map(|x| x.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        ("core.path", "normalize") => {
            let p = as_string(one(0)?, span)?;
            Ok(CtValue::Str(
                std::path::Path::new(p)
                    .components()
                    .collect::<std::path::PathBuf>()
                    .to_string_lossy()
                    .into_owned(),
            ))
        }
        // --- core.encoding.json ---
        ("core.encoding.json", "parse") => {
            let text = as_string(one(0)?, span)?;
            match super::super::JSONInterp::parse_json(text) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(
                    super::super::JSONInterp::json_error_value(e),
                ))),
            }
        }
        ("core.encoding.json", "decode") => {
            // D-JSON3's lenient coercions emit structured audit records. The
            // comptime interpreter has no runtime log-effect seam, so claiming
            // this call would silently drop observable behavior. Stop at the
            // honest boundary; default dev transparently executes the AOT TIR.
            Err(unsupported("JSON lenient decode coercion audit effects", span))
        }
        ("core.encoding.json", "to_string") => {
            let v = one(0)?;
            Ok(CtValue::Str(super::super::JSONInterp::render_json_pretty(
                v, false, 0,
            )))
        }
        ("core.encoding.json", "to_string_pretty") => {
            let v = one(0)?;
            Ok(CtValue::Str(super::super::JSONInterp::render_json_pretty(
                v, true, 0,
            )))
        }
        // --- card #392 pass 4 / #1394: core.encoding.json.canonical/events ---
        ("core.encoding.json", "canonical") => {
            let v = one(0)?;
            if jet_foundation::PackageEdition::package_edition_at_least("2027") {
                let limits = if args.len() >= 2 {
                    match super::super::EncodingLite::encoding_limits_from_value(one(1)?) {
                        Ok(limits) => limits,
                        Err(message) => {
                            return Err(unsupported(&message, span));
                        }
                    }
                } else {
                    super::super::EncodingLite::EncodingLimitsLite::safe()
                };
                match super::super::EncodingLite::json_canonical_jcs(v, &limits) {
                    Ok(text) => Ok(CtValue::Present(Box::new(CtValue::Str(text)))),
                    Err(error) => Ok(CtValue::failed(Box::new(error))),
                }
            } else {
                Ok(CtValue::Str(super::super::EncodingLite::json_canonical(v)))
            }
        }
        ("core.encoding.json", "events") => {
            Ok(CtValue::Str(super::super::EncodingLite::json_events(one(0)?)))
        }
        // --- core.encoding.jsonl (ported verbatim, `EncodingLite.rs`) ---
        ("core.encoding.jsonl", "parse") => {
            let text = as_string(one(0)?, span)?;
            match super::super::EncodingLite::jsonl_parse(text) {
                Ok(rows) => Ok(CtValue::Present(Box::new(CtValue::List(rows)))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        ("core.encoding.jsonl", "to_string") => {
            let rows = match one(0)? {
                CtValue::List(xs) => xs.clone(),
                _ => return Err(unsupported("core.encoding.jsonl.to_string: expected a list", span)),
            };
            Ok(CtValue::Str(super::super::EncodingLite::jsonl_render(&rows)))
        }
        // --- core.encoding.csv (ported verbatim, `EncodingLite.rs`) ---
        ("core.encoding.csv", "parse") => {
            let text = as_string(one(0)?, span)?;
            match super::super::EncodingLite::csv_parse(text) {
                Ok(rows) => Ok(CtValue::Present(Box::new(CtValue::List(
                    rows.into_iter()
                        .map(|row| CtValue::List(row.into_iter().map(CtValue::Str).collect()))
                        .collect(),
                )))),
                Err(e) => Ok(CtValue::failed(Box::new(CtValue::Str(e)))),
            }
        }
        ("core.encoding.csv", "to_string") => {
            // Two shapes, same as AOT and the resident JIT: dynamic `[[String]]`
            // rows, or a typed `[T]` list of `#Codable` values (#1269).
            let arg = one(0)?;
            let rows = match csv_rows_from_records(arg) {
                Some(rows) => rows,
                None => as_string_rows(arg, span)?,
            };
            Ok(CtValue::Str(super::super::EncodingLite::csv_render(&rows)))
        }
        // --- core.encoding.toml (ported verbatim, `EncodingLite.rs`) ---
        ("core.encoding.toml", "parse") => {
            let text = as_string(one(0)?, span)?;
            match super::super::EncodingLite::toml_parse(text) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        ("core.encoding.toml", "to_string") => {
            Ok(CtValue::Str(super::super::EncodingLite::toml_render(one(0)?)))
        }
        // --- core.encoding.yaml (ported verbatim, `EncodingLite.rs`) ---
        ("core.encoding.yaml", "parse") => {
            let text = as_string(one(0)?, span)?;
            match super::super::EncodingLite::yaml_parse(text) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        ("core.encoding.yaml", "to_string") => {
            Ok(CtValue::Str(super::super::EncodingLite::yaml_render(one(0)?)))
        }
        // --- core.encoding.xml (ported verbatim, `EncodingLite.rs`) ---
        ("core.encoding.xml", "parse") => {
            let text = as_string(one(0)?, span)?;
            match super::super::EncodingLite::xml_parse(text) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(super::super::EncodingLite::xml_error_value(e)))),
            }
        }
        ("core.encoding.xml", "parse_with") => {
            let text = as_string(one(0)?, span)?;
            match super::super::EncodingLite::xml_parse_with(text, one(1)?) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(super::super::EncodingLite::xml_error_value(e)))),
            }
        }
        ("core.encoding.xml", "parse_bytes") => {
            let bytes = as_bytes(one(0)?, span)?;
            match super::super::EncodingLite::xml_parse_bytes(&bytes, args.get(1)) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(super::super::EncodingLite::xml_source_error_value(e)))),
            }
        }
        ("core.encoding.xml", "to_string") => {
            Ok(CtValue::Str(super::super::EncodingLite::xml_render(one(0)?)))
        }
        ("core.encoding.xml", "to_bytes") => {
            match super::super::EncodingLite::xml_to_bytes(one(0)?, args.get(1)) {
                Ok(bytes) => Ok(CtValue::Present(Box::new(CtValue::Bytes(bytes)))),
                Err(error) => Ok(CtValue::failed(Box::new(error))),
            }
        }
        // D-ENCXML-PROJECTION1=A: focused helpers (shared foundation projection).
        ("core.encoding.xml", "root") => {
            match super::super::EncodingLite::xml_root(one(0)?) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        ("core.encoding.xml", "expanded_name") => {
            match super::super::EncodingLite::xml_expanded_name(one(0)?) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        ("core.encoding.xml", "attribute") => {
            let name = as_string(one(1)?, span)?;
            match super::super::EncodingLite::xml_attribute(one(0)?, name) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        ("core.encoding.xml", "content") => {
            match super::super::EncodingLite::xml_content(one(0)?) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(e))),
            }
        }
        // --- core.encoding.cbor (ported verbatim, `EncodingLite.rs`) ---
        // D-ENC-CBOR-SURFACE1: current whole-value names return the same
        // Result shape as AOT. Edition compatibility names remain below.
        ("core.encoding.cbor", "to_bytes") => Ok(CtValue::Present(Box::new(
            CtValue::Bytes(super::super::EncodingLite::cbor_encode(one(0)?)),
        ))),
        ("core.encoding.cbor", "to_bytes_canonical") => Ok(CtValue::Present(Box::new(
            CtValue::Bytes(super::super::EncodingLite::cbor_encode_canonical(one(0)?)),
        ))),
        ("core.encoding.cbor", "parse") => {
            let bytes = as_bytes(one(0)?, span)?;
            let options = match super::super::EncodingLite::cbor_options(args.get(1)) {
                Ok(options) => options,
                Err(error) => {
                    return Ok(CtValue::failed(Box::new(
                        super::super::EncodingLite::cbor_error_value(error),
                    )))
                }
            };
            match super::super::EncodingLite::cbor_decode(&bytes, &options, false) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(error) => Ok(CtValue::failed(Box::new(
                    super::super::EncodingLite::cbor_error_value(error),
                ))),
            }
        }
        ("core.encoding.cbor", "encode") => {
            Ok(CtValue::Bytes(super::super::EncodingLite::cbor_encode(one(0)?)))
        }
        ("core.encoding.cbor", "decode") => {
            let bytes = as_bytes(one(0)?, span)?;
            let options = super::super::EncodingLite::cbor_safe_options();
            match super::super::EncodingLite::cbor_decode(&bytes, &options, false) {
                Ok(v) => Ok(CtValue::Present(Box::new(v))),
                Err(e) => Ok(CtValue::failed(Box::new(CtValue::Str(e.reason)))),
            }
        }
        // --- core.time pure constructors ---
        // D-DET1: testing.fake_clock is the test-facing spelling of the
        // caller-seeded deterministic Clock capability built by Clock.new.
        ("core.testing", "fake_clock") => {
            let seed = match one(0)? {
                CtValue::Int(v) => *v,
                _ => {
                    return Err(unsupported("testing.fake_clock expects an Int seed", span));
                }
            };
            Ok(CtValue::Struct {
                type_name: crate::Syntax::CLOCK_TYPE.to_string(),
                fields: vec![("now".to_string(), CtValue::Int(seed))],
            })
        }
        // --- core.regex / core.regex (D-REGEXENGINE1) ---
        ("core.regex", "literal") => {
            let pattern = as_string(one(0)?, span)?;
            jet_foundation::RegexSyntax::validate(pattern).map_err(|error| {
                Diagnostic::error(
                    "E0152",
                    format!(
                        "this regex pattern is invalid at position {}",
                        error.offset
                    ),
                    error.reason,
                    "fix the pattern at the reported position".to_string(),
                    Some(span),
                )
            })?;
            Ok(CtValue::Struct {
                type_name: "__JetRegex".to_string(),
                fields: vec![("pattern".to_string(), CtValue::Str(pattern.to_string()))],
            })
        }
        ("core.regex", "is_match") => regex_is_match(args, span),
        ("core.regex", "find") => regex_find(args, span),
        ("core.regex", "find_all") => regex_find_all(args, span),
        ("core.regex", "matches") => regex_matches(args, span),
        ("core.regex", "split") => regex_split(args, span),
        ("core.regex", "split_limit") => regex_split_limit(args, span),
        ("core.regex", "replace") => regex_replace(args, span, false),
        ("core.regex", "replace_all") => regex_replace(args, span, true),
        ("core.regex", "match") => regex_match(args, span),
        // --- core.random (ambient; seed for deterministic REPL transcripts) ---
        ("core.random", "seed") => {
            let seed = match one(0)? {
                CtValue::Int(n) => *n as u64,
                _ => return Err(unsupported("random.seed expects an Int", span)),
            };
            with_ambient_rng(|st| *st = seed);
            Ok(CtValue::Unit)
        }
        ("core.random", "int") => {
            let low = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.int expects Int bounds", span)),
            };
            let high = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.int expects Int bounds", span)),
            };
            Ok(CtValue::Int(with_ambient_rng(|st| {
                random_int(st, low, high)
            })))
        }
        ("core.random", "float") => Ok(CtValue::Float(CtFloat::f64(with_ambient_rng(|st| {
            random_float(st)
        })))),
        // D-DET1: testing.fake_rng is the test-facing spelling of the same
        // caller-seeded deterministic Rng capability as random.rng.
        ("core.random", "rng") | ("core.testing", "fake_rng") => {
            let seed = match one(0)? {
                CtValue::Int(n) => *n as u64,
                _ => {
                    let api = if method == "rng" {
                        "random.rng"
                    } else {
                        "testing.fake_rng"
                    };
                    return Err(unsupported(&format!("{api} expects an Int seed"), span));
                }
            };
            Ok(CtValue::Struct {
                type_name: crate::Syntax::RNG_TYPE.to_string(),
                fields: vec![("state".to_string(), CtValue::Int(seed as i64))],
            })
        }
        ("core.random", "split") => {
            let seed = match one(0)? {
                CtValue::Int(n) => *n as u64,
                _ => return Err(unsupported("random.split expects an Int seed", span)),
            };
            let mixed = with_ambient_rng(|st| seed ^ splitmix64(st).rotate_left(17));
            Ok(CtValue::Struct {
                type_name: crate::Syntax::RNG_TYPE.to_string(),
                fields: vec![("state".to_string(), CtValue::Int(mixed as i64))],
            })
        }
        ("core.random", "float_range") => {
            let low = as_float(one(0)?, span)?;
            let high = as_float(one(1)?, span)?;
            Ok(CtValue::Float(CtFloat::f64(with_ambient_rng(|st| {
                random_float_range(st, low, high)
            }))))
        }
        ("core.random", "bool") => {
            let p = as_float(one(0)?, span)?;
            Ok(CtValue::Bool(with_ambient_rng(|st| random_bool_p(st, p))))
        }
        ("core.random", "normal") => {
            let mean = as_float(one(0)?, span)?;
            let stddev = as_float(one(1)?, span)?;
            Ok(CtValue::Float(CtFloat::f64(with_ambient_rng(|st| {
                random_normal(st, mean, stddev)
            }))))
        }
        ("core.random", "exponential") => {
            let lambda = as_float(one(0)?, span)?;
            Ok(CtValue::Float(CtFloat::f64(with_ambient_rng(|st| {
                random_exponential(st, lambda)
            }))))
        }
        ("core.random", "bytes") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.bytes expects an Int count", span)),
            };
            Ok(CtValue::Bytes(with_ambient_rng(|st| random_bytes(st, n))))
        }
        ("core.random", "pick") => {
            let CtValue::List(xs) = one(0)? else {
                return Err(unsupported("random.pick needs a list", span));
            };
            Ok(match with_ambient_rng(|st| random_pick_ct(st, xs)) {
                Some(v) => CtValue::Present(Box::new(v)),
                None => CtValue::absent(Type::Int),
            })
        }
        ("core.random", "weighted_pick") => {
            let CtValue::List(xs) = one(0)? else {
                return Err(unsupported("random.weighted_pick needs a list", span));
            };
            let CtValue::List(ws) = one(1)? else {
                return Err(unsupported(
                    "random.weighted_pick needs a [Float] weights list",
                    span,
                ));
            };
            let weights: Vec<f64> = ws
                .iter()
                .map(|w| as_float(w, span))
                .collect::<Result<_, _>>()?;
            Ok(
                match with_ambient_rng(|st| random_weighted_pick_ct(st, xs, &weights)) {
                    Some(v) => CtValue::Present(Box::new(v)),
                    None => CtValue::absent(Type::Int),
                },
            )
        }
        ("core.random", "sample") => {
            let CtValue::List(xs) = one(0)? else {
                return Err(unsupported("random.sample needs a list", span));
            };
            let k = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.sample count must be Int", span)),
            };
            Ok(CtValue::List(with_ambient_rng(|st| {
                random_sample_ct(st, xs, k)
            })))
        }
        // --- core.fmt (pure text formatting; mirrors AOT's `jet_fmt_*`, DataFmt.rs) ---
        ("core.fmt", "number") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.number expects an Int", span)),
            };
            Ok(CtValue::Str(comma_int_ct(n)))
        }
        ("core.fmt", "decimal") => {
            let value = as_float(one(0)?, span)?;
            let precision = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.decimal precision must be Int", span)),
            };
            Ok(CtValue::Str(fmt_decimal_ct(value, precision)))
        }
        ("core.fmt", "percent") => {
            let value = as_float(one(0)?, span)?;
            let precision = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.percent precision must be Int", span)),
            };
            Ok(CtValue::Str(format!(
                "{}%",
                fmt_decimal_ct(value * 100.0, precision)
            )))
        }
        ("core.fmt", "bytes") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.bytes expects an Int", span)),
            };
            Ok(CtValue::Str(fmt_bytes_ct(n)))
        }
        ("core.fmt", "duration") => {
            let ms = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.duration expects an Int (ms)", span)),
            };
            Ok(CtValue::Str(fmt_duration_ct(ms)))
        }
        ("core.fmt", "ordinal") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.ordinal expects an Int", span)),
            };
            let abs = n.abs();
            let suffix = if (11..=13).contains(&(abs % 100)) {
                "th"
            } else {
                match abs % 10 {
                    1 => "st",
                    2 => "nd",
                    3 => "rd",
                    _ => "th",
                }
            };
            Ok(CtValue::Str(format!("{}{}", comma_int_ct(n), suffix)))
        }
        ("core.fmt", "plural") => {
            let count = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.plural count must be Int", span)),
            };
            let singular = as_string(one(1)?, span)?;
            let plural = as_string(one(2)?, span)?;
            let word = if count.abs() == 1 { singular } else { plural };
            Ok(CtValue::Str(format!("{} {}", comma_int_ct(count), word)))
        }
        ("core.fmt", "pad_left") => {
            let text = as_string(one(0)?, span)?;
            let width = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.pad_left width must be Int", span)),
            };
            let fill = as_string(one(2)?, span)?;
            let need = pad_need_ct(text, width);
            Ok(CtValue::Str(format!("{}{}", pad_fill_ct(fill, need), text)))
        }
        ("core.fmt", "pad_right") => {
            let text = as_string(one(0)?, span)?;
            let width = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.pad_right width must be Int", span)),
            };
            let fill = as_string(one(2)?, span)?;
            let need = pad_need_ct(text, width);
            Ok(CtValue::Str(format!("{}{}", text, pad_fill_ct(fill, need))))
        }
        ("core.fmt", "pad_center") => {
            let text = as_string(one(0)?, span)?;
            let width = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.pad_center width must be Int", span)),
            };
            let fill = as_string(one(2)?, span)?;
            let need = pad_need_ct(text, width);
            let left = need / 2;
            let right = need - left;
            Ok(CtValue::Str(format!(
                "{}{}{}",
                pad_fill_ct(fill, left),
                text,
                pad_fill_ct(fill, right)
            )))
        }
        // --- D-ANY-JAI1: core.reflect (the runtime reflection floor, pure).
        // `"__Reflect"`/`"__ReflectField"` are internal-only tags (like
        // `"TypeInfo"`/`"Match"`/`"IOError"` elsewhere in this file) — never a
        // real Jet type name a user can write, so no `Syntax.rs` entry (I7 is
        // about user-typeable names). `.type_name`/`.fields` are plain reads
        // (`Builtins::apply_method`); `.display` needs `&mut self` (it may
        // run a user `Display` impl), so it's dispatched in `eval_method`.
        ("core.reflect", "of") => Ok(CtValue::Struct {
            type_name: "__Reflect".to_string(),
            fields: vec![("value".to_string(), one(0)?.clone())],
        }),
        // --- D-UUIDENC1=A: core.encoding.hex / core.encoding.base64 (pure) ---
        ("core.encoding.hex", "encode") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(hex_encode(bytes)))
        }
        ("core.encoding.hex", "decode") => {
            let s = as_string(one(0)?, span)?;
            Ok(match hex_decode(s) {
                Some(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                None => CtValue::failed(Box::new(CtValue::Str(format!("`{}` isn't valid hex", s)))),
            })
        }
        ("core.encoding.base64", "encode") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(base64_encode(bytes)))
        }
        ("core.encoding.base64", "decode") => {
            let s = as_string(one(0)?, span)?;
            let allow_whitespace = args_bool(1, false)?;
            let allow_missing_padding = args_bool(2, false)?;
            let edition = jet_foundation::PackageEdition::package_edition();
            Ok(match jet_foundation::base_encoding_dispatch::decode_base64(
                &edition,
                s,
                allow_whitespace,
                allow_missing_padding,
            ) {
                Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            })
        }
        // --- core.encoding.base64 URL-safe variant (pure; mirrors AOT's
        // `jet_std_b64url_*`, EncodingCodecs.rs — the same alphabet with
        // `+`/`/` swapped for `-`/`_` and no padding) ---
        ("core.encoding.base64", "encode_url") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(
                base64_encode(bytes)
                    .trim_end_matches('=')
                    .replace('+', "-")
                    .replace('/', "_"),
            ))
        }
        ("core.encoding.base64", "decode_url") => {
            let s = as_string(one(0)?, span)?;
            let allow_whitespace = args_bool(1, false)?;
            let allow_padding = args_bool(2, false)?;
            let edition = jet_foundation::PackageEdition::package_edition();
            Ok(match jet_foundation::base_encoding_dispatch::decode_base64url(
                &edition,
                s,
                allow_whitespace,
                allow_padding,
            ) {
                Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            })
        }
        // --- core.encoding.base32 (pure; mirrors AOT's `jet_std_base32_*`,
        // EncodingCodecs.rs, byte-for-byte — same alphabet, same bit-packing) ---
        ("core.encoding.base32", "encode") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(base32_encode(&bytes)))
        }
        ("core.encoding.base32", "decode") => {
            let s = as_string(one(0)?, span)?;
            let allow_whitespace = args_bool(1, false)?;
            let allow_missing_padding = args_bool(2, false)?;
            let allow_lowercase = args_bool(3, false)?;
            let edition = jet_foundation::PackageEdition::package_edition();
            Ok(match jet_foundation::base_encoding_dispatch::decode_base32(
                &edition,
                s,
                allow_whitespace,
                allow_missing_padding,
                allow_lowercase,
            ) {
                Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
            })
        }
        // --- D-URL1=A: core.url (pure RFC-3986-shaped parser, ported
        // verbatim from AOT's `JetURL`/`jet_url_*` in `UrlMime.rs` — see
        // `UrlLite.rs`) ---
        ("core.url", "parse") => {
            let s = as_string(one(0)?, span)?;
            Ok(match super::super::UrlLite::UrlParts::parse(s) {
                Ok(u) => CtValue::Present(Box::new(url_parts_to_ct(&u))),
                Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
            })
        }
        ("core.url", "from_parts") => {
            let scheme = as_string(one(0)?, span)?.to_string();
            let host = as_string(one(1)?, span)?.to_string();
            let path = as_string(one(2)?, span)?.to_string();
            let query = as_string_rows(one(3)?, span)?;
            let fragment = as_string(one(4)?, span)?.to_string();
            Ok(
                match super::super::UrlLite::UrlParts::from_parts(&scheme, &host, &path, &query, &fragment)
                {
                    Ok(u) => CtValue::Present(Box::new(url_parts_to_ct(&u))),
                    Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
                },
            )
        }
        ("core.url", "file") => {
            let path = as_string(one(0)?, span)?;
            Ok(url_parts_to_ct(&super::super::UrlLite::UrlParts::file(path)))
        }
        ("core.url", "data") => {
            // `mime` arg is a `CtValue::Struct { type_name: "Mime", .. }`
            // (D-URL1's `Mime` type) with `top`/`sub`/`params` fields — the
            // `core.mime` module port isn't in this card's slice, so render
            // its essence + params here the same way AOT's
            // `JetMIME::to_string_value` does, matching field-for-field.
            let mime = one(0)?;
            let text = as_string(one(1)?, span)?;
            let rendered = match mime {
                CtValue::Struct { type_name, fields } if type_name == "Mime" => {
                    let get = |name: &str| {
                        fields
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, v)| v.clone())
                    };
                    let top = match get("top") {
                        Some(CtValue::Str(s)) => s,
                        _ => return Err(unsupported("core.url.data: mime.top must be String", span)),
                    };
                    let sub = match get("sub") {
                        Some(CtValue::Str(s)) => s,
                        _ => return Err(unsupported("core.url.data: mime.sub must be String", span)),
                    };
                    let mut out = format!("{}/{}", top, sub);
                    if let Some(CtValue::List(params)) = get("params") {
                        for p in params {
                            if let CtValue::List(kv) = p {
                                if let [CtValue::Str(k), CtValue::Str(v)] = &kv[..] {
                                    out.push_str("; ");
                                    out.push_str(k);
                                    out.push('=');
                                    out.push_str(v);
                                }
                            }
                        }
                    }
                    out
                }
                _ => return Err(unsupported("core.url.data: first argument must be a Mime", span)),
            };
            Ok(url_parts_to_ct(&super::super::UrlLite::UrlParts::data(
                &rendered, text,
            )))
        }
        ("core.url", "query") => {
            let rows = as_string_rows(one(0)?, span)?;
            let pairs: Vec<(String, String)> = rows
                .iter()
                .filter(|r| !r.is_empty())
                .map(|r| {
                    (
                        r.get(0).cloned().unwrap_or_default(),
                        r.get(1).cloned().unwrap_or_default(),
                    )
                })
                .collect();
            Ok(CtValue::Str(super::super::UrlLite::url_render_query(&pairs)))
        }
        ("core.url", "percent_encode") => {
            let s = as_string(one(0)?, span)?;
            Ok(CtValue::Str(super::super::UrlLite::url_percent_encode(s, false)))
        }
        ("core.url", "percent_decode") => {
            let s = as_string(one(0)?, span)?;
            Ok(match super::super::UrlLite::url_percent_decode_str(s) {
                Ok(v) => CtValue::Present(Box::new(CtValue::Str(v))),
                Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
            })
        }
        // D-COMPUTE1=D / I9: same Prelude as AOT (`ComputeLite` includes Compute.rs).
        ("core.compute", method) => super::super::ComputeLite::apply(method, &args, span),
        // D-SERVICE1=D / I9: same Prelude as AOT (`ServicesLite` includes Services.rs).
        ("core.services", method) => super::super::ServicesLite::apply(method, &args, span),
        // D-AUTH1=A / I9: session batteries (JWT/PASETO stay on AOT/subset path).
        // Stateful store ops are Tier-2 (`is_tier2_core_call`) so pure
        // `evaluate_constant` cannot fold them into Ok(literals) while leaving
        // the runtime `JET_AUTH_STORE` empty. AuthLite still serves impure /
        // interpreter ambient via `apply_impure_core_call` → here.
        ("core.auth", method)
            if matches!(
                method,
                "register_user"
                    | "password_login"
                    | "session_validate"
                    | "session_show"
                    | "session_user"
                    | "session_cookie"
                    | "session_id"
                    | "magic_link_issue"
                    | "magic_link_consume"
                    | "oauth_begin"
                    | "oauth_finish"
            ) =>
        {
            super::super::AuthLite::apply(method, &args, span)
        }
        // D-SYNC1=A / D-DBPOLICY1=A / I9.
        ("core.sync", method) => super::super::SyncLite::apply(method, &args, span),
        // D-LIVEQUERY1=A / I9: same Prelude as AOT (`AppLite` includes LiveQuery.rs).
        ("app" | "core.web", method)
            if matches!(
                method,
                "live"
                    | "subscribe"
                    | "invalidate"
                    | "transact_invalidate"
                    | "signal_push"
                    | "live_get"
                    | "live_show"
                    | "live_stats"
                    | "auth"
                    | "auth_oauth"
                    | "auth_routes"
                    | "auth_show"
                    | "sync_over"
                    | "sync"
            ) =>
        {
            if matches!(
                method,
                "auth" | "auth_oauth" | "auth_routes" | "auth_show"
            ) {
                super::super::AuthLite::apply(method, &args, span)
            } else if matches!(method, "sync_over" | "sync") {
                super::super::SyncLite::apply(method, &args, span)
            } else {
                super::super::AppLite::apply(method, &args, span)
            }
        }
        // --- D-DATA-SURFACE1/PLOT1/STATUS1: core.data's fixed-signature
        // stats + plot surface. #1657 / I9: every arm below calls the one
        // Prelude kernel (`data_kernel`, included from
        // `Prelude/CoreLib/Top/DataStats.rs`) that AOT embeds and the JIT host
        // includes. The generic call-site-typed table/lazy-pipeline half of
        // `core.data` is a separate design pass and isn't here.
        ("core.data", "sum" | "mean" | "min" | "max" | "median" | "variance" | "stddev") => {
            let values = as_float_list(one(0)?, span)?;
            let (checked, unchecked): (_, fn(&Vec<f64>) -> f64) = match method {
                "sum" => (
                    data_kernel::jet_data_sum_checked(&values),
                    data_kernel::jet_data_sum,
                ),
                "mean" => (
                    data_kernel::jet_data_mean_checked(&values),
                    data_kernel::jet_data_mean,
                ),
                "min" => (
                    data_kernel::jet_data_min_checked(&values),
                    data_kernel::jet_data_min,
                ),
                "max" => (
                    data_kernel::jet_data_max_checked(&values),
                    data_kernel::jet_data_max,
                ),
                "median" => (
                    data_kernel::jet_data_median_checked(&values),
                    data_kernel::jet_data_median,
                ),
                "variance" => (
                    data_kernel::jet_data_variance_checked(&values),
                    data_kernel::jet_data_variance,
                ),
                _ => (
                    data_kernel::jet_data_stddev_checked(&values),
                    data_kernel::jet_data_stddev,
                ),
            };
            Ok(data_result_value(
                checked,
                || unchecked(&values),
                data_float_value,
            ))
        }
        ("core.data", "quantile") => {
            let values = as_float_list(one(0)?, span)?;
            let q = as_float(one(1)?, span)?;
            Ok(data_result_value(
                data_kernel::jet_data_quantile_checked(&values, q),
                || data_kernel::jet_data_quantile(&values, q),
                data_float_value,
            ))
        }
        ("core.data", "rolling_mean") => {
            let values = as_float_list(one(0)?, span)?;
            let width = as_int(one(1)?, span)?;
            Ok(data_result_value(
                data_kernel::jet_data_rolling_mean_checked(&values, width),
                || data_kernel::jet_data_rolling_mean(&values, width),
                |means| CtValue::List(means.into_iter().map(data_float_value).collect()),
            ))
        }
        ("core.data", "describe") => {
            let values = as_float_list(one(0)?, span)?;
            Ok(data_result_value(
                data_kernel::jet_data_describe_checked(&values),
                || data_kernel::jet_data_describe(&values),
                |summary| CtValue::Struct {
                    type_name: "DataSummary".to_string(),
                    fields: vec![
                        ("count".to_string(), CtValue::Int(summary.count)),
                        ("sum".to_string(), data_float_value(summary.sum)),
                        ("mean".to_string(), data_float_value(summary.mean)),
                        ("min".to_string(), data_float_value(summary.min)),
                        ("max".to_string(), data_float_value(summary.max)),
                        ("median".to_string(), data_float_value(summary.median)),
                        ("variance".to_string(), data_float_value(summary.variance)),
                        ("stddev".to_string(), data_float_value(summary.stddev)),
                    ],
                },
            ))
        }
        ("core.data", "status") => Ok(CtValue::List(
            data_kernel::jet_data_status()
                .into_iter()
                .map(|row| CtValue::Struct {
                    type_name: "DataStatus".to_string(),
                    fields: vec![
                        ("step".to_string(), CtValue::Str(row.step)),
                        ("path".to_string(), CtValue::Str(row.path)),
                        ("copy".to_string(), CtValue::Str(row.copy)),
                        ("ownership".to_string(), CtValue::Str(row.ownership)),
                        ("trust".to_string(), CtValue::Str(row.trust)),
                        ("fallback".to_string(), CtValue::Str(row.fallback)),
                        ("replacement".to_string(), CtValue::Str(row.replacement)),
                    ],
                })
                .collect(),
        )),
        ("core.data", "require_bridge") => {
            let provider = as_string(one(0)?, span)?.to_string();
            Ok(
                match data_kernel::jet_data_require_bridge(&provider) {
                    Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                    Err(error) => CtValue::failed(Box::new(data_error_value(&error))),
                },
            )
        }
        ("core.data", "bar_text") => {
            let groups = as_data_groups(one(0)?, span)?;
            Ok(data_result_value(
                data_kernel::jet_data_bar_text_checked(&groups),
                || data_kernel::jet_data_bar_text(&groups),
                CtValue::Str,
            ))
        }
        ("core.data", "bar_svg") => {
            let groups = as_data_groups(one(0)?, span)?;
            Ok(data_result_value(
                data_kernel::jet_data_bar_svg_checked(&groups),
                || data_kernel::jet_data_bar_svg(&groups),
                CtValue::Str,
            ))
        }
        ("core.data", "line_text" | "line_svg") => apply_data_line_call(method, args, span),
        // --- core.text.unicode (std-only Unicode scalar helpers, pure) ---
        ("core.text.unicode", "scalar_count") => Ok(CtValue::Int(
            as_string(one(0)?, span)?.chars().count() as i64,
        )),
        ("core.text.unicode", "byte_count") => {
            Ok(CtValue::Int(as_string(one(0)?, span)?.len() as i64))
        }
        ("core.text.unicode", "is_ascii") => {
            Ok(CtValue::Bool(as_string(one(0)?, span)?.is_ascii()))
        }
        ("core.text.unicode", "lower") => {
            Ok(CtValue::Str(super::super::TextLite::lower(as_string(one(0)?, span)?)))
        }
        ("core.text.unicode", "upper") => {
            Ok(CtValue::Str(super::super::TextLite::upper(as_string(one(0)?, span)?)))
        }
        ("core.text.unicode", "scalars") => Ok(CtValue::List(
            as_string(one(0)?, span)?
                .chars()
                .map(|c| CtValue::Str(c.to_string()))
                .collect(),
        )),
        // --- impure / build-time I/O → teaching diagnostic (reached only when
        // no #Impure gate intercepts first in eval_method) ---
        ("core.files", _)
        | ("core.env", _)
        | ("core.io", _)
        | ("core.exec", _)
        | ("core.net", _)
        | ("core.tls", _) => Err(Diagnostic::error(
            "E3410",
            format!(
                "`{}.{}()` is a Tier-2 comptime effect — it requires a `#Impure` gate",
                module, method
            ),
            "ambient I/O (filesystem, environment, process) is not allowed in \
                 pure comptime evaluation"
                .to_string(),
            format!(
                "wrap the comptime binding in `#Impure(\"reason\") {{ … }}` and \
                         pass `--allow-impure` to the build"
            ),
            Some(span),
        )),
        // --- unknown / not yet whitelisted ---
        _ => {
            if repl_mode {
                if let Some(_) = repl_native_only_module(module) {
                    return Err(repl_native_module_diag(module, method, span));
                }
            }
            Err(unsupported(
                &format!("`{}.{}()` at comptime", module, method),
                span,
            ))
        }
    }
}

/// D-CTEFFECT1: execute a Tier-2 ambient comptime I/O effect (or REPL sandbox I/O).
/// Only called when `impure_depth > 0` and `allow_impure` (comptime) or from the
/// runtime TIR evaluator used by `jet run` deopt (#778).
pub fn apply_impure_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    base_dir: &std::path::Path,
    sink: Option<&mut super::super::Interpreter::DevSink>,
    repl_mode: bool,
    pinned_executable: Option<&std::fs::File>,
    verified_root: Option<&std::fs::File>,
) -> Result<CtValue, Diagnostic> {
    // Pure CorePureParity surfaces (crypto.expert, net.socket_*, datetime, …)
    // must still resolve under ambient impure depth — same as apply_core_call.
    if let Some(result) = core_pure_parity::evaluate(module, method, &args, span) {
        return result;
    }
    if let Some(result) = crate::Comptime::try_ambient_core_call(module, method, args.clone(), span)
    {
        return result;
    }
    let one = |i: usize| {
        args.get(i).ok_or_else(|| {
            unsupported(
                &format!("`{}.{}` (wrong number of arguments)", module, method),
                span,
            )
        })
    };
    match (module, method) {
        ("core.files", "read") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::read_to_string(&path) {
                Ok(s) => Ok(CtValue::Present(Box::new(CtValue::Str(s)))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.files", "read_bytes") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::read(&path) {
                Ok(bs) => Ok(CtValue::Present(Box::new(CtValue::Bytes(bs)))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        // D-FILES-APPEND1=A: whole-file one-shot is `append_all` (not `append`,
        // which names the streaming handle's method).
        ("core.files", "write" | "append_all") => {
            let path_str = as_string(one(0)?, span)?;
            let content = as_string(one(1)?, span)?;
            let path = base_dir.join(path_str);
            let result = if method == "append_all" {
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .and_then(|mut f| f.write_all(content.as_bytes()).map(|_| ()))
            } else {
                std::fs::write(&path, content)
            };
            match result {
                Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.files", "exists" | "is_dir") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            let meta = std::fs::metadata(&path);
            Ok(CtValue::Bool(match (method, meta) {
                ("exists", Ok(_)) => true,
                ("exists", Err(_)) => false,
                ("is_dir", Ok(m)) => m.is_dir(),
                ("is_dir", Err(_)) => false,
                _ => false,
            }))
        }
        ("core.files", "create_dir") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::create_dir_all(&path) {
                Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        // D-LSDIR1: mirror AOT jet_std_fs_list_dir (sorted by name).
        ("core.files", "list_dir") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::read_dir(&path) {
                Ok(rd) => {
                    let mut entries = Vec::new();
                    let mut err: Option<std::io::Error> = None;
                    for entry in rd {
                        match entry {
                            Ok(entry) => {
                                let name = entry.file_name().to_string_lossy().to_string();
                                let full_path = path
                                    .join(&name)
                                    .to_string_lossy()
                                    .to_string();
                                let is_dir =
                                    entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                                entries.push((name, full_path, is_dir));
                            }
                            Err(e) => {
                                err = Some(e);
                                break;
                            }
                        }
                    }
                    if let Some(e) = err {
                        Ok(CtValue::failed(Box::new(io_error_value(
                            &path.to_string_lossy(),
                            e,
                        ))))
                    } else {
                        entries.sort_by(|a, b| a.0.cmp(&b.0));
                        Ok(CtValue::Present(Box::new(CtValue::List(
                            entries
                                .into_iter()
                                .map(|(name, full_path, is_dir)| CtValue::Struct {
                                    type_name: "DirEntry".to_string(),
                                    fields: vec![
                                        ("name".to_string(), CtValue::Str(name)),
                                        ("path".to_string(), CtValue::Str(full_path)),
                                        ("is_dir".to_string(), CtValue::Bool(is_dir)),
                                    ],
                                })
                                .collect(),
                        ))))
                    }
                }
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.files", "remove") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match result {
                Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.env", "get") => {
            let key = as_string(one(0)?, span)?;
            match std::env::var(key) {
                Ok(v) => Ok(CtValue::Present(Box::new(CtValue::Str(v)))),
                Err(_) => Ok(CtValue::absent(crate::AST::Type::String)),
            }
        }
        ("core.env", "set") => {
            let key = as_string(one(0)?, span)?;
            let val = as_string(one(1)?, span)?;
            std::env::set_var(key, val);
            Ok(CtValue::Unit)
        }
        ("core.env", "current_dir") => match std::env::current_dir() {
            Ok(p) => Ok(CtValue::Present(Box::new(CtValue::Str(
                p.to_string_lossy().into_owned(),
            )))),
            Err(e) => Ok(CtValue::failed(Box::new(io_error_value(".", e)))),
        },
        ("core.env", "home_dir") => Ok(
            match std::env::var("HOME")
                .ok()
                .or_else(|| std::env::var("USERPROFILE").ok())
            {
                Some(v) => CtValue::Present(Box::new(CtValue::Str(v))),
                None => CtValue::absent(crate::AST::Type::String),
            },
        ),
        ("core.io", "args") => {
            // Prefer argv installed for this jet run/deopt. Never fall back to
            // the host process argv — `cargo test` flags would leak into output.
            let argv = super::super::Interpreter::runtime_argv()
                .unwrap_or_else(|| vec!["jet".to_string()]);
            Ok(CtValue::List(argv.into_iter().map(CtValue::Str).collect()))
        }
        ("core.io", "progress") => {
            let Some(source) = args.first() else {
                return Err(unsupported("`core.io.progress` needs a source", span));
            };
            if let CtValue::Str(text) = source {
                if args.len() != 1 {
                    return Err(unsupported(
                        "`core.io.progress` text form takes one argument",
                        span,
                    ));
                }
                if let Some(sink) = sink {
                    sink.stdout.push_str(text);
                    sink.stdout.push('\n');
                }
                return Ok(CtValue::Unit);
            }
            let CtValue::List(items) = source else {
                return Err(unsupported(
                    "`core.io.progress` expects a List or Iter source",
                    span,
                ));
            };
            let description = args
                .get(1)
                .map(|value| as_string(value, span))
                .transpose()?
                .unwrap_or("Progress")
                .to_string();
            let format = args
                .get(2)
                .map(|value| as_string(value, span))
                .transpose()?
                .unwrap_or("")
                .to_string();
            // Keep the adapter lazy in the TIR interpreter.  The loop evaluator
            // unwraps this erased carrier and renders one update per pulled
            // item.  Rendering here would report progress before the caller
            // consumes anything and would diverge from AOT/JIT.
            let started_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs_f64())
                .unwrap_or(0.0);
            Ok(CtValue::Struct {
                type_name: "__JetProgressIter".to_string(),
                fields: vec![
                    ("items".to_string(), CtValue::List(items.clone())),
                    ("description".to_string(), CtValue::Str(description)),
                    ("format".to_string(), CtValue::Str(format)),
                    ("started_at".to_string(), CtValue::Float(crate::AST::CtFloat::f64(started_at))),
                    (
                        "pulls".to_string(),
                        CtValue::List(vec![CtValue::Int(1); items.len()]),
                    ),
                    ("tail".to_string(), CtValue::Int(0)),
                    ("total".to_string(), CtValue::Int(items.len() as i64)),
                    ("known_total".to_string(), CtValue::Bool(true)),
                ],
            })
        }
        // D-VERDICT-1321-1: variadic — each argument renders on its own line.
        ("core.io", "print") => {
            let text = args
                .iter()
                .map(|v| v.jet_show())
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(s) = sink {
                s.stdout.push_str(&text);
                s.stdout.push('\n');
            }
            Ok(CtValue::Unit)
        }
        ("core.io", "eprint") => {
            let text = args
                .iter()
                .map(|v| v.jet_show())
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(s) = sink {
                s.stderr.push_str(&text);
                s.stderr.push('\n');
            }
            Ok(CtValue::Unit)
        }
        ("core.io", "input") | ("core.io", "read_all_input") => {
            if repl_mode {
                Err(repl_native_module_diag("core.io", method, span))
            } else {
                Ok(CtValue::Present(Box::new(CtValue::Str(String::new()))))
            }
        }
        ("core.io", "stdin") if repl_mode => Err(repl_native_module_diag("core.io", method, span)),
        ("core.io", "stdin") => Ok(CtValue::Struct {
            type_name: "StdinHandle".to_string(),
            fields: vec![],
        }),
        ("core.process", "exit") => {
            let code = match one(0)? {
                CtValue::Int(n) => *n,
                _ => 0,
            };
            // In-process interpreter/deopt must not kill the host (cargo test,
            // jet dev). Soft-exit via the sink; bare comptime keeps hard exit.
            if let Some(s) = sink {
                s.exit_code = Some(code as i32);
                return Err(Diagnostic::error(
                    "SOFT_EXIT",
                    code.to_string(),
                    "process.exit requested".to_string(),
                    String::new(),
                    Some(span),
                ));
            }
            std::process::exit(code as i32);
        }
        ("core.process", "run") => {
            let cmd = match one(0)? {
                CtValue::List(items) => items.iter().map(|v| v.jet_show()).collect::<Vec<_>>(),
                _ => {
                    return Err(unsupported(
                        "process.run expects a list of command words",
                        span,
                    ))
                }
            };
            if cmd.is_empty() {
                return Ok(CtValue::failed(Box::new(CtValue::Struct {
                    type_name: "IOError".to_string(),
                    fields: vec![(
                        "message".to_string(),
                        CtValue::Str("process.run needs at least one command word".to_string()),
                    )],
                })));
            }
            match run_repl_process(
                &cmd,
                base_dir,
                pinned_executable,
                verified_root,
                std::time::Duration::from_secs(30),
            ) {
                Ok(out) => Ok(CtValue::Present(Box::new(CtValue::Struct {
                    type_name: "ProcessResult".to_string(),
                    fields: vec![
                        (
                            "code".to_string(),
                            CtValue::Int(out.status.code().unwrap_or(-1) as i64),
                        ),
                        (
                            "output".to_string(),
                            CtValue::Str(String::from_utf8_lossy(&out.stdout).into_owned()),
                        ),
                        (
                            "errors".to_string(),
                            CtValue::Str(String::from_utf8_lossy(&out.stderr).into_owned()),
                        ),
                    ],
                }))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(&cmd[0], e)))),
            }
        }
        ("core.tls", _) => Err(Diagnostic::error(
            "E3412",
            format!("`core.tls.{}()` is not available at comptime", method),
            "live TLS sessions cannot be opened during compile-time evaluation".to_string(),
            "move the TLS operation to runtime; use `core.net.fetch(url, sha256: \"<hash>\")` for content-hash-pinned build-time downloads"
                .to_string(),
            Some(span),
        )),
        // Pure compress/archive/encoding codecs live on apply_core_call; reuse
        // them when the runtime evaluator has ambient impure depth open (#778
        // deopt / #715 default-dev encoding parity). Whole-value encoding must
        // not die as E0956 impure-tier after silent deopt.
        ("core.compress.gzip", _)
        | ("core.compress.zstd", _)
        | ("core.archive", _)
        | ("core.perf", _) => apply_core_call(module, method, args, span, repl_mode),
        (module, _) if module.starts_with("core.encoding.") => {
            apply_core_call(module, method, args, span, repl_mode)
        }
        // Ambient impure depth must not block pure-tier CorePureParity surfaces
        // that TirBridge already evaluates (date/math/measurement/testing/…).
        // Pure style/net helpers share the AST allowlist so impure_depth>0
        // (TirBridge / jet run deopt) still hits CorePureParity.
        ("core.io", method)
            if jet_foundation::Effects::core_effect("core.io", method).is_none() =>
        {
            apply_core_call(module, method, args, span, repl_mode)
        }
        ("core.random", _) | ("core.testing", "fake_rng") => {
            apply_core_call(module, method, args, span, repl_mode)
        }
        ("core.time.date", _)
        | ("core.time.duration", _)
        | ("core.time.instant", _)
        | ("core.math", _)
        | ("core.measurement", _)
        | ("core.testing", _)
        | ("core.data", _)
        | ("core.compute", _)
        | ("core.services", _)
        | ("core.auth", _)
        | ("core.sync", _)
        | ("app", _)
        | ("core.ui", _)
        | ("core.crypto", _)
        | ("core.crypto.expert", _)
        | ("core.linalg", _)
        | ("core.email", _)
        | ("core.xml", _)
        | ("core.json", _)
        | ("core.regex", _)
        | ("core.color", _)
        | ("core.units", _)
        | ("core.time", _)
        | ("core.time.datetime", _)
        | ("core.science.measurement", _) => apply_core_call(module, method, args, span, repl_mode),
        // Pure net helpers (e.g. ip_addr, socket_addr_parse) — not live sockets.
        // Keep E3412 for the rest. D-META-EFFECT1: "pure" is what the effect
        // table says, so both tiers agree without a second list here.
        ("core.net", method)
            if jet_foundation::Effects::core_effect("core.net", method).is_none() =>
        {
            apply_core_call(module, method, args, span, repl_mode)
        }
        ("core.net", _) => Err(Diagnostic::error(
            "E3412",
            format!("`core.net.{}()` is not available at comptime", method),
            "only `core.net.fetch(url, sha256:)` is supported at compile time".to_string(),
            "use `core.net.fetch(url, sha256: \"<hash>\")` for content-hash-pinned downloads"
                .to_string(),
            Some(span),
        )),
        _ => Err(unsupported(
            &format!("`{}.{}()` at comptime (impure tier)", module, method),
            span,
        )),
    }
}
