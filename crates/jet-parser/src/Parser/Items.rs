use super::*;

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): what a `#Target(…)` marker parsed to — a
/// partition-ceiling `Bucket` (`Wasm`/`Js`, existing D-WASM1 meaning),
/// `DefaultWeb` (`Web` — this file's default CLI backend, a different axis),
/// or `Os` (D-OSTARGET1=A: `Os.Linux`/`Os.Macos`/`Os.Windows` — the native
/// platform-gating axis, item-scoped rather than file/module-scoped).
pub(super) enum TargetMarker {
    Bucket(crate::Syntax::WebBucket),
    DefaultWeb,
    Os(crate::Syntax::OsTarget),
}

include!("Items/imports_policy.rs");
include!("Items/external_tests_ffi.rs");
include!("Items/reactive_unsafe_c.rs");
include!("Items/markers_contracts.rs");
include!("Items/visibility_items.rs");
include!("Items/functions_params.rs");
include!("Items/enums_traits.rs");
include!("Items/marker_groups.rs");
include!("Items/type_methods_fields.rs");
include!("Items/distinct_units_structs.rs");
include!("Items/states_protocols.rs");
include!("Items/helpers.rs");
