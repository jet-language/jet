use super::alloc_ptrs::{db_error_ty, db_row_ty, io_error_ty, result_ty};
use super::core_types::{encoding_error_ty, json_ty, u8_ty, unit_ty};
use crate::Syntax;
use crate::AST::{AccessConvention, FunctionCallMetadata, Type};

/// c109 Phase 20: the polymorphic core specials whose return type is resolved by
/// `infer_core_call`'s bespoke arg-type logic (NOT the fixed `core_fixed_sig`
/// table). Sema writes the resolved return back onto the `Expr::MethodCall`
/// `resolved_ret` field for exactly these, so the TIR reads it totally (I3).
/// `io.input` is excluded — it IS in `core_fixed_sig` (`String?`), covered by
/// Phase 10. `core.mem` ptr ops have their own Phase-18 lowering.
pub fn is_polymorphic_core_special(module: &str, name: &str) -> bool {
    matches!(
        (module, name),
        ("core.prelude", "keep")
            // D-TEST-HISTORY1=A: the operation type is explicit while the
            // callback/result shape is resolved by the Core call arm.
            | ("core.testing", "histories")
            | ("core.math", "abs")
            | ("core.math", "min")
            | ("core.math", "max")
            | ("core.text.fmt", "pretty" | "decimal" | "grouped")
            | ("core.math", "clamp")
            // D-FLOATW1: sqrt/floor/ceil/pow are width-generic (Float→Float, F32→F32);
            // their return type is arg-type-dependent, so they use resolved_ret.
            | ("core.math", "sqrt")
            | ("core.math", "floor")
            | ("core.math", "ceil")
            | ("core.math", "pow")
            | ("core.math", "sin")
            | ("core.math", "cos")
            | ("core.math", "tan")
            | ("core.math", "asin")
            | ("core.math", "acos")
            | ("core.math", "atan")
            | ("core.math", "atan2")
            | ("core.math", "sinh")
            | ("core.math", "cosh")
            | ("core.math", "tanh")
            | ("core.math", "exp")
            | ("core.math", "ln")
            | ("core.math", "log2")
            | ("core.math", "log10")
            | ("core.math", "hypot")
            | ("core.math", "trunc")
            | ("core.math", "fract")
            | ("core.math", "sign")
            | ("core.math", "is_nan")
            | ("core.math", "is_inf")
            | ("core.math", "is_finite")
            | ("core.math", "to_bits")
            | ("core.math", "from_bits")
            | ("core.math", "degrees")
            | ("core.math", "radians")
            | ("core.math", "lerp")
            | ("core.math", "checked_add")
            | ("core.math", "checked_sub")
            | ("core.math", "checked_mul")
            | ("core.math", "checked_pow")
            | ("core.math", "saturating_add")
            | ("core.math", "saturating_sub")
            | ("core.math", "saturating_mul")
            | ("core.math", "int_pow")
            | ("core.math", "gcd")
            | ("core.math", "lcm")
            | ("core.math", "acosh")
            | ("core.math", "asinh")
            | ("core.math", "atanh")
            | ("core.math", "cbrt")
            | ("core.math", "copysign")
            | ("core.math", "exp2")
            | ("core.math", "exp_m1")
            | ("core.math", "ln_1p")
            | ("core.math", "log")
            | ("core.math", "signum")
            | ("core.math", "fma")
            | ("core.math", "is_even")
            | ("core.math", "is_odd")
            | ("core.math", "isqrt")
            | ("core.math", "factorial")
            | ("core.math", "checked_abs")
            | ("core.math", "checked_neg")
            | ("core.math", "checked_div")
            | ("core.math", "checked_rem")
            | ("core.math", "is_normal")
            | ("core.math", "is_subnormal")
            | ("core.math", "is_canonical")
            | ("core.math", "is_signed")
            | ("core.math", "is_zero")
            | ("core.math", "is_integer")
            | ("core.math", "sign_bit")
            | ("core.math", "leading_ones")
            | ("core.math", "trailing_ones")
            | ("core.math", "next_up")
            | ("core.math", "next_down")
            | ("core.math", "next_after")
            | ("core.math", "sin_cos")
            | ("core.math", "binomial")
            | ("core.math", "cmp")
            | ("core.math", "copy")
            | ("core.math", "cot")
            | ("core.math", "div_mod")
            | ("core.math", "div_rem")
            | ("core.math", "erf")
            | ("core.math", "erfc")
            | ("core.math", "frexp")
            | ("core.math", "gamma")
            | ("core.math", "lgamma")
            | ("core.math", "ilogb")
            | ("core.math", "inv")
            | ("core.math", "ldexp")
            | ("core.math", "logb")
            | ("core.math", "modf")
            | ("core.math", "radix")
            | ("core.math", "significand")
            | ("core.math", "ulp")
            | ("core.math", "zero")
            | ("core.math", "scaleb")
            | ("core.math", "digits")
            | ("core.math.random", "pick")
            | ("core.math.random", "weighted_pick")
            | ("core.math.random", "sample")
            | ("core.math.random", "shuffle")
            | ("core.term", "eprint")
            | ("core.term", "print")
            | ("core.term", "progress")
            // D-TEXTWIDTH1=B: `text.display_width(s)` returns `Int`, while
            // `text.display_width(s, policy: p)` returns `Int !TextError` —
            // the `.Reject` control policy can fail. One call, two return
            // types chosen by arity, so no `core_fixed_sig` row can state it:
            // that table holds exactly one param list and one return, and
            // `core_fixed_sig_for_row` pins the list length to the call's
            // arity. The bespoke `infer_core_call` arm already dispatches both
            // forms; this row is what makes sema write that answer onto
            // `resolved_ret`, so every tier reads the one resolved type
            // instead of falling back to Unit (I3).
            | ("core.text", "display_width")
            // D-ENC1 / D-GENERIC-CALL1 / D-SERDE6: typed encode/decode return
            // types depend on the value type / call-site `<T>`, so codegen reads
            // them from resolved_ret (I3).
            | (
                "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
                | "core.encoding.yaml",
                "to_string" | "to_string_pretty" | "decode",
            )
            | ("core.sys", "decode")
            | ("core.db", "decode")
            | ("core.encoding.csv", "query")
            | ("core.encoding.cbor", "parse" | "decode" | "to_bytes" | "to_bytes_canonical")
            | ("core.encoding.xml", "decode" | "decode_bytes" | "expanded_name")
            // D-REACT1=B: the reactive producers return `Signal<T>`/`Derived<T>` whose
            // element type is inferred from the initial value / closure return — not in
            // `core_fixed_sig`, so codegen reads it from resolved_ret (I3).
            | ("core.reactive", "signal" | "derived")
            | ("core.tasks", "after")
            | (
                "core.data",
                "csv" | "json" | "csv_reader" | "json_reader" | "count" | "schema" | "plot"
                    | "load" | "load_default" | "file" | "file_member" | "url" | "database"
                    | "value" | "snapshot"
                    | "inner_join" | "left_join" | "pivot_sum" | "query" | "track",
            )
            | (
                "core.data.plot",
                "plot" | "inspect" | "inspect_json" | "text" | "svg" | "show" | "render",
            )
            | ("core.crypto.vault", "current" | "versions" | "load" | "status"
                | "prepare_generate" | "prepare_store" | "prepare_rotate" | "prepare_retire" | "prepare_revoke"
                | "authorize_write" | "commit_generate" | "commit_store" | "commit_rotate" | "commit_retire" | "commit_revoke"
                | "export_to_recipients" | "export_to_passphrase" | "prepare_import_wrapped"
                | "authorize_wrapped_import" | "commit_import_wrapped")
            | ("core.compute", "gradient" | "value_and_gradient" | "vjp" | "jvp")
            // D-FLAGSHIP-WEBAPI1=A: generic table/store wrappers resolve their
            // element type in `infer_core_call`, not in this erased fallback.
            | (
                "core.web.table",
                "new" | "new_keyed" | "column" | "with_column" | "with_server_page"
                    | "state" | "facts" | "keys" | "page_state" | "sort" | "filter" | "page"
                    | "sort_by" | "filter_by" | "paginate" | "set_rows" | "set_selected"
                    | "toggle_selection" | "clear_selection" | "focus" | "clear_focus" | "focused_key"
                    | "selected_keys" | "selected_rows"
                    | "insert_row" | "replace_row" | "update_row" | "remove_row"
                    | "first_page" | "next_page" | "last_page" | "visible_rows",
            )
            | (
                "core.web.virtual",
                "window" | "window_measured" | "slice" | "indices"
                    | "plan" | "plan_measured" | "plan_from_sizes" | "plan_indices"
                    | "plan_slice" | "plan_viewport" | "plan_measure"
                    | "plan_viewport_state" | "plan_scroll_to" | "plan_resize"
                    | "plan_viewport_measure" | "plan_facts",
            )
            | (
                "core.web.store",
                "new" | "with_history" | "value" | "transaction" | "update" | "batch"
                    | "set" | "set_state" | "optimistic" | "patch" | "patch_active"
                    | "patch_commit" | "patch_generation" | "patch_rollback"
                    | "patch_transaction" | "back" | "forward" | "jump" | "scrub" | "restore"
                    | "history" | "history_at" | "events" | "events_since" | "clear_history"
                    | "history_enabled" | "history_limit" | "set_history_limit" | "cursor"
                    | "current_generation" | "subscribe" | "subscribe_selector"
                    | "subscription_active" | "subscription_unsubscribe" | "derived" | "selector"
                    | "inspect" | "facts_json" | "event_json",
            )
    )
}

pub fn core_fixed_sig(
    module: &str,
    name: &str,
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    if is_polymorphic_core_special(module, name) {
        return None;
    }
    core_fixed_sig_impl(module, name)
}

/// Project the signature used by a plain Core-call consumer.
///
/// A registered row is authoritative for the ordinary call path. Some
/// polymorphic spellings still have a fixed default shape in this module; use
/// that shape only when the row projection cannot carry the resolved type.
/// The inner `None` remains the raw no-return marker; callers that need a
/// concrete return type should use `core_call_semantic_signature`.
pub fn core_call_signature(
    module: &str,
    name: &str,
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    if let Some(row) = Syntax::core_call(module, name) {
        if let Some(signature) = core_fixed_sig_for_row(row) {
            return Some(signature);
        }
    }
    core_fixed_sig_impl(module, name)
}
/// Project a checked Core call into its concrete semantic return type.
///
/// The fixed-signature table uses the inner `None` to record a registered
/// no-return call. The outer `None` remains the distinct "no checking fact"
/// state, so an unknown call can never acquire `Unit` by projection.
pub fn core_call_semantic_signature(
    module: &str,
    name: &str,
) -> Option<(Vec<(AccessConvention, Type)>, Type)> {
    core_call_signature(module, name)
        .map(|(params, ret)| (params, ret.unwrap_or_else(unit_ty)))
}

/// Complete the signature projection for polymorphic Core spellings whose
/// ordinary Canvas/default-call shape is still known to sema. The row and
/// fixed-signature paths remain first; this is only the resolved surface
/// fallback for generic math and encoding calls.
pub fn core_call_surface_signature(
    module: &str,
    name: &str,
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    if let Some(signature) = core_call_signature(module, name) {
        return Some(signature);
    }
    let read = AccessConvention::Read;
    let int = Type::Int;
    let float = Type::Float;
    let result = |value| {
        Some((
            vec![(read, Type::String)],
            Some(result_ty(value, Type::String)),
        ))
    };
    match (module, name) {
        ("core.math", "abs") => Some((vec![(read, int.clone())], Some(int))),
        (
            "core.math",
            "sqrt" | "floor" | "ceil" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh"
            | "cosh" | "tanh" | "exp" | "ln" | "log2" | "log10" | "trunc" | "fract" | "sign"
            | "degrees" | "radians",
        ) => Some((vec![(read, float.clone())], Some(float))),
        ("core.math", "round") => Some((vec![(read, float)], Some(int))),
        ("core.math", "int_pow" | "gcd" | "lcm") => {
            Some((vec![(read, int.clone()), (read, int.clone())], Some(int)))
        }
        ("core.math", "pow" | "atan2" | "hypot") => Some((
            vec![(read, float.clone()), (read, float)],
            Some(Type::Float),
        )),
        ("core.math", "min" | "max") => {
            Some((vec![(read, int.clone()), (read, int.clone())], Some(int)))
        }
        ("core.math", "clamp") => Some((
            vec![(read, int.clone()), (read, int.clone()), (read, int)],
            Some(Type::Int),
        )),
        ("core.math", "lerp") => Some((
            vec![(read, float.clone()), (read, float.clone()), (read, float)],
            Some(Type::Float),
        )),
        ("core.math", "pi" | "e") => Some((vec![], Some(Type::Float))),
        ("core.math", "is_nan" | "is_inf" | "is_finite" | "is_even" | "is_odd") => {
            Some((vec![(read, Type::Float)], Some(Type::Bool)))
        }
        (
            "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
            | "core.encoding.yaml",
            "parse" | "decode",
        ) => result(Type::String),
        (
            "core.encoding.hex" | "core.encoding.base64" | "core.encoding.base32",
            "decode" | "decode_url",
        ) => result(Type::String),
        ("core.encoding.xml" | "core.encoding.cbor", "parse" | "decode" | "decode_bytes") => {
            result(Type::String)
        }
        ("core.event", "policy_sync") => {
            Some((vec![], Some(Type::Named("EventPolicy".to_string()))))
        }
        ("core.event", "scope") => Some((vec![], Some(Type::Named("EventScope".to_string())))),
        _ => None,
    }
}

/// Whether the canonical sema signature for a plain Core call is fallible.
pub fn core_call_is_fallible(module: &str, name: &str) -> bool {
    core_call_surface_signature(module, name)
        .and_then(|(_, ret)| ret)
        .is_some_and(|ret| ret.is_fallible())
}

/// Whether Canvas may synthesize safe starter arguments for this Core call.
pub fn core_call_has_safe_defaults(module: &str, name: &str) -> bool {
    module == "core.math"
        || matches!(
            (module, name),
            ("core.encoding.json", "parse" | "decode")
                | (
                    "core.encoding.hex" | "core.encoding.base64" | "core.encoding.base32",
                    "decode" | "decode_url"
                )
        )
}

fn web_named(name: &str) -> Type {
    Type::Named(name.to_string())
}

fn web_list(inner: Type) -> Type {
    Type::List(Box::new(inner))
}

fn web_map_string() -> Type {
    Type::Map {
        key: Box::new(Type::String),
        key_span: None,
        value: Box::new(Type::String),
    }
}

fn web_fn(param: Type, ret: Type) -> Type {
    Type::Fn {
        params: vec![param],
        ret: Some(Box::new(ret)),
        effect_bound: None,
        return_view_provenance: None,
        param_contract: None,
        call_metadata: None,
    }
}

fn web_apply(name: &str, arg: Type) -> Type {
    Type::Apply {
        name: name.to_string(),
        args: vec![arg],
    }
}

fn web_result(ok: Type) -> Type {
    result_ty(ok, Type::String)
}
fn web_callback_result(ok: Type) -> Type {
    result_ty(ok, Type::Named(Syntax::TYPE_ERR.to_string()))
}

fn web_fixed_sig(
    module: &str,
    name: &str,
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    let read = AccessConvention::Read;
    let string = Type::String;
    let int = Type::Int;
    let bool_ = Type::Bool;
    let t = web_named("T");
    let table = web_apply("WebTable", t.clone());
    let table_column = web_apply("WebTableColumn", t.clone());
    let table_page = web_apply("WebTablePage", t.clone());
    let table_row = web_apply("WebTableRow", t.clone());
    let table_state = web_named("WebTableState");
    let table_direction = web_named("WebTableSortDirection");
    let virtual_window = web_named("WebVirtualWindow");
    let virtual_plan = web_named("WebVirtualPlan");
    let virtual_viewport = web_named("WebVirtualPlanViewport");
    let router = web_named("WebRouter");
    let router_field = web_named("WebRouterField");
    let router_codec = web_named("WebRouterSearchCodec");
    let navigation = web_named("WebNavigation");
    let http_router = web_named("HTTPRouter");
    let query = web_named("WebQuery");
    let query_mode = web_named("WebQueryNetworkMode");
    let mutation_state = web_named("WebMutationState");
    let query_mutation = web_fn(
        string.clone(),
        web_callback_result(string.clone()),
    );
    let query_replay = web_fn(string.clone(), web_callback_result(unit_ty()));
    let form_action_error = web_named("WebFormActionError");
    let signal = web_apply("Signal", t.clone());
    let form = web_named("WebForm");
    let form_input = web_named("WebFormInput");
    let form_field = web_named("WebFormFieldSpec");
    let form_typed = web_named("WebFormTyped");
    let form_chain = web_named("WebFormValidationChain");
    let form_decoded = web_named("WebFormDecodedInput");
    let form_errors = web_named("WebFormErrorState");
    let form_lifecycle = web_named("WebFormLifecycle");
    let form_validation = web_named("WebFormTypedValidation");
    let form_timing = web_named("WebFormValidationTiming");
    let form_validator = web_fn(string.clone(), web_callback_result(unit_ty()));
    let form_submission = web_named("WebFormTypedSubmission");
    let form_subscription = web_apply("Derived", web_named("WebFormFieldState"));
    match (module, name) {
        // D-WEBFORM1=A: the ratified `web.form(Model, action: handler)`
        // surface lowers to the existing typed-form runtime boundary. Sema
        // replaces the model/action source values with a derived input and
        // stable action name before this signature is consumed.
        ("core.web", "openapi") => Some((vec![(read, http_router)], Some(string.clone()))),
        ("core.web", "form") => Some((
            vec![(read, form_input.clone()), (read, string.clone())],
            Some(form_typed.clone()),
        )),
        ("core.web.router", "route") => Some((
            vec![
                (read, router.clone()),
                (read, string.clone()),
                (read, web_list(router_field.clone())),
                (read, web_list(router_field.clone())),
                (read, web_fn(navigation.clone(), web_callback_result(string.clone()))),
            ],
            Some(web_result(router.clone())),
        )),
        ("core.web.router", "route_with_search_codec") => Some((
            vec![
                (read, router.clone()),
                (read, string.clone()),
                (read, web_list(router_field.clone())),
                (read, web_list(router_field.clone())),
                (read, router_codec.clone()),
                (read, web_fn(navigation.clone(), web_callback_result(string.clone()))),
            ],
            Some(web_result(router.clone())),
        )),
        ("core.web.router", "not_found") => Some((
            vec![
                (read, router.clone()),
                (read, web_fn(string.clone(), string.clone())),
            ],
            Some(web_result(router.clone())),
        )),
        ("core.web.router", "navigate" | "preload") => Some((
            vec![(read, router.clone()), (read, string.clone())],
            Some(web_result(web_named("WebNavigation"))),
        )),
        ("core.web.router", "abort") => {
            Some((vec![(read, router.clone())], Some(bool_.clone())))
        }
        ("core.web.router", "stale" | "invalidate" | "collect") => Some((
            vec![(read, router.clone()), (read, string.clone())],
            Some(int.clone()),
        )),
        ("core.web.router", "cache_state") => Some((
            vec![(read, router.clone()), (read, string.clone())],
            Some(web_named("WebRouterCacheState")),
        )),
        ("core.web.router", "cache_show") => {
            Some((vec![(read, router)], Some(string.clone())))
        }
        ("core.web.router", "current") => Some((
            vec![(read, router)],
            Some(web_named("WebNavigationState")),
        )),
        ("core.web.router", "show") => Some((vec![(read, router)], Some(string.clone()))),
        ("core.web.router", "link") => Some((
            vec![
                (read, router),
                (read, string.clone()),
                (read, web_map_string()),
                (read, web_map_string()),
            ],
            Some(web_result(string.clone())),
        )),
        ("core.web.query", "new") => Some((
            vec![(read, string.clone()), (read, web_named("LiveQuery"))],
            Some(query.clone()),
        )),
        ("core.web.query", "live") => Some((
            vec![
                (read, string.clone()),
                (read, string.clone()),
                (read, string.clone()),
                (read, Type::Fn {
                    params: vec![],
                    ret: Some(Box::new(web_callback_result(string.clone()))),
                    effect_bound: None,
                    return_view_provenance: None,
                    param_contract: None,
                    call_metadata: None,
                }),
            ],
            Some(query.clone()),
        )),
        ("core.web.query", "mutate") => Some((
            vec![
                (read, query.clone()),
                (read, string.clone()),
                (read, Type::Option(Box::new(string.clone()))),
                (read, query_mutation.clone()),
            ],
            Some(mutation_state.clone()),
        )),
        ("core.web.query", "mutate_with_invalidations") => Some((
            vec![
                (read, query.clone()),
                (read, string.clone()),
                (read, Type::Option(Box::new(string.clone()))),
                (read, web_list(string.clone())),
                (read, query_mutation),
            ],
            Some(mutation_state.clone()),
        )),
        ("core.web.query", "retry") => Some((
            vec![(read, query.clone()), (read, query_replay)],
            Some(web_result(int.clone())),
        )),
        ("core.web.query", "subscribe") => {
            Some((vec![(read, string.clone())], Some(query.clone())))
        }
        ("core.web.query", "invalidate") => {
            Some((vec![(read, query.clone())], Some(int.clone())))
        }
        ("core.web.query", "get" | "show") => {
            Some((vec![(read, query.clone())], Some(string.clone())))
        }
        ("core.web.query", "state") => Some((
            vec![(read, query.clone())],
            Some(web_named("WebQueryState")),
        )),
        ("core.web.query", "state_signal") => Some((
            vec![(read, query.clone())],
            Some(web_apply("Signal", web_named("WebQueryState"))),
        )),
        ("core.web.query", "mutation_state") => Some((
            vec![(read, query.clone())],
            Some(mutation_state.clone()),
        )),
        ("core.web.query", "mutation_signal") => Some((
            vec![(read, query.clone())],
            Some(web_apply("Signal", mutation_state.clone())),
        )),
        ("core.web.query", "queue") => Some((
            vec![(read, query.clone()), (read, string.clone())],
            Some(web_result(int.clone())),
        )),
        ("core.web.query", "refresh") => {
            Some((vec![(read, query)], Some(web_result(unit_ty()))))
        }
        ("core.web.query", "facts") => {
            Some((vec![(read, query.clone())], Some(string.clone())))
        }
        ("core.web.query", "cancel") => {
            Some((vec![(read, query.clone())], Some(bool_.clone())))
        }
        ("core.web.query", "set_online") => Some((
            vec![(read, query.clone()), (read, bool_.clone())],
            Some(unit_ty()),
        )),
        ("core.web.query", "set_mode") => Some((
            vec![(read, query), (read, query_mode)],
            Some(unit_ty()),
        )),
        ("core.web.forms", "new") => Some((vec![(read, string.clone())], Some(form.clone()))),
        ("core.web.forms", "field") => Some((
            vec![
                (read, form.clone()),
                (read, string.clone()),
                (read, string.clone()),
                (read, bool_.clone()),
            ],
            Some(web_result(form.clone())),
        )),
        ("core.web.forms", "set") => Some((
            vec![
                (read, form.clone()),
                (read, string.clone()),
                (read, string.clone()),
            ],
            Some(web_result(unit_ty())),
        )),
        ("core.web.forms", "blur") => Some((
            vec![(read, form.clone()), (read, string.clone())],
            Some(web_result(unit_ty())),
        )),
        ("core.web.forms", "validate") => {
            Some((vec![(read, form.clone())], Some(web_result(unit_ty()))))
        }
        ("core.web.forms", "validate_async") => Some((
            vec![(read, form.clone())],
            Some(form.clone()),
        )),
        ("core.web.forms", "submit" | "no_script") => Some((
            vec![(read, form.clone())],
            Some(web_result(string.clone())),
        )),
        ("core.web.forms", "html" | "show") => {
            Some((vec![(read, form.clone())], Some(string.clone())))
        }
        ("core.web.forms", "input") => Some((
            vec![
                (read, string.clone()),
                (read, web_list(form_field.clone())),
            ],
            Some(web_result(form_input.clone())),
        )),
        ("core.web.forms", "input_rename") => Some((
            vec![
                (read, form_input.clone()),
                (read, string.clone()),
                (read, string.clone()),
            ],
            Some(web_result(form_input.clone())),
        )),
        ("core.web.forms", "input_exclude") => Some((
            vec![(read, form_input.clone()), (read, string.clone())],
            Some(web_result(form_input.clone())),
        )),
        ("core.web.forms", "input_group") => Some((
            vec![
                (read, form_input.clone()),
                (read, string.clone()),
                (read, string.clone()),
            ],
            Some(web_result(form_input.clone())),
        )),
        ("core.web.forms", "input_replace") => Some((
            vec![
                (read, form_input.clone()),
                (read, string.clone()),
                (read, form_field.clone()),
            ],
            Some(web_result(form_input.clone())),
        )),
        ("core.web.forms", "typed") => Some((
            vec![(read, form_input.clone()), (read, string.clone())],
            Some(web_result(form_typed.clone())),
        )),
        ("core.web.forms", "typed_set") => Some((
            vec![
                (read, form_typed.clone()),
                (read, string.clone()),
                (read, string.clone()),
            ],
            Some(web_result(unit_ty())),
        )),
        ("core.web.forms", "typed_set_async_validator") => Some((
            vec![
                (read, form_typed.clone()),
                (read, string.clone()),
                (read, form_timing.clone()),
                (read, int.clone()),
                (read, form_validator.clone()),
            ],
            Some(web_result(unit_ty())),
        )),
        ("core.web.forms", "typed_set_action") => Some((
            vec![
                (read, form_typed.clone()),
                (
                    read,
                    web_fn(
                        form_decoded.clone(),
                        result_ty(string.clone(), form_action_error.clone()),
                    ),
                ),
            ],
            Some(unit_ty()),
        )),
        ("core.web.forms", "action_error") => Some((vec![], Some(form_action_error.clone()))),
        ("core.web.forms", "action_field_error") => Some((
            vec![
                (read, form_action_error.clone()),
                (read, string.clone()),
                (read, string.clone()),
            ],
            Some(form_action_error.clone()),
        )),
        ("core.web.forms", "action_form_error") => Some((
            vec![(read, form_action_error.clone()), (read, string.clone())],
            Some(form_action_error.clone()),
        )),
        ("core.web.forms", "typed_blur") => Some((
            vec![(read, form_typed.clone()), (read, string.clone())],
            Some(web_result(unit_ty())),
        )),
        ("core.web.forms", "typed_validate") => Some((
            vec![(read, form_typed.clone())],
            Some(web_result(unit_ty())),
        )),
        ("core.web.forms", "typed_validate_async") => Some((
            vec![(read, form_typed.clone())],
            Some(form_validation.clone()),
        )),
        ("core.web.forms", "typed_validate_field") => Some((
            vec![
                (read, form_typed.clone()),
                (read, string.clone()),
                (read, form_timing.clone()),
                (read, form_validator.clone()),
            ],
            Some(form_chain.clone()),
        )),
        ("core.web.forms", "typed_validation_render") => Some((
            vec![(read, form_chain)],
            Some(string.clone()),
        )),
        ("core.web.forms", "typed_submit") | ("core.web.forms", "typed_no_script") => {
            Some((
                vec![(read, form_typed.clone())],
                Some(web_result(string.clone())),
            ))
        }
        ("core.web.forms", "typed_submit_async") => Some((
            vec![(read, form_typed.clone())],
            Some(form_submission.clone()),
        )),
        ("core.web.forms", "typed_post") => Some((
            vec![(read, form_typed.clone()), (read, string.clone())],
            Some(web_result(string.clone())),
        )),
        ("core.web.forms", "typed_decode_post") => Some((
            vec![(read, form_typed.clone()), (read, string.clone())],
            Some(web_result(form_decoded.clone())),
        )),
        ("core.web.forms", "typed_html" | "typed_show") => Some((
            vec![(read, form_typed.clone())],
            Some(string.clone()),
        )),
        ("core.web.forms", "typed_state") => Some((
            vec![(read, form_typed.clone())],
            Some(web_named("WebFormState")),
        )),
        ("core.web.forms", "typed_lifecycle") => Some((
            vec![(read, form_typed.clone())],
            Some(form_lifecycle.clone()),
        )),
        ("core.web.forms", "typed_errors") => Some((
            vec![(read, form_typed.clone())],
            Some(form_errors.clone()),
        )),
        ("core.web.forms", "typed_focus") => Some((
            vec![(read, form_typed.clone()), (read, string.clone())],
            Some(web_result(unit_ty())),
        )),
        ("core.web.forms", "typed_cancel") => Some((
            vec![(read, form_typed.clone())],
            Some(unit_ty()),
        )),
        ("core.web.forms", "typed_select_field") => Some((
            vec![(read, form_typed), (read, string)],
            Some(web_result(form_subscription)),
        )),
        ("core.web.forms", "typed_validation_wait") => Some((
            vec![(AccessConvention::Move, form_validation.clone())],
            Some(web_result(unit_ty())),
        )),
        ("core.web.forms", "typed_validation_cancel") => Some((
            vec![(read, form_validation)],
            Some(unit_ty()),
        )),
        ("core.web.forms", "typed_submission_wait") => Some((
            vec![(AccessConvention::Move, form_submission.clone())],
            Some(web_result(string.clone())),
        )),
        ("core.web.forms", "typed_submission_cancel") => Some((
            vec![(read, form_submission)],
            Some(unit_ty()),
        )),
        ("core.web.table", "new") => Some((
            vec![(read, string.clone()), (read, web_list(t.clone()))],
            Some(table.clone()),
        )),
        ("core.web.table", "new_keyed") => Some((
            vec![
                (read, string.clone()),
                (read, web_list(t.clone())),
                (read, web_fn(t.clone(), string.clone())),
            ],
            Some(table.clone()),
        )),
        ("core.web.table", "column") => Some((
            vec![
                (read, string.clone()),
                (read, string.clone()),
                (read, web_fn(t.clone(), string.clone())),
            ],
            Some(table_column.clone()),
        )),
        ("core.web.table", "with_column") => Some((
            vec![(read, table.clone()), (read, table_column)],
            Some(table.clone()),
        )),
        ("core.web.table", "with_server_page") => Some((
            vec![
                (read, table.clone()),
                (
                    read,
                    web_fn(
                        table_state.clone(),
                        web_callback_result(table_page.clone()),
                    ),
                ),
            ],
            Some(table.clone()),
        )),
        ("core.web.table", "state") => Some((vec![(read, table.clone())], Some(table_state.clone()))),
        ("core.web.table", "facts") => Some((vec![(read, table.clone())], Some(string.clone()))),
        ("core.web.table", "keys") => Some((
            vec![(read, table.clone())],
            Some(web_result(web_list(string.clone()))),
        )),
        ("core.web.table", "page_state") => Some((
            vec![(read, table.clone())],
            Some(web_result(table_page.clone())),
        )),
        ("core.web.table", "sort") => Some((
            vec![
                (read, web_list(t.clone())),
                (read, bool_.clone()),
                (read, web_fn(t.clone(), string.clone())),
            ],
            Some(web_list(t.clone())),
        )),
        ("core.web.table", "filter") => Some((
            vec![
                (read, web_list(t.clone())),
                (read, web_fn(t.clone(), bool_.clone())),
            ],
            Some(web_list(t.clone())),
        )),
        ("core.web.table", "page") => Some((
            vec![
                (read, web_list(t.clone())),
                (read, int.clone()),
                (read, int.clone()),
            ],
            Some(table_page.clone()),
        )),
        ("core.web.table", "sort_by") => Some((
            vec![(read, table.clone()), (read, string.clone()), (read, table_direction)],
            Some(table.clone()),
        )),
        ("core.web.table", "filter_by") => Some((
            vec![(read, table.clone()), (read, string.clone()), (read, string.clone())],
            Some(table.clone()),
        )),
        ("core.web.table", "paginate") => Some((
            vec![(read, table.clone()), (read, int.clone()), (read, int.clone())],
            Some(table.clone()),
        )),
        ("core.web.table", "set_rows") => Some((
            vec![(read, table.clone()), (read, web_list(t.clone()))],
            Some(web_result(unit_ty())),
        )),
        ("core.web.table", "set_selected") => Some((
            vec![(read, table.clone()), (read, string.clone()), (read, bool_.clone())],
            Some(web_result(table.clone())),
        )),
        ("core.web.table", "toggle_selection") => Some((
            vec![(read, table.clone()), (read, string.clone())],
            Some(web_result(table.clone())),
        )),
        ("core.web.table", "focus") => Some((
            vec![(read, table.clone()), (read, string.clone())],
            Some(web_result(table.clone())),
        )),
        ("core.web.table", "clear_focus") => {
            Some((vec![(read, table.clone())], Some(table.clone())))
        }
        ("core.web.table", "focused_key") => Some((
            vec![(read, table.clone())],
            Some(Type::Option(Box::new(string.clone()))),
        )),
        ("core.web.table", "clear_selection") => {
            Some((vec![(read, table.clone())], Some(table.clone())))
        }
        ("core.web.table", "selected_keys") => Some((
            vec![(read, table.clone())],
            Some(web_list(string.clone())),
        )),
        ("core.web.table", "selected_rows") => Some((
            vec![(read, table.clone())],
            Some(web_result(web_list(table_row.clone()))),
        )),
        ("core.web.table", "insert_row") => Some((
            vec![(read, table.clone()), (read, t.clone())],
            Some(web_result(string.clone())),
        )),
        ("core.web.table", "replace_row") => Some((
            vec![(read, table.clone()), (read, string.clone()), (read, t.clone())],
            Some(web_result(unit_ty())),
        )),
        ("core.web.table", "update_row") => Some((
            vec![
                (read, table.clone()),
                (read, string.clone()),
                (read, web_fn(t.clone(), t.clone())),
            ],
            Some(web_result(unit_ty())),
        )),
        ("core.web.table", "remove_row") => Some((
            vec![(read, table.clone()), (read, string.clone())],
            Some(web_result(t.clone())),
        )),
        ("core.web.table", "first_page" | "next_page") => Some((
            vec![(read, table.clone())],
            Some(table.clone()),
        )),
        ("core.web.table", "last_page") => Some((
            vec![(read, table.clone())],
            Some(web_result(table.clone())),
        )),
        ("core.web.table", "visible_rows") => Some((
            vec![(read, table.clone()), (read, virtual_plan.clone())],
            Some(web_result(web_list(table_row))),
        )),
        ("core.web.virtual", "window") => Some((
            vec![
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
            ],
            Some(virtual_window.clone()),
        )),
        ("core.web.virtual", "window_measured") => Some((
            vec![
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, web_list(int.clone())),
            ],
            Some(virtual_window.clone()),
        )),
        ("core.web.virtual", "slice") => Some((
            vec![(read, web_list(t.clone())), (read, virtual_window)],
            Some(web_list(t.clone())),
        )),
        ("core.web.virtual", "indices") => Some((
            vec![(read, virtual_window)],
            Some(web_list(int.clone())),
        )),
        ("core.web.virtual", "plan") => Some((
            vec![
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
            ],
            Some(virtual_plan.clone()),
        )),
        ("core.web.virtual", "plan_measured") => Some((
            vec![
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, Type::List(Box::new(Type::Tuple(vec![
                    ("index".to_string(), Box::new(int.clone())),
                    ("size".to_string(), Box::new(int.clone())),
                ])))),
            ],
            Some(virtual_plan.clone()),
        )),
        ("core.web.virtual", "plan_from_sizes") => Some((
            vec![
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
                (read, web_list(int.clone())),
            ],
            Some(virtual_plan.clone()),
        )),
        ("core.web.virtual", "plan_measure") => Some((
            vec![(read, virtual_plan.clone()), (read, int.clone()), (read, int.clone())],
            Some(virtual_plan.clone()),
        )),
        ("core.web.virtual", "plan_slice") => Some((
            vec![(read, web_list(t.clone())), (read, virtual_plan.clone())],
            Some(web_list(t.clone())),
        )),
        ("core.web.virtual", "plan_indices") => Some((
            vec![(read, virtual_plan.clone())],
            Some(web_list(int.clone())),
        )),
        ("core.web.virtual", "plan_viewport") => Some((
            vec![(read, virtual_plan)],
            Some(virtual_viewport.clone()),
        )),
        ("core.web.virtual", "plan_viewport_state") => Some((
            vec![(read, virtual_viewport.clone())],
            Some(virtual_plan.clone()),
        )),
        ("core.web.virtual", "plan_scroll_to") => Some((
            vec![(read, virtual_viewport.clone()), (read, int.clone())],
            Some(unit_ty()),
        )),
        ("core.web.virtual", "plan_resize") => Some((
            vec![
                (read, virtual_viewport.clone()),
                (read, int.clone()),
                (read, int.clone()),
            ],
            Some(unit_ty()),
        )),
        ("core.web.virtual", "plan_viewport_measure") => Some((
            vec![
                (read, virtual_viewport),
                (read, int.clone()),
                (read, int),
            ],
            Some(unit_ty()),
        )),
        ("core.web.store", "value") => Some((
            vec![(read, web_apply("WebStore", t.clone()))],
            Some(t.clone()),
        )),
        ("core.web.store", "signal" | "state_signal") => Some((
            vec![(read, web_apply("WebStore", t.clone()))],
            Some(signal.clone()),
        )),
        ("core.web.store", "new") => Some((
            vec![(read, string.clone()), (read, t.clone())],
            Some(web_apply("WebStore", t.clone())),
        )),
        ("core.web.store", "with_history") => Some((
            vec![
                (read, string.clone()),
                (read, t.clone()),
                (read, int.clone()),
            ],
            Some(web_apply("WebStore", t.clone())),
        )),
        ("core.web.store", "transaction" | "update" | "batch") => Some((
            vec![
                (read, web_apply("WebStore", t.clone())),
                (read, string.clone()),
                (read, web_list(string.clone())),
                (read, web_fn(t.clone(), unit_ty())),
            ],
            Some(web_apply("WebStoreTransaction", t.clone())),
        )),
        ("core.web.store", "set" | "set_state") => Some((
            vec![(read, web_apply("WebStore", t.clone())), (read, t.clone())],
            Some(web_apply("WebStoreTransaction", t.clone())),
        )),
        ("core.web.store", "optimistic" | "patch") => Some((
            vec![
                (read, web_apply("WebStore", t.clone())),
                (read, string.clone()),
                (read, web_list(string.clone())),
                (read, web_fn(t.clone(), unit_ty())),
            ],
            Some(web_apply("WebStorePatch", t.clone())),
        )),
        ("core.web.store", "patch_generation") => Some((
            vec![(read, web_apply("WebStorePatch", t.clone()))],
            Some(int.clone()),
        )),
        ("core.web.store", "patch_transaction") => Some((
            vec![(read, web_apply("WebStorePatch", t.clone()))],
            Some(web_apply("WebStoreTransaction", t.clone())),
        )),
        ("core.web.store", "patch_active") => Some((
            vec![(read, web_apply("WebStorePatch", t.clone()))],
            Some(bool_.clone()),
        )),
        ("core.web.store", "patch_commit") => Some((
            vec![(AccessConvention::Move, web_apply("WebStorePatch", t.clone()))],
            Some(web_apply("WebStoreTransaction", t.clone())),
        )),
        ("core.web.store", "patch_rollback") => Some((
            vec![(AccessConvention::Move, web_apply("WebStorePatch", t.clone()))],
            Some(Type::Option(Box::new(t.clone()))),
        )),
        ("core.web.store", "back" | "forward" | "restore") => Some((
            vec![(read, web_apply("WebStore", t.clone()))],
            Some(Type::Option(Box::new(t.clone()))),
        )),
        ("core.web.store", "jump" | "scrub") => Some((
            vec![(read, web_apply("WebStore", t.clone())), (read, int.clone())],
            Some(Type::Option(Box::new(t.clone()))),
        )),
        ("core.web.store", "history") => Some((
            vec![(read, web_apply("WebStore", t.clone()))],
            Some(web_list(web_apply("WebStoreTransaction", t.clone()))),
        )),
        ("core.web.store", "history_at") => Some((
            vec![(read, web_apply("WebStore", t.clone())), (read, int.clone())],
            Some(Type::Option(Box::new(web_apply("WebStoreTransaction", t.clone())))),
        )),
        ("core.web.store", "events") => Some((
            vec![(read, web_apply("WebStore", t.clone()))],
            Some(web_list(web_named("WebStoreEvent"))),
        )),
        ("core.web.store", "events_since") => Some((
            vec![(read, web_apply("WebStore", t.clone())), (read, int.clone())],
            Some(web_list(web_named("WebStoreEvent"))),
        )),
        ("core.web.store", "clear_history" | "set_history_limit") => Some((
            if name == "clear_history" {
                vec![(read, web_apply("WebStore", t.clone()))]
            } else {
                vec![
                    (read, web_apply("WebStore", t.clone())),
                    (read, int.clone()),
                ]
            },
            Some(unit_ty()),
        )),
        ("core.web.store", "history_enabled") => Some((
            vec![(read, web_apply("WebStore", t.clone()))],
            Some(bool_.clone()),
        )),
        ("core.web.store", "history_limit" | "cursor" | "current_generation") => Some((
            vec![(read, web_apply("WebStore", t.clone()))],
            Some(int.clone()),
        )),
        ("core.web.store", "subscribe") => Some((
            vec![
                (read, web_apply("WebStore", t.clone())),
                (read, web_fn(t.clone(), unit_ty())),
            ],
            Some(web_named("WebStoreSubscription")),
        )),
        ("core.web.store", "subscription_unsubscribe") => Some((
            vec![(read, web_named("WebStoreSubscription"))],
            Some(unit_ty()),
        )),
        ("core.web.store", "subscription_active") => Some((
            vec![(read, web_named("WebStoreSubscription"))],
            Some(bool_.clone()),
        )),
        ("core.web.store", "subscribe_selector") => Some((
            vec![
                (read, web_apply("WebStore", t.clone())),
                (read, web_fn(t.clone(), web_named("U"))),
                (read, web_fn(web_named("U"), unit_ty())),
            ],
            Some(web_named("WebStoreSubscription")),
        )),
        ("core.web.store", "derived" | "selector") => Some((
            vec![
                (read, web_apply("WebStore", t.clone())),
                (read, web_fn(t.clone(), web_named("U"))),
            ],
            Some(web_apply("Derived", web_named("U"))),
        )),
        ("core.web.store", "inspect") => Some((
            vec![(read, web_apply("WebStore", t.clone()))],
            Some(web_apply("WebStoreInspection", t.clone())),
        )),
        ("core.web.store", "facts_json" | "event_json") => Some((
            vec![(read, web_apply("WebStore", t))],
            Some(string),
        )),
        _ => None,
    }
}


fn core_fixed_sig_impl(
    module: &str,
    name: &str,
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    let read = AccessConvention::Read;
    let moved = AccessConvention::Move;
    let string = Type::String;
    let int = Type::Int;
    let float = Type::Float;
    let bool_ = Type::Bool;
    let unit = unit_ty();
    let measurement = Type::Apply {
        name: Syntax::TYPE_MEASUREMENT.to_string(),
        args: vec![Type::Float],
    };
    let io = io_error_ty();
    let json = json_ty();
    let list_u8 = Type::List(Box::new(u8_ty()));
    let path = Type::Union(vec![Type::String, Type::Named("Path".to_string())]);
    let io_unit = result_ty(unit.clone(), io.clone());
    let ui_error = Type::Named("UiHostError".to_string());
    let ui_result = |ok| result_ty(ok, ui_error.clone());
    let ui_preview = Type::Named("UiPreview".to_string());
    let ui_node_callback = Type::Fn {
        params: vec![],
        ret: Some(Box::new(Type::Named("UiNode".to_string()))),
        effect_bound: None,
        return_view_provenance: None,
        param_contract: None,
        call_metadata: None,
    };
    let comparison_data = Type::Named("DataTree".to_string());
    let comparison_fn = Type::Fn {
        params: vec![comparison_data.clone()],
        ret: Some(Box::new(comparison_data.clone())),
        effect_bound: None,
        return_view_provenance: None,
        param_contract: None,
        call_metadata: None,
    };
    if let Some(signature) = web_fixed_sig(module, name) {
        return Some(signature);
    }

    match (module, name) {
        ("core.files", "scope") => Some((
            vec![(read, Type::Named(Syntax::TYPE_AUTHORITY.to_string()))],
            Some(Type::Named("FileScope".to_string())),
        )),
        ("core.files", "read") => Some((vec![(read, path)], Some(result_ty(string, io.clone())))),
        ("core.files", "read_bytes") => Some((
            vec![(
                read,
                Type::Union(vec![Type::String, Type::Named("Path".to_string())]),
            )],
            Some(result_ty(list_u8, io.clone())),
        )),
        ("core.files", "map") => Some((
            vec![(read, path)],
            Some(result_ty(Type::Named("MappedFile".to_string()), io)),
        )),
        // D-FILES-WRITE1 (merge) + D-FILES-APPEND1=A: `write`/`append_all` are the
        // whole-file convenience twins of the streaming `open`/`create`/`append`
        // handle constructors below. `append_all` (not `append`) so it doesn't
        // collide with the streaming handle's `.append(text)` method in the same
        // `core.files` namespace.
        ("core.files", "write" | "append_all") => {
            Some((vec![(read, path), (read, Type::String)], Some(io_unit)))
        }
        ("core.files", "write_bytes") => {
            Some((vec![(read, path), (read, list_u8.clone())], Some(io_unit)))
        }
        ("core.files", "exists" | "is_dir") => Some((vec![(read, path)], Some(bool_))),
        (
            "core.files",
            "remove" | "create_dir" | "create_dir_all" | "remove_dir" | "remove_all",
        ) => Some((
            vec![(read, path)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "stat") => Some((
            vec![(read, path)],
            Some(result_ty(Type::Named("Stat".to_string()), io_error_ty())),
        )),
        ("core.files", "set_mode") => Some((
            vec![(read, path), (read, int)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "canonicalize" | "absolute") => Some((
            vec![(read, path)],
            Some(result_ty(Type::String, io_error_ty())),
        )),
        // D-LSDIR1=A: returns [DirEntry] ({name, path, is_dir}) — full path + type in one step.
        ("core.files", "list_dir") => Some((
            vec![(read, path)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("DirEntry".to_string()))),
                io_error_ty(),
            )),
        )),
        ("core.files", "copy" | "rename") => Some((
            vec![(read, path.clone()), (read, path.clone())],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "copy_dir") => Some((
            vec![(read, path.clone()), (read, path.clone())],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "symlink" | "hard_link") => Some((
            vec![(read, path.clone()), (read, path.clone())],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "read_link") => Some((
            vec![(read, path)],
            Some(result_ty(Type::String, io_error_ty())),
        )),
        // `walk` and `walk_parallel` share one ignore-aware surface and one
        // result shape; the second argument names the ignore file.
        ("core.files", "walk" | "walk_parallel" | "walk_files") => Some((
            vec![(read, path), (read, Type::Option(Box::new(Type::String)))],
            Some(result_ty(
                Type::List(Box::new(Type::Named("WalkEntry".to_string()))),
                io_error_ty(),
            )),
        )),
        ("core.files", "glob") => Some((
            vec![(read, path)],
            Some(result_ty(Type::List(Box::new(Type::String)), io_error_ty())),
        )),
        ("core.files", "read_at") => Some((
            vec![(read, path), (read, Type::Int), (read, Type::Int)],
            Some(result_ty(Type::List(Box::new(u8_ty())), io_error_ty())),
        )),
        ("core.files", "write_at") => Some((
            vec![
                (read, path),
                (read, Type::Int),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "fsync") => Some((
            vec![(read, path.clone())],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "write_atomic") => Some((
            vec![(read, path), (read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "temp_dir") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("TempDir".to_string()), io_error_ty())),
        )),
        ("core.files", "temp_file") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("TempFile".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.files", "lock") => Some((
            vec![(
                read,
                Type::Union(vec![Type::String, Type::Named("Path".to_string())]),
            )],
            Some(result_ty(
                Type::Named("FileLock".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.watcher", "files") => Some((
            vec![(
                read,
                Type::Union(vec![Type::String, Type::Named("Path".to_string())]),
            )],
            Some(result_ty(
                Type::Named("WatchHandle".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.watcher", "process_pid") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("WatchHandle".to_string())),
        )),
        ("core.watcher", "port") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(Type::Named("WatchHandle".to_string())),
        )),
        ("core.watcher", "set") => Some((vec![], Some(Type::Named("WatchSet".to_string())))),
        ("core.process", "argv" | "args") => {
            Some((vec![], Some(Type::List(Box::new(Type::String)))))
        }
        ("core.term", "confirm") => Some((vec![(read, Type::String)], Some(Type::Bool))),
        ("core.term", "choose") => Some((
            vec![
                (read, Type::String),
                (read, Type::List(Box::new(Type::String))),
            ],
            Some(result_ty(Type::String, io_error_ty())),
        )),
        ("core.term", "input_secret") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, io_error_ty())),
        )),
        ("core.term", "read_all_input") => {
            Some((vec![], Some(result_ty(Type::String, io_error_ty()))))
        }
        // #1480: remaining core.term ledger gaps — thin wrappers over existing IO.
        ("core.term", "readline") => Some((vec![], Some(result_ty(Type::String, io_error_ty())))),
        ("core.term", "read_until") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, io_error_ty())),
        )),
        ("core.term", "take") => Some((
            vec![(read, Type::Int)],
            Some(result_ty(list_u8.clone(), io_error_ty())),
        )),
        ("core.term", "buffered") => Some((vec![], Some(Type::Named("StdinHandle".to_string())))),
        ("core.term", "binread") => Some((
            vec![(
                read,
                Type::Union(vec![Type::String, Type::Named("Path".to_string())]),
            )],
            Some(result_ty(list_u8.clone(), io_error_ty())),
        )),
        ("core.term", "binwrite") => Some((
            vec![
                (
                    read,
                    Type::Union(vec![Type::String, Type::Named("Path".to_string())]),
                ),
                (read, list_u8.clone()),
            ],
            Some(result_ty(unit.clone(), io_error_ty())),
        )),
        // D-STDIN1=A: streaming line-by-line stdin.
        ("core.term", "stdin") => Some((vec![], Some(Type::Named("StdinHandle".to_string())))),
        ("core.term", "stdout") => Some((vec![], Some(Type::Named("Stdout".to_string())))),
        ("core.term", "stderr") => Some((vec![], Some(Type::Named("Stderr".to_string())))),
        ("core.term", "terminal_width" | "terminal_height") => Some((vec![], Some(Type::Int))),
        ("core.term", "style" | "style_force") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::String),
        )),
        ("core.term", "progress") => Some((
            vec![(read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.sys", "get") => Some((
            vec![(read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.sys", "set") => Some((vec![(read, Type::String), (read, Type::String)], None)),
        ("core.sys", "unset") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Bool, Type::Named("EnvError".to_string()))),
        )),
        ("core.sys", "vars") => Some((
            vec![],
            Some(result_ty(
                Type::List(Box::new(Type::String)),
                Type::Named("EnvError".to_string()),
            )),
        )),
        ("core.sys", "current_dir") => Some((vec![], Some(result_ty(Type::String, io_error_ty())))),
        ("core.sys", "home_dir") => Some((vec![], Some(Type::Option(Box::new(Type::String))))),
        (
            "core.sys",
            "name" | "family" | "arch" | "temp_dir" | "executable" | "hostname" | "username"
            | "release" | "version",
        ) => Some((vec![], Some(Type::String))),
        ("core.sys", "expand") => Some((vec![(read, Type::String)], Some(Type::String))),
        (
            "core.sys",
            "pid" | "getpid" | "cpu_count" | "getppid" | "getuid" | "geteuid" | "getgid"
            | "getegid" | "getpgrp",
        ) => Some((vec![], Some(Type::Int))),
        ("core.sys", "umask" | "exitcode") => Some((vec![(read, Type::Int)], Some(Type::Int))),
        ("core.sys", "success") => Some((vec![(read, Type::Int)], Some(Type::Bool))),
        ("core.sys", "uptime") => Some((vec![], Some(Type::Float))),
        ("core.sys", "getgroups") => Some((vec![], Some(Type::List(Box::new(Type::Int))))),
        ("core.sys", "loadavg" | "times") => {
            Some((vec![], Some(Type::List(Box::new(Type::Float)))))
        }
        ("core.sys", "getpgid" | "getsid") => Some((
            vec![(read, Type::Int)],
            Some(result_ty(Type::Int, io_error_ty())),
        )),
        ("core.sys", "getpriority") => Some((
            vec![(read, Type::Int)],
            Some(result_ty(Type::Int, io_error_ty())),
        )),
        ("core.sys", "setpriority") => Some((
            vec![(read, Type::Int), (read, Type::Int)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.sys", "utime") => Some((
            vec![(read, path), (read, Type::Int), (read, Type::Int)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.sys", "sync") => Some((vec![], None)),
        ("core.sys", "stop") => Some((vec![(read, Type::Int)], None)),
        ("core.sys", "set_current_dir") => Some((
            vec![(read, path)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.sys", "on_interrupt") => Some((
            vec![(
                read,
                Type::Fn {
                    params: vec![],
                    ret: None,
                    effect_bound: None,
                    return_view_provenance: None,
                    param_contract: None,
                    call_metadata: None,
                },
            )],
            None,
        )),
        ("core.sys", "atexit") => Some((
            vec![(
                read,
                Type::Fn {
                    params: vec![],
                    ret: None,
                    effect_bound: None,
                    return_view_provenance: None,
                    param_contract: None,
                    call_metadata: None,
                },
            )],
            None,
        )),
        ("core.sys", "fork" | "setsid" | "wait") => {
            Some((vec![], Some(result_ty(Type::Int, io_error_ty()))))
        }
        ("core.sys", "waitpid") => Some((
            vec![(read, Type::Int), (read, Type::Int)],
            Some(result_ty(Type::Int, io_error_ty())),
        )),
        ("core.sys", "setuid" | "setgid") => Some((
            vec![(read, Type::Int)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.sys", "setpgrp") => Some((vec![], Some(result_ty(unit_ty(), io_error_ty())))),
        ("core.sys", "setpgid" | "kill") => Some((
            vec![(read, Type::Int), (read, Type::Int)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.sys", "initgroups") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.sys", "pipe") => Some((
            vec![],
            Some(result_ty(Type::List(Box::new(Type::Int)), io_error_ty())),
        )),
        ("core.sys", "close_fd") => Some((vec![(read, Type::Int)], None)),
        ("core.sys", "mkfifo") => Some((
            vec![(read, path), (read, Type::Int)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        // U13 (D-JPK-SECRETCRYPTO1): `core.crypto.vault.get(name)` — a decrypted repo
        // secret, `None` if `name` isn't in the store. Same "may be missing"
        // shape as `core.sys.get`.
        ("core.crypto.vault", "get") => Some((
            vec![(read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        // D-AUTH2=A / D-AUTH-TOKENPOLICY1=A: closed token verification. The
        // three-argument form is the beginner surface; verify_jwt's fixed
        // signature also describes its five-parameter registry ABI. The
        // arity-sensitive optional controls remain in core_call.rs.
        ("core.auth", "verify_jwt") => Some((
            vec![
                (read, Type::String),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::String),
                (read, Type::String),
                (read, Type::Named("Duration".into())),
            ],
            Some(result_ty(
                Type::Named("Claims".into()),
                Type::Named("AuthError".into()),
            )),
        )),
        ("core.auth", "verify_paseto") => Some((
            vec![
                (read, Type::String),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("Claims".into()),
                Type::Named("AuthError".into()),
            )),
        )),
        ("core.auth", "register_user") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(unit_ty(), Type::String)),
        )),
        ("core.auth", "password_login" | "oauth_finish") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
                (read, Type::Int),
            ],
            Some(result_ty(Type::Named("Session".into()), Type::String)),
        )),
        ("core.auth", "magic_link_consume") => Some((
            vec![(read, Type::String), (read, Type::Int), (read, Type::Int)],
            Some(result_ty(Type::Named("Session".into()), Type::String)),
        )),
        ("core.auth", "session_validate") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(Type::Named("Session".into()), Type::String)),
        )),
        ("core.auth", "session_show" | "session_user" | "session_cookie" | "session_id") => Some((
            vec![(read, Type::Named("Session".into()))],
            Some(Type::String),
        )),
        ("core.auth", "magic_link_issue") => Some((
            vec![(read, Type::String), (read, Type::Int), (read, Type::Int)],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.auth", "oauth_begin") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.sync", "text_new") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::Named("SyncText".into())),
        )),
        ("core.sync", "text_set") => Some((
            vec![
                (read, Type::Named("SyncText".into())),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(Type::Named("SyncText".into())),
        )),
        ("core.sync", "text_merge") => Some((
            vec![
                (read, Type::Named("SyncText".into())),
                (read, Type::Named("SyncText".into())),
            ],
            Some(Type::Named("SyncText".into())),
        )),
        ("core.sync", "text_show") => Some((
            vec![(read, Type::Named("SyncText".into()))],
            Some(Type::String),
        )),
        // D-SYNC1: a positioned edit is what makes
        // SyncText a text CRDT rather than a whole-document overwrite.
        ("core.sync", "text_edit") => Some((
            vec![
                (read, Type::Named("SyncText".into())),
                (read, Type::String),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::String),
            ],
            Some(Type::Named("SyncText".into())),
        )),
        // D-SYNC1: the expert view of the merge bookkeeping.
        ("core.sync", "text_metadata") => Some((
            vec![(read, Type::Named("SyncText".into()))],
            Some(Type::String),
        )),
        ("core.sync", "counter_new") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(Type::Named("SyncCounter".into())),
        )),
        ("core.sync", "counter_inc") => Some((
            vec![
                (read, Type::Named("SyncCounter".into())),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(Type::Named("SyncCounter".into())),
        )),
        ("core.sync", "counter_merge") => Some((
            vec![
                (read, Type::Named("SyncCounter".into())),
                (read, Type::Named("SyncCounter".into())),
            ],
            Some(Type::Named("SyncCounter".into())),
        )),
        ("core.sync", "counter_value") => Some((
            vec![(read, Type::Named("SyncCounter".into()))],
            Some(Type::Int),
        )),
        ("core.sync", "map_new") => Some((vec![], Some(Type::Named("SyncMap".into())))),
        ("core.sync", "list_new") => Some((vec![], Some(Type::Named("SyncList".into())))),
        ("core.sync", "map_set") => Some((
            vec![
                (read, Type::Named("SyncMap".into())),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(Type::Named("SyncMap".into())),
        )),
        ("core.sync", "map_get") => Some((
            vec![(read, Type::Named("SyncMap".into())), (read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.sync", "map_merge") => Some((
            vec![
                (read, Type::Named("SyncMap".into())),
                (read, Type::Named("SyncMap".into())),
            ],
            Some(Type::Named("SyncMap".into())),
        )),
        ("core.sync", "map_show") => Some((
            vec![(read, Type::Named("SyncMap".into()))],
            Some(Type::String),
        )),
        ("core.sync", "list_push") => Some((
            vec![
                (read, Type::Named("SyncList".into())),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(Type::Named("SyncList".into())),
        )),
        ("core.sync", "list_merge") => Some((
            vec![
                (read, Type::Named("SyncList".into())),
                (read, Type::Named("SyncList".into())),
            ],
            Some(Type::Named("SyncList".into())),
        )),
        ("core.sync", "list_show") => Some((
            vec![(read, Type::Named("SyncList".into()))],
            Some(Type::String),
        )),
        ("core.sync", "policy_new") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(Type::Named("RowPolicy".into()), Type::String)),
        )),
        ("core.sync", "policy_allows") => Some((
            vec![
                (read, Type::Named("RowPolicy".into())),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(Type::Bool),
        )),
        ("core.sync", "policy_show") => Some((
            vec![(read, Type::Named("RowPolicy".into()))],
            Some(Type::String),
        )),
        ("core.process", "exit") => Some((
            vec![(read, int)],
            Some(Type::Named(Syntax::TYPE_NEVER.to_string())),
        )),
        // D-FOUND-LIFECYCLE1=A: subscribe the root cancellation lifecycle to a process signal.
        ("core.process", "on_signal") => Some((
            vec![(read, Type::Named("ProcessSignal".to_string()))],
            Some(unit.clone()),
        )),
        ("core.process", "workspace") => Some((
            vec![],
            Some(Type::Named(Syntax::TYPE_AUTHORITY.to_string())),
        )),
        ("core.process", "run") => Some((
            vec![(read, Type::Named(Syntax::TYPE_SH.to_string()))],
            Some(result_ty(
                Type::Named("ProcessReceipt".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.process", "cmd") => Some((
            vec![(read, Type::List(Box::new(Type::String)))],
            Some(Type::Named("ProcessSpec".to_string())),
        )),
        // D-PROCESS1=A: pipeline takes a list of `ProcessSpec` (built via
        // `process.cmd(argv)...`), not raw argv lists — one canonical builder (I8).
        ("core.process", "pipeline") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::Named("ProcessSpec".to_string()))),
            )],
            Some(result_ty(
                Type::Named("ProcessReceipt".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.testing", "snap" | "golden") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::Bool),
        )),
        ("core.testing", "fixture" | "temp_dir") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.testing", "corpus") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.testing", "fake_clock") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("Clock".to_string())),
        )),
        ("core.testing", "fake_rng") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("Rng".to_string())),
        )),
        ("core.testing", "fake_data") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("Fake".to_string())),
        )),
        ("core.testing", "test_suite") => {
            Some((vec![], Some(Type::Named("TestSuite".to_string()))))
        }
        ("core.testing", "world") => Some((
            vec![(
                read,
                Type::Fn {
                    params: vec![Type::Named(
                        Syntax::DETERMINISTIC_WORLD_TYPE.to_string(),
                    )],
                    ret: Some(Box::new(unit.clone())),
                    effect_bound: None,
                    return_view_provenance: None,
                    param_contract: None,
                    call_metadata: None,
                },
            )],
            Some(unit.clone()),
        )),
        ("core.testing", "compare") => Some((
            vec![
                (read, Type::List(Box::new(comparison_data.clone()))),
                (read, comparison_fn.clone()),
                (read, comparison_fn),
                (read, Type::String),
            ],
            Some(Type::Named("TestComparison".to_string())),
        )),
        ("core.testing", "assert_equal") => Some((
            vec![(read, Type::Named("TestComparison".to_string()))],
            Some(Type::Bool),
        )),
        ("core.testing", "status") => Some((
            vec![(read, Type::Named("TestComparison".to_string()))],
            Some(Type::String),
        )),
        ("core.math", "sqrt" | "floor" | "ceil") => {
            Some((vec![(read, float.clone())], Some(float)))
        }
        ("core.math", "pow") => Some((
            vec![(read, Type::Float), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.math", "round") => Some((vec![(read, Type::Float)], Some(Type::Int))),
        ("core.math.random", "int") => {
            Some((vec![(read, Type::Int), (read, Type::Int)], Some(Type::Int)))
        }
        ("core.math.random", "float") => Some((vec![], Some(Type::Float))),
        ("core.math.random", "float_range") => Some((
            vec![(read, Type::Float), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.math.random", "bool") => Some((vec![(read, Type::Float)], Some(Type::Bool))),
        ("core.math.random", "normal") => Some((
            vec![(read, Type::Float), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.math.random", "exponential") => Some((vec![(read, Type::Float)], Some(Type::Float))),
        ("core.math.random", "seed") => Some((vec![(read, Type::Int)], None)),
        // D-RANDSPLIT1=A: seedable PRNG bytes — fast, NOT cryptographically secure.
        // Returns raw `[Int8N]`; for crypto contexts use `core.crypto.random.bytes`.
        ("core.math.random", "bytes") => {
            Some((vec![(read, Type::Int)], Some(Type::List(Box::new(u8_ty())))))
        }
        // D-CRYPTO-RNG1=A: fail-closed bytes from the target's tier-1 OS CSPRNG.
        // Edition 2026 keeps the infallible source shape and takes E3001/exit 70
        // on invalid length or provider failure; no weak fallback exists.
        ("core.crypto.random", "bytes") => {
            Some((vec![(read, Type::Int)], Some(Type::List(Box::new(u8_ty())))))
        }
        // D-DET1: deterministic injected RNG capability. `random.rng(seed)` builds a
        // reproducible `Rng` from a caller-supplied seed (a pure value); a `#Pure fn`
        // may draw randomness through it (`rng.int(lo, hi)` / `rng.float()`) while the
        // ambient `random.int(…)` stays E3403.
        ("core.math.random", "rng") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named(crate::Syntax::RNG_TYPE.to_string())),
        )),
        ("core.math.random", "split") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named(crate::Syntax::RNG_TYPE.to_string())),
        )),
        ("core.units", "from") => Some((
            vec![(read, float.clone()), (read, float)],
            Some(measurement),
        )),
        ("core.time", "sleep") => Some((
            vec![(read, Type::Named(crate::Syntax::DURATION_TYPE.to_string()))],
            None,
        )),
        ("core.time", "sleep_until") => Some((
            vec![(read, Type::Named("Instant".to_string()))],
            None,
        )),
        ("core.rt", "callback") => Some((
            vec![
                (read, Type::Int),
                (read, Type::Int),
                (
                    read,
                    Type::Fn {
                        params: vec![Type::List(Box::new(Type::Float))],
                        ret: None,
                        effect_bound: Some(Vec::new()),
                        return_view_provenance: None,
                        param_contract: None,
                        call_metadata: Some(FunctionCallMetadata {
                            conventions: vec![AccessConvention::Read],
                            ..FunctionCallMetadata::default()
                        }),
                    },
                ),
            ],
            Some(Type::Named("RealtimeStream".to_string())),
        )),
        ("core.tasks", "timeout") => Some((
            vec![(read, Type::Named(crate::Syntax::DURATION_TYPE.to_string()))],
            Some(unit),
        )),
        ("core.tasks", "interval") => Some((
            vec![(read, Type::Named("Duration".to_string()))],
            Some(Type::Apply {
                name: "Receiver".to_string(),
                args: vec![Type::Int],
            }),
        )),
        ("core.tasks", "yield_now") => Some((vec![], Some(unit))),
        ("core.tasks", "current_task") => Some((vec![], Some(string.clone()))),
        ("core.time", "start") => Some((vec![], Some(Type::Named("Stopwatch".to_string())))),
        ("core.time", "instant") => Some((vec![], Some(Type::Named("Instant".to_string())))),
        ("core.time", "now_utc") => Some((vec![], Some(Type::Named("DateTime".to_string())))),
        ("core.time", "from_unix_ms") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("DateTime".to_string())),
        )),
        ("core.time", "from_unix_seconds")
        | ("core.time", "from_unix_microseconds")
        | ("core.time", "from_unix_nanoseconds") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("DateTime".to_string())),
        )),
        ("core.time", "today") => Some((vec![], Some(Type::Named("LocalDate".to_string())))),
        ("core.time", "parse_rfc3339") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("DateTime".to_string()), Type::String)),
        )),
        ("core.time", "parse_iso_week_date") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("LocalDate".to_string()),
                Type::String,
            )),
        )),
        ("core.time", "from_iso_week") => Some((
            vec![(read, Type::Int), (read, Type::Int), (read, Type::Int)],
            Some(result_ty(
                Type::Named("LocalDate".to_string()),
                Type::String,
            )),
        )),
        ("core.time", "parse_zoned") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("ZonedDateTime".to_string()),
                Type::String,
            )),
        )),
        ("core.time", "datetime") => Some((
            vec![
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
            ],
            Some(Type::Named("DateTime".to_string())),
        )),
        ("core.time", "time") | ("core.time", "local_time") => Some((
            vec![(read, Type::Int), (read, Type::Int), (read, Type::Int)],
            Some(Type::Named("LocalTime".to_string())),
        )),
        ("core.time", "days_in_month") => {
            Some((vec![(read, Type::Int), (read, Type::Int)], Some(Type::Int)))
        }
        ("core.time", "is_leap_year") => Some((vec![(read, Type::Int)], Some(Type::Bool))),
        ("core.time", "parse_time") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("LocalTime".to_string()),
                Type::String,
            )),
        )),
        ("core.time", "period") => Some((
            vec![(read, Type::Int), (read, Type::Int), (read, Type::Int)],
            Some(Type::Named("Period".to_string())),
        )),
        ("core.time", "period_days" | "period_months" | "period_years") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("Period".to_string())),
        )),
        ("core.time", "zone") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Zone".to_string()), Type::String)),
        )),
        ("core.time", "utc") => Some((vec![], Some(Type::Named("Zone".to_string())))),
        ("core.time", "zoned") => Some((
            vec![
                (read, Type::Named("DateTime".to_string())),
                (read, Type::Named("Zone".to_string())),
            ],
            Some(Type::Named("ZonedDateTime".to_string())),
        )),
        ("core.time", "zoned_local") => Some((
            vec![
                (read, Type::Named("LocalDate".to_string())),
                (read, Type::Named("LocalTime".to_string())),
                (read, Type::Named("Zone".to_string())),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("ZonedDateTime".to_string()),
                Type::String,
            )),
        )),
        ("core.game", "run") => Some((
            vec![
                (read, Type::Named("GameScene".to_string())),
                (read, Type::Named("GameReplay".to_string())),
                (read, Type::Named("GameBackend".to_string())),
                (read, Type::Int),
            ],
            Some(Type::String),
        )),
        // D-ENC1 + D-JSONVERB1: unified encoding. `parse` → dynamic JSON value; `decode`
        // → lenient typed decode (D-JSON3); `to_string`/`to_string_pretty` → serialize.
        ("core.encoding.json", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), encoding_error_ty())),
        )),
        ("core.encoding.json", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), encoding_error_ty())),
        )),
        ("core.encoding.json", "to_string" | "to_string_pretty") => {
            Some((vec![(read, json)], Some(Type::String)))
        }
        // D-JSONCANON1=A: edition 2026 keeps the infallible prototype; 2027 is
        // fallible RFC 8785 JCS with optional EncodingLimits.
        ("core.encoding.json", "canonical") => {
            if super::super::Edition::edition_at_least("2027") {
                Some((
                    vec![
                        (read, json.clone()),
                        (read, Type::Named("EncodingLimits".to_string())),
                    ],
                    Some(result_ty(Type::String, encoding_error_ty())),
                ))
            } else {
                Some((vec![(read, json.clone())], Some(Type::String)))
            }
        }
        ("core.encoding.json", "events") => Some((vec![(read, json.clone())], Some(Type::String))),
        ("core.encoding.json", "reader") => Some((
            vec![
                (moved, Type::Named("FileReader".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
            ],
            Some(result_ty(
                Type::Named("JSONReader".to_string()),
                encoding_error_ty(),
            )),
        )),
        ("core.encoding.json", "writer") => Some((
            vec![
                (moved, Type::Named("FileWriter".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
                (read, Type::Bool),
            ],
            Some(result_ty(
                Type::Named("JSONWriter".to_string()),
                encoding_error_ty(),
            )),
        )),
        ("core.encoding.jsonl", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(json.clone())),
                encoding_error_ty(),
            )),
        )),
        ("core.encoding.jsonl", "to_string") => Some((
            vec![(read, Type::List(Box::new(json.clone())))],
            Some(Type::String),
        )),
        ("core.encoding.jsonl", "reader") => Some((
            vec![
                (moved, Type::Named("FileReader".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
            ],
            Some(result_ty(
                Type::Named("JSONLReader".to_string()),
                encoding_error_ty(),
            )),
        )),
        ("core.encoding.jsonl", "writer") => Some((
            vec![
                (moved, Type::Named("FileWriter".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
            ],
            Some(result_ty(
                Type::Named("JSONLWriter".to_string()),
                encoding_error_ty(),
            )),
        )),
        // jet.csv → core.encoding.csv: parse text into field rows. `rows`
        // exposes the same records with their physical opening line.
        ("core.encoding.csv", "parse") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Bool),
                (read, Type::Bool),
            ],
            Some(result_ty(
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
                Type::String,
            )),
        )),
        ("core.encoding.csv", "rows") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Bool),
                (read, Type::Bool),
            ],
            Some(result_ty(
                Type::List(Box::new(Type::Named("CSVRow".to_string()))),
                Type::String,
            )),
        )),
        ("core.encoding.csv", "to_string") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
            )],
            Some(Type::String),
        )),
        ("core.encoding.csv", "reader") => Some((
            vec![
                (moved, Type::Named("FileReader".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
                (read, Type::String),
                (read, Type::Bool),
                (read, Type::Bool),
            ],
            Some(result_ty(
                Type::Named("CSVReader".to_string()),
                encoding_error_ty(),
            )),
        )),
        ("core.encoding.csv", "writer") => Some((
            vec![
                (moved, Type::Named("FileWriter".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
            ],
            Some(result_ty(
                Type::Named("CSVWriter".to_string()),
                encoding_error_ty(),
            )),
        )),
        // D-DX-QUEUE1=A: `jobs.queue()` validates authority and opens the
        // provider, so the public factory preserves its Result.
        ("core.jobs", "queue") => Some((
            vec![],
            Some(result_ty(
                Type::Named("JobQueue".to_string()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.data.loader", "snapshot_reusable") => Some((
            vec![
                (read, Type::Named("DataSnapshotIdentity".to_string())),
                (read, Type::Named("DataSnapshotIdentity".to_string())),
            ],
            Some(Type::Bool),
        )),
        // D-DATA-SURFACE1=A / D-DATA-PLOT1=A / D-DATA-STATUS1=A: core.data
        // facade fixed-shape calls. Generic typed-list calls are handled in
        // infer_core_call so selectors stay typed by sema.
        ("core.data", "sum" | "mean" | "min" | "max" | "median" | "variance" | "stddev") => {
            let args = vec![(read, Type::List(Box::new(Type::Float)))];
            Some((
                args,
                Some(result_ty(Type::Float, Type::Named("DataError".to_string()))),
            ))
        }
        // D-COMPUTE1=D / D-COMPUTE-TYPE1=D (#443): Tensor storage and aliases.
        ("core.compute", "zeros" | "ones") => Some((
            vec![(read, Type::List(Box::new(Type::Int)))],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "full") => Some((
            vec![(read, Type::List(Box::new(Type::Int))), (read, Type::Float)],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "from_list") => Some((
            vec![(read, Type::List(Box::new(Type::Float)))],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "matrix") => Some((
            vec![(read, Type::Int), (read, Type::Int), (read, Type::Float)],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "vec") => Some((
            vec![(read, Type::Int), (read, Type::Float)],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "add" | "mul" | "matmul") => Some((
            vec![
                (read, Type::Named("Tensor".to_string())),
                (read, Type::Named("Tensor".to_string())),
            ],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "reshape") => Some((
            vec![
                (read, Type::Named("Tensor".to_string())),
                (read, Type::List(Box::new(Type::Int))),
            ],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "get") => Some((
            vec![
                (read, Type::Named("Tensor".to_string())),
                (read, Type::List(Box::new(Type::Int))),
            ],
            Some(result_ty(
                Type::Float,
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "set") => Some((
            vec![
                (AccessConvention::Write, Type::Named("Tensor".to_string())),
                (read, Type::List(Box::new(Type::Int))),
                (read, Type::Float),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "shape") => Some((
            vec![(read, Type::Named("Tensor".to_string()))],
            Some(Type::List(Box::new(Type::Int))),
        )),
        ("core.compute", "to_list") => Some((
            vec![(read, Type::Named("Tensor".to_string()))],
            Some(Type::List(Box::new(Type::Float))),
        )),
        ("core.compute", "rank" | "numel") => Some((
            vec![(read, Type::Named("Tensor".to_string()))],
            Some(Type::Int),
        )),
        ("core.compute", "device" | "placement") => Some((
            vec![(read, Type::Named("Tensor".to_string()))],
            Some(Type::String),
        )),
        (
            "core.compute",
            "device_cpu" | "device_auto" | "device_metal" | "device_cuda" | "device_vulkan"
            | "device_webgpu",
        ) => Some((vec![], Some(Type::Named("ComputeDevice".to_string())))),
        ("core.compute", "on_device") => Some((
            vec![
                (read, Type::Named("Tensor".to_string())),
                (read, Type::Named("ComputeDevice".to_string())),
            ],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        // #1136 ndarray ops — same Tensor substrate (D-COMPUTE-TYPE1).
        ("core.compute", "broadcast_to") => Some((
            vec![
                (read, Type::Named("Tensor".to_string())),
                (read, Type::List(Box::new(Type::Int))),
            ],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "transpose" | "negate" | "abs" | "exp" | "log" | "sqrt") => Some((
            vec![(read, Type::Named("Tensor".to_string()))],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "sum_axis") => Some((
            vec![(read, Type::Named("Tensor".to_string())), (read, Type::Int)],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "sub" | "div" | "maximum" | "minimum") => Some((
            vec![
                (read, Type::Named("Tensor".to_string())),
                (read, Type::Named("Tensor".to_string())),
            ],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "eye") => Some((
            vec![(read, Type::Int)],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "det") => Some((
            vec![(read, Type::Named("Tensor".to_string()))],
            Some(result_ty(
                Type::Float,
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "inv" | "fft") => Some((
            vec![(read, Type::Named("Tensor".to_string()))],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "solve") => Some((
            vec![
                (read, Type::Named("Tensor".to_string())),
                (read, Type::Named("Tensor".to_string())),
            ],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "stream_new") => {
            Some((vec![], Some(Type::Named("ComputeStream".to_string()))))
        }
        ("core.compute", "stream_new_on") => Some((
            vec![(read, Type::Named("ComputeDevice".to_string()))],
            Some(result_ty(
                Type::Named("ComputeStream".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "stream_sync") => Some((
            vec![(read, Type::Named("ComputeStream".to_string()))],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "stream_show") => Some((
            vec![(read, Type::Named("ComputeStream".to_string()))],
            Some(Type::String),
        )),
        ("core.compute", "transfer_show") => Some((
            vec![(read, Type::Named("Tensor".to_string()))],
            Some(Type::String),
        )),
        ("core.compute", "transfer") => Some((
            vec![
                (read, Type::Named("Tensor".to_string())),
                (read, Type::Named("ComputeDevice".to_string())),
            ],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "kernel_bounds_ok") => Some((
            vec![
                (read, Type::List(Box::new(Type::Int))),
                (read, Type::List(Box::new(Type::Int))),
            ],
            Some(result_ty(
                Type::Bool,
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "sparse_show") => Some((
            vec![(read, Type::Named("SparseTensor".to_string()))],
            Some(Type::String),
        )),
        ("core.compute", "serialize") => Some((
            vec![(read, Type::Named("Tensor".to_string()))],
            Some(result_ty(
                Type::String,
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "mse_loss") => Some((
            vec![
                (read, Type::Named("Tensor".to_string())),
                (read, Type::Named("Tensor".to_string())),
            ],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "sgd_step") => Some((
            vec![
                (read, Type::Named("Tensor".to_string())),
                (read, Type::Named("Tensor".to_string())),
                (read, Type::Float),
            ],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "deserialize") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "to_sparse") => Some((
            vec![(read, Type::Named("Tensor".to_string()))],
            Some(result_ty(
                Type::Named("SparseTensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "sparse_nnz") => Some((
            vec![(read, Type::Named("SparseTensor".to_string()))],
            Some(Type::Int),
        )),
        ("core.compute", "sparse_mv") => Some((
            vec![
                (read, Type::Named("SparseTensor".to_string())),
                (read, Type::Named("Tensor".to_string())),
            ],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "matmul_f32_tile") => Some((
            vec![
                (read, Type::Named("Tensor".to_string())),
                (read, Type::Named("Tensor".to_string())),
            ],
            Some(result_ty(
                Type::Named("Tensor".to_string()),
                Type::Named("ComputeError".to_string()),
            )),
        )),
        ("core.compute", "profile_f32_strict" | "profile_show") => {
            Some((vec![], Some(Type::String)))
        }
        // D-SERVICE1=D (#444): the typed tree value is the public topology
        // surface; worker declarations are methods on that value.
        ("core.service", "tree") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("ServiceTree".to_string())),
        )),
        ("core.service", "tree_show") => Some((
            vec![(read, Type::Named("ServiceTree".to_string()))],
            Some(Type::String),
        )),
        // The rows below are retained as private Prelude contracts while the
        // typed builder migrates the rest of the service tree. They are not in
        // `core_module_items`, so no old string-keyed surface is user-visible.
        ("core.service", "runtime") => Some((
            vec![
                (read, Type::String),
                (read, Type::Named("Duration".to_string())),
            ],
            Some(Type::Named("ServiceRuntime".to_string())),
        )),
        ("core.service", "state_store") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("ServiceStateStore".to_string()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        (
            "core.service",
            "restart_one_for_one" | "restart_one_for_all" | "restart_rest_for_one",
        ) => Some((vec![], Some(Type::Named("ServiceRestart".to_string())))),
        ("core.service", "delivery_at_most_once" | "delivery_durable") => {
            Some((vec![], Some(Type::Named("ServiceDelivery".to_string()))))
        }
        ("core.service", "set_restart") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::Named("ServiceRestart".to_string())),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "set_delivery") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::Named("ServiceDelivery".to_string())),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "worker") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::String),
                (
                    read,
                    Type::Fn {
                        params: Vec::new(),
                        ret: None,
                        effect_bound: None,
                        param_contract: None,
                        call_metadata: None,
                        return_view_provenance: None,
                    },
                ),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("ServiceEndpoint".to_string()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "group") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::String),
                (read, Type::List(Box::new(Type::String))),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "start" | "stop") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("ServiceTree".to_string()),
            )],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "send") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::Named("ServiceEndpoint".to_string())),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "send_durable") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::Named("ServiceEndpoint".to_string())),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("Delivery".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "receive") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::Named("ServiceEndpoint".to_string())),
            ],
            Some(result_ty(
                Type::String,
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "mailbox_depth" | "restarts") => Some((
            vec![
                (read, Type::Named("ServiceTree".to_string())),
                (read, Type::Named("ServiceEndpoint".to_string())),
            ],
            Some(result_ty(
                Type::Int,
                Type::Named("ServiceError".to_string()),
            )),
        )),
        (
            "core.service",
            "fail_worker" | "drain_worker" | "partition_worker" | "reconcile_worker",
        ) => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::Named("ServiceEndpoint".to_string())),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "dead_letter_count" | "event_count" | "directory_generation") => Some((
            vec![(read, Type::Named("ServiceTree".to_string()))],
            Some(Type::Int),
        )),
        ("core.service", "drain_dead_letters") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("ServiceTree".to_string()),
            )],
            Some(result_ty(
                Type::Int,
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "set_state_empty") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("ServiceTree".to_string()),
            )],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "set_state_snapshot" | "set_state_event_log") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::Named("ServiceStateStore".to_string())),
                (read, Type::String),
                (read, Type::Int),
                // The migration policy the adapter is opened under. It decides
                // whether a later rollback is possible, so it is written at the
                // call rather than defaulted out of sight.
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "commit_snapshot" | "append_event") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "restore_snapshot") => Some((
            vec![(read, Type::Named("ServiceTree".to_string()))],
            Some(result_ty(
                Type::String,
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "replay_events" | "observe") => Some((
            vec![(read, Type::Named("ServiceTree".to_string()))],
            Some(Type::String),
        )),
        ("core.service", "workflow_start") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("ServiceWorkflow".to_string()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "workflow_sleep") => Some((
            vec![
                (read, Type::Named("ServiceWorkflow".to_string())),
                (read, Type::Named("Duration".to_string())),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "workflow_activity_wait") => Some((
            vec![
                (read, Type::Named("ServiceWorkflow".to_string())),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::String,
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "workflow_all") => Some((
            vec![
                (read, Type::Named("ServiceWorkflow".to_string())),
                (read, Type::List(Box::new(Type::String))),
            ],
            Some(result_ty(
                Type::List(Box::new(Type::String)),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "workflow_step") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::Int),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "workflow_activity") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::Int),
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("TaskStatus".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "workflow_activity_retry") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::Int),
                (read, Type::String),
                (read, Type::Named("TaskOutcome".to_string())),
            ],
            Some(result_ty(
                Type::Named("TaskStatus".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "workflow_activity_complete") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::Int),
                (read, Type::String),
                (read, Type::Named("TaskOutcome".to_string())),
            ],
            Some(result_ty(
                Type::Named("TaskOutcome".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "workflow_history") => Some((
            vec![
                (read, Type::Named("ServiceTree".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::String,
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "workflow_outcome") => Some((
            vec![
                (read, Type::Named("ServiceTree".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("TaskOutcome".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "directory_register") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("ServiceTree".to_string()),
                ),
                (read, Type::String),
                (read, Type::Named("ServiceEndpoint".to_string())),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "directory_resolve") => Some((
            vec![
                (read, Type::Named("ServiceTree".to_string())),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("ServiceEndpoint".to_string()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "handoff_generation" | "rollback_generation" | "chaos_fail") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("ServiceTree".to_string()),
            )],
            Some(result_ty(
                Type::Int,
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "upgrade_receipt") => Some((
            vec![(read, Type::Named("ServiceTree".to_string()))],
            Some(result_ty(
                Type::Named("ServiceUpgradeReceipt".to_string()),
                Type::Named("ServiceError".to_string()),
            )),
        )),
        ("core.service", "endpoint_show") => Some((
            vec![(read, Type::Named("ServiceEndpoint".to_string()))],
            Some(Type::String),
        )),
        ("core.service", "delivery_state_show") => Some((
            vec![(read, Type::Named("DeliveryState".to_string()))],
            Some(Type::String),
        )),
        ("core.data", "quantile") => {
            let args = vec![
                (read, Type::List(Box::new(Type::Float))),
                (read, Type::Float),
            ];
            Some((
                args,
                Some(result_ty(Type::Float, Type::Named("DataError".to_string()))),
            ))
        }
        ("core.data", "rolling_mean") => {
            let args = vec![(read, Type::List(Box::new(Type::Float))), (read, Type::Int)];
            Some((
                args,
                Some(result_ty(
                    Type::List(Box::new(Type::Float)),
                    Type::Named("DataError".to_string()),
                )),
            ))
        }
        ("core.data", "describe") => {
            let args = vec![(read, Type::List(Box::new(Type::Float)))];
            Some((
                args,
                Some(result_ty(
                    Type::Named("DataSummary".to_string()),
                    Type::Named("DataError".to_string()),
                )),
            ))
        }
        ("core.data", "status") => Some((
            vec![],
            Some(Type::List(Box::new(Type::Named("DataStatus".to_string())))),
        )),
        ("core.data", "require_bridge") => Some((
            vec![(read, Type::String)],
            Some(result_ty(unit_ty(), Type::Named("DataError".to_string()))),
        )),
        // D-QUERY-RETAIN1=A: plot reducers consume ordinary `[Group<K, V>]`
        // lists. The fixed surface uses string keys; the runtime adapter
        // accepts and preserves the nominal key value.
        ("core.data", "bar_text" | "bar_svg") => {
            let args = vec![(
                read,
                Type::List(Box::new(Type::Apply {
                    name: "Group".to_string(),
                    args: vec![Type::String, Type::Int],
                })),
            )];
            Some((
                args,
                Some(result_ty(
                    Type::String,
                    Type::Named("DataError".to_string()),
                )),
            ))
        }
        ("core.data", "line_text" | "line_svg") => {
            let args = vec![
                (
                    read,
                    Type::List(Box::new(Type::Apply {
                        name: "Group".to_string(),
                        args: vec![Type::String, Type::Float],
                    })),
                ),
                (read, Type::Named("DataLineOptions".to_string())),
            ];
            Some((
                args,
                Some(result_ty(
                    Type::String,
                    Type::Named("DataError".to_string()),
                )),
            ))
        }
        ("core.text.fmt", "number" | "bytes" | "duration" | "ordinal") => {
            Some((vec![(read, Type::Int)], Some(Type::String)))
        }
        ("core.text.fmt", "decimal" | "grouped" | "percent" | "sci") => Some((
            vec![(read, Type::Float), (read, Type::Int)],
            Some(Type::String),
        )),
        ("core.text.fmt", "hex") => Some((
            vec![(read, Type::Int), (read, Type::Int)],
            Some(Type::String),
        )),
        ("core.text.fmt", "bin" | "oct") => Some((vec![(read, Type::Int)], Some(Type::String))),
        ("core.text.fmt", "plural") => Some((
            vec![
                (read, Type::Int),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(Type::String),
        )),
        ("core.text.fmt", "pad_left" | "pad_right" | "pad_center") => Some((
            vec![
                (read, Type::String),
                (read, Type::Int),
                (read, Type::String),
            ],
            Some(Type::String),
        )),
        ("core.text.fmt", "pad") => Some((
            vec![
                (read, Type::String),
                (read, Type::Int),
                (read, Type::String),
            ],
            Some(Type::String),
        )),
        // D-ENC-DYN1=A+ (c152): TOML is a full adapter over the rich `Data` value —
        // `parse` returns `TOML` (= `Data`); `to_string` takes any encodable value.
        ("core.encoding.toml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), encoding_error_ty())),
        )),
        ("core.encoding.toml", "to_string") => {
            Some((vec![(read, json.clone())], Some(Type::String)))
        }
        // D-ENC-YAML1 = A (c152): YAML is a full adapter over the rich `Data` value.
        ("core.encoding.yaml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), encoding_error_ty())),
        )),
        ("core.encoding.yaml", "to_string") => {
            Some((vec![(read, json.clone())], Some(Type::String)))
        }
        ("core.encoding.xml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), Type::Named("XMLError".to_string()))),
        )),
        ("core.encoding.xml", "parse_with") => Some((
            vec![
                (read, Type::String),
                (read, Type::Named("XMLParseOptions".to_string())),
            ],
            Some(result_ty(json.clone(), Type::Named("XMLError".to_string()))),
        )),
        ("core.encoding.xml", "parse_bytes") => Some((
            vec![
                (read, list_u8.clone()),
                (read, Type::Named("XMLParseOptions".to_string())),
            ],
            Some(result_ty(json.clone(), Type::Named("XMLError".to_string()))),
        )),
        ("core.encoding.xml", "to_string") => {
            Some((vec![(read, json.clone())], Some(Type::String)))
        }
        ("core.encoding.xml", "to_bytes") => Some((
            vec![
                (read, json.clone()),
                (read, Type::Named("XMLRenderOptions".to_string())),
            ],
            Some(result_ty(list_u8, Type::Named("XMLError".to_string()))),
        )),
        ("core.encoding.xml", "canonical") => Some((
            vec![
                (read, json.clone()),
                (read, Type::Named("XMLCanonical".to_string())),
            ],
            Some(result_ty(Type::String, Type::Named("XMLError".to_string()))),
        )),
        // D-ENCXML-PROJECTION1=A: focused helpers. `decode`/`decode_bytes`/`expanded_name`
        // return types come from core_call (turbofish / named tuple).
        ("core.encoding.xml", "root") => Some((
            vec![(read, json.clone())],
            Some(result_ty(json.clone(), Type::Named("XMLError".to_string()))),
        )),
        ("core.encoding.xml", "attribute") => Some((
            vec![(read, json.clone()), (read, Type::String)],
            Some(result_ty(
                Type::Option(Box::new(Type::String)),
                Type::Named("XMLError".to_string()),
            )),
        )),
        ("core.encoding.xml", "content") => Some((
            vec![(read, json.clone())],
            Some(result_ty(
                Type::List(Box::new(json.clone())),
                Type::Named("XMLError".to_string()),
            )),
        )),
        ("core.encoding.xml", "reader") => Some((
            vec![
                (moved, Type::Named("FileReader".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
                (read, Type::Named("XMLParseOptions".to_string())),
            ],
            Some(result_ty(
                Type::Named("XMLReader".to_string()),
                encoding_error_ty(),
            )),
        )),
        ("core.encoding.xml", "writer") => Some((
            vec![
                (moved, Type::Named("FileWriter".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
                (read, Type::Named("XMLRenderOptions".to_string())),
            ],
            Some(result_ty(
                Type::Named("XMLWriter".to_string()),
                encoding_error_ty(),
            )),
        )),
        ("core.encoding.cbor", "reader") => Some((
            vec![
                (moved, Type::Named("FileReader".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
            ],
            Some(result_ty(
                Type::Named("CBORReader".to_string()),
                encoding_error_ty(),
            )),
        )),
        ("core.encoding.cbor", "writer") => Some((
            vec![
                (moved, Type::Named("FileWriter".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
            ],
            Some(result_ty(
                Type::Named("CBORWriter".to_string()),
                encoding_error_ty(),
            )),
        )),
        // E2-M7: streaming file handles (D-IO2).
        ("core.files", "open") => Some((
            vec![(read, path.clone())],
            Some(result_ty(Type::Named("FileReader".to_string()), io.clone())),
        )),
        ("core.files", "append" | "create") => Some((
            vec![(read, path)],
            Some(result_ty(Type::Named("FileWriter".to_string()), io.clone())),
        )),
        // D-URL1=A: typed URLs, query strings, component escaping, and MIME values.
        ("core.net.url", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Url".to_string()), Type::String)),
        )),
        ("core.net.url", "from_parts") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::String),
                (
                    read,
                    Type::List(Box::new(Type::List(Box::new(Type::String)))),
                ),
                (read, Type::String),
            ],
            Some(result_ty(Type::Named("Url".to_string()), Type::String)),
        )),
        ("core.net.url", "file") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("Url".to_string())),
        )),
        ("core.net.url", "data") => Some((
            vec![
                (read, Type::Named("Mime".to_string())),
                (read, Type::String),
            ],
            Some(Type::Named("Url".to_string())),
        )),
        ("core.net.url", "query") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
            )],
            Some(Type::String),
        )),
        ("core.net.url", "percent_encode") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.net.url", "percent_decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.net.mime", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Mime".to_string()), Type::String)),
        )),
        ("core.net.mime", "from_extension" | "extension") => Some((
            vec![(read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        // D-EMAIL1=A: one bounded native message/MIME construction path.
        ("core.email", "address") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("Address".to_string()),
                Type::Named("EmailError".to_string()),
            )),
        )),
        ("core.email", "attachment") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, list_u8.clone()),
            ],
            Some(result_ty(
                Type::Named("Attachment".to_string()),
                Type::Named("EmailError".to_string()),
            )),
        )),
        ("core.email", "message") => Some((
            vec![
                (read, Type::Named("Address".to_string())),
                (
                    read,
                    Type::List(Box::new(Type::Named("Address".to_string()))),
                ),
                (
                    read,
                    Type::List(Box::new(Type::Named("Address".to_string()))),
                ),
                (read, Type::String),
                (read, Type::String),
                (read, Type::Named("HTML".to_string())),
                (
                    read,
                    Type::List(Box::new(Type::Named("Attachment".to_string()))),
                ),
            ],
            Some(result_ty(
                Type::Named("Message".to_string()),
                Type::Named("EmailError".to_string()),
            )),
        )),
        ("core.email", "envelope") => Some((
            vec![
                (read, Type::Named("Address".to_string())),
                (
                    read,
                    Type::List(Box::new(Type::Named("Address".to_string()))),
                ),
            ],
            Some(result_ty(
                Type::Named("Envelope".to_string()),
                Type::Named("EmailError".to_string()),
            )),
        )),
        ("core.email", "serialize") => Some((
            vec![(read, Type::Named("Message".to_string()))],
            Some(result_ty(list_u8, Type::Named("EmailError".to_string()))),
        )),
        ("core.email", "smtp") => Some((
            vec![(
                AccessConvention::Read,
                Type::Named("SMTPConfig".to_string()),
            )],
            Some(result_ty(
                Type::Named("Mailer".to_string()),
                Type::Named("EmailError".to_string()),
            )),
        )),
        ("core.email", "smtp_from_env") => Some((
            vec![],
            Some(result_ty(
                Type::Named("Mailer".to_string()),
                Type::Named("EmailError".to_string()),
            )),
        )),
        // D-TEXTUNICODE1: std-only Unicode scalar helpers.
        ("core.text", "scalar_count" | "byte_count") => {
            Some((vec![(read, Type::String)], Some(Type::Int)))
        }
        ("core.text", "is_ascii") => Some((vec![(read, Type::String)], Some(Type::Bool))),
        ("core.text", "lower" | "upper") => Some((vec![(read, Type::String)], Some(Type::String))),
        ("core.text", "scalars") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.text", "nfc" | "nfd" | "nfkc" | "nfkd" | "casefold") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.text", "graphemes" | "words" | "sentences" | "inspect") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.text", "grapheme_views" | "word_views" | "line_views") => Some((
            vec![(read, Type::String)],
            Some(crate::Collections::view_iter_ty(Type::Apply {
                name: "View".to_string(),
                args: vec![Type::Named("str".to_string())],
            })),
        )),
        ("core.text", "byte_views") => Some((
            vec![(read, Type::String)],
            Some(crate::Collections::view_iter_ty(Type::Apply {
                name: "View".to_string(),
                args: vec![Type::List(Box::new(u8_ty()))],
            })),
        )),
        ("core.text", "is_alphabetic" | "is_numeric" | "is_whitespace") => {
            Some((vec![(read, Type::String)], Some(Type::Bool)))
        }
        ("core.text", "splitn" | "rsplitn") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.text", "trim" | "trim_start" | "trim_end") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.text", "pad_start" | "pad_end" | "center") => Some((
            vec![
                (read, Type::String),
                (read, Type::Int),
                (read, Type::String),
            ],
            Some(Type::String),
        )),
        ("core.text", "starts_any" | "ends_any") => Some((
            vec![
                (read, Type::String),
                (read, Type::List(Box::new(Type::String))),
            ],
            Some(Type::Bool),
        )),
        ("core.text", "char_indices") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        // core.log: structured logging, typed fields, spans, sinks.
        ("core.log", "info" | "warn" | "error" | "debug" | "critical" | "fatal") => {
            Some((vec![(read, string.clone())], None))
        }
        ("core.log", "field") => Some((
            vec![(read, string.clone()), (read, string.clone())],
            Some(Type::Named("LogField".to_string())),
        )),
        ("core.log", "int") => Some((
            vec![(read, string.clone()), (read, Type::Int)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("core.log", "float") => Some((
            vec![(read, string.clone()), (read, Type::Float)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("core.log", "bool") => Some((
            vec![(read, string.clone()), (read, Type::Bool)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("core.log", "redact") => Some((
            vec![(read, string.clone())],
            Some(Type::Named("LogField".to_string())),
        )),
        ("core.log", "info_fields" | "warn_fields" | "error_fields" | "debug_fields") => Some((
            vec![
                (read, string.clone()),
                (
                    read,
                    Type::List(Box::new(Type::Named("LogField".to_string()))),
                ),
            ],
            None,
        )),
        ("core.log", "span") => Some((
            vec![(read, string.clone())],
            Some(Type::Named("LogSpan".to_string())),
        )),
        ("core.log", "enter" | "close") => {
            Some((vec![(read, Type::Named("LogSpan".to_string()))], None))
        }
        ("core.log", "set_sink") => {
            Some((vec![(read, string.clone()), (read, string.clone())], None))
        }
        ("core.log", "sample_every") => Some((vec![(read, Type::Int)], None)),
        ("core.log", "counter") => Some((
            vec![(read, string.clone()), (read, Type::Int)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("core.log", "otlp_file") => Some((vec![(read, string.clone())], None)),
        ("core.log", "set_level") => Some((vec![(read, Type::String)], None)),
        ("core.log", "disable" | "flush") => Some((vec![], None)),
        ("core.log", "enabled") => Some((vec![(read, Type::String)], Some(bool_))),
        // D-OBS3: set OTel trace_id for all subsequent log entries on this thread.
        ("core.log", "set_trace_id") => Some((vec![(read, Type::String)], None)),
        // D-LOGFMT1=A: override log output format ("json" | "text").
        ("core.log", "setup") => Some((vec![(read, Type::String)], None)),
        // core.crypto: vetted hash functions (D-LR3).
        ("core.crypto", "sha256") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::Named("Digest256".into())),
        )),
        (
            "core.crypto",
            "sha1" | "sha224" | "sha384" | "sha3_224" | "sha3_256" | "sha3_384" | "sha3_512",
        ) => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::String),
        )),
        ("core.crypto", "hmac_sha256") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.crypto", "pbkdf2_hmac") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::Int),
                (read, Type::Int),
            ],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.crypto", "constant_time_equal_bytes") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(Type::Bool),
        )),
        ("core.crypto", "hkdf_sha256") => Some((
            vec![
                (read, Type::Named("Secret".into())),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("Secret".into()),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto", "x25519_public") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("core.crypto", "x25519_shared") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("core.crypto", "password_hash") => Some((
            vec![(read, Type::Named("Secret".into()))],
            Some(result_ty(
                Type::Named("PasswordHash".into()),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto", "password_hash_with_salt") => Some((
            vec![(read, Type::String), (read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.crypto", "password_verify") => Some((
            vec![
                (read, Type::Named("Secret".into())),
                (read, Type::Named("PasswordHash".into())),
            ],
            Some(result_ty(Type::Bool, Type::Named("CryptoError".into()))),
        )),
        // D-CRYPTOENV1=A: misuse-resistant envelope (RustCrypto via FFI bridge).
        ("core.crypto", "seal") => Some((
            vec![
                (
                    read,
                    Type::List(Box::new(Type::Named("X25519PublicKey".into()))),
                ),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(
                Type::Named("Sealed".into()),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto", "open") => Some((
            vec![
                (read, Type::Named("X25519SecretKey".into())),
                (read, Type::Named("Sealed".into())),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(
                Type::List(Box::new(u8_ty())),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto", "file_seal") => Some((
            vec![
                (
                    read,
                    Type::List(Box::new(Type::Named("X25519PublicKey".into()))),
                ),
                (read, Type::Named("Path".into())),
                (read, Type::Named("Path".into())),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("FileCryptoError".into()),
            )),
        )),
        ("core.crypto", "file_open") => Some((
            vec![
                (read, Type::Named("X25519SecretKey".into())),
                (read, Type::Named("Path".into())),
                (read, Type::Named("Path".into())),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("FileCryptoError".into()),
            )),
        )),
        ("core.crypto", "sign") => Some((
            vec![
                (read, Type::Named("SigningKey".into())),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(
                Type::Named("Signature".into()),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto", "verify") => Some((
            vec![
                (read, Type::Named("VerifyKey".into())),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::Named("Signature".into())),
            ],
            Some(result_ty(Type::Bool, Type::Named("CryptoError".into()))),
        )),
        // D-CRYPTO-API1=A typed safe surface. Existing edition-2026 raw calls
        // are diagnosed/migrated separately; these signatures are nominal.
        ("core.crypto", "wrap") => Some((
            vec![
                (read, Type::Named("Secret".into())),
                (read, Type::Named("X25519PublicKey".into())),
            ],
            Some(result_ty(
                Type::Named("WrappedKey".into()),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto", "unwrap") => Some((
            vec![
                (read, Type::Named("X25519SecretKey".into())),
                (read, Type::Named("WrappedKey".into())),
            ],
            Some(result_ty(
                Type::Named("Secret".into()),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto", "x25519") => Some((
            vec![
                (read, Type::Named("X25519SecretKey".into())),
                (read, Type::Named("X25519PublicKey".into())),
            ],
            Some(result_ty(
                Type::Named("SharedSecret".into()),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto", "constant_time_equal") => Some((
            vec![
                (read, Type::Named("Secret".into())),
                (read, Type::Named("Secret".into())),
            ],
            Some(Type::Bool),
        )),
        ("core.crypto", "blake3") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::Named("Digest256".into())),
        )),
        ("core.crypto", "sha512") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::Named("Digest512".into())),
        )),
        // D-CRYPTO-API1=A: exact #Unsafe expert API. Bounds remain runtime
        // checked; lexical gating never waives memory safety or cleanup.
        ("core.crypto.expert", "xchacha20poly1305_seal" | "aes256gcm_seal") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(
                Type::List(Box::new(u8_ty())),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto.expert", "xchacha20poly1305_open" | "aes256gcm_open") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(
                Type::List(Box::new(u8_ty())),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto.expert", "open_v1") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(
                Type::List(Box::new(u8_ty())),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto.expert", "migrate_v1") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::Named("Path".into())),
                (
                    read,
                    Type::List(Box::new(Type::Named("X25519PublicKey".into()))),
                ),
                (read, Type::Named("Path".into())),
            ],
            Some(result_ty(
                Type::Named("Unit".into()),
                Type::Named("FileCryptoError".into()),
            )),
        )),
        ("core.crypto.expert", "ed25519_sign") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(
                Type::Named("Signature".into()),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto.expert", "ed25519_verify_strict") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::Bool, Type::Named("CryptoError".into()))),
        )),
        ("core.crypto.expert", "x25519_raw") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(
                Type::Named("Secret".into()),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto.expert", "hkdf_sha256_raw") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("Secret".into()),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto.expert", "argon2id") => Some((
            vec![
                (read, Type::Named("Secret".into())),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("Secret".into()),
                Type::Named("CryptoError".into()),
            )),
        )),
        ("core.crypto.expert", "secret_bytes") => Some((
            vec![(read, Type::Named("Secret".into()))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.crypto.expert", "signing_key_bytes") => Some((
            vec![(read, Type::Named("SigningKey".into()))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.crypto.expert", "x25519_secret_bytes") => Some((
            vec![(read, Type::Named("X25519SecretKey".into()))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.crypto.expert", "shared_secret_bytes") => Some((
            vec![(read, Type::Named("SharedSecret".into()))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.crypto.vault", "prepare_import_signing") => Some((
            vec![(read, Type::String), (moved, Type::List(Box::new(u8_ty())))],
            Some(result_ty(
                Type::Apply {
                    name: "MutationPlan".into(),
                    args: vec![Type::Named("SigningKey".into())],
                },
                Type::Named("VaultError".into()),
            )),
        )),
        ("core.crypto.vault", "prepare_import_x25519") => Some((
            vec![(read, Type::String), (moved, Type::List(Box::new(u8_ty())))],
            Some(result_ty(
                Type::Apply {
                    name: "MutationPlan".into(),
                    args: vec![Type::Named("X25519SecretKey".into())],
                },
                Type::Named("VaultError".into()),
            )),
        )),
        ("core.crypto.vault", "commit_import_signing") => Some((
            vec![
                (
                    moved,
                    Type::Apply {
                        name: "VaultWrite".into(),
                        args: vec![Type::Named("SigningKey".into())],
                    },
                ),
                (
                    moved,
                    Type::Apply {
                        name: "MutationPlan".into(),
                        args: vec![Type::Named("SigningKey".into())],
                    },
                ),
            ],
            Some(result_ty(
                Type::Apply {
                    name: "KeyRef".into(),
                    args: vec![Type::Named("SigningKey".into())],
                },
                Type::Named("VaultError".into()),
            )),
        )),
        ("core.crypto.vault", "commit_import_x25519") => Some((
            vec![
                (
                    moved,
                    Type::Apply {
                        name: "VaultWrite".into(),
                        args: vec![Type::Named("X25519SecretKey".into())],
                    },
                ),
                (
                    moved,
                    Type::Apply {
                        name: "MutationPlan".into(),
                        args: vec![Type::Named("X25519SecretKey".into())],
                    },
                ),
            ],
            Some(result_ty(
                Type::Apply {
                    name: "KeyRef".into(),
                    args: vec![Type::Named("X25519SecretKey".into())],
                },
                Type::Named("VaultError".into()),
            )),
        )),
        // E2-M10: core.net — blocking TCP/UDP sockets (std::net, zero external deps).
        ("core.net", "tcp_listen") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("TcpListener".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "ip_addr") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("IPAddr".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "ip_to_string") => Some((
            vec![(read, Type::Named("IPAddr".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "ip_is_ipv4") => Some((
            vec![(read, Type::Named("IPAddr".to_string()))],
            Some(Type::Bool),
        )),
        ("core.net", "socket_addr") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "socket_addr_parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "socket_host" | "socket_to_string") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "socket_port") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(Type::Int),
        )),
        ("core.net", "tcp_listen_addr") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(result_ty(
                Type::Named("TcpListener".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_accept") => Some((
            vec![(
                AccessConvention::Read,
                Type::Named("TcpListener".to_string()),
            )],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_connect") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_connect_addr") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_connect_timeout") => Some((
            vec![
                (read, Type::Named("SocketAddr".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_connect_happy") => Some((
            vec![(read, Type::String), (read, Type::Int), (read, Type::Int)],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_read") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("TcpStream".to_string()),
            )],
            Some(result_ty(Type::String, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_write") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_read_bytes") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(u8_ty())),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_read_text") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Int),
            ],
            Some(result_ty(Type::String, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_write_bytes") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::Int, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_write_all_bytes") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_write_text") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_shutdown") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Named("NetShutdown".to_string())),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_close") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("TcpStream".to_string()),
            )],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_ready") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Named("NetReadyInterest".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("NetReady".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "ready_readable" | "ready_writable") => Some((
            vec![(read, Type::Named("NetReady".to_string()))],
            Some(Type::Bool),
        )),
        ("core.net", "error_operation" | "error_message") => Some((
            vec![(read, Type::Named("NetError".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "error_address" | "error_name") => Some((
            vec![(read, Type::Named("NetError".to_string()))],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.net", "error_os_code") => Some((
            vec![(read, Type::Named("NetError".to_string()))],
            Some(Type::Option(Box::new(Type::Int))),
        )),
        ("core.net", "tcp_local_addr" | "tcp_peer_addr") => Some((
            vec![(read, Type::Named("TcpStream".to_string()))],
            Some(result_ty(Type::String, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_local_socket_addr" | "tcp_peer_socket_addr") => Some((
            vec![(read, Type::Named("TcpStream".to_string()))],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "listener_local_socket_addr") => Some((
            vec![(read, Type::Named("TcpListener".to_string()))],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "set_timeout") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Int),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "set_read_timeout" | "set_write_timeout") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Int),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "nodelay") => Some((
            vec![(read, Type::Named("TcpStream".to_string()))],
            Some(result_ty(Type::Bool, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "set_nodelay") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Bool),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "ttl") => Some((
            vec![(read, Type::Named("TcpStream".to_string()))],
            Some(result_ty(Type::Int, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "set_ttl") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Int),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "socket_type") => Some((
            vec![(read, Type::Named("TcpStream".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "sendfile") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(Type::Int, Type::Named("NetError".to_string()))),
        )),
        // Convenience: send a complete HTTP/1.1 response and close the stream.
        ("core.net", "tcp_reply") => Some((
            vec![
                (AccessConvention::Move, Type::Named("TcpStream".to_string())),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "udp_bind") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("UdpSocket".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "udp_bind_addr") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(result_ty(
                Type::Named("UdpSocket".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "udp_local_addr") => Some((
            vec![(read, Type::Named("UdpSocket".to_string()))],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "udp_set_timeout") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "udp_send_to") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::String),
                (read, Type::Named("SocketAddr".to_string())),
            ],
            Some(result_ty(Type::Int, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "udp_recv_from") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("UDPPacket".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "udp_send_bytes_to") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::Named("SocketAddr".to_string())),
            ],
            Some(result_ty(Type::Int, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "udp_receive") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("UDPPacket".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "udp_packet_data") => Some((
            vec![(read, Type::Named("UDPPacket".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "udp_packet_addr") => Some((
            vec![(read, Type::Named("UDPPacket".to_string()))],
            Some(Type::Named("SocketAddr".to_string())),
        )),
        ("core.net", "udp_packet_bytes") => Some((
            vec![(read, Type::Named("UDPPacket".to_string()))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.net", "udp_packet_original_len") => Some((
            vec![(read, Type::Named("UDPPacket".to_string()))],
            Some(Type::Int),
        )),
        ("core.net", "udp_packet_truncated") => Some((
            vec![(read, Type::Named("UDPPacket".to_string()))],
            Some(Type::Bool),
        )),
        ("core.net", "unix_listen") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("UnixListener".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "unix_accept") => Some((
            vec![(read, Type::Named("UnixListener".to_string()))],
            Some(result_ty(
                Type::Named("UnixStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "unix_connect") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("UnixStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "unix_read") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("UnixStream".to_string()),
            )],
            Some(result_ty(Type::String, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "unix_write") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("UnixStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "unix_read_bytes") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("UnixStream".to_string()),
                ),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(u8_ty())),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "unix_write_all_bytes") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("UnixStream".to_string()),
                ),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "unix_shutdown") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("UnixStream".to_string()),
                ),
                (read, Type::Named("NetShutdown".to_string())),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "unix_close") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("UnixStream".to_string()),
            )],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "dns_a" | "dns_aaaa") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("IPAddr".to_string()))),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "dns_a_at" | "dns_aaaa_at") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(Type::Named("IPAddr".to_string()))),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "dns_txt") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::List(Box::new(Type::String)),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "dns_txt_at") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(Type::String)),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "dns_ptr") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::List(Box::new(Type::String)),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "getservbyname") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Int, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "getservbyport") => Some((
            vec![(read, Type::Int)],
            Some(result_ty(Type::String, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "dns_srv") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("DNSSrv".to_string()))),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "dns_srv_at") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(Type::Named("DNSSrv".to_string()))),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "dns_srv_target") => Some((
            vec![(read, Type::Named("DNSSrv".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "dns_srv_port" | "dns_srv_priority" | "dns_srv_weight") => Some((
            vec![(read, Type::Named("DNSSrv".to_string()))],
            Some(Type::Int),
        )),
        ("core.net", "tls_connect") => Some((
            vec![
                (AccessConvention::Move, Type::Named("TcpStream".to_string())),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("TLSStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tls_read") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("TLSStream".to_string()),
            )],
            Some(result_ty(
                Type::String,
                Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
            )),
        )),
        ("core.net", "tls_write") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TLSStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(
                unit_ty(),
                Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
            )),
        )),
        ("core.net", "tls_close") => Some((
            vec![(AccessConvention::Move, Type::Named("TLSStream".to_string()))],
            Some(result_ty(
                unit_ty(),
                Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
            )),
        )),
        ("core.net.tls", "client") => Some((
            vec![
                (AccessConvention::Move, Type::Named("TcpStream".to_string())),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("TLSStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net.tls", "read") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TLSStream".to_string()),
                ),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(u8_ty())),
                Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
            )),
        )),
        ("core.net.tls", "read_text") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TLSStream".to_string()),
                ),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::String,
                Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
            )),
        )),
        ("core.net.tls", "write") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TLSStream".to_string()),
                ),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(
                Type::Int,
                Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
            )),
        )),
        ("core.net.tls", "write_all") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TLSStream".to_string()),
                ),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(
                unit_ty(),
                Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
            )),
        )),
        ("core.net.tls", "write_text") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TLSStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(
                unit_ty(),
                Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
            )),
        )),
        ("core.net.tls", "close") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("TLSStream".to_string()),
            )],
            Some(result_ty(
                unit_ty(),
                Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
            )),
        )),
        // E2-M10: core.http — HTTP client/server over blocking I/O.
        // GET / HEAD / DELETE requests (no body sent).
        ("core.http", "get") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("HTTPResponse".to_string()),
                Type::Named("HTTPError".to_string()),
            )),
        )),
        // POST / PUT / PATCH requests (body sent).
        ("core.http", "post") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(
                Type::Named("HTTPResponse".to_string()),
                Type::Named("HTTPError".to_string()),
            )),
        )),
        // HTTP server bind/serve carry Jet optional controls, while their
        // native non-direct rows consume Rust `Option` values.
        ("core.http.server", "bind") => Some((
            vec![
                (read, Type::String),
                (read, Type::Named("HTTPMux".to_string())),
                (
                    read,
                    Type::Option(Box::new(Type::Named("HTTPServerTls".to_string()))),
                ),
                (
                    read,
                    Type::Option(Box::new(Type::Named("Duration".to_string()))),
                ),
            ],
            Some(result_ty(
                Type::Named("HTTPServer".to_string()),
                Type::Named("HTTPError".to_string()),
            )),
        )),
        ("core.http.server", "serve") => Some((
            vec![
                (read, Type::String),
                (read, Type::Named("HTTPMux".to_string())),
                (
                    read,
                    Type::Option(Box::new(Type::Named("HTTPServerTls".to_string()))),
                ),
                (
                    read,
                    Type::Option(Box::new(Type::Named("Duration".to_string()))),
                ),
            ],
            Some(result_ty(unit_ty(), Type::Named("HTTPError".to_string()))),
        )),
        // D-REGEXENGINE1=A / D-REGEX-LIT1=D: runtime compilation stays
        // fallible; one-shot calls take a compile-checked Regex value.
        ("core.regex", "flags") => Some((
            vec![(read, Type::Bool), (read, Type::Bool), (read, Type::Bool)],
            Some(Type::Named("RegexFlags".to_string())),
        )),
        ("core.regex", "escape") => Some((vec![(read, Type::String)], Some(Type::String))),
        ("core.regex", "compile") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Regex".to_string()), Type::String)),
        )),
        ("core.regex", "compile_with") => Some((
            vec![
                (read, Type::String),
                (read, Type::Named("RegexFlags".to_string())),
            ],
            Some(result_ty(Type::Named("Regex".to_string()), Type::String)),
        )),
        ("core.regex", "is_match") => Some((
            vec![
                (read, Type::Named(Syntax::TYPE_REGEX.to_string())),
                (read, Type::String),
            ],
            Some(Type::Bool),
        )),
        ("core.regex", "full_match") => Some((
            vec![
                (read, Type::Named(Syntax::TYPE_REGEX.to_string())),
                (read, Type::String),
            ],
            Some(Type::Bool),
        )),
        // First match anywhere: `Match?` (none when nothing matches).
        ("core.regex", "match") => Some((
            vec![
                (read, Type::Named(Syntax::TYPE_REGEX.to_string())),
                (read, Type::String),
            ],
            Some(Type::Option(Box::new(Type::Named("Match".to_string())))),
        )),
        // First matched substring, or none.
        ("core.regex", "find") => Some((
            vec![
                (read, Type::Named(Syntax::TYPE_REGEX.to_string())),
                (read, Type::String),
            ],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.regex", "find_all" | "split") => Some((
            vec![
                (read, Type::Named(Syntax::TYPE_REGEX.to_string())),
                (read, Type::String),
            ],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.regex", "matches") => Some((
            vec![
                (read, Type::Named(Syntax::TYPE_REGEX.to_string())),
                (read, Type::String),
            ],
            Some(Type::List(Box::new(Type::Named("Match".to_string())))),
        )),
        ("core.regex", "split_limit") => Some((
            vec![
                (read, Type::Named(Syntax::TYPE_REGEX.to_string())),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.regex", "replace" | "replace_first") => Some((
            vec![
                (read, Type::Named(Syntax::TYPE_REGEX.to_string())),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(Type::String),
        )),
        // D-CORE-COMPRESS1=A / D-DEP-ARCHIVE1=A: core.archive owns only
        // container formats. Stream gzip lives in core.archive.gzip.
        // zip_compress creates a single-entry zip archive.
        // Takes (name: String, data: [U8]) → [U8].
        ("core.archive", "zip_compress") => Some((
            vec![(read, Type::String), (read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: zip_decompress — extract first entry from a zip archive.
        // Takes [U8] → [U8]. Returns empty list on invalid input.
        ("core.archive", "zip_decompress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.archive", "crc32" | "adler32") => {
            Some((vec![(read, Type::List(Box::new(u8_ty())))], Some(Type::Int)))
        }
        ("core.archive", "deflate" | "inflate" | "zip_open" | "zip_close") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.archive", "zip_names_json") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::String),
        )),
        ("core.archive", "zip_next") => Some((
            vec![(read, Type::List(Box::new(u8_ty()))), (read, Type::Int)],
            Some(Type::String),
        )),
        ("core.archive", "zip_read") => Some((
            vec![(read, Type::List(Box::new(u8_ty()))), (read, Type::String)],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.archive", "zip_write") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::String),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.archive", "zip_extract" | "unzip") => Some((
            vec![(read, Type::List(Box::new(u8_ty()))), (read, Type::String)],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_add — append/replace a named entry in a tar archive.
        // Takes (archive: [U8], name: String, data: [U8]) → [U8].
        ("core.archive", "tar_add") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::String),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_get — extract a named entry from a tar archive.
        // Takes (archive: [U8], name: String) → [U8]. Empty on not-found or bad input.
        ("core.archive", "tar_get") => Some((
            vec![(read, Type::List(Box::new(u8_ty()))), (read, Type::String)],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_names_json — list entry names as a JSON array string.
        // Takes [U8] → String. Returns "[]" on empty or invalid archive.
        ("core.archive", "tar_names_json") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::String),
        )),
        // D-RAYLIB1=A / D-FLAGSHIP-RAYLIB1=A: first bounded `core.game.raylib`
        // bridge. The surface is intentionally tiny and display-gated.
        ("core.game.raylib", "window_open") => Some((
            vec![(read, Type::Int), (read, Type::Int), (read, Type::String)],
            Some(Type::Named("RaylibWindow".to_string())),
        )),
        ("core.game.raylib", "window_should_close") => Some((
            vec![(read, Type::Named("RaylibWindow".to_string()))],
            Some(Type::Bool),
        )),
        ("core.game.raylib", "window_ready") => Some((
            vec![(read, Type::Named("RaylibWindow".to_string()))],
            Some(Type::Bool),
        )),
        ("core.game.raylib", "begin_drawing") => {
            Some((vec![(read, Type::Named("RaylibWindow".to_string()))], None))
        }
        ("core.game.raylib", "clear_background") => {
            Some((vec![(read, Type::Named("RaylibColor".to_string()))], None))
        }
        ("core.game.raylib", "draw_rectangle") => Some((
            vec![
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Named("RaylibColor".to_string())),
            ],
            None,
        )),
        ("core.game.raylib", "draw_text") => Some((
            vec![
                (read, Type::String),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Named("RaylibColor".to_string())),
            ],
            None,
        )),
        ("core.game.raylib", "end_drawing") => Some((vec![], None)),
        ("core.game.raylib", "close_window") => {
            Some((vec![(read, Type::Named("RaylibWindow".to_string()))], None))
        }
        ("core.game.raylib", "key_down") => Some((vec![(read, Type::String)], Some(Type::Bool))),
        ("core.game.raylib", "set_target_fps") => Some((vec![(read, Type::Int)], None)),
        ("core.game.raylib", "color") => Some((
            vec![
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
            ],
            Some(Type::Named("RaylibColor".to_string())),
        )),
        ("core.game.raylib", "load_sound") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("RaylibSound".to_string())),
        )),
        ("core.game.raylib", "play_sound") => Some((
            vec![(read, Type::Named("RaylibSound".to_string()))],
            Some(Type::Bool),
        )),
        ("core.game.raylib", "gamepad_down") => Some((
            vec![(read, Type::Int), (read, Type::String)],
            Some(Type::Bool),
        )),
        ("core.game.raylib", "gamepad_axis") => Some((
            vec![(read, Type::Int), (read, Type::String)],
            Some(Type::Float),
        )),
        ("core.game.raylib", "load_texture_atlas") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("RaylibTextureAtlas".to_string())),
        )),
        ("core.game.raylib", "draw_sprite") => Some((
            vec![
                (read, Type::Named("RaylibTextureAtlas".to_string())),
                (read, Type::String),
                (read, Type::Int),
                (read, Type::Int),
            ],
            None,
        )),
        // D-CORE-COMPRESS1=A / D-CODECS1: core.archive.gzip / zstd are the
        // only public stream-codec APIs. `compress` takes `[U8]` and is infallible;
        // `decompress` is fallible (malformed compressed stream → `Err(String)`),
        // following the same house style as core.encoding.hex/base64 `decode`.
        ("core.archive.gzip", "compress") | ("core.archive.zstd", "compress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.archive.gzip", "decompress") | ("core.archive.zstd", "decompress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        // D-DBPOLICY-BIND1: core.db — SQLite via rusqlite (bundled). `open`/`open_memory`
        // produce an unscoped `DBConnection`; `policy` creates a typed policy
        // and `with_policy` produces the only row-capable `DBScope`.
        // handle (mirrors `core.files`'s `open`/`create` producing a `FileReader`/
        // `FileWriter`). Every other operation — `query`/`query_one`/`execute`/
        // `begin`/`commit`/`rollback`/`close` — is an INSTANCE method dispatched
        // by the receiver's `DBConnection` type (see `check_db_connection_method`
        // below), not a second module-call surface. There is no raw-string
        // `execute(sql)` escape (D-DBDRIVER1's build plan: "must not expose a
        // generic `execute_raw(sql)` escape").
        ("core.db", "open") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("DBConnection".to_string())),
        )),
        ("core.db", "open_memory") => Some((vec![], Some(Type::Named("DBConnection".to_string())))),
        ("core.db", "pool") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::Named("DbPool".to_string()),
                db_error_ty(),
            )),
        )),
        ("core.db", "policy") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(
                Type::Named("RowPolicy".to_string()),
                Type::String,
            )),
        )),
        // D-DBPOLICY1: audit is scoped to the active DBScope, so it cannot
        // inspect or manufacture authority for another connection.
        ("core.db", "policy_audit") => Some((
            vec![(read, Type::Named("DBScope".to_string()))],
            Some(Type::String),
        )),
        ("core.db", "row_value") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())),
                err: Box::new(Type::String),
            }),
        )),
        ("core.db", "row_int") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Int),
                err: Box::new(Type::String),
            }),
        )),
        ("core.db", "row_float") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Float),
                err: Box::new(Type::String),
            }),
        )),
        ("core.db", "row_text") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::String),
            }),
        )),
        ("core.db", "row_bool") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Bool),
                err: Box::new(Type::String),
            }),
        )),
        ("core.db", "transaction") | ("core.db", "migrate") => Some((
            vec![
                (read, Type::Named("DBScope".to_string())),
                (read, Type::String),
                (read, Type::List(Box::new(Type::Named("SQL".to_string())))),
            ],
            Some(result_ty(Type::Int, db_error_ty())),
        )),
        // D-DEP-WASM1=A / D-PLUGIN1=B (c81): `core.plugin` — sandboxed WASM
        // Component Model plugin loader. `load(path, authority)` is the one
        // authority-bearing boundary; the host never supplies an ambient
        // policy when the second argument is omitted.
        ("core.plugin", "load") => Some((
            vec![
                (read, Type::String),
                (read, Type::Named("Authority".to_string())),
            ],
            Some(Type::Named("Plugin".to_string())),
        )),
        // D-LIB-CALLGRANT1=A: the grant is label-only so the load site cannot
        // accidentally swap path and authority by position.
        ("core.mod", "load") => Some((
            vec![
                (read, Type::String),
                (read, Type::Named("ModGrant".to_string())),
            ],
            Some(result_ty(Type::Named("Mod".to_string()), Type::String)),
        )),
        // D-UUIDENC1=A: hex and base64 codecs. `encode` is infallible; `decode`
        // returns `[Byte] !String` (invalid input → Err).
        ("core.encoding.hex", "encode") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.hex", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        ("core.encoding.base64", "encode") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.base64", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        ("core.encoding.base64", "encode_url") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.base64", "decode_url") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        ("core.encoding.base32", "encode") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.base32", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        // D-UUIDENC1=A: UUID v4 (system CSPRNG) and v7 (injectable Clock).
        // `v4()` reads /dev/urandom; `v7(clock)` extracts the timestamp from the
        // injected Clock so tests can produce a deterministic UUID.
        ("core.crypto.uuid", "v4") => Some((vec![], Some(Type::String))),
        // #1481: `v5` is the deterministic namespace+name sibling (RFC 4122
        // SHA-1); `parse` validates/normalizes a UUID string. Both fail on a
        // malformed UUID rather than panic (fallible input, D-FAIL-*).
        ("core.crypto.uuid", "v5") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.crypto.uuid", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.crypto.uuid", "v7") => Some((
            vec![(read, Type::Named(crate::Syntax::CLOCK_TYPE.to_string()))],
            Some(Type::String),
        )),
        ("core.args", "spec") => Some((vec![], Some(Type::Named("ArgsSpec".to_string())))),
        // D-TERM1 (ratified 2026-06-22): terminal direct-input.
        // `term.read_key()` → `Key` (the key-event enum). No arguments.
        ("core.term", "read_key") => Some((
            vec![],
            Some(Type::Named(crate::Syntax::TYPE_KEY.to_string())),
        )),
        // D-FIDELITY-API1=A: runtime-global fidelity signal.
        ("core.perf", "fidelity") => Some((vec![], Some(float))),
        ("core.perf", "default_fidelity") => Some((vec![], Some(float.clone()))),
        ("core.perf", "override_fidelity") => Some((
            vec![(read, float)],
            Some(result_ty(unit.clone(), Type::String)),
        )),
        ("core.perf", "reset_fidelity") => Some((vec![], Some(unit))),
        // D-DECIMAL1: exact decimal parse from string.
        ("core.math", "decimal") => Some((
            vec![(read, string.clone())],
            Some(Type::Named(crate::Syntax::TYPE_DECIMAL.to_string())),
        )),
        // Exact whole-number math keeps the native i64 boundary explicit.
        ("core.math", "isqrt" | "factorial") => Some((
            vec![(read, int.clone())],
            Some(Type::Option(Box::new(int.clone()))),
        )),
        // D-NUMTYPE1=A: exact ratio, or nothing when the bottom is zero.
        ("core.math", "fraction") => Some((
            vec![(read, int.clone()), (read, int.clone())],
            Some(Type::Option(Box::new(Type::Named(
                crate::Syntax::TYPE_FRACTION.to_string(),
            )))),
        )),
        // D-NAMEDPREVIEW1=A: registry-free named preview values share one
        // typed callback tree and use an optional viewport label.
        ("core.ui", "phone" | "tablet" | "desktop") => Some((
            vec![],
            Some(Type::Named("UiPreviewViewport".to_string())),
        )),
        ("core.ui", "preview" | "playground") => Some((
            vec![
                (read, string.clone()),
                (
                    read,
                    Type::Result {
                        ok: Box::new(Type::Named("UiPreviewViewport".to_string())),
                        err: Box::new(Type::Named("Absent".to_string())),
                    },
                ),
                (read, ui_node_callback),
            ],
            Some(ui_preview),
        )),
        // D-RENDERTGT2=A (c133 M1): UI geometry constructors.
        ("core.ui", "null_backend") => Some((vec![], Some(Type::Named("NullBackend".to_string())))),
        ("core.ui", "tui_backend") => Some((vec![], Some(Type::Named("TuiBackend".to_string())))),
        // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend constructor.
        ("core.ui", "gtk_backend") => Some((vec![], Some(Type::Named("GtkBackend".to_string())))),
        ("core.ui", "point") => Some((
            vec![(read, float.clone()), (read, float)],
            Some(Type::Named("Point".to_string())),
        )),
        ("core.ui", "size") => Some((
            vec![(read, float.clone()), (read, float)],
            Some(Type::Named("Size".to_string())),
        )),
        ("core.ui", "rect") => Some((
            vec![
                (read, float.clone()),
                (read, float.clone()),
                (read, float.clone()),
                (read, float),
            ],
            Some(Type::Named("Rect".to_string())),
        )),
        ("core.ui", "constraint") => Some((
            vec![
                (read, float.clone()),
                (read, float.clone()),
                (read, float.clone()),
                (read, float),
            ],
            Some(Type::Named("SizeConstraint".to_string())),
        )),
        ("core.ui", "node") => Some((
            vec![(read, string.clone()), (read, float.clone()), (read, float)],
            Some(Type::Named("UiNode".to_string())),
        )),
        // D-UITREE1=A: canonical typed constructors. These are plain Core
        // functions returning the same UiNode tree consumed by each backend.
        // `button` is bespoke in core_call (arity 1, or 2 with `on_click:`).
        ("core.ui", "text") => Some((
            vec![(read, string.clone())],
            Some(Type::Named("UiNode".to_string())),
        )),
        ("core.ui", "box") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::Named("UiNode".to_string()))),
            )],
            Some(Type::Named("UiNode".to_string())),
        )),
        ("core.ui", "key_event") => Some((
            vec![(read, string)],
            Some(Type::Named("InputEvent".to_string())),
        )),
        ("core.ui", "resize_event") => Some((
            vec![(read, float.clone()), (read, float)],
            Some(Type::Named("InputEvent".to_string())),
        )),
        // D-A11YGATE1=B (c134 Phase 6): accessible-role node constructor + role
        // constants. `node_role` is the a11y-checked sibling of `node` — it's the
        // only UiNode constructor that carries a role, so it's the only one E2930
        // (unlabeled interactive control) needs to watch.
        ("core.ui", "node_role") => Some((
            vec![
                (read, string),
                (read, float.clone()),
                (read, float),
                (read, Type::Named("UiAriaRole".to_string())),
            ],
            Some(Type::Named("UiNode".to_string())),
        )),
        // D-STYLESHAPE1=A wiring: a node with an explicit fill color (a `#RRGGBB`
        // string, matching `JetPaintCmd::FillRect`'s existing color representation —
        // no new opaque type needed, this just makes the field settable from Jet).
        ("core.ui", "node_color") => Some((
            vec![
                (read, string.clone()),
                (read, float.clone()),
                (read, float),
                (read, string),
            ],
            Some(Type::Named("UiNode".to_string())),
        )),
        (
            "core.ui",
            "aria_role_button" | "aria_role_text_input" | "aria_role_label" | "aria_role_container",
        ) => Some((vec![], Some(Type::Named("UiAriaRole".to_string())))),
        // D-FOUND-PLATFORM1=A: shared font model and scoped host service
        // surface. Result aliases retain Completed/Cancelled/Failed as one
        // typed value instead of collapsing cancellation into an I/O error.
        ("core.font", "system") => Some((
            vec![(read, Type::Named("FontStyle".to_string()))],
            Some(Type::Named("FontFace".to_string())),
        )),
        ("core.font", "shape") => Some((
            vec![
                (read, string.clone()),
                (read, Type::Named("FontFace".to_string())),
            ],
            Some(Type::Named("GlyphRun".to_string())),
        )),
        ("core.ui", "node_accessibility") => Some((
            vec![
                (read, Type::Named("UiNode".to_string())),
                (read, Type::Named("UiAccessibility".to_string())),
            ],
            Some(Type::Named("UiNode".to_string())),
        )),
        ("core.ui", "node_shortcut") => Some((
            vec![
                (read, Type::Named("UiNode".to_string())),
                (read, Type::Named("UiShortcut".to_string())),
            ],
            Some(Type::Named("UiNode".to_string())),
        )),
        ("core.ui", "text_input") => Some((
            vec![
                (read, string.clone()),
                (read, Type::Named("UiImeMode".to_string())),
            ],
            Some(Type::Named("UiNode".to_string())),
        )),
        // D-DX-TUIKIT1=A: one typed model/update/view support vocabulary. The
        // constructors stay ordinary Core calls; the shared Prelude owns
        // event, style, constraint, and widget semantics for every tier.
        ("core.tui", "capabilities") => Some((
            vec![],
            Some(Type::Named("TuiCapabilities".to_string())),
        )),
        ("core.tui", "key_event") => Some((
            vec![(read, string.clone())],
            Some(Type::Named("TuiEvent".to_string())),
        )),
        ("core.tui", "key_event_modifiers") => Some((
            vec![(read, string.clone()), (read, int.clone())],
            Some(Type::Named("TuiEvent".to_string())),
        )),
        ("core.tui", "resize_event") => Some((
            vec![(read, float.clone()), (read, float.clone())],
            Some(Type::Named("TuiEvent".to_string())),
        )),
        ("core.tui", "timer_event") => Some((
            vec![(read, string.clone()), (read, int.clone())],
            Some(Type::Named("TuiEvent".to_string())),
        )),
        ("core.tui", "io_event") => Some((
            vec![
                (read, string.clone()),
                (moved, list_u8.clone()),
            ],
            Some(Type::Named("TuiEvent".to_string())),
        )),
        ("core.tui", "focus_event") => Some((
            vec![(read, bool_)],
            Some(Type::Named("TuiEvent".to_string())),
        )),
        ("core.tui", "interrupt_event" | "close_event") => {
            Some((vec![], Some(Type::Named("TuiEvent".to_string()))))
        }
        ("core.tui", "color_ansi16" | "color_ansi256") => Some((
            vec![(read, int.clone())],
            Some(Type::Named("TuiColor".to_string())),
        )),
        ("core.tui", "color_rgb") => Some((
            vec![
                (read, int.clone()),
                (read, int.clone()),
                (read, int.clone()),
            ],
            Some(Type::Named("TuiColor".to_string())),
        )),
        ("core.tui", "style") => Some((
            vec![],
            Some(Type::Named("TuiStyle".to_string())),
        )),
        ("core.tui", "style_foreground" | "style_background") => Some((
            vec![
                (moved, Type::Named("TuiStyle".to_string())),
                (read, Type::Named("TuiColor".to_string())),
            ],
            Some(Type::Named("TuiStyle".to_string())),
        )),
        ("core.tui", "style_bold" | "style_dim" | "style_underline") => Some((
            vec![
                (moved, Type::Named("TuiStyle".to_string())),
                (read, bool_),
            ],
            Some(Type::Named("TuiStyle".to_string())),
        )),
        ("core.tui", "style_text") => Some((
            vec![
                (read, string.clone()),
                (read, Type::Named("TuiStyle".to_string())),
                (read, Type::Named("TuiCapabilities".to_string())),
            ],
            Some(string.clone()),
        )),
        ("core.tui", "ascii") => Some((
            vec![(read, string.clone())],
            Some(string.clone()),
        )),
        ("core.tui", "display_width") => Some((
            vec![(read, string.clone())],
            Some(int.clone()),
        )),
        ("core.tui", "length" | "min" | "max" | "percent" | "fill") => Some((
            vec![(read, float.clone())],
            Some(Type::Named("TuiConstraint".to_string())),
        )),
        ("core.tui", "horizontal" | "vertical") => Some((
            vec![],
            Some(Type::Named("TuiDirection".to_string())),
        )),
        ("core.tui", "layout") => Some((
            vec![
                (read, Type::Named("Rect".to_string())),
                (read, Type::Named("TuiDirection".to_string())),
                (
                    moved,
                    Type::List(Box::new(Type::Named("TuiConstraint".to_string()))),
                ),
            ],
            Some(Type::List(Box::new(Type::Named("Rect".to_string())))),
        )),
        ("core.tui", "list") => Some((
            vec![(
                moved,
                Type::List(Box::new(string.clone())),
            )],
            Some(Type::Named("UiNode".to_string())),
        )),
        ("core.tui", "table") => Some((
            vec![
                (moved, Type::List(Box::new(string.clone()))),
                (
                    moved,
                    Type::List(Box::new(Type::List(Box::new(string.clone())))),
                ),
            ],
            Some(Type::Named("UiNode".to_string())),
        )),
        ("core.tui", "list_state") => Some((
            vec![],
            Some(Type::Named("TuiListState".to_string())),
        )),
        ("core.tui", "list_state_select" | "list_state_offset") => Some((
            vec![
                (moved, Type::Named("TuiListState".to_string())),
                (read, int.clone()),
            ],
            Some(Type::Named("TuiListState".to_string())),
        )),
        ("core.tui", "list_state_selected") => Some((
            vec![(read, Type::Named("TuiListState".to_string()))],
            Some(int),
        )),
        ("core.ui.host", "capabilities") => Some((
            vec![],
            Some(Type::Named("UiCapabilityFacts".to_string())),
        )),
        ("core.ui.host", "file_filter") => Some((
            vec![
                (read, string.clone()),
                (read, Type::List(Box::new(string.clone()))),
                (read, Type::List(Box::new(string.clone()))),
            ],
            Some(ui_result(Type::Named("UiFileFilter".to_string()))),
        )),
        ("core.ui.host", "file_filter_text") => Some((
            vec![],
            Some(Type::Named("UiFileFilter".to_string())),
        )),
        ("core.ui.host", "fs_rights_read" | "fs_rights_write" | "fs_rights_read_write") => {
            Some((vec![], Some(Type::Named("UiFsRights".to_string()))))
        }
        ("core.ui.host", "fs_grant") => Some((
            vec![
                (read, string.clone()),
                (read, Type::Named("UiFsRights".to_string())),
            ],
            Some(ui_result(Type::Named("UiFsGrant".to_string()))),
        )),
        ("core.ui.host", "open_request" | "save_request") => Some((
            vec![(read, Type::Named("UiFsGrant".to_string()))],
            Some(Type::Named("UiFileDialogRequest".to_string())),
        )),
        ("core.ui.host", "open_file" | "save_file") => Some((
            vec![(read, Type::Named("UiFileDialogRequest".to_string()))],
            Some(ui_result(Type::Named("UiFileDialogSelection".to_string()))),
        )),
        ("core.ui.host", "shortcut") => Some((
            vec![
                (read, string.clone()),
                (read, Type::Named("UiShortcutModifiers".to_string())),
            ],
            Some(ui_result(Type::Named("UiShortcut".to_string()))),
        )),
        ("core.ui.host", "accessibility") => Some((
            vec![(read, string.clone()), (read, string.clone())],
            Some(ui_result(Type::Named("UiAccessibility".to_string()))),
        )),
        ("core.ui.host.clipboard", "read_text") => Some((
            vec![],
            Some(ui_result(Type::Named("UiClipboardText".to_string()))),
        )),
        ("core.ui.host.clipboard", "write_text") => Some((
            vec![(read, string.clone())],
            Some(ui_result(Type::Named("UiClipboardWrite".to_string()))),
        )),
        ("core.ui.host.ime", "poll") => Some((
            vec![],
            Some(ui_result(Type::Option(Box::new(Type::Named(
                "UiImeEvent".to_string(),
            ))))),
        )),
        ("core.ui.host.drag_drop", "poll") => Some((
            vec![],
            Some(ui_result(Type::Option(Box::new(Type::Named(
                "UiDragEvent".to_string(),
            ))))),
        )),
        ("core.ui.host.shortcuts", "binding") => Some((
            vec![
                (read, Type::Named("UiShortcut".to_string())),
                (read, string.clone()),
            ],
            Some(ui_result(Type::Named("UiShortcutBinding".to_string()))),
        )),
        ("core.ui.host.shortcuts", "register") => Some((
            vec![(read, Type::Named("UiShortcutBinding".to_string()))],
            Some(ui_result(Type::Named("UiShortcutBinding".to_string()))),
        )),
        ("core.ui.host.shortcuts", "dispatch") => Some((
            vec![(read, Type::Named("UiShortcut".to_string()))],
            Some(ui_result(Type::Named("UiShortcutDispatch".to_string()))),
        )),
        ("core.ui.host.accessibility", "attach") => Some((
            vec![
                (read, Type::Named("UiNode".to_string())),
                (read, Type::Named("UiAccessibility".to_string())),
            ],
            Some(ui_result(unit.clone())),
        )),
        ("core.ui.host.accessibility", "project") => Some((
            vec![
                (read, Type::Named("UiNode".to_string())),
                (read, Type::Named("UiNodeId".to_string())),
            ],
            Some(ui_result(Type::Option(Box::new(Type::Named(
                "UiAccessibilityProjection".to_string(),
            ))))),
        )),
        // D-FLAGSHIP-WEBAPI1=A: first-party browser API for web flagship slices.
        ("core.web", "on") => Some((
            vec![
                (read, string.clone()),
                (read, string.clone()),
                (
                    read,
                    Type::Fn {
                        params: vec![Type::Named("WebEvent".to_string())],
                        ret: None,
                        effect_bound: None,
                        return_view_provenance: None,
                        param_contract: None,
                        call_metadata: None,
                    },
                ),
            ],
            None,
        )),
        ("core.web", "value") => Some((vec![(read, string.clone())], Some(Type::String))),
        // D-WEBAPP1=D: `web.app()` → App builder; `web.page(title, body)` → WebPage.
        ("core.web", "app") => Some((vec![], Some(Type::Named("App".to_string())))),
        ("core.web", "page") => Some((
            vec![(read, string.clone()), (read, string)],
            Some(Type::Named("WebPage".to_string())),
        )),
        ("app" | "core.web", "live") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::Named("LiveQuery".to_string())),
        )),
        ("app" | "core.web", "subscribe") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("LiveQuery".to_string())),
        )),
        ("app" | "core.web", "invalidate" | "transact_invalidate") => {
            Some((vec![(read, Type::String)], Some(Type::Int)))
        }
        ("app" | "core.web", "signal_push") => Some((
            vec![
                (read, Type::Named("LiveQuery".to_string())),
                (read, Type::String),
            ],
            Some(Type::Named("LiveQuery".to_string())),
        )),
        ("app" | "core.web", "live_get" | "live_show") => Some((
            vec![(read, Type::Named("LiveQuery".to_string()))],
            Some(Type::String),
        )),
        ("app" | "core.web", "live_stats") => Some((vec![], Some(Type::String))),
        ("app" | "core.web", "auth") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("Auth".to_string())),
        )),
        ("app" | "core.web", "auth_oauth") => Some((
            vec![
                (read, Type::Named("Auth".to_string())),
                (read, Type::String),
            ],
            Some(Type::Named("Auth".to_string())),
        )),
        ("app" | "core.web", "auth_routes" | "auth_show") => Some((
            vec![(read, Type::Named("Auth".to_string()))],
            Some(Type::String),
        )),
        ("app" | "core.web", "sync") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::String),
        )),
        ("core.web.storage.local" | "core.web.storage.session", "get") => Some((
            vec![(read, string.clone())],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.web.storage.local" | "core.web.storage.session", "set") => {
            Some((vec![(read, string.clone()), (read, string.clone())], None))
        }
        ("core.web.storage.local" | "core.web.storage.session", "remove") => {
            Some((vec![(read, string)], None))
        }
        ("core.web.storage.local" | "core.web.storage.session", "clear") => Some((vec![], None)),
        // c-devserver (owner-directed 2026-07-01): `devserver.for_app(file)` —
        // the constructor for a configurable `jet dev` server value. The
        // builder methods (`.html`/`.port`/`.serve`) are instance methods on
        // `DevServer`, dispatched through `devserver_method_return` (mirrors
        // `ui_backend_method_return`), not module-level names here.
        ("core.web.devserver", "for_app") => Some((
            vec![(read, string)],
            Some(Type::Named("DevServer".to_string())),
        )),
        // `devserver.app()` — zero-arg: watch the file `jet dev` launched
        // (passed to the running program via JET_DEV_FILE). The common case:
        // the file defining `fn dev()` is the file to watch, so no path is
        // spelled out at all.
        ("core.web.devserver", "app") => Some((vec![], Some(Type::Named("DevServer".to_string())))),
        _ => None,
    }
}

/// Project the typed sema signature for a foundation Core-call row.
///
/// The foundation owns the erased ABI and the sema owns Jet `Type` values.
/// Keeping this adapter here makes the dependency boundary explicit while
/// rejecting a row whose typed parameter count has drifted from its ABI.
pub fn core_fixed_sig_for_row(
    row: &Syntax::CoreCallRecord,
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    let signature = core_fixed_sig(row.module, row.member)?;
    (signature.0.len() == row.arity() && row.signature.max_arity == row.arity())
        .then_some(signature)
}

/// D-APILABEL1=A: the public call contract of a Core library function.
///
/// The signature table above carries only conventions and types, so a Core
/// parameter has no name for a label to bind to. This table gives the ones
/// that need it a label, a zone, and — where the parameter is optional — the
/// Jet expression its default is written as.
///
/// Declaring the default here is what lets `json.writer(^out, canonical: true)`
/// work: the binder fills `limits` with a real argument, so every engine
/// receives the same value instead of each spelling its own fallback.
pub struct CoreParam {
    /// The label a caller writes. A parameter with no entry here is
    /// positional-only.
    pub label: &'static str,
    pub zone: crate::AST::ParamZone,
    /// The default this parameter takes when a call skips it. `None` means the
    /// parameter is required.
    pub default: Option<CoreDefault>,
}

/// A Core parameter default, as a shape the binder builds into ordinary Jet
/// AST. Core signatures are a table rather than Jet source, so there is no
/// written expression to clone.
#[derive(Clone, Copy)]
pub enum CoreDefault {
    Bool(bool),
    Int(i64),
    String(&'static str),
    EmptyList,
    /// An omitted optional Core handle. The binder inserts the slot; the
    /// Core emitter turns this declaration-side absence into `None`.
    Absent,
    /// A no-argument static call on a Core type, such as `EncodingLimits.safe()`.
    /// The type name resolves without the caller importing its module.
    StaticCall {
        type_name: &'static str,
        method: &'static str,
    },
    /// A unit variant of a compiler-owned Core enum.
    StaticEnum {
        type_name: &'static str,
        variant: &'static str,
    },
}

impl CoreDefault {
    pub fn build(self, span: crate::Diagnostics::Span) -> crate::AST::Expr {
        match self {
            CoreDefault::Bool(value) => crate::AST::Expr::Bool(value, span),
            CoreDefault::Int(value) => crate::AST::Expr::Int(value, span, None, None),
            CoreDefault::String(value) => {
                crate::AST::Expr::Str(vec![crate::AST::StrPart::Lit(value.to_string())], span)
            }
            CoreDefault::EmptyList => crate::AST::Expr::ListLit(Vec::new(), span),
            CoreDefault::Absent => crate::AST::Expr::Absent(span),
            CoreDefault::StaticCall { type_name, method } => crate::AST::Expr::MethodCall {
                receiver: Box::new(crate::AST::Expr::Ident(type_name.to_string(), span)),
                method: method.to_string(),
                method_span: span,
                owner_type_args: Vec::new(),
                type_args: Vec::new(),
                args: Vec::new(),
                recv_type: None,
                resolved_ret: None,
                operator_rhs: None,
                checked_widen: false,
            },
            CoreDefault::StaticEnum { type_name, variant } => crate::AST::Expr::EnumLit {
                type_name: type_name.to_string(),
                variant: variant.to_string(),
                variant_span: None,
                args: Vec::new(),
                leading_dot: false,
                span,
            },
        }
    }
}

const fn required(label: &'static str) -> CoreParam {
    CoreParam {
        label,
        zone: crate::AST::ParamZone::PositionalOnly,
        default: None,
    }
}
const fn required_either(label: &'static str) -> CoreParam {
    CoreParam {
        label,
        zone: crate::AST::ParamZone::Either,
        default: None,
    }
}

const fn optional(label: &'static str, default: CoreDefault) -> CoreParam {
    CoreParam {
        label,
        zone: crate::AST::ParamZone::Either,
        default: Some(default),
    }
}

const fn optional_label_only(label: &'static str) -> CoreParam {
    CoreParam {
        label,
        zone: crate::AST::ParamZone::LabelOnly,
        default: Some(CoreDefault::Absent),
    }
}


/// The bounded-encoding reader/writer family (D-ENCSTREAM-SURFACE1=A). The file
/// handle is required and positional; the policy arguments are labelled so a
/// call can name only the policy it changes.
const ENCODING_LIMITS_DEFAULT: CoreDefault = CoreDefault::StaticCall {
    type_name: "EncodingLimits",
    method: "safe",
};

pub fn core_param_contract(module: &str, name: &str) -> Option<Vec<CoreParam>> {
    match (module, name) {
        // D-TEST-HISTORY1=A: histories accepts either the concise positional
        // form or the ratified named fields without a second argument schema.
        // Strategy is the sole optional label and remains in the canonical
        // third slot when omitted.
        ("core.testing", "histories") => Some(vec![
            required_either("seed"),
            required_either("cases"),
            optional_label_only("strategy"),
            required_either("model"),
            required_either("actual"),
            required_either("observe"),
        ]),
        // D-DX-PLUGIN1=D: publication is a checked two-label boundary. The
        // field selector and value are both explicit; no positional fallback.
        ("core.devtools", "publish") => Some(vec![
            CoreParam {
                label: "field",
                zone: crate::AST::ParamZone::LabelOnly,
                default: None,
            },
            CoreParam {
                label: "value",
                zone: crate::AST::ParamZone::LabelOnly,
                default: None,
            },
        ]),
        // D-NAMEDPREVIEW1=A: the viewport is an optional label between the
        // required preview name and callback.
        ("core.ui", "preview" | "playground") => Some(vec![
            required("name"),
            optional_label_only("viewport"),
            required("callback"),
        ]),
        // D-UI-CLOSURE1=A: button keeps one fixed four-word runtime shape.
        // The two optional metadata labels are absent when omitted; the
        // trailing block binds to the existing click callback slot.
        ("core.ui", "button") => Some(vec![
            required("display"),
            optional_label_only("shortcut"),
            optional_label_only("label"),
            optional_label_only("on_click"),
        ]),
        // D-UI-DROP1=A: text input keeps its existing state/IME constructor
        // and adds one optional typed drop callback at the source boundary.
        ("core.ui", "text_input") => Some(vec![
            required("state"),
            required("ime"),
            optional_label_only("on_drop"),
        ]),
        // D-FOUND-REALTIME1=A: the callback registration surface names all
        // three inputs, including the escaping function value.
        ("core.rt", "callback") => Some(vec![
            required("rate"),
            required("frames"),
            required("fn"),
        ]),
        // D-TIMEDEPTH1=A: local-to-zoned construction has one explicit DST
        // policy, with Temporal's compatible choice as the beginner default.
        ("core.time", "zoned_local") => Some(vec![
            required("date"),
            required("time"),
            required("zone"),
            optional("disambiguation", CoreDefault::String("compatible")),
        ]),
        // D-CONFIG-ENV1: labels are part of the runtime config surface, while
        // the defaults keep `env.decode<T>()` as the beginner one-call form.
        ("core.sys", "decode") => Some(vec![
            optional("prefix", CoreDefault::String("")),
            optional("file", CoreDefault::String(".env")),
            optional("allow", CoreDefault::EmptyList),
        ]),
        // D-FOUND-COREAPI1=A: filesystem walks accept one optional, shared
        // ignore-file selector; omitted means no ignore rules.
        ("core.files", "walk" | "walk_parallel" | "walk_files") => Some(vec![
            required("root"),
            optional("ignore", CoreDefault::Absent),
        ]),
        ("core.game", "run") => Some(vec![
            required("scene"),
            CoreParam {
                label: "replay",
                zone: crate::AST::ParamZone::Either,
                default: Some(CoreDefault::Absent),
            },
            CoreParam {
                label: "backend",
                zone: crate::AST::ParamZone::Either,
                default: Some(CoreDefault::Absent),
            },
            CoreParam {
                label: "frames",
                zone: crate::AST::ParamZone::Either,
                default: Some(CoreDefault::Absent),
            },
        ]),
        ("core.http.server", "bind" | "serve") => Some(vec![
            required("address"),
            required("mux"),
            CoreParam {
                label: "tls",
                zone: crate::AST::ParamZone::LabelOnly,
                default: Some(CoreDefault::Absent),
            },
            CoreParam {
                label: "deadline",
                zone: crate::AST::ParamZone::LabelOnly,
                default: Some(CoreDefault::Absent),
            },
        ]),
        ("core.http.server", "cors_policy") => Some(vec![
            required_either("origins"),
            optional("methods", CoreDefault::EmptyList),
            optional("headers", CoreDefault::EmptyList),
            optional("credentials", CoreDefault::Bool(false)),
            optional("max_age", CoreDefault::Int(86_400)),
        ]),
        ("core.encoding.csv", "parse") => Some(vec![
            required("text"),
            optional("delimiter", CoreDefault::String(",")),
            optional("header", CoreDefault::Bool(false)),
            optional("skip_blank", CoreDefault::Bool(false)),
        ]),
        ("core.encoding.csv", "rows") => Some(vec![
            required("text"),
            optional("delimiter", CoreDefault::String(",")),
            optional("header", CoreDefault::Bool(false)),
            optional("skip_blank", CoreDefault::Bool(false)),
        ]),
        ("core.encoding.csv", "reader") => Some(vec![
            required("source"),
            optional("limits", ENCODING_LIMITS_DEFAULT),
            optional("delimiter", CoreDefault::String(",")),
            optional("header", CoreDefault::Bool(false)),
            optional("skip_blank", CoreDefault::Bool(false)),
        ]),
        ("core.encoding.json" | "core.encoding.jsonl" | "core.encoding.cbor", "reader") => {
            Some(vec![
                required("source"),
                optional("limits", ENCODING_LIMITS_DEFAULT),
            ])
        }
        ("core.encoding.json", "writer") => Some(vec![
            required("target"),
            optional("limits", ENCODING_LIMITS_DEFAULT),
            optional("canonical", CoreDefault::Bool(false)),
        ]),
        ("core.encoding.json", "canonical") if super::super::Edition::edition_at_least("2027") => {
            Some(vec![
                required("data"),
                optional("limits", ENCODING_LIMITS_DEFAULT),
            ])
        }
        ("core.encoding.jsonl" | "core.encoding.csv" | "core.encoding.cbor", "writer") => {
            Some(vec![
                required("target"),
                optional("limits", ENCODING_LIMITS_DEFAULT),
            ])
        }
        ("core.mod", "load") => Some(vec![
            required("path"),
            CoreParam {
                label: "grant",
                zone: crate::AST::ParamZone::LabelOnly,
                default: None,
            },
        ]),
        ("core.db", "pool") => Some(vec![
            required("url"),
            CoreParam {
                label: "max",
                zone: crate::AST::ParamZone::LabelOnly,
                default: None,
            },
        ]),
        _ => None,
    }
}
/// Apply a Core call's table-defined defaults before the pre-sema comptime
/// evaluator runs. This uses the same binder and `CoreParam` contract as the
/// ordinary sema path, so a folded call receives the same fixed argument list.
pub(crate) fn apply_core_call_defaults(
    module: &str,
    name: &str,
    args: &mut Vec<crate::AST::CallArg>,
    call_span: crate::Diagnostics::Span,
) -> bool {
    let Some(contract) = core_param_contract(module, name) else {
        return false;
    };
    let params: Vec<crate::Sema::CallBinder::BindParam<'_>> = contract
        .iter()
        .map(|param| crate::Sema::CallBinder::BindParam {
            label: param.label,
            name: param.label,
            zone: param.zone,
            default: None,
            convention: crate::AST::AccessConvention::Read,
            ty: None,
            variadic: false,
            core_default: param.default,
        })
        .collect();
    let mut ignored_diagnostics = Vec::new();
    crate::Sema::CallBinder::bind_call_args(
        name,
        &params,
        args,
        call_span,
        &mut ignored_diagnostics,
    )
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::{core_fixed_sig, result_ty};
    use crate::AST::Type;

    #[test]
    fn ui_host_result_signatures_are_structural_results() {
        let ui_error = Type::Named("UiHostError".to_string());
        let expected = [
            ("core.ui.host", "file_filter", Type::Named("UiFileFilter".to_string())),
            ("core.ui.host", "fs_grant", Type::Named("UiFsGrant".to_string())),
            (
                "core.ui.host",
                "open_file",
                Type::Named("UiFileDialogSelection".to_string()),
            ),
            ("core.ui.host", "shortcut", Type::Named("UiShortcut".to_string())),
            (
                "core.ui.host",
                "accessibility",
                Type::Named("UiAccessibility".to_string()),
            ),
            (
                "core.ui.host.clipboard",
                "read_text",
                Type::Named("UiClipboardText".to_string()),
            ),
            (
                "core.ui.host.clipboard",
                "write_text",
                Type::Named("UiClipboardWrite".to_string()),
            ),
            (
                "core.ui.host.ime",
                "poll",
                Type::Option(Box::new(Type::Named("UiImeEvent".to_string()))),
            ),
            (
                "core.ui.host.drag_drop",
                "poll",
                Type::Option(Box::new(Type::Named("UiDragEvent".to_string()))),
            ),
            (
                "core.ui.host.shortcuts",
                "binding",
                Type::Named("UiShortcutBinding".to_string()),
            ),
            (
                "core.ui.host.shortcuts",
                "register",
                Type::Named("UiShortcutBinding".to_string()),
            ),
            (
                "core.ui.host.shortcuts",
                "dispatch",
                Type::Named("UiShortcutDispatch".to_string()),
            ),
            (
                "core.ui.host.accessibility",
                "attach",
                Type::Named("Unit".to_string()),
            ),
            (
                "core.ui.host.accessibility",
                "project",
                Type::Option(Box::new(Type::Named(
                    "UiAccessibilityProjection".to_string(),
                ))),
            ),
        ];
        for (module, name, ok) in expected {
            let (_, ret) = core_fixed_sig(module, name).expect("UI host row must have a signature");
            assert_eq!(ret, Some(result_ty(ok, ui_error.clone())), "{module}::{name}");
        }
    }
}
