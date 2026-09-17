//! Curated Core calls shared by comptime and REPL evaluation.

use std::cell::Cell;
pub(super) use std::cell::RefCell;
pub(super) use std::rc::Rc;
pub(super) use super::super::Builtins::{as_int, exact_big, exact_int_value};
use super::super::Diagnostics::unsupported;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtFloat, Type};
use crate::AST::{CtReport, CtValue};
pub(super) use jet_foundation::TestingHistory::{
    EventId, HandleId, HistoryBounds, HistoryCase, HistoryCommand, HistoryDistribution,
    HistoryOperation, HistoryPrecondition, HistoryProvenance, HistoryRng, HistoryScheduleChoice,
    HistoryStrategyBehavior, HistoryValue, TaskId, TypedHistoryCase,
};
pub(super) use jet_foundation::Prelude::jet_as_bytes as as_bytes;

pub(super) use super::super::TextLite::IoErrorOperation;
pub(super) use super::time_deadline_kernel;
#[path = "../CorePureParity.rs"]
mod core_pure_parity;

/// The foundation row decides whether a plain Core call may use the pure
/// comptime evaluator. Unknown rows remain available for the typed/internal
/// forms that have not entered the plain-call registry yet.
fn core_call_allows_pure_parity(row: &jet_foundation::Syntax::CoreCallRecord) -> bool {
    row.coverage
        .contains(jet_foundation::Syntax::CoreCallCoverage::COMPTIME)
        && matches!(
            row.interpreter_route,
            jet_foundation::Syntax::CoreCallInterpreterRoute::Pure(_)
                | jet_foundation::Syntax::CoreCallInterpreterRoute::Ambient
        )
        && row.pure_route != jet_foundation::Syntax::CoreCallPureRoute::None
        && !row.is_receiver()
        && row.effect().is_none()
}

fn csv_row_value(record: jet_foundation::CsvKernel::CsvRecord) -> CtValue {
    CtValue::Struct {
        type_name: "CSVRow".to_string(),
        fields: vec![
            (
                "fields".to_string(),
                CtValue::List(record.fields.into_iter().map(CtValue::Str).collect()),
            ),
            ("line".to_string(), CtValue::Int(record.line)),
        ],
    }
}

fn validate_interpreter_route(module: &str, method: &str, span: Span) -> Result<(), Diagnostic> {
    if let Some(row) = jet_foundation::Syntax::core_call(module, method) {
        if row
            .coverage
            .contains(jet_foundation::Syntax::CoreCallCoverage::INTERPRETER)
            && !row.interpreter_route.is_executable()
        {
            return Err(unsupported(
                &format!("{}.{}(): no interpreter route is declared", module, method),
                span,
            ));
        }
    }
    Ok(())
}

fn validate_core_call_projection(
    module: &str,
    method: &str,
    actual_arity: usize,
    projection: u8,
    span: Span,
) -> Result<(), Diagnostic> {
    if jet_foundation::Syntax::core_call(module, method).is_none() {
        return Ok(());
    }
    match jet_foundation::Syntax::core_call_projection(module, method, projection, actual_arity) {
        Ok(_) => Ok(()),
        Err(jet_foundation::Syntax::CoreCallProjectionError::Arity { expected, actual }) => {
            Err(unsupported(
                &format!(
                    "{}.{}(): expected {} argument(s), got {}",
                    module, method, expected, actual
                ),
                span,
            ))
        }
        // I4: the projection bit is spelled COMPTIME in the table, but this
        // adapter also serves default `jet run`'s evaluator, so the message
        // names the evaluator that lacks the row instead of a phase the reader
        // may not be in.
        Err(jet_foundation::Syntax::CoreCallProjectionError::Uncovered { .. }) => Err(unsupported(
            &format!(
                "{}.{}(): no shared-evaluator projection is declared",
                module, method
            ),
            span,
        )),
        Err(jet_foundation::Syntax::CoreCallProjectionError::Unknown) => Err(unsupported(
            &format!("{}.{}(): unknown Core-call row", module, method),
            span,
        )),
    }
}

/// Marshal typed Path values at the shared erased Core boundary. Strings stay
/// strings, so this does not add an implicit conversion to the language.
pub(super) fn normalize_path_args(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<Vec<CtValue>, Diagnostic> {
    let Some(row) = jet_foundation::Syntax::core_call(module, method) else {
        return Ok(args);
    };
    args.into_iter()
        .enumerate()
        .map(|(index, value)| {
            if !row.path_arg(index) {
                return Ok(value);
            }
            let CtValue::Struct { type_name, fields } = &value else {
                return Ok(value);
            };
            if type_name != "Path" {
                return Ok(value);
            }
            let path = fields
                .iter()
                .find_map(|(name, value)| match (name.as_str(), value) {
                    ("inner", CtValue::Str(path)) => Some(path.clone()),
                    _ => None,
                });
            path.map(CtValue::Str)
                .ok_or_else(|| unsupported("malformed Path value", span))
        })
        .collect()
}

#[allow(dead_code)]
mod path_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/Path.rs");
}

fn apply_path_call(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let text = |index: usize| -> Result<&String, Diagnostic> {
        match args.get(index) {
            Some(CtValue::Str(value)) => Ok(value),
            Some(CtValue::Struct { type_name, fields }) if type_name == "Path" => fields
                .iter()
                .find_map(|(name, value)| match (name.as_str(), value) {
                    ("inner", CtValue::Str(value)) => Some(value),
                    _ => None,
                })
                .ok_or_else(|| unsupported("malformed Path value", span)),
            _ => Err(unsupported("Path operation requires a checked path or text value", span)),
        }
    };
    let path = |value: String| CtValue::Struct {
        type_name: "Path".to_string(),
        fields: vec![("inner".to_string(), CtValue::Str(value))],
    };
    Ok(match method {
        "path.from" => path(text(0)?.clone()),
        "path.home" => path(path_kernel::jet_std_path_home()),
        "path.join" => path(path_kernel::jet_std_path_join(text(0)?, text(1)?)),
        "path.normalize" => path(path_kernel::jet_std_path_normalize(text(0)?)),
        "path.is_within" => CtValue::Bool(path_kernel::jet_std_path_is_within(text(0)?, text(1)?)),
        "path.parent" => path_kernel::jet_std_path_parent_opt(text(0)?)
            .map(|value| CtValue::Present(Box::new(path(value))))
            .unwrap_or_else(|| CtValue::absent(Type::Named("Path".to_string()))),
        "path.extension" | "path.stem" => {
            let value = if method == "path.extension" {
                path_kernel::jet_std_path_extension_opt(text(0)?)
            } else {
                path_kernel::jet_std_path_stem_opt(text(0)?)
            };
            value
                .map(|value| CtValue::Present(Box::new(CtValue::Str(value))))
                .unwrap_or_else(|| CtValue::absent(Type::String))
        }
        "path.walk" => CtValue::List(
            path_kernel::jet_std_path_walk(text(0)?)
                .into_iter()
                .map(path)
                .collect(),
        ),
        _ => return Err(unsupported("Path operation has no interpreter binding", span)),
    })
}
#[path = "core_calls/regex.rs"]
mod regex;
#[allow(unused_imports)]
use self::regex::*;
#[path = "core_calls/random.rs"]
mod random;
use self::random::ambient_random_kernel;

/// Install the active deterministic-world RNG providers for the duration of a
/// synchronous adapter call. Outside the scope, MathRandomFns keeps its
/// ordinary thread-local stream.
pub fn with_world_rng_provider<T>(
    next: fn() -> Option<u64>,
    seed: fn(i64) -> bool,
    callback: impl FnOnce() -> T,
) -> T {
    ambient_random_kernel::with_world_rng_provider(next, seed, callback)
}


mod log_state {
    include!("../../../../jet-codegen/src/Prelude/Core/LogState.rs");
}
// Keep the interpreter's ambient logging calls on the same Log.rs kernel as
// the generated native program. The trace carrier stays in `log_state`, so
// plain/default and impure/interpreter dispatch share one thread-local state.
#[allow(dead_code)]
mod log_kernel {
    pub(super) mod jet_std {
        #[derive(Clone, Debug, PartialEq)]
        pub struct LogField {
            pub(crate) key: String,
            pub(crate) value: String,
            pub(crate) kind: String,
            pub(crate) redacted: bool,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct LogSpan {
            pub(crate) id: i64,
            pub(crate) name: String,
        }
    }

    use super::log_state::JET_LOG_TRACE_ID;
    use jet_foundation::Devtools::{jet_devtools_publish_event, JetDevtoolsEvent};

    fn jet_log_write_line(line: &str) {
        eprintln!("{}", line);
    }

    fn jet_log_process_exit(code: i64) {
        std::process::exit(code as i32);
    }

    include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/Log.rs");

    pub(super) fn set_trace_id(id: &str) {
        super::log_state::jet_ring_log_set_trace_id(id);
    }

    pub(super) fn debug(message: &String) {
        jet_ring_log_debug(message);
    }

    pub(super) fn info(message: &String) {
        jet_ring_log_info(message);
    }

    pub(super) fn warn(message: &String) {
        jet_ring_log_warn(message);
    }

    pub(super) fn error(message: &String) {
        jet_ring_log_error(message);
    }

    pub(super) fn critical(message: &String) {
        jet_ring_log_critical(message);
    }

    pub(super) fn fatal(message: &String) {
        jet_ring_log_fatal(message);
    }

    pub(super) fn disable() {
        jet_ring_log_disable();
    }

    pub(super) fn flush() {
        jet_ring_log_flush();
    }

    pub(super) fn enabled(level: &String) -> bool {
        jet_ring_log_enabled(level)
    }

    pub(super) fn field(key: &String, value: &String) -> jet_std::LogField {
        jet_ring_log_field(key, value)
    }

    pub(super) fn int(key: &String, value: i64) -> jet_std::LogField {
        jet_ring_log_int(key, value)
    }

    pub(super) fn float(key: &String, value: f64) -> jet_std::LogField {
        jet_ring_log_float(key, value)
    }

    pub(super) fn bool_field(key: &String, value: bool) -> jet_std::LogField {
        jet_ring_log_bool(key, value)
    }

    pub(super) fn redact(key: &String) -> jet_std::LogField {
        jet_ring_log_redact(key)
    }

    pub(super) fn counter(name: &String, value: i64) -> jet_std::LogField {
        jet_ring_log_counter(name, value)
    }

    pub(super) fn info_fields(message: &String, fields: &Vec<jet_std::LogField>) {
        jet_ring_log_info_fields(message, fields);
    }

    pub(super) fn warn_fields(message: &String, fields: &Vec<jet_std::LogField>) {
        jet_ring_log_warn_fields(message, fields);
    }

    pub(super) fn error_fields(message: &String, fields: &Vec<jet_std::LogField>) {
        jet_ring_log_error_fields(message, fields);
    }

    pub(super) fn debug_fields(message: &String, fields: &Vec<jet_std::LogField>) {
        jet_ring_log_debug_fields(message, fields);
    }

    pub(super) fn span(name: &String) -> jet_std::LogSpan {
        jet_ring_log_span(name)
    }

    pub(super) fn enter(span: &jet_std::LogSpan) {
        jet_ring_log_enter(span);
    }

    pub(super) fn close(span: &jet_std::LogSpan) {
        jet_ring_log_close(span);
    }

    pub(super) fn set_sink(kind: &String, path: &String) {
        jet_ring_log_set_sink(kind, path);
    }

    pub(super) fn sample_every(value: i64) {
        jet_ring_log_sample_every(value);
    }

    pub(super) fn otlp_file(path: &String) {
        jet_ring_log_otlp_file(path);
    }

    pub(super) fn set_level(level: &String) {
        jet_ring_log_set_level(level);
    }

    pub(super) fn setup(format: &String) {
        jet_ring_log_setup(format);
    }
}
/// Set the shared Prelude log trace context for the MIR evaluator.
///
/// Keep the state mutation behind `log_kernel`, so default/impure dispatch
/// and direct MIR handling all use the same thread-local carrier.
pub fn set_trace_id(id: &str) {
    log_kernel::set_trace_id(id);
}

mod term_semantics {
    include!("../../../../jet-codegen/src/Prelude/Term.rs");
}

pub(crate) use term_semantics::jet_term_print_frame;

mod math_lib_pure {
    include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/MathLibPure.rs");
}

mod encoding_base_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/EncodingBase.rs");
}

mod encoding_error_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/EncodingError.rs");
}

mod field_error_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/FieldError.rs");
}

mod fmt_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/Fmt.rs");
}

mod seeded_random_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/SeededRandom.rs");
}

mod fake_data_kernel {
    pub(crate) mod jet_std {
        #[derive(Clone, Debug, PartialEq)]
        pub(crate) struct Fake {
            pub(crate) state: u64,
            pub(crate) locale: u8,
        }
    }

    use super::seeded_random_kernel::jet_seeded_rng_int;
    include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/FakeData.rs");
}

pub fn apply_fake_method(
    recv: &mut CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let CtValue::Struct { type_name, fields } = recv else {
        return Err(unsupported("Fake receiver", span));
    };
    if type_name != crate::Syntax::FAKE_TYPE {
        return Err(unsupported("Fake receiver", span));
    }
    let state = fields
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("state", CtValue::Int(value)) => Some(*value as u64),
            _ => None,
        })
        .unwrap_or(0);
    let locale = fields
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("locale", CtValue::Int(value)) => Some(*value as u8),
            _ => None,
        })
        .unwrap_or(0);
    let mut fake = fake_data_kernel::jet_std::Fake { state, locale };
    let result = match method {
        "locale" => {
            let locale = as_string(
                args.first()
                    .ok_or_else(|| unsupported("Fake.locale argument", span))?,
                span,
            )?
            .to_string();
            let next = fake_data_kernel::jet_fake_locale(&fake, &locale);
            CtValue::Struct {
                type_name: crate::Syntax::FAKE_TYPE.to_string(),
                fields: vec![
                    ("state".to_string(), CtValue::Int(next.state as i64)),
                    ("locale".to_string(), CtValue::Int(next.locale as i64)),
                ],
            }
        }
        "name" => CtValue::Str(fake_data_kernel::jet_fake_name(&mut fake)),
        "email" => CtValue::Str(fake_data_kernel::jet_fake_email(&mut fake)),
        "host" => CtValue::Str(fake_data_kernel::jet_fake_host(&mut fake)),
        "address" => CtValue::Str(fake_data_kernel::jet_fake_address(&mut fake)),
        _ => return Err(unsupported("this Fake method", span)),
    };
    if method != "locale" {
        *recv = CtValue::Struct {
            type_name: crate::Syntax::FAKE_TYPE.to_string(),
            fields: vec![
                ("state".to_string(), CtValue::Int(fake.state as i64)),
                ("locale".to_string(), CtValue::Int(fake.locale as i64)),
            ],
        };
    }
    Ok(result)
}

mod net_pure_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/NetPure.rs");
}

mod loadable_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/Loadable.rs");
}

fn loadable_variant(state: &str) -> &'static str {
    let tag = match state {
        "idle" => loadable_kernel::JET_LOADABLE_IDLE,
        "loading" => loadable_kernel::JET_LOADABLE_LOADING,
        "loaded" => loadable_kernel::JET_LOADABLE_LOADED,
        _ => loadable_kernel::JET_LOADABLE_FAILED,
    };
    match tag {
        loadable_kernel::JET_LOADABLE_IDLE => "Idle",
        loadable_kernel::JET_LOADABLE_LOADING => "Loading",
        loadable_kernel::JET_LOADABLE_LOADED => "Loaded",
        _ => "Failed",
    }
}

mod mime_kernel {
    include!("../../../../jet-codegen/src/Prelude/CoreLib/JetStd/Mime.rs");
}

mod solver_kernel {
    pub(crate) mod jet_std {
        #[derive(Clone)]
        pub(crate) struct Solver {
            pub(crate) seed: i64,
            pub(crate) checked: i64,
            pub(crate) failures: i64,
        }
    }

    include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/Solver.rs");
}

mod sketch_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/Sketch.rs");
}

mod time_kernel {
    fn jet_scheduler_world_now_ms() -> Option<i64> {
        None
    }
    pub(crate) use jet_foundation::Monotonic::jet_time_monotonic_now_ns;
    include!("../../../../jet-codegen/src/Prelude/Core/Duration.rs");
    include!("../../../../jet-codegen/src/Prelude/Core/Time.rs");
}
mod raylib_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/Raylib.rs");
}

mod crypto_entropy_kernel {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
    use jet_crypto_entropy::{jet_crypto_entropy_fill, JetCryptoEntropyError};
}

fn runtime_date_value(date: time_kernel::JetDate) -> CtValue {
    CtValue::Struct {
        type_name: "LocalDate".to_string(),
        fields: vec![
            ("year".to_string(), CtValue::Int(date.year())),
            ("month".to_string(), CtValue::Int(date.month())),
            ("day".to_string(), CtValue::Int(date.day())),
        ],
    }
}

fn runtime_datetime_value(datetime: time_kernel::JetDateTime) -> CtValue {
    CtValue::Struct {
        type_name: "DateTime".to_string(),
        fields: vec![
            (
                "secs".to_string(),
                CtValue::Int(datetime.unix_seconds_anchor()),
            ),
            ("nanos".to_string(), CtValue::Int(datetime.nanosecond())),
            (
                "leap_second".to_string(),
                CtValue::Bool(datetime.is_leap_second()),
            ),
        ],
    }
}

/// D-BOUND-HEAD1: typed DateTime heads validate against the same pure Prelude
/// parser used by the runtime `core.time.parse_rfc3339` call.
pub(crate) fn validate_datetime_literal(value: &str) -> Result<(), String> {
    time_kernel::JetDateTime::parse_rfc3339(value).map(|_| ())
}

/// D-BOUND-HEAD1: typed DateTime heads use the existing pure-parity value
/// projection without entering the ambient `core.time` effect gate.
pub(crate) fn evaluate_typed_datetime_literal(
    value: &str,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let row = jet_foundation::Syntax::core_call("core.time", "parse_rfc3339")
        .expect("core.time.parse_rfc3339 row is registered");
    core_pure_parity::evaluate(row, &[CtValue::Str(value.to_string())], span)
        .expect("core.time.parse_rfc3339 pure-parity evaluator is registered")
}

pub(super) mod duration_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/Duration.rs");
}

mod measurement_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/Measurement.rs");
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

        /// D-QUERY-RETAIN1=A: grouped results retain the nominal key and
        /// the reducer's exact value type.
        #[derive(Clone, Debug, PartialEq)]
        pub(crate) struct GroupValue<K, V> {
            pub(crate) key: K,
            pub(crate) value: V,
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

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub(crate) struct DataLimits {
            pub(crate) max_groups: i64,
            pub(crate) max_sort_rows: i64,
            pub(crate) max_join_rows: i64,
            pub(crate) max_output_rows: i64,
        }
        impl DataLimits {
            pub(crate) fn safe() -> Self {
                Self {
                    max_groups: 100_000,
                    max_sort_rows: 1_000_000,
                    max_join_rows: 1_000_000,
                    max_output_rows: 1_000_000,
                }
            }
        }

    }

    include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs");
}

// #1657: `core.data` line renderers share AOT's `DataPlot.rs` the same way.
pub(crate) mod data_plot_rt {
    pub(crate) use super::data_kernel::jet_std;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/DataPlot.rs");
}

#[path = "core_calls/data.rs"]
mod data;
#[path = "core_calls/impure.rs"]
mod impure;
#[path = "core_calls/values.rs"]
mod values;
pub use data::{apply_data_line_call, data_status_rows};
pub(crate) use data::eval_data_describe;
#[allow(unused_imports)]
use data::{
    as_data_bar_groups, as_float_list, data_error_value, data_float_value, data_result_value,
    data_result_value_for_type, data_summary_value,
};
pub use impure::{
    apply_impure_core_call, apply_impure_core_call_with_type,
    apply_impure_core_call_with_type_args,
};
pub(crate) use values::as_string;
pub(crate) use values::url_parts_to_ct;
#[allow(unused_imports)]
use values::{as_string_rows, csv_rows_from_records, named_tuple};
pub(super) use values::{url_parts_from_ct, URL_INTERNAL_PREFIX};

pub(crate) fn eval_data_pivot_sum<F>(
    args: &[CtValue],
    span: Span,
    call: F,
) -> Result<CtValue, Diagnostic>
where
    F: FnMut(&CtValue, Vec<CtValue>, Span) -> Result<CtValue, Diagnostic>,
{
    data::data_pivot_sum(args, span, call)
}


pub fn apply_core_pure_method(
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    core_pure_parity::evaluate_method(recv, method, args, span)
}

pub fn sketch_add(
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

pub(in super::super) fn solver_new(args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
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
// D-META-EFFECT1: this is the implementation dispatch for Core calls that the
// shared effect facts admit at comptime. Eligibility is decided by
// `Effects::core_effect`; this table only supplies evaluator implementations.
// ---------------------------------------------------------------------------

pub(in super::super) fn as_float(v: &CtValue, span: Span) -> Result<f64, Diagnostic> {
    match v {
        CtValue::Float(value) => Ok(value.as_f64()),
        CtValue::Int(n) => Ok(*n as f64),
        // I4: these adapters run on default `jet run`'s evaluator too, so the
        // `what` names the argument and never a phase.
        _ => Err(unsupported(
            "non-numeric argument to a Core math call",
            span,
        )),
    }
}

fn as_ct_float(v: &CtValue, span: Span) -> Result<CtFloat, Diagnostic> {
    match v {
        CtValue::Float(value) => Ok(*value),
        // Both exact carriers cross into Float at the irrational-result math
        // functions. The checker and the AOT lowerer admit Fraction and
        // Decimal alike, so this evaluator must too or default `jet run`
        // disagrees with the type that was already accepted.
        CtValue::Struct { type_name, .. } if type_name == crate::Syntax::TYPE_FRACTION => {
            let fraction = crate::Numeric::CtFraction::from_value(v)
                .map_err(|_| unsupported("malformed Fraction", span))?;
            Ok(CtFloat::F64(fraction.to_float()))
        }
        CtValue::Struct { type_name, .. } if type_name == crate::Syntax::TYPE_DECIMAL => {
            let decimal = crate::Numeric::CtDecimal::from_value(v)
                .map_err(|_| unsupported("malformed Decimal", span))?;
            Ok(CtFloat::F64(decimal.to_f64()))
        }
        _ => Err(unsupported("non-float argument to a Core math call", span)),
    }
}

fn core_math_float_min(left: CtFloat, right: CtFloat, span: Span) -> Result<CtFloat, Diagnostic> {
    match (left, right) {
        (CtFloat::F32(left), CtFloat::F32(right)) => Ok(CtFloat::F32(
            math_lib_pure::jet_std_math_min_f32(left, right),
        )),
        (CtFloat::F64(left), CtFloat::F64(right)) => Ok(CtFloat::F64(
            math_lib_pure::jet_std_math_min_f64(left, right),
        )),
        _ => Err(unsupported("mixing float widths", span)),
    }
}

fn core_math_float_abs(value: CtFloat) -> CtFloat {
    match value {
        CtFloat::F32(value) => CtFloat::F32(math_lib_pure::jet_std_math_abs_f32(value)),
        CtFloat::F64(value) => CtFloat::F64(math_lib_pure::jet_std_math_abs_f64(value)),
    }
}

fn core_math_float_max(left: CtFloat, right: CtFloat, span: Span) -> Result<CtFloat, Diagnostic> {
    match (left, right) {
        (CtFloat::F32(left), CtFloat::F32(right)) => Ok(CtFloat::F32(
            math_lib_pure::jet_std_math_max_f32(left, right),
        )),
        (CtFloat::F64(left), CtFloat::F64(right)) => Ok(CtFloat::F64(
            math_lib_pure::jet_std_math_max_f64(left, right),
        )),
        _ => Err(unsupported("mixing float widths", span)),
    }
}

fn core_math_float_clamp(
    value: CtFloat,
    low: CtFloat,
    high: CtFloat,
    span: Span,
) -> Result<CtFloat, Diagnostic> {
    match (value, low, high) {
        (CtFloat::F32(value), CtFloat::F32(low), CtFloat::F32(high)) => Ok(CtFloat::F32(
            math_lib_pure::jet_std_math_clamp_f32(value, low, high),
        )),
        (CtFloat::F64(value), CtFloat::F64(low), CtFloat::F64(high)) => Ok(CtFloat::F64(
            math_lib_pure::jet_std_math_clamp_f64(value, low, high),
        )),
        _ => Err(unsupported("mixing float widths", span)),
    }
}

fn hex_encode(bytes: Vec<u8>) -> String {
    encoding_base_kernel::jet_std_hex_encode(&bytes)
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    encoding_base_kernel::jet_std_hex_decode(&s.to_string())
}

fn base64_encode(bytes: Vec<u8>) -> String {
    encoding_base_kernel::jet_std_b64_encode(&bytes)
}

fn base32_encode(bytes: &[u8]) -> String {
    encoding_base_kernel::jet_std_base32_encode(&bytes.to_vec())
}

/// Core modules the REPL interpreter cannot run (native FFI / threads / HTTP stack).
fn repl_native_only_module(module: &str) -> Option<&'static str> {
    match module {
        "core.http" | "core.http.client" | "core.http.server" => {
            Some("the HTTP client/server (`core.http`)")
        }
        "core.db" => Some("`core.db` (SQLite)"),
        "core.net" => Some("network sockets (`core.net`)"),
        "core.crypto" => Some("`core.crypto`"),
        "core.auth" => Some("`core.auth` token verification"),
        "core.tasks" | "core.channels" => Some("tasks/channels (`core.tasks`)"),
        "core.mem" => Some("`core.mem` (low-level memory tier)"),
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

pub(super) fn io_error_value(
    operation: super::super::TextLite::IoErrorOperation,
    path: &str,
    e: std::io::Error,
) -> CtValue {
    super::super::TextLite::io_error_value(operation, path, e)
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

fn raylib_field<'a>(
    fields: &'a [(String, CtValue)],
    name: &str,
) -> Option<&'a CtValue> {
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
}

fn raylib_atlas_value(spec: raylib_kernel::JetRaylibAtlasSpec) -> CtValue {
    let regions = spec
        .regions
        .into_iter()
        .map(|region| CtValue::Struct {
            type_name: "RaylibAtlasRegion".to_string(),
            fields: vec![
                ("name".to_string(), CtValue::Str(region.name)),
                ("x".to_string(), CtValue::Int(region.x)),
                ("y".to_string(), CtValue::Int(region.y)),
                ("width".to_string(), CtValue::Int(region.width)),
                ("height".to_string(), CtValue::Int(region.height)),
            ],
        })
        .collect();
    let texture_path = spec
        .texture_path
        .map(|path| CtValue::Present(Box::new(CtValue::Str(path))))
        .unwrap_or_else(|| CtValue::absent(Type::String));
    CtValue::Struct {
        type_name: "RaylibTextureAtlas".to_string(),
        fields: vec![
            ("path".to_string(), CtValue::Str(spec.path)),
            ("name".to_string(), CtValue::Str(spec.name)),
            ("texture_path".to_string(), texture_path),
            ("regions".to_string(), CtValue::List(regions)),
        ],
    }
}

fn raylib_atlas_spec(
    value: &CtValue,
    span: Span,
) -> Result<raylib_kernel::JetRaylibAtlasSpec, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("expected a RaylibTextureAtlas", span));
    };
    if type_name != "RaylibTextureAtlas" {
        return Err(unsupported("expected a RaylibTextureAtlas", span));
    }
    let path = match raylib_field(fields, "path") {
        Some(CtValue::Str(path)) => path.clone(),
        _ => return Err(unsupported("malformed RaylibTextureAtlas path", span)),
    };
    let name = match raylib_field(fields, "name") {
        Some(CtValue::Str(name)) => name.clone(),
        _ => return Err(unsupported("malformed RaylibTextureAtlas name", span)),
    };
    let texture_path = match raylib_field(fields, "texture_path") {
        Some(CtValue::Present(value)) => match value.as_ref() {
            CtValue::Str(path) => Some(path.clone()),
            _ => return Err(unsupported("malformed RaylibTextureAtlas texture", span)),
        },
        Some(CtValue::Failed(CtReport::Clean(_))) => None,
        _ => return Err(unsupported("malformed RaylibTextureAtlas texture", span)),
    };
    let Some(CtValue::List(region_values)) = raylib_field(fields, "regions") else {
        return Err(unsupported("malformed RaylibTextureAtlas regions", span));
    };
    let mut regions = Vec::with_capacity(region_values.len());
    for value in region_values {
        let CtValue::Struct {
            type_name,
            fields,
        } = value
        else {
            return Err(unsupported("malformed RaylibTextureAtlas region", span));
        };
        if type_name != "RaylibAtlasRegion" {
            return Err(unsupported("malformed RaylibTextureAtlas region", span));
        }
        let int_field = |name: &str| match raylib_field(fields, name) {
            Some(CtValue::Int(value)) => Ok(*value),
            _ => Err(unsupported("malformed RaylibTextureAtlas region", span)),
        };
        let name = match raylib_field(fields, "name") {
            Some(CtValue::Str(name)) => name.clone(),
            _ => return Err(unsupported("malformed RaylibTextureAtlas region", span)),
        };
        regions.push(raylib_kernel::JetRaylibAtlasRegion {
            name,
            x: int_field("x")?,
            y: int_field("y")?,
            width: int_field("width")?,
            height: int_field("height")?,
        });
    }
    Ok(raylib_kernel::JetRaylibAtlasSpec {
        path,
        name,
        texture_path,
        regions,
    })
}

fn apply_raylib_core_call(
    module: &str,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if module != "core.game.raylib" {
        return None;
    }
    let one = |index: usize| {
        args.get(index).ok_or_else(|| {
            unsupported(
                &format!("`{module}.{method}` (wrong number of arguments)"),
                span,
            )
        })
    };
    Some((|| -> Result<CtValue, Diagnostic> {
        match method {
            "window_open" => Ok(CtValue::Struct {
            type_name: "RaylibWindow".to_string(),
            fields: vec![
                ("width".to_string(), CtValue::Int(as_int(one(0)?, span)?)),
                ("height".to_string(), CtValue::Int(as_int(one(1)?, span)?)),
                ("title".to_string(), CtValue::Str(as_string(one(2)?, span)?.to_string())),
                ("native".to_string(), CtValue::Bool(false)),
            ],
        }),
        "window_should_close" => {
            let _ = one(0)?;
            Ok(CtValue::Bool(true))
        }
        "window_ready" => {
            let _ = one(0)?;
            Ok(CtValue::Bool(false))
        }
        "begin_drawing" | "close_window" => {
            let _ = one(0)?;
            Ok(CtValue::Unit)
        }
        "clear_background" => {
            let _ = one(0)?;
            Ok(CtValue::Unit)
        }
        "draw_rectangle" => {
            for index in 0..4 {
                let _ = as_int(one(index)?, span)?;
            }
            let _ = one(4)?;
            Ok(CtValue::Unit)
        }
        "draw_text" => {
            let _ = as_string(one(0)?, span)?;
            for index in 1..4 {
                let _ = as_int(one(index)?, span)?;
            }
            let _ = one(4)?;
            Ok(CtValue::Unit)
        }
        "end_drawing" => Ok(CtValue::Unit),
        "key_down" => {
            let _ = as_string(one(0)?, span)?;
            Ok(CtValue::Bool(false))
        }
        "set_target_fps" => {
            let _ = as_int(one(0)?, span)?;
            Ok(CtValue::Unit)
        }
        "color" => Ok(CtValue::Struct {
            type_name: "RaylibColor".to_string(),
            fields: vec![
                ("r".to_string(), CtValue::Int(as_int(one(0)?, span)?)),
                ("g".to_string(), CtValue::Int(as_int(one(1)?, span)?)),
                ("b".to_string(), CtValue::Int(as_int(one(2)?, span)?)),
                ("a".to_string(), CtValue::Int(as_int(one(3)?, span)?)),
            ],
        }),
        "gamepad_down" => {
            let gamepad = as_int(one(0)?, span)?;
            let button = as_string(one(1)?, span)?;
            let _ = i32::try_from(gamepad)
                .ok()
                .filter(|value| *value >= 0)
                .and_then(|_| raylib_kernel::jet_raylib_button_code(button));
            Ok(CtValue::Bool(false))
        }
        "gamepad_axis" => {
            let gamepad = as_int(one(0)?, span)?;
            let axis = as_string(one(1)?, span)?;
            let _ = i32::try_from(gamepad)
                .ok()
                .filter(|value| *value >= 0)
                .and_then(|_| raylib_kernel::jet_raylib_axis_code(axis));
            Ok(CtValue::Float(CtFloat::f64(0.0)))
        }
        "load_sound" => Ok(CtValue::Struct {
            type_name: "RaylibSound".to_string(),
            fields: vec![(
                "path".to_string(),
                CtValue::Str(as_string(one(0)?, span)?.to_string()),
            )],
        }),
        "play_sound" => {
            let CtValue::Struct { type_name, fields } = one(0)? else {
                return Err(unsupported("expected a RaylibSound", span));
            };
            if type_name != "RaylibSound" {
                return Err(unsupported("expected a RaylibSound", span));
            }
            let path = match raylib_field(fields, "path") {
                Some(CtValue::Str(path)) => path,
                _ => return Err(unsupported("malformed RaylibSound", span)),
            };
            Ok(CtValue::Bool(!path.is_empty()))
        }
        "load_texture_atlas" => Ok(raylib_atlas_value(
            raylib_kernel::jet_raylib_load_texture_atlas_spec(as_string(one(0)?, span)?),
        )),
        "draw_sprite" => {
            let atlas = raylib_atlas_spec(one(0)?, span)?;
            let region = as_string(one(1)?, span)?;
            let x = as_int(one(2)?, span)?;
            let y = as_int(one(3)?, span)?;
            if let Some(call) =
                raylib_kernel::jet_raylib_sprite_draw_call(&atlas.name, &atlas.regions, region, x, y)
            {
                raylib_kernel::jet_raylib_record_draw_call(call);
            }
            Ok(CtValue::Unit)
        }
        _ => Err(unsupported(&format!("`{module}.{method}()`"), span)),
        }
    })())
}

/// Direct ambient adapter for `core.game.raylib`.
///
/// This deliberately bypasses `apply_core_call_with_type` so registering it
/// in the JIT interpreter ambient context cannot recursively re-enter the
/// ambient dispatcher.
pub fn apply_raylib_ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut crate::Comptime::DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    apply_raylib_core_call(module, method, &args, span)
}

fn realtime_stream_fields(
    value: &CtValue,
    span: Span,
) -> Result<&Vec<(String, CtValue)>, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("realtime stream method expects a RealtimeStream", span));
    };
    if type_name != "RealtimeStream" {
        return Err(unsupported("realtime stream method expects a RealtimeStream", span));
    }
    Ok(fields)
}

fn realtime_stream_field<'a>(
    value: &'a CtValue,
    name: &str,
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    realtime_stream_fields(value, span)?
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
        .ok_or_else(|| unsupported("malformed RealtimeStream value", span))
}

#[path = "core_calls/history.rs"]
mod history;
#[path = "core_calls/plain_calls.rs"]
mod plain_calls;
pub(super) use super::{
    invoke_standalone_closure, invoke_standalone_closure_mut_args,
};
pub(super) use history::apply_testing_histories;
pub use history::{
    HistoryCommandSchema, history_command_schema_from_mir,
    apply_history_rng_method, history_callback_fingerprint,
};
pub use plain_calls::{
    apply_core_call, apply_core_pure_call, apply_core_call_with_type,
    apply_core_call_without_ambient,
    apply_core_call_without_ambient_with_type,
    apply_core_call_without_ambient_with_type_args,
    apply_core_call_without_ambient_with_type_args_and_history_schema,
};
