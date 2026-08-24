/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): what a `#Target(…)` marker parsed to — a
/// partition-ceiling `Bucket` (`Wasm`/`JS`, existing D-WASM1 meaning),
/// `DefaultWeb` (`Web` — this file's default CLI backend, a different axis),
/// or `OS` (D-OSTARGET1=A: `OS.Linux`/`OS.MacOS`/`OS.Windows` — the native
/// platform-gating axis, item-scoped rather than file/module-scoped).
pub(super) enum TargetMarker {
    Bucket(crate::Syntax::WebBucket),
    DefaultWeb,
    OS(crate::Syntax::OSTarget),
}

#[path = "Items/distinct_units_structs.rs"]
mod distinct_units_structs;
#[path = "Items/enums_traits.rs"]
mod enums_traits;
#[path = "Items/external_tests_ffi.rs"]
mod external_tests_ffi;
#[path = "Items/functions_params.rs"]
mod functions_params;
#[path = "Items/helpers.rs"]
mod helpers;
#[path = "Items/imports_policy.rs"]
mod imports_policy;
#[path = "Items/marker_groups.rs"]
mod marker_groups;
#[path = "Items/markers_contracts.rs"]
mod markers_contracts;
#[path = "Items/reactive_unsafe_c.rs"]
mod reactive_unsafe_c;
#[path = "Items/states_protocols.rs"]
mod states_protocols;
#[path = "Items/type_methods_fields.rs"]
mod type_methods_fields;
#[path = "Items/view_from.rs"]
mod view_from;
#[path = "Items/visibility_items.rs"]
mod visibility_items;
