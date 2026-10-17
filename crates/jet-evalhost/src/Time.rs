//! `core.time` / civil-time marshalling hosts for the shared Prelude kernel.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

use super::Concurrency;
use jet_foundation::AST::{CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_codegen::Comptime::DevSink;

pub(crate) mod time_rt {
    use jet_codegen::scheduler::jet_time_wall_override_ms;
    // `TimeInstant.rs` names the shared sleep seam unqualified; the resident
    // adapter below routes it to the ambient scheduler.
    use super::jet_std_time_sleep_duration_ns;
    fn jet_scheduler_world_monotonic_now_ns() -> Option<i64> {
        jet_codegen::scheduler::jet_scheduler_world_monotonic_now_ns()
    }
    fn jet_scheduler_world_reject_uncontrolled(effect: &'static str) {
        jet_codegen::scheduler::jet_scheduler_world_reject_uncontrolled(effect);
    }
    include!("../../jet-codegen/src/Prelude/Core/TimeMonotonic.rs");
    include!("../../jet-codegen/src/Prelude/Core/Duration.rs");
    fn jet_scheduler_world_now_ms() -> Option<i64> {
        jet_codegen::scheduler::jet_scheduler_world_now_ms()
    }
    include!("../../jet-codegen/src/Prelude/Core/Time.rs");
    include!("../../jet-codegen/src/Prelude/Core/TimeFacts.rs");
    include!("../../jet-codegen/src/Prelude/Core/TimeInstant.rs");
    include!("../../jet-codegen/src/Prelude/Core/Realtime.rs");
}
/// The JIT adapter keeps the canonical nanosecond sleep contract while
/// delegating parking and cancellation to the shared scheduler host.
pub(crate) fn jet_std_time_sleep_duration_ns(nanos: i64) {
    Concurrency::ambient_time_sleep(nanos);
}


fn unsigned_value(value: u128) -> CtValue {
    match i64::try_from(value) {
        Ok(value) => CtValue::Int(value),
        Err(_) => CtValue::BigInt(
            jet_foundation::Numeric::CtBigInt::from_str(&value.to_string())
                .expect("a host unsigned sample is an exact integer"),
        ),
    }
}

fn optional_value(value: Option<CtValue>, ty: Type) -> CtValue {
    value.map_or_else(|| CtValue::absent(ty), |value| CtValue::Present(Box::new(value)))
}

/// Marshal raw samples only; civil policy and clock selection stay in Core.
pub(crate) fn ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    if module != "core.time" {
        return None;
    }
    use jet_codegen::scheduler;
    let value = match (method, args.as_slice()) {
        ("__wall_sample", []) => {
            let (seconds, nanoseconds, before_epoch) = time_rt::jet_time_wall_parts();
            CtValue::Struct {
                type_name: "(seconds,nanoseconds,before_epoch)".to_string(),
                fields: vec![
                    ("seconds".to_string(), unsigned_value(seconds as u128)),
                    ("nanoseconds".to_string(), CtValue::Int(nanoseconds as i64)),
                    ("before_epoch".to_string(), CtValue::Bool(before_epoch)),
                ],
            }
        }
        ("__world_wall_ms", []) => optional_value(
            scheduler::jet_scheduler_world_now_ms().map(CtValue::Int), Type::Int,
        ),
        ("__wall_override_ms", []) => optional_value(
            scheduler::jet_time_wall_override_ms().map(CtValue::Int), Type::Int,
        ),
        ("__controlled_monotonic_ns", []) => optional_value(
            scheduler::jet_time_monotonic_controlled_now_ns().map(CtValue::Int), Type::Int,
        ),
        ("__world_monotonic_ns", []) => optional_value(
            scheduler::jet_scheduler_world_monotonic_now_ns().map(CtValue::Int), Type::Int,
        ),
        ("__host_monotonic_ns" | "__clock_mono_ns", []) =>
            unsigned_value(scheduler::jet_time_monotonic_host_sample()),
        ("__sleep_ns", [nanos]) => {
            let nanos = match nanos {
                CtValue::Int(value) => Some(*value),
                CtValue::BigInt(value) => value.try_i64(),
                _ => None,
            };
            let Some(nanos) = nanos else {
                return Some(Err(jet_foundation::Prelude::jet_e0956_unsupported(
                    "core.time.__sleep_ns requires an exact Int fitting the native i64 ABI",
                    span,
                )));
            };
            jet_std_time_sleep_duration_ns(nanos);
            CtValue::Unit
        }
        ("__reject_uncontrolled", [CtValue::Str(effect)]) => {
            scheduler::jet_scheduler_world_reject_uncontrolled(effect);
            CtValue::Unit
        }
        ("__zone_env", [CtValue::Str(name)]) => optional_value(
            time_rt::jet_time_zone_environment(name).map(CtValue::Str), Type::String,
        ),
        ("__zone_file_bytes", [CtValue::Str(path)]) => optional_value(
            time_rt::jet_time_zone_file(path).map(CtValue::Bytes),
            Type::List(Box::new(Type::IntN { signed: false, bits: 8 })),
        ),
        ("__embedded_zone_bytes", [CtValue::Str(_)]) => CtValue::absent(
            Type::List(Box::new(Type::IntN { signed: false, bits: 8 })),
        ),
        _ => return None,
    };
    Some(Ok(value))
}
