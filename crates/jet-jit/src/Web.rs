//! D-WEBAPP1 / c-devserver / D-FLAGSHIP-WEBAPI1: resident-JIT web host.
//! Thin opaque-handle adapters over Prelude/App.rs + DevServer.rs.
//! `core.web.on` / `core.web.value` are native no-ops (match AOT emit).

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;

pub(crate) mod web_rt {
    /// The active resident runtime is the only policy carrier. Querying it at
    /// the Prelude hook keeps the policy invocation-local and avoids a stale
    /// caller-thread TLS value leaking into a later run or HTTP worker.
    pub(crate) fn jet_web_runtime_devtools_enabled() -> bool {
        crate::Concurrency::with_runtime_mut(|runtime| runtime.web_runtime_devtools_enabled())
    }

    fn jet_web_runtime_history_enabled() -> bool {
        crate::Concurrency::with_runtime_mut(|runtime| runtime.web_runtime_history_enabled())
    }
    fn jet_runtime_stop(code: &'static str, _file: &str, line: u32, message: &str) -> ! {
        crate::runtime_host::runtime_stop_unwind(code, line, message)
    }

    use jet_foundation::Devtools::jet_devtools_publish_event;
    pub(crate) use crate::Reactive::{
        JetWebMutationState, JetWebMutationStatus, JetWebQuery, JetWebQueryNetworkMode,
        JetWebQueryState, JetWebQueryStatus, jet_web_query_live,
        jet_web_query_subscribe, jet_web_query_invalidate, jet_web_query_get,
        jet_web_query_state, jet_web_query_state_signal, jet_web_query_mutation_signal,
        jet_web_query_show, jet_web_query_facts, jet_web_query_queue, jet_web_query_refresh,
        jet_web_query_cancel, jet_web_query_set_online, jet_web_query_set_mode,
        jet_web_query_mutation_state, jet_web_query_mutate,
        jet_web_query_mutate_with_invalidations, jet_web_query_retry,
    };
    pub(crate) use crate::net_http_rt::{
        console_http_router_from_mux, jet_app_http_assets, jet_app_http_mount,
        jet_app_http_mux_new, jet_app_http_page_response, jet_app_http_reload,
        jet_app_http_request_url, jet_app_http_serve, jet_std, jet_web_server_fn_new,
        jet_web_server_fn_register_http, jet_web_server_fn_typed, ConsoleHttpRouter, JetHTTPMux,
        JetHTTPRequest, JetHTTPRouter, JetWebServerFnError, JetWebServerFnHandler,
        JetWebServerFunction,
    };
    // The keyed live-query entries WebTable.rs names and the query-panel
    // projections App.rs names live with the resident reactive host, which
    // includes the one LiveQuery.rs / WebQuery.rs source.
    pub(crate) use crate::Reactive::{
        jet_live_publish_transport,
        jet_app_invalidate_key, jet_app_live_keyed, jet_web_query_dev_panel,
        jet_web_query_facts_json,
    };
    // EncodingTraits.rs' map codec, inline-range refinement, and CSV rows are
    // the same kernels the collection, runtime, and encoding hosts compile.
    pub(crate) use crate::Collections::collection_semantics::JetMap;
    pub(crate) use crate::runtime_host::inline_range_kernel::jet_inline_range_from_int;
    fn jet_ring_csv_parse(
        text: &String,
        delimiter: &String,
        header: bool,
        skip_blank: bool,
    ) -> Result<Vec<Vec<String>>, String> {
        crate::Encoding::csv_parse(text, delimiter, header, skip_blank)
            .map(|rows| rows.into_iter().map(|row| row.fields).collect())
    }
    // EncodingTraits.rs names these unqualified; the canonical definitions
    // are the JIT modules reexported through the net_http_rt jet_std shim.
    #[allow(unused_imports)]
    pub(crate) use jet_std::{
        jet_codec_date_decode, jet_codec_date_encode, jet_codec_datetime_decode,
        jet_codec_datetime_encode, jet_codec_decimal_decode_int, jet_codec_decimal_decode_text,
        jet_codec_decimal_encode, jet_codec_duration_decode, jet_codec_duration_encode,
        jet_codec_local_time_decode, jet_codec_local_time_encode, JetDate, JetDateTime,
        JetDecimal, JetLocalTime,
    };
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/EncodingTraits.rs");
    const MAX_SYNC_TEXT: usize = 1024 * 1024;
    const MAX_SYNC_REPLICAS: usize = 4096;
    const MAX_SYNC_ENTRIES: usize = 100_000;
    const MAX_SYNC_SESSION: usize = 256;
    const MAX_SYNC_DOCUMENT: usize = 4 * 1024 * 1024;
    fn jet_sync_counter_total(counts: &[(String, u64, u64)]) -> Option<i64> {
        let mut total = 0i128;
        for (_, positive, negative) in counts {
            total = total
                .checked_add(i128::from(*positive))?
                .checked_sub(i128::from(*negative))?;
        }
        i64::try_from(total).ok()
    }
    pub(crate) fn jet_sync_token_is_valid(value: &str) -> bool {
        !value.trim().is_empty()
            && value.len() <= MAX_SYNC_TEXT
            && !value.chars().any(char::is_control)
    }
    include!("../../jet-codegen/src/Prelude/CoreLib/SyncPublish.rs");
    mod jet_app_middleware {
        include!("../../jet-foundation/src/AppMiddleware.rs");
    }
    include!("../../jet-codegen/src/Prelude/App.rs");
    // Browser storage has no native AOT row, so App.rs does not re-export it;
    // the resident host marshals these entries directly. `jet_web_page` is a
    // canonical Core row and reaches every host through the App.rs root.
    pub(crate) use jet_app_impl::{
        jet_web_storage_clear, jet_web_storage_get, jet_web_storage_remove, jet_web_storage_set,
    };
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/HTTPRoute.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/Core/WebPending.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebRouter.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/OpenAPI.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebForms.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebTable.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebVirtual.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebStore.rs");
    include!("../../jet-codegen/src/Prelude/DevServer.rs");
}

#[cfg(test)]
mod tests {
    use super::web_rt::__jet_Decode;
    use jet_foundation::{DataTree::DataTree, Numeric::JetInt};

    #[test]
    fn web_decode_preserves_exact_integers_and_fixed_bounds() {
        for expected in [i64::MIN, i64::MAX] {
            let tree = DataTree::Number(expected.to_string());
            assert_eq!(<i64 as __jet_Decode>::jet_decode(&tree).unwrap(), expected);
        }
        for text in ["-9223372036854775809", "9223372036854775808"] {
            let tree = DataTree::Number(text.to_string());
            assert!(<i64 as __jet_Decode>::jet_decode(&tree).is_err());
            assert_eq!(
                <JetInt as __jet_Decode>::jet_decode(&tree).unwrap().to_string(),
                text,
            );
        }
        let tree = DataTree::Number(u64::MAX.to_string());
        assert_eq!(<u64 as __jet_Decode>::jet_decode(&tree).unwrap(), u64::MAX);
        for text in ["-1", "18446744073709551616"] {
            assert!(
                <u64 as __jet_Decode>::jet_decode(&DataTree::Number(text.to_string()))
                    .is_err()
            );
        }
        let text = "12345678901234567890123456789012345678901234567890";
        assert_eq!(
            <JetInt as __jet_Decode>::jet_decode(&DataTree::Number(text.to_string()))
                .unwrap()
                .to_string(),
            text,
        );
        assert!(<JetInt as __jet_Decode>::jet_decode(&DataTree::Number("1.5".to_string())).is_err());
        assert!(<JetInt as __jet_Decode>::jet_decode(&DataTree::TypedText("12".to_string())).is_err());
    }
}


#[derive(Default)]
pub(crate) struct WebState {
    pub(crate) apps: Vec<web_rt::JetApp>,
    pub(crate) pages: Vec<web_rt::JetWebPage>,
    pub(crate) routers: Vec<web_rt::JetWebRouter>,
    pub(crate) queries: Vec<web_rt::JetWebQuery>,
    pub(crate) forms: Vec<web_rt::JetWebForm>,
    pub(crate) form_inputs: Vec<web_rt::JetWebFormInput>,
    pub(crate) typed_forms: Vec<web_rt::JetWebFormTyped>,
    pub(crate) form_validations: Vec<Option<web_rt::JetWebFormTypedValidation>>,
    pub(crate) form_submissions: Vec<Option<web_rt::JetWebFormTypedSubmission>>,
    pub(crate) tables: Vec<web_rt::JetWebTable<i64>>,
    pub(crate) table_columns: Vec<web_rt::JetWebTableColumn<i64>>,
    pub(crate) stores: Vec<web_rt::JetWebStore<i64>>,
    pub(crate) store_subscriptions: Vec<web_rt::JetWebStoreSubscription>,
    pub(crate) store_patches: Vec<Option<web_rt::JetWebStorePatch<i64>>>,
    pub(crate) servers: Vec<web_rt::JetDevServer>,
    pub(crate) virtual_windows: Vec<web_rt::JetWebVirtualWindow>,
    pub(crate) virtual_plan_viewports: Vec<web_rt::JetWebVirtualPlanViewport>,
}

fn with_rt<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::runtime_host::JitRuntime) -> R,
{
    Concurrency::with_runtime_mut(|rt| Some(f(rt))).expect("Web host requires an active JIT runtime")
}

fn jet_jit_web_on(_sel: i64, _ev: i64, _fn: i64) {
    // Native no-op — real registration is JS/Wasm only (emit/core_calls.rs).
}

fn jet_jit_web_value(_selector: i64) -> i64 {
    with_rt(|rt| rt.heap.alloc_string(String::new()))
}

fn jet_jit_web_app() -> i64 {
    with_rt(|rt| {
        rt.web.apps.push(web_rt::jet_app());
        rt.web.apps.len() as i64
    })
}

fn jet_jit_web_page(title: i64, body: i64) -> i64 {
    with_rt(|rt| {
        let title = rt.heap.clone_string(title).unwrap_or_default();
        let body = rt.heap.clone_string(body).unwrap_or_default();
        rt.web.pages.push(web_rt::jet_web_page(title, body));
        rt.web.pages.len() as i64
    })
}

/// Entry-boundary adapter for an App returned by `fn run`. The operation is
/// still the Prelude App `serve` method; this function only supplies the
/// opaque JIT handle and releases the runtime borrow before the server blocks.
pub(crate) fn serve_app(app: i64) {
    let app_handle = with_rt(|rt| rt.web.apps.get(app.saturating_sub(1) as usize).cloned())
        .expect("jit web app: bad handle");
    app_handle.serve();
}
/// Build the typed console adapter from an assembled application mux without
/// opening a listener. CLI hosts use this to dispatch console requests through
/// the same route graph as `serve`.
pub(crate) fn console_app_router(
    app: i64,
) -> Result<web_rt::ConsoleHttpRouter, String> {
    let app = with_rt(|rt| rt.web.apps.get(app.saturating_sub(1) as usize).cloned());
    let Some(app) = app else {
        return Err("jit web app: bad handle".to_string());
    };
    Ok(web_rt::console_http_router_from_mux(app.console_http_mux()))
}

fn web_route_error_text(rt: &mut crate::runtime_host::JitRuntime, bits: i64) -> String {
    if let Some(message) = rt.heap.clone_string(bits) {
        return message;
    }
    let len = rt.heap.list_len(bits).unwrap_or(0);
    if len == 0 {
        return "typed route callback failed".to_string();
    }
    let mut rendered = Vec::with_capacity(len as usize);
    for index in 0..len {
        let record = rt.heap.list_get_int(bits, index).unwrap_or(0);
        let path = rt
            .heap
            .record_get_string(record, 0)
            .and_then(|id| rt.heap.clone_string(id))
            .unwrap_or_default();
        let reason = rt
            .heap
            .record_get_string(record, 1)
            .and_then(|id| rt.heap.clone_string(id))
            .unwrap_or_else(|| "typed route input is invalid".to_string());
        if path.is_empty() {
            rendered.push(reason);
        } else {
            rendered.push(format!("{path}: {reason}"));
        }
    }
    rendered.join("; ")
}

fn web_route_result(result: i64) -> Result<i64, String> {
    with_rt(|rt| {
        let Some((ok, bits)) = crate::runtime_host::jit_result_parts(rt, result) else {
            return Err("typed route callback returned an invalid result".to_string());
        };
        if ok {
            Ok(bits as i64)
        } else {
            Err(web_route_error_text(rt, bits as i64))
        }
    })
}

fn jet_jit_web_route_decode_tree(tree: i64, type_key: i64) -> i64 {
    let tree = crate::Encoding::read_datatree(tree);
    let type_key = with_rt(|rt| rt.heap.clone_string(type_key));
    let Some(tree) = tree else {
        return with_rt(|rt| {
            let message = rt
                .heap
                .alloc_string("typed route input has an invalid DataTree");
            crate::runtime_host::alloc_jit_result(rt, false, message as u64)
        });
    };
    let Some(type_key) = type_key else {
        return with_rt(|rt| {
            let message = rt
                .heap
                .alloc_string("typed route input has no type identity");
            crate::runtime_host::alloc_jit_result(rt, false, message as u64)
        });
    };
    match crate::Encoding::decode_datatree_for_type(&tree, &type_key) {
        Ok(value) => with_rt(|rt| crate::runtime_host::alloc_jit_result(rt, true, value as u64)),
        Err(errors) => with_rt(|rt| {
            let list = rt.heap.alloc_empty_list();
            for error in errors {
                let record = rt.heap.alloc_record(2);
                let path = rt.heap.alloc_string(error.path);
                let reason = rt.heap.alloc_string(error.reason);
                let _ = rt.heap.record_set_string(record, 0, path);
                let _ = rt.heap.record_set_string(record, 1, reason);
                let _ = rt.heap.list_push_int(list, record);
            }
            crate::runtime_host::alloc_jit_result(rt, false, list as u64)
        }),
    }
}

fn web_route_type_descriptor(
    rt: &crate::runtime_host::JitRuntime,
    type_key: &str,
) -> Option<crate::runtime_host::RuntimeTypeDescriptor> {
    type_key
        .strip_prefix("id:")
        .and_then(|id| id.parse::<u64>().ok())
        .and_then(|id| rt.runtime_type_descriptor(id).cloned())
        .or_else(|| rt.runtime_type_descriptor_by_name(type_key).cloned())
}

enum WebEncodedValue {
    Text(String),
    Tree(crate::Encoding::json_rt::DataTree),
}

fn web_route_encoded_value(
    rt: &mut crate::runtime_host::JitRuntime,
    value: i64,
    type_key: i64,
) -> Result<WebEncodedValue, String> {
    let type_key = rt
        .heap
        .clone_string(type_key)
        .ok_or_else(|| "typed route callback has no result type identity".to_string())?;
    let descriptor = web_route_type_descriptor(rt, &type_key)
        .ok_or_else(|| format!("typed route callback has unknown result type `{type_key}`"))?;
    if matches!(
        descriptor.kind,
        crate::runtime_host::RuntimeValueKind::String
    ) {
        return rt
            .heap
            .clone_string(value)
            .map(WebEncodedValue::Text)
            .ok_or_else(|| "typed route callback returned invalid text".to_string());
    }
    crate::Receipt::encode_jit_value(rt, value, &descriptor)
        .map(WebEncodedValue::Tree)
}

fn web_encoded_text(value: WebEncodedValue) -> Result<String, String> {
    match value {
        WebEncodedValue::Text(value) => Ok(value),
        WebEncodedValue::Tree(tree) => Ok(crate::Encoding::json_rt::render_datatree_json(
            &tree, false, 0,
        )),
    }
}

fn web_result_error(text: String) -> i64 {
    with_rt(|rt| {
        let message = rt.heap.alloc_string(text);
        crate::runtime_host::alloc_jit_result(rt, false, message as u64)
    })
}


fn jet_jit_web_route_encode_result(result: i64, ok_type: i64, err_type: i64) -> i64 {
    let Some((ok, bits)) =
        with_rt(|rt| crate::runtime_host::jit_result_parts(rt, result))
    else {
        return web_result_error("typed route callback returned an invalid result".to_string());
    };
    let type_key = if ok { ok_type } else { err_type };
    let encoded = with_rt(|rt| web_route_encoded_value(rt, bits as i64, type_key))
        .and_then(web_encoded_text);
    match encoded {
        Ok(encoded) => with_rt(|rt| {
            let value = rt.heap.alloc_string(encoded);
            crate::runtime_host::alloc_jit_result(rt, ok, value as u64)
        }),
        Err(error) => web_result_error(error),
    }
}

fn jet_jit_web_route_stringify_error(result: i64, err_type: i64) -> i64 {
    let Some((ok, bits)) =
        with_rt(|rt| crate::runtime_host::jit_result_parts(rt, result))
    else {
        return web_result_error("typed route callback returned an invalid result".to_string());
    };
    if ok {
        return result;
    }
    let encoded = with_rt(|rt| web_route_encoded_value(rt, bits as i64, err_type))
        .and_then(web_encoded_text);
    match encoded {
        Ok(encoded) => with_rt(|rt| {
            let value = rt.heap.alloc_string(encoded);
            crate::runtime_host::alloc_jit_result(rt, false, value as u64)
        }),
        Err(error) => web_result_error(error),
    }
}


fn web_pack_route_trees(trees: &[web_rt::jet_std::DataTree]) -> i64 {
    let handles = trees
        .iter()
        .map(crate::Encoding::alloc_datatree)
        .collect::<Vec<_>>();
    with_rt(|rt| {
        let list = rt.heap.alloc_empty_list();
        for handle in handles {
            let _ = rt.heap.list_push_int(list, handle);
        }
        list
    })
}

/// Invoke a retained route callback on an HTTP worker and resolve its result
/// while the worker still holds the resident runtime.
///
/// Workers are raw OS threads: `try_with_http_jet_runtime_at` pins the
/// published runtime onto the thread only for the closure it runs, so every
/// heap read of the callback's result — unpacking the `Result`, cloning the
/// page or text it names — has to happen inside that same closure.  Reading
/// after it returns runs with no runtime and aborts the request.
fn web_invoke_route_callback<R>(
    callback: Option<(usize, crate::runtime_host::JitCallableSlot)>,
    trees: &[web_rt::jet_std::DataTree],
    resolve: impl FnOnce(&mut crate::runtime_host::JitRuntime, i64) -> Result<R, String>,
) -> Result<R, String> {
    let Some((epoch, callback)) = callback else {
        return Err("typed route callback unavailable".to_string());
    };
    Concurrency::try_with_http_jet_runtime_at(epoch, || {
        let packed = web_pack_route_trees(trees);
        let result = crate::runtime_host::invoke_universal_unary(callback, packed)
            .ok_or_else(|| "typed route callback invocation failed".to_string())?;
        let value = web_route_result(result)?;
        with_rt(|rt| resolve(rt, value))
    })
    .ok_or_else(|| "typed route callback runtime unavailable".to_string())?
}

fn web_page_callback(
    callback: Option<(usize, crate::runtime_host::JitCallableSlot)>,
    trees: &[web_rt::jet_std::DataTree],
) -> Result<web_rt::JetWebPage, String> {
    web_invoke_route_callback(callback, trees, |rt, page| {
        rt.web
            .pages
            .get(page.saturating_sub(1) as usize)
            .cloned()
            .ok_or_else(|| "typed route callback returned an invalid page".to_string())
    })
}

fn web_encoded_callback(
    callback: Option<(usize, crate::runtime_host::JitCallableSlot)>,
    trees: &[web_rt::jet_std::DataTree],
) -> Result<String, String> {
    web_invoke_route_callback(callback, trees, |rt, encoded| {
        rt.heap
            .clone_string(encoded)
            .ok_or_else(|| "typed route callback returned non-text data".to_string())
    })
}

fn jet_jit_web_app_method(
    app: i64,
    method: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    output_type: i64,
    binding: i64,
) -> i64 {
    let method_name = with_rt(|rt| rt.heap.clone_string(method).unwrap_or_default());
    if method_name == "facts_json" {
        return with_rt(|rt| {
            let app_handle = rt
                .web
                .apps
                .get(app.saturating_sub(1) as usize)
                .expect("jit app: bad handle")
                .clone();
            rt.heap.alloc_string(app_handle.facts_json())
        });
    }
    if method_name == "serve" || method_name == "serve_on" {
        let app_handle = with_rt(|rt| rt.web.apps.get(app.saturating_sub(1) as usize).cloned())
            .expect("jit app: bad handle");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if method_name == "serve_on" {
                app_handle.serve_on(a0);
            } else {
                app_handle.serve();
            }
        }));
        if let Err(payload) = result {
            if !payload.is::<crate::runtime_host::JitRuntimeStop>() {
                std::panic::resume_unwind(payload);
            }
        }
        return app;
    }
    let (key, data_type, binding) = with_rt(|rt| {
        (
            rt.heap.clone_string(a0).unwrap_or_default(),
            rt.heap.clone_string(output_type).unwrap_or_default(),
            rt.heap.clone_string(binding).unwrap_or_default(),
        )
    });
    let app_handle = with_rt(|rt| {
        rt.web
            .apps
            .get(app.saturating_sub(1) as usize)
            .cloned()
    })
    .expect("jit web app: bad handle");
    let next = match method_name.as_str() {
        "csr" => app_handle.csr(),
        "ssr" => app_handle.ssr(),
        "ssg" => app_handle.ssg(),
        "stream" => app_handle.stream(),
        "streaming" => app_handle.streaming(),
        "island" => app_handle.island(),
        "hydration_dev" => app_handle.hydration_dev(),
        "hydration_release" => app_handle.hydration_release(),
        "route" | "page" | "layout" => {
            let callback = Concurrency::http_callable_snapshot(a1);
            let arity = web_rt::jet_app_route_binding(&binding).len();
            let handler = web_rt::JetAppTreeRouteCallback::new(
                arity,
                std::sync::Arc::new(move |trees| web_page_callback(callback, trees)),
            );
            match method_name.as_str() {
                "route" => web_rt::jet_app_route(&app_handle, key, handler, binding),
                "page" => web_rt::jet_app_page(&app_handle, key, handler, binding),
                _ => web_rt::jet_app_layout(&app_handle, key, handler, binding),
            }
        }
        "loader" => {
            let callback = Concurrency::http_callable_snapshot(a1);
            let binding = web_rt::jet_app_route_binding(&binding);
            // `loader_encoded` takes the encoded result as the route output,
            // so the tree callback's own failure channel stays empty.
            let handler = web_rt::JetAppTreeRouteCallback::new(
                binding.len(),
                std::sync::Arc::new(move |trees| Ok(web_encoded_callback(callback, trees))),
            );
            app_handle.loader_encoded(handler, binding, data_type, a2 != 0)
        }
        "pending" | "not_found" | "error" => {
            let callback = Concurrency::http_callable_snapshot(a0);
            let handler: std::sync::Arc<
                dyn Fn() -> Result<web_rt::JetWebPage, String> + Send + Sync + 'static,
            > = std::sync::Arc::new(move || web_page_callback(callback, &[]));
            match method_name.as_str() {
                "pending" => app_handle.pending(handler),
                "not_found" => app_handle.not_found(handler),
                _ => app_handle.error(handler),
            }
        }
        "action" | "form" | "data" => {
            let callback = Concurrency::http_callable_snapshot(a1);
            if a2 == 0 {
                let handler: std::sync::Arc<
                    dyn Fn() -> Result<String, String> + Send + Sync + 'static,
                > = std::sync::Arc::new(move || web_encoded_callback(callback, &[]));
                match method_name.as_str() {
                    "action" => app_handle.action(key, handler),
                    "form" => app_handle.form(key, handler),
                    _ => app_handle.data(key, handler),
                }
            } else {
                let handler: std::sync::Arc<
                    dyn Fn(&String) -> Result<String, String> + Send + Sync + 'static,
                > = std::sync::Arc::new(move |body: &String| {
                    web_encoded_callback(
                        callback,
                        &[web_rt::jet_std::DataTree::Text(body.clone())],
                    )
                });
                match method_name.as_str() {
                    "action" => app_handle.action(key, handler),
                    "form" => app_handle.form(key, handler),
                    _ => app_handle.data(key, handler),
                }
            }
        }
        "mount" => {
            let callback = Concurrency::http_callable_snapshot(a1);
            app_handle.mount(
                key,
                move |path: &String| {
                    if let Some((epoch, callback)) = callback {
                        let _ = Concurrency::try_with_http_jet_runtime_at(epoch, || {
                            let path = with_rt(|rt| rt.heap.alloc_string(path.clone()));
                            unsafe {
                                web_invoke_unit(callback, path);
                            }
                        });
                    }
                },
            )
        }
        "routes" => app_handle.routes(key),
        "security" => app_handle.security(key),
        "assets" => app_handle.assets(key),
        "split" => app_handle.split(key),
        "code_split" => app_handle.code_split(key),
        "cache" => app_handle.cache(key),
        "a11y" => app_handle.a11y(key),
        "adapter" => app_handle.adapter(key),
        _ => app_handle,
    };
    with_rt(|rt| {
        rt.web.apps.push(next);
        rt.web.apps.len() as i64
    })
}
fn jet_jit_app_sync(doc_show: i64, session_id: i64) -> i64 {
    let (doc_show, session_id) = with_rt(|rt| {
        (
            rt.heap.clone_string(doc_show).unwrap_or_default(),
            rt.heap.clone_string(session_id).unwrap_or_default(),
        )
    });
    let shown = web_rt::jet_app_sync(doc_show, session_id);
    with_rt(|rt| rt.heap.alloc_string(shown))
}


fn jet_jit_devserver_app() -> i64 {
    with_rt(|rt| {
        let server = web_rt::jet_devserver_app();
        rt.web.servers.push(server);
        rt.web.servers.len() as i64
    })
}

fn jet_jit_devserver_for_app(path: i64) -> i64 {
    with_rt(|rt| {
        let path = rt.heap.clone_string(path).unwrap_or_default();
        rt.web.servers.push(web_rt::jet_devserver_for_app(&path));
        rt.web.servers.len() as i64
    })
}

fn jet_jit_devserver_html(server: i64, path: i64) -> i64 {
    with_rt(|rt| {
        let path = rt.heap.clone_string(path).unwrap_or_default();
        let next = rt
            .web
            .servers
            .get(server.saturating_sub(1) as usize)
            .expect("jit devserver html: bad handle")
            .html(path);
        rt.web.servers.push(next);
        rt.web.servers.len() as i64
    })
}

fn jet_jit_devserver_port(server: i64, port: i64) -> i64 {
    with_rt(|rt| {
        let next = rt
            .web
            .servers
            .get(server.saturating_sub(1) as usize)
            .expect("jit devserver port: bad handle")
            .port(port);
        rt.web.servers.push(next);
        rt.web.servers.len() as i64
    })
}

fn jet_jit_devserver_serve(server: i64) {
    // Do not block forever in JIT/AOT ProgramOutput tests — `fn run()` never
    // calls serve; compiling `fn dev()` only needs the symbol to exist.
    let _ = server;
}

fn jet_jit_web_storage_get(key: i64) -> i64 {
    let key = with_rt(|rt| rt.heap.clone_string(key).unwrap_or_default());
    match web_rt::jet_web_storage_get(&key) {
        Some(value) => with_rt(|rt| rt.heap.alloc_string(value)),
        None => 0,
    }
}

fn jet_jit_web_storage_remove(key: i64) {
    let key = with_rt(|rt| rt.heap.clone_string(key).unwrap_or_default());
    web_rt::jet_web_storage_remove(&key);
}
fn jet_jit_web_storage_set(key: i64, value: i64) {
    let (key, value) = with_rt(|rt| {
        (
            rt.heap.clone_string(key).unwrap_or_default(),
            rt.heap.clone_string(value).unwrap_or_default(),
        )
    });
    web_rt::jet_web_storage_set(&key, &value);
}

fn jet_jit_web_storage_clear() {
    web_rt::jet_web_storage_clear();
}

fn web_int_value(rt: &crate::runtime_host::JitRuntime, value: i64) -> i64 {
    rt.heap.int_to_i64(value).unwrap_or(value)
}

fn web_int_handle(rt: &mut crate::runtime_host::JitRuntime, value: i64) -> i64 {
    rt.heap.int_from_i64(value)
}

fn web_string_map(
    rt: &mut crate::runtime_host::JitRuntime,
    map: i64,
) -> Option<std::collections::BTreeMap<String, String>> {
    let count = rt.heap.map_len(map)?;
    let mut result = std::collections::BTreeMap::new();
    for index in 0..count {
        let key = rt.heap.map_key_at(map, index)?;
        let value = rt.heap.map_value_at(map, index)?;
        result.insert(rt.heap.clone_string(key)?, rt.heap.clone_string(value)?);
    }
    Some(result)
}

fn web_map_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    values: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
) -> i64 {
    let map = rt.heap.alloc_empty_map();
    for (key, value) in values {
        let key = rt.heap.alloc_string(key.into());
        let value = rt.heap.alloc_string(value.into());
        let _ = rt.heap.map_insert(map, key, value);
    }
    map
}

fn web_raw_list(rt: &crate::runtime_host::JitRuntime, list: i64) -> Option<Vec<i64>> {
    let count = rt.heap.list_len(list)?;
    (0..count).map(|index| rt.heap.list_get_int(list, index)).collect()
}

fn web_string_list_value(rt: &crate::runtime_host::JitRuntime, list: i64) -> Vec<String> {
    let count = rt.heap.list_len(list).expect("checked Web string list");
    (0..count).map(|index| {
        rt.heap.list_get_string(list, index).expect("checked Web string list element")
    }).collect()
}

fn web_result_handle<T>(
    result: Result<T, String>,
    encode: impl FnOnce(&mut crate::runtime_host::JitRuntime, T) -> i64,
) -> i64 {
    with_rt(|rt| match result {
        Ok(value) => {
            let value = encode(rt, value);
            crate::runtime_host::alloc_jit_result(rt, true, value as u64)
        }
        Err(error) => {
            let error = rt.heap.alloc_string(error);
            crate::runtime_host::alloc_jit_result(rt, false, error as u64)
        }
    })
}

fn web_raw_list_handle(rt: &mut crate::runtime_host::JitRuntime, values: Vec<i64>) -> i64 {
    let list = rt.heap.alloc_empty_list();
    for value in values {
        let _ = rt.heap.list_push_int(list, value);
    }
    list
}
fn web_string_list_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    values: impl IntoIterator<Item = impl Into<String>>,
) -> i64 {
    let list = rt.heap.alloc_empty_list();
    for value in values {
        let value = rt.heap.alloc_string(value.into());
        let _ = rt.heap.list_push_int(list, value);
    }
    list
}

fn web_enum_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    discriminant: i64,
) -> i64 {
    let value = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(value, 0, discriminant);
    value
}

fn web_navigation_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    navigation: &web_rt::JetWebNavigation,
) -> i64 {
    let value = rt.heap.alloc_record(4);
    let url = rt.heap.alloc_string(navigation.url.clone());
    let route = rt.heap.alloc_string(navigation.route.clone());
    let params = web_map_handle(rt, &navigation.params);
    let search = web_map_handle(rt, &navigation.search);
    let _ = rt.heap.record_set_string(value, 0, url);
    let _ = rt.heap.record_set_string(value, 1, route);
    let _ = rt.heap.record_set_int(value, 2, params);
    let _ = rt.heap.record_set_int(value, 3, search);
    value
}

fn web_navigation_status(status: web_rt::JetWebNavigationStatus) -> i64 {
    match status {
        web_rt::JetWebNavigationStatus::Idle => 0,
        web_rt::JetWebNavigationStatus::Preloading => 1,
        web_rt::JetWebNavigationStatus::Pending => 2,
        web_rt::JetWebNavigationStatus::Ready => 3,
        web_rt::JetWebNavigationStatus::Error => 4,
        web_rt::JetWebNavigationStatus::Aborted => 5,
    }
}

fn web_optional_string_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    value: Option<String>,
) -> i64 {
    match value {
        Some(value) => {
            let value = rt.heap.alloc_string(value);
            crate::runtime_host::alloc_jit_result(rt, true, value as u64)
        }
        None => crate::runtime_host::alloc_jit_result(rt, false, 0),
    }
}

fn web_navigation_state_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    state: &web_rt::JetWebNavigationState,
) -> i64 {
    let value = rt.heap.alloc_record(8);
    let status = web_enum_handle(rt, web_navigation_status(state.status));
    let current = match state.current.as_ref() {
        Some(navigation) => {
            let navigation = web_navigation_handle(rt, navigation);
            crate::runtime_host::alloc_jit_result(rt, true, navigation as u64)
        }
        None => crate::runtime_host::alloc_jit_result(rt, false, 0),
    };
    let data = rt.heap.alloc_string(state.data.clone());
    let error = rt.heap.alloc_string(state.error.clone());
    let pending_boundary_id = web_optional_string_handle(
        rt,
        state
            .pending_boundary_id
            .as_ref()
            .map(|boundary| boundary.as_str().to_string()),
    );
    let error_boundary_id = web_optional_string_handle(
        rt,
        state
            .error_boundary_id
            .as_ref()
            .map(|boundary| boundary.as_str().to_string()),
    );
    let island_identity = web_optional_string_handle(
        rt,
        state.island_identity.as_ref().map(|identity| identity.compact()),
    );
    let hydration_trigger = web_optional_string_handle(
        rt,
        state
            .hydration_trigger
            .map(|trigger| trigger.as_str().to_string()),
    );
    let _ = rt.heap.record_set_int(value, 0, status);
    let _ = rt.heap.record_set_int(value, 1, current);
    let _ = rt.heap.record_set_string(value, 2, data);
    let _ = rt.heap.record_set_string(value, 3, error);
    let _ = rt.heap.record_set_int(value, 4, pending_boundary_id);
    let _ = rt.heap.record_set_int(value, 5, error_boundary_id);
    let _ = rt.heap.record_set_int(value, 6, island_identity);
    let _ = rt.heap.record_set_int(value, 7, hydration_trigger);
    value
}
fn web_router_cache_status(status: web_rt::JetWebRouterCacheStatus) -> i64 {
    match status {
        web_rt::JetWebRouterCacheStatus::Fresh => 0,
        web_rt::JetWebRouterCacheStatus::Stale => 1,
        web_rt::JetWebRouterCacheStatus::Invalidated => 2,
        web_rt::JetWebRouterCacheStatus::Collected => 3,
    }
}

fn web_router_cache_state_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    state: &web_rt::JetWebRouterCacheState,
) -> i64 {
    let value = rt.heap.alloc_record(5);
    let identity = rt.heap.alloc_string(state.identity.clone());
    let status = web_enum_handle(rt, web_router_cache_status(state.status));
    let data = rt.heap.alloc_string(state.data.clone());
    let dependencies = web_string_list_handle(rt, &state.dependencies);
    let generation = web_int_handle(rt, state.generation as i64);
    let _ = rt.heap.record_set_string(value, 0, identity);
    let _ = rt.heap.record_set_int(value, 1, status);
    let _ = rt.heap.record_set_string(value, 2, data);
    let _ = rt.heap.record_set_int(value, 3, dependencies);
    let _ = rt.heap.record_set_int(value, 4, generation);
    value
}

fn web_mutation_status(status: web_rt::JetWebMutationStatus) -> i64 {
    match status {
        web_rt::JetWebMutationStatus::Idle => 0,
        web_rt::JetWebMutationStatus::Pending => 1,
        web_rt::JetWebMutationStatus::Success => 2,
        web_rt::JetWebMutationStatus::Error => 3,
        web_rt::JetWebMutationStatus::Settled => 4,
    }
}

fn web_mutation_state_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    state: web_rt::JetWebMutationState,
) -> i64 {
    let value = rt.heap.alloc_record(11);
    let status = web_enum_handle(rt, web_mutation_status(state.status));
    let generation = web_int_handle(rt, state.generation as i64);
    let optimistic = state.optimistic;
    let mutation_value = rt.heap.alloc_string(state.value);
    let result = rt.heap.alloc_string(state.result);
    let error = rt.heap.alloc_string(state.error);
    let context = rt.heap.alloc_string(state.context);
    let rollback = state.rollback;
    let paused = state.paused;
    let queued = web_int_handle(rt, state.queued as i64);
    let replayed = web_int_handle(rt, state.replayed as i64);
    let _ = rt.heap.record_set_int(value, 0, status);
    let _ = rt.heap.record_set_int(value, 1, generation);
    let _ = rt.heap.record_set_bool(value, 2, optimistic);
    let _ = rt.heap.record_set_string(value, 3, mutation_value);
    let _ = rt.heap.record_set_string(value, 4, result);
    let _ = rt.heap.record_set_string(value, 5, error);
    let _ = rt.heap.record_set_string(value, 6, context);
    let _ = rt.heap.record_set_bool(value, 7, rollback);
    let _ = rt.heap.record_set_bool(value, 8, paused);
    let _ = rt.heap.record_set_int(value, 9, queued);
    let _ = rt.heap.record_set_int(value, 10, replayed);
    value
}

fn web_query_status(status: web_rt::JetWebQueryStatus) -> i64 {
    match status {
        web_rt::JetWebQueryStatus::Pending => 0,
        web_rt::JetWebQueryStatus::Fresh => 1,
        web_rt::JetWebQueryStatus::Stale => 2,
        web_rt::JetWebQueryStatus::Fetching => 3,
        web_rt::JetWebQueryStatus::Error => 4,
        web_rt::JetWebQueryStatus::Offline => 5,
    }
}
fn web_query_mode(
    rt: &crate::runtime_host::JitRuntime,
    mode: i64,
) -> web_rt::JetWebQueryNetworkMode {
    match rt.heap.record_get_int(mode, 0).unwrap_or(0) {
        1 => web_rt::JetWebQueryNetworkMode::Always,
        2 => web_rt::JetWebQueryNetworkMode::OfflineFirst,
        _ => web_rt::JetWebQueryNetworkMode::Online,
    }
}


fn web_query_state_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    state: web_rt::JetWebQueryState,
) -> i64 {
    let value = rt.heap.alloc_record(6);
    let status = web_enum_handle(rt, web_query_status(state.status));
    let query_value = rt.heap.alloc_string(state.value);
    let error = rt.heap.alloc_string(state.error);
    let generation = web_int_handle(rt, state.generation as i64);
    let queued = web_int_handle(rt, state.queued as i64);
    let _ = rt.heap.record_set_int(value, 0, status);
    let _ = rt.heap.record_set_string(value, 1, query_value);
    let _ = rt.heap.record_set_string(value, 2, error);
    let _ = rt.heap.record_set_int(value, 3, generation);
    let _ = rt.heap.record_set_int(value, 4, queued);
    let _ = rt.heap.record_set_bool(value, 5, state.offline);
    value
}

fn web_virtual_window_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    window: &web_rt::JetWebVirtualWindow,
) -> i64 {
    let value = rt.heap.alloc_record(9);
    let fields = [
        window.total_count,
        window.scroll_offset,
        window.viewport_size,
        window.estimated_item_size,
        window.overscan,
        window.start,
        window.end,
        window.total_size,
        window.measured_count,
    ];
    for (index, field) in fields.into_iter().enumerate() {
        let field = web_int_handle(rt, field);
        let _ = rt.heap.record_set_int(value, index as i64, field);
    }
    value
}

fn web_virtual_window_value(
    rt: &crate::runtime_host::JitRuntime,
    value: i64,
) -> Option<web_rt::JetWebVirtualWindow> {
    let mut fields = [0_i64; 9];
    for (index, field) in fields.iter_mut().enumerate() {
        let raw = rt.heap.record_get_int(value, index as i64)?;
        *field = web_int_value(rt, raw);
    }
    Some(web_rt::JetWebVirtualWindow {
        total_count: fields[0],
        scroll_offset: fields[1],
        viewport_size: fields[2],
        estimated_item_size: fields[3],
        overscan: fields[4],
        start: fields[5],
        end: fields[6],
        total_size: fields[7],
        measured_count: fields[8],
    })
}
fn jet_jit_web_virtual_window_facts(window: i64) -> i64 {
    with_rt(|rt| {
        let facts = web_virtual_window_value(rt, window)
            .map(|window| window.facts_json())
            .unwrap_or_default();
        rt.heap.alloc_string(facts)
    })
}

fn web_virtual_plan_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    plan: &web_rt::JetWebVirtualPlan,
) -> i64 {
    let value = rt.heap.alloc_record(13);
    let fields = [
        plan.total_count, plan.scroll_offset, plan.viewport_width, plan.viewport_height,
        plan.estimated_item_size, plan.overscan, plan.start, plan.end, plan.total_size,
        plan.measured_count, plan.anchor_index, plan.anchor_offset,
    ];
    for (index, field) in fields.into_iter().enumerate() {
        let field = web_int_handle(rt, field);
        let _ = rt.heap.record_set_int(value, index as i64, field);
    }
    let measurements = rt.heap.alloc_empty_list();
    for (index, size) in &plan.measurements {
        let measurement = rt.heap.alloc_record(2);
        let index = web_int_handle(rt, *index);
        let size = web_int_handle(rt, *size);
        let _ = rt.heap.record_set_int(measurement, 0, index);
        let _ = rt.heap.record_set_int(measurement, 1, size);
        let _ = rt.heap.list_push_int(measurements, measurement);
    }
    let _ = rt.heap.record_set_int(value, 12, measurements);
    value
}

fn web_virtual_measurements(
    rt: &crate::runtime_host::JitRuntime,
    list: i64,
) -> Option<Vec<(i64, i64)>> {
    (0..rt.heap.list_len(list)?).map(|index| {
        let measurement = rt.heap.list_get_int(list, index)?;
        let index = rt.heap.record_get_int(measurement, 0)?;
        let size = rt.heap.record_get_int(measurement, 1)?;
        Some((web_int_value(rt, index), web_int_value(rt, size)))
    }).collect()
}

fn web_virtual_plan_value(
    rt: &crate::runtime_host::JitRuntime,
    value: i64,
) -> Option<web_rt::JetWebVirtualPlan> {
    let mut fields = [0_i64; 12];
    for (index, field) in fields.iter_mut().enumerate() {
        *field = web_int_value(rt, rt.heap.record_get_int(value, index as i64)?);
    }
    Some(web_rt::JetWebVirtualPlan {
        total_count: fields[0],
        scroll_offset: fields[1],
        viewport_width: fields[2],
        viewport_height: fields[3],
        estimated_item_size: fields[4],
        overscan: fields[5],
        start: fields[6],
        end: fields[7],
        total_size: fields[8],
        measured_count: fields[9],
        anchor_index: fields[10],
        anchor_offset: fields[11],
        measurements: web_virtual_measurements(rt, rt.heap.record_get_int(value, 12)?)?,
    })
}

fn jet_jit_web_virtual_plan(
    total: i64, offset: i64, width: i64, height: i64, estimate: i64, overscan: i64,
) -> i64 {
    with_rt(|rt| {
        let [total, offset, width, height, estimate, overscan] =
            [total, offset, width, height, estimate, overscan].map(|raw| web_int_value(rt, raw));
        let plan = web_rt::jet_web_virtual_plan(total, offset, width, height, estimate, overscan);
        web_virtual_plan_handle(rt, &plan)
    })
}

fn jet_jit_web_virtual_plan_measured(
    total: i64, offset: i64, width: i64, height: i64, estimate: i64, overscan: i64,
    measurements: i64,
) -> i64 {
    with_rt(|rt| {
        let [total, offset, width, height, estimate, overscan] =
            [total, offset, width, height, estimate, overscan].map(|raw| web_int_value(rt, raw));
        let measurements = web_virtual_measurements(rt, measurements)
            .expect("Web virtual measurements require checked row carriers");
        let plan = web_rt::jet_web_virtual_plan_measured(
            total, offset, width, height, estimate, overscan, &measurements,
        );
        web_virtual_plan_handle(rt, &plan)
    })
}

fn jet_jit_web_virtual_plan_from_sizes(
    total: i64, offset: i64, width: i64, height: i64, estimate: i64, overscan: i64,
    sizes: i64,
) -> i64 {
    with_rt(|rt| {
        let [total, offset, width, height, estimate, overscan] =
            [total, offset, width, height, estimate, overscan].map(|raw| web_int_value(rt, raw));
        let sizes = web_raw_list(rt, sizes)
            .expect("Web virtual sizes require a checked Int list")
            .into_iter().map(|raw| web_int_value(rt, raw)).collect();
        let plan = web_rt::jet_web_virtual_plan_from_sizes(
            total, offset, width, height, estimate, overscan, &sizes,
        );
        web_virtual_plan_handle(rt, &plan)
    })
}

fn jet_jit_web_virtual_plan_measure(plan: i64, index: i64, size: i64) -> i64 {
    with_rt(|rt| {
        let plan = web_virtual_plan_value(rt, plan)
            .expect("Web virtual plan requires its checked record carrier");
        let plan = web_rt::jet_web_virtual_plan_measure(
            &plan, web_int_value(rt, index), web_int_value(rt, size),
        );
        web_virtual_plan_handle(rt, &plan)
    })
}

fn jet_jit_web_virtual_plan_slice(rows: i64, plan: i64) -> i64 {
    with_rt(|rt| {
        let plan = web_virtual_plan_value(rt, plan)
            .expect("Web virtual plan requires its checked record carrier");
        let rows = rt.heap.list_value(rows)
            .expect("Web virtual rows require an initialized list carrier");
        let values = match rows {
            jet_rt::JetVal::IntList(rows) => {
                let values = web_rt::jet_web_virtual_plan_slice(rows, &plan);
                return rt.heap.alloc_int_list(values);
            }
            jet_rt::JetVal::List(rows) | jet_rt::JetVal::UninitList { values: rows, .. } =>
                web_rt::jet_web_virtual_plan_slice(rows, &plan),
            _ => unreachable!("list_value only returns initialized list carriers"),
        };
        rt.heap.alloc_list_values(values)
    })
}

fn jet_jit_web_virtual_plan_indices(plan: i64) -> i64 {
    with_rt(|rt| {
        let plan = web_virtual_plan_value(rt, plan)
            .expect("Web virtual plan requires its checked record carrier");
        let values = web_rt::jet_web_virtual_plan_indices(&plan).into_iter()
            .map(|index| web_int_handle(rt, index)).collect();
        web_raw_list_handle(rt, values)
    })
}

fn jet_jit_web_virtual_plan_facts(plan: i64) -> i64 {
    with_rt(|rt| {
        let plan = web_virtual_plan_value(rt, plan)
            .expect("Web virtual plan requires its checked record carrier");
        rt.heap.alloc_string(web_rt::jet_web_virtual_plan_facts(&plan))
    })
}

fn web_virtual_plan_viewport(handle: i64) -> web_rt::JetWebVirtualPlanViewport {
    with_rt(|rt| rt.web.virtual_plan_viewports.get(handle.wrapping_sub(1) as usize).cloned())
        .expect("Web virtual viewport requires its registered owner")
}

fn jet_jit_web_virtual_plan_viewport(plan: i64) -> i64 {
    let plan = with_rt(|rt| web_virtual_plan_value(rt, plan))
        .expect("Web virtual plan requires its checked record carrier");
    let viewport = web_rt::jet_web_virtual_plan_viewport(plan);
    with_rt(|rt| {
        rt.web.virtual_plan_viewports.push(viewport);
        rt.web.virtual_plan_viewports.len() as i64
    })
}

fn jet_jit_web_virtual_plan_viewport_state(viewport: i64) -> i64 {
    let plan = web_rt::jet_web_virtual_plan_viewport_state(&web_virtual_plan_viewport(viewport));
    with_rt(|rt| web_virtual_plan_handle(rt, &plan))
}

fn jet_jit_web_virtual_plan_scroll_to(viewport: i64, offset: i64) {
    let viewport = web_virtual_plan_viewport(viewport);
    let offset = with_rt(|rt| web_int_value(rt, offset));
    web_rt::jet_web_virtual_plan_scroll_to(&viewport, offset);
}

fn jet_jit_web_virtual_plan_resize(viewport: i64, width: i64, height: i64) {
    let viewport = web_virtual_plan_viewport(viewport);
    let [width, height] = with_rt(|rt| [width, height].map(|raw| web_int_value(rt, raw)));
    web_rt::jet_web_virtual_plan_resize(&viewport, width, height);
}

fn jet_jit_web_virtual_plan_viewport_measure(viewport: i64, index: i64, size: i64) {
    let viewport = web_virtual_plan_viewport(viewport);
    let [index, size] = with_rt(|rt| [index, size].map(|raw| web_int_value(rt, raw)));
    web_rt::jet_web_virtual_plan_viewport_measure(&viewport, index, size);
}

fn web_table_page_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    page: &web_rt::JetWebTablePage<i64>,
) -> i64 {
    let value = rt.heap.alloc_record(6);
    let rows = web_raw_list_handle(rt, page.rows.clone());
    let row_keys = web_string_list_handle(rt, &page.row_keys);
    let total_rows = web_int_handle(rt, page.total_rows);
    let page_index = web_int_handle(rt, page.page_index);
    let page_size = web_int_handle(rt, page.page_size);
    let page_count = web_int_handle(rt, page.page_count);
    let _ = rt.heap.record_set_int(value, 0, rows);
    let _ = rt.heap.record_set_int(value, 1, row_keys);
    let _ = rt.heap.record_set_int(value, 2, total_rows);
    let _ = rt.heap.record_set_int(value, 3, page_index);
    let _ = rt.heap.record_set_int(value, 4, page_size);
    let _ = rt.heap.record_set_int(value, 5, page_count);
    value
}

fn web_callable(handle: i64) -> Option<crate::runtime_host::JitCallableSlot> {
    with_rt(|rt| crate::runtime_host::jit_callable_parts(rt, handle))
}

fn web_invoke_string(
    slot: crate::runtime_host::JitCallableSlot,
    value: i64,
) -> String {
    let result = crate::runtime_host::invoke_universal_unary(slot, value)
        .expect("Web callback requires its checked unary thunk");
    with_rt(|rt| rt.heap.clone_string(result)
        .expect("Web string callback returned an invalid String carrier"))
}

fn web_invoke_bool(
    slot: crate::runtime_host::JitCallableSlot,
    value: i64,
) -> bool {
    crate::runtime_host::invoke_universal_unary(slot, value)
        .expect("Web callback requires its checked unary thunk") != 0
}

unsafe fn web_invoke_unit(
    slot: crate::runtime_host::JitCallableSlot,
    value: i64,
) {
    if slot.has_env {
        let callback: unsafe extern "C" fn(i64, i64) =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(slot.env, value);
    } else {
        let callback: unsafe extern "C" fn(i64) =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(value);
    }
}

fn jet_jit_web_router_new() -> i64 {
    with_rt(|rt| {
        rt.web.routers.push(web_rt::jet_web_router_new());
        rt.web.routers.len() as i64
    })
}


fn jet_jit_web_router_navigate(router: i64, url: i64) -> i64 {
    with_rt(|rt| {
        let router = rt
            .web
            .routers
            .get(router.saturating_sub(1) as usize)
            .cloned();
        let url = rt.heap.clone_string(url).unwrap_or_default();
        let Some(router) = router else {
            return crate::runtime_host::alloc_jit_result(rt, false, 0);
        };
        match web_rt::jet_web_router_navigate(&router, url) {
            Ok(navigation) => {
                let navigation = web_navigation_handle(rt, &navigation);
                crate::runtime_host::alloc_jit_result(rt, true, navigation as u64)
            }
            Err(error) => {
                let message = rt.heap.alloc_string(error);
                crate::runtime_host::alloc_jit_result(rt, false, message as u64)
            }
        }
    })
}

fn jet_jit_web_router_preload(router: i64, url: i64) -> i64 {
    with_rt(|rt| {
        let router = rt
            .web
            .routers
            .get(router.saturating_sub(1) as usize)
            .cloned();
        let url = rt.heap.clone_string(url).unwrap_or_default();
        let Some(router) = router else {
            return crate::runtime_host::alloc_jit_result(rt, false, 0);
        };
        match web_rt::jet_web_router_preload(&router, url) {
            Ok(navigation) => {
                let navigation = web_navigation_handle(rt, &navigation);
                crate::runtime_host::alloc_jit_result(rt, true, navigation as u64)
            }
            Err(error) => {
                let message = rt.heap.alloc_string(error);
                crate::runtime_host::alloc_jit_result(rt, false, message as u64)
            }
        }
    })
}

fn jet_jit_web_router_current(router: i64) -> i64 {
    with_rt(|rt| {
        let router = rt
            .web
            .routers
            .get(router.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_default();
        web_navigation_state_handle(rt, &web_rt::jet_web_router_current(&router))
    })
}

fn jet_jit_web_router_show(router: i64) -> i64 {
    with_rt(|rt| {
        let router = rt
            .web
            .routers
            .get(router.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_default();
        rt.heap.alloc_string(web_rt::jet_web_router_show(&router))
    })
}
fn jet_jit_web_router_abort(router: i64) -> i64 {
    with_rt(|rt| {
        let router = rt
            .web
            .routers
            .get(router.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_default();
        i64::from(web_rt::jet_web_router_abort(&router))
    })
}

fn jet_jit_web_router_stale(router: i64, identity: i64) -> i64 {
    with_rt(|rt| {
        let Some(router) = rt
            .web
            .routers
            .get(router.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let identity = rt.heap.clone_string(identity).unwrap_or_default();
        web_rt::jet_web_router_stale(&router, identity)
    })
}

fn jet_jit_web_router_invalidate(router: i64, identity: i64) -> i64 {
    with_rt(|rt| {
        let Some(router) = rt
            .web
            .routers
            .get(router.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let identity = rt.heap.clone_string(identity).unwrap_or_default();
        web_rt::jet_web_router_invalidate(&router, identity)
    })
}

fn jet_jit_web_router_collect(router: i64, identity: i64) -> i64 {
    with_rt(|rt| {
        let Some(router) = rt
            .web
            .routers
            .get(router.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let identity = rt.heap.clone_string(identity).unwrap_or_default();
        web_rt::jet_web_router_collect(&router, identity)
    })
}

fn jet_jit_web_router_cache_state(router: i64, identity: i64) -> i64 {
    with_rt(|rt| {
        let router = rt
            .web
            .routers
            .get(router.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_default();
        let identity = rt.heap.clone_string(identity).unwrap_or_default();
        let state = web_rt::jet_web_router_cache_state(&router, identity);
        web_router_cache_state_handle(rt, &state)
    })
}

fn jet_jit_web_router_cache_show(router: i64) -> i64 {
    with_rt(|rt| {
        let router = rt
            .web
            .routers
            .get(router.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_default();
        rt.heap.alloc_string(web_rt::jet_web_router_cache_show(&router))
    })
}

fn jet_jit_web_router_link(router: i64, pattern: i64, params: i64, search: i64) -> i64 {
    with_rt(|rt| {
        let router = rt
            .web
            .routers
            .get(router.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_default();
        let pattern = rt.heap.clone_string(pattern).unwrap_or_default();
        let params = web_string_map(rt, params).unwrap_or_default();
        let search = web_string_map(rt, search).unwrap_or_default();
        match web_rt::jet_web_router_link(&router, pattern, params, search) {
            Ok(value) => {
                let value = rt.heap.alloc_string(value);
                crate::runtime_host::alloc_jit_result(rt, true, value as u64)
            }
            Err(error) => {
                let error = rt.heap.alloc_string(error);
                crate::runtime_host::alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn web_query(query: i64) -> web_rt::JetWebQuery {
    with_rt(|rt| {
        rt.web.queries.get(query.saturating_sub(1) as usize)
            .cloned().expect("checked WebQuery handle")
    })
}

fn web_query_handle(query: web_rt::JetWebQuery) -> i64 {
    with_rt(|rt| {
        rt.web.queries.push(query);
        rt.web.queries.len() as i64
    })
}

fn web_record_text(rt: &crate::runtime_host::JitRuntime, record: i64, field: i64) -> String {
    match rt.heap.record_get(record, field) {
        Some(jet_rt::JetVal::String(value)) => value.clone(),
        _ => panic!("checked Web record string field"),
    }
}

fn web_record_integer(rt: &crate::runtime_host::JitRuntime, record: i64, field: i64) -> i64 {
    let value = rt.heap.record_get_int(record, field).expect("checked Web record integer field");
    web_int_value(rt, value)
}

fn web_query_state_value(rt: &crate::runtime_host::JitRuntime, value: i64) -> web_rt::JetWebQueryState {
    let status = rt.heap.record_get_int(value, 0).expect("checked Query status");
    let status = match rt.heap.record_get_int(status, 0) {
        Some(0) => web_rt::JetWebQueryStatus::Pending,
        Some(1) => web_rt::JetWebQueryStatus::Fresh,
        Some(2) => web_rt::JetWebQueryStatus::Stale,
        Some(3) => web_rt::JetWebQueryStatus::Fetching,
        Some(4) => web_rt::JetWebQueryStatus::Error,
        Some(5) => web_rt::JetWebQueryStatus::Offline,
        _ => panic!("checked Query status variant"),
    };
    web_rt::JetWebQueryState {
        status,
        value: web_record_text(rt, value, 1),
        error: web_record_text(rt, value, 2),
        generation: web_record_integer(rt, value, 3) as u64,
        queued: web_record_integer(rt, value, 4) as usize,
        offline: rt.heap.record_get_bool(value, 5).expect("checked Query offline field"),
    }
}

fn web_mutation_state_value(
    rt: &crate::runtime_host::JitRuntime,
    value: i64,
) -> web_rt::JetWebMutationState {
    let status = rt.heap.record_get_int(value, 0).expect("checked mutation status");
    let status = match rt.heap.record_get_int(status, 0) {
        Some(0) => web_rt::JetWebMutationStatus::Idle,
        Some(1) => web_rt::JetWebMutationStatus::Pending,
        Some(2) => web_rt::JetWebMutationStatus::Success,
        Some(3) => web_rt::JetWebMutationStatus::Error,
        Some(4) => web_rt::JetWebMutationStatus::Settled,
        _ => panic!("checked mutation status variant"),
    };
    web_rt::JetWebMutationState {
        status,
        generation: web_record_integer(rt, value, 1) as u64,
        optimistic: rt.heap.record_get_bool(value, 2).expect("checked mutation optimistic field"),
        value: web_record_text(rt, value, 3),
        result: web_record_text(rt, value, 4),
        error: web_record_text(rt, value, 5),
        context: web_record_text(rt, value, 6),
        rollback: rt.heap.record_get_bool(value, 7).expect("checked mutation rollback field"),
        paused: rt.heap.record_get_bool(value, 8).expect("checked mutation paused field"),
        queued: web_record_integer(rt, value, 9) as usize,
        replayed: web_record_integer(rt, value, 10) as u64,
    }
}

fn web_optional_string_value(rt: &crate::runtime_host::JitRuntime, value: i64) -> Option<String> {
    let (present, value) = crate::runtime_host::jit_result_parts(rt, value)
        .expect("checked Web optional string");
    present.then(|| rt.heap.clone_string(value as i64).expect("checked Web optional string value"))
}

/// Keep the checked `Err` carrier structured until the shared Web kernel
/// boundary. The kernel still stores callback failures as `String`; the
/// `_for_kernel` helper below is the single intentional narrowing seam.
fn web_query_callback(
    callback: crate::runtime_host::JitCallableSlot,
    payload: i64,
) -> Result<i64, jet_foundation::Outcome::JetErr> {
    let result = crate::runtime_host::invoke_universal_unary(callback, payload)
        .expect("checked Query callback thunk");
    with_rt(|rt| {
        let (ok, value) = crate::runtime_host::jit_result_parts(rt, result)
            .expect("checked Query callback outcome");
        if ok {
            Ok(value as i64)
        } else {
            Err(crate::runtime_host::jit_error(rt, value as i64)
                .expect("checked Query callback default Err carrier"))
        }
    })
}

fn web_query_callback_for_kernel(
    callback: crate::runtime_host::JitCallableSlot,
    payload: i64,
) -> Result<i64, String> {
    web_query_callback(callback, payload)
        .map_err(|error| jet_foundation::Outcome::jet_err_message(&error))
}

fn jet_jit_web_query_state_signal(query: i64) -> i64 {
    let signal = web_rt::jet_web_query_state_signal(&web_query(query));
    let signal = crate::Reactive::SignalSlot::typed(
        signal,
        |state| with_rt(|rt| web_query_state_handle(rt, state)),
        |value| with_rt(|rt| web_query_state_value(rt, value)),
    );
    with_rt(|rt| {
        rt.reactive.signals.push(signal);
        rt.reactive.signals.len() as i64
    })
}

fn jet_jit_web_query_mutation_signal(query: i64) -> i64 {
    let signal = web_rt::jet_web_query_mutation_signal(&web_query(query));
    let signal = crate::Reactive::SignalSlot::typed(
        signal,
        |state| with_rt(|rt| web_mutation_state_handle(rt, state)),
        |value| with_rt(|rt| web_mutation_state_value(rt, value)),
    );
    with_rt(|rt| {
        rt.reactive.signals.push(signal);
        rt.reactive.signals.len() as i64
    })
}

fn jet_jit_web_query_mutate(query: i64, payload: i64, optimistic: i64, callback: i64) -> i64 {
    let callback = web_callable(callback).expect("checked Query mutation callback");
    let (payload, optimistic) = with_rt(|rt| (
        rt.heap.clone_string(payload).expect("checked Query mutation payload"),
        web_optional_string_value(rt, optimistic),
    ));
    let state = web_rt::jet_web_query_mutate(&web_query(query), payload, optimistic, |payload| {
        let payload = with_rt(|rt| rt.heap.alloc_string(payload));
        let value = web_query_callback_for_kernel(callback, payload)?;
        Ok(with_rt(|rt| rt.heap.clone_string(value).expect("checked Query mutation response")))
    });
    with_rt(|rt| web_mutation_state_handle(rt, state))
}

fn jet_jit_web_query_mutate_with_invalidations(
    query: i64, payload: i64, optimistic: i64, invalidations: i64, callback: i64,
) -> i64 {
    let callback = web_callable(callback).expect("checked Query mutation callback");
    let (payload, optimistic, invalidations) = with_rt(|rt| (
        rt.heap.clone_string(payload).expect("checked Query mutation payload"),
        web_optional_string_value(rt, optimistic),
        web_string_list_value(rt, invalidations),
    ));
    let state = web_rt::jet_web_query_mutate_with_invalidations(
        &web_query(query), payload, optimistic, invalidations, |payload| {
            let payload = with_rt(|rt| rt.heap.alloc_string(payload));
            let value = web_query_callback_for_kernel(callback, payload)?;
            Ok(with_rt(|rt| rt.heap.clone_string(value).expect("checked Query mutation response")))
        },
    );
    with_rt(|rt| web_mutation_state_handle(rt, state))
}

fn jet_jit_web_query_retry(query: i64, callback: i64) -> i64 {
    let callback = web_callable(callback).expect("checked Query retry callback");
    let result = web_rt::jet_web_query_retry(&web_query(query), |payload| {
        let payload = with_rt(|rt| rt.heap.alloc_string(payload));
        web_query_callback_for_kernel(callback, payload).map(|_| ())
    });
    web_result_handle(result, web_int_handle)
}

fn jet_jit_web_query_new(key: i64, live: i64) -> i64 {
    let key = with_rt(|rt| rt.heap.clone_string(key).expect("checked Query key"));
    web_query_handle(crate::Reactive::web_query_from_live(key, live))
}

fn jet_jit_web_query_live(key: i64, footprint: i64, initial: i64, fetch: i64) -> i64 {
    let fetch = web_callable(fetch).expect("checked Query fetch callback");
    let (key, footprint, initial) = with_rt(|rt| (
        rt.heap.clone_string(key).expect("checked Query key"),
        rt.heap.clone_string(footprint).expect("checked Query footprint"),
        rt.heap.clone_string(initial).expect("checked Query initial value"),
    ));
    web_query_handle(web_rt::jet_web_query_live(key, footprint, initial, move || {
        let value = web_query_callback_for_kernel(fetch, 0)?;
        Ok(with_rt(|rt| rt.heap.clone_string(value).expect("checked Query fetch response")))
    }))
}

fn jet_jit_web_query_subscribe(source: i64) -> i64 {
    let source = with_rt(|rt| rt.heap.clone_string(source).expect("checked Query source"));
    web_query_handle(web_rt::jet_web_query_subscribe(source))
}

fn jet_jit_web_query_invalidate(query: i64) -> i64 {
    let generation = web_rt::jet_web_query_invalidate(&web_query(query));
    with_rt(|rt| web_int_handle(rt, generation))
}
fn jet_jit_web_query_cancel(query: i64) -> i64 {
    i64::from(web_rt::jet_web_query_cancel(&web_query(query)))
}


fn jet_jit_web_query_get(query: i64) -> i64 {
    let value = web_rt::jet_web_query_get(&web_query(query));
    with_rt(|rt| rt.heap.alloc_string(value))
}

fn jet_jit_web_query_state(query: i64) -> i64 {
    let state = web_rt::jet_web_query_state(&web_query(query));
    with_rt(|rt| web_query_state_handle(rt, state))
}
fn jet_jit_web_query_mutation_state(query: i64) -> i64 {
    let state = web_rt::jet_web_query_mutation_state(&web_query(query));
    with_rt(|rt| web_mutation_state_handle(rt, state))
}

fn jet_jit_web_query_show(query: i64) -> i64 {
    let value = web_rt::jet_web_query_show(&web_query(query));
    with_rt(|rt| rt.heap.alloc_string(value))
}
fn jet_jit_web_query_facts(query: i64) -> i64 {
    let value = web_rt::jet_web_query_facts(&web_query(query));
    with_rt(|rt| rt.heap.alloc_string(value))
}


fn jet_jit_web_query_queue(query: i64, payload: i64) -> i64 {
    let payload = with_rt(|rt| rt.heap.clone_string(payload).expect("checked Query queue payload"));
    web_result_handle(web_rt::jet_web_query_queue(&web_query(query), payload), web_int_handle)
}

fn jet_jit_web_query_refresh(query: i64) -> i64 {
    web_result_handle(web_rt::jet_web_query_refresh(&web_query(query)), |_, ()| 0)
}
fn jet_jit_web_query_set_online(query: i64, online: i64) {
    web_rt::jet_web_query_set_online(&web_query(query), online != 0);
}

fn jet_jit_web_query_set_mode(query: i64, mode: i64) {
    let mode = with_rt(|rt| web_query_mode(rt, mode));
    web_rt::jet_web_query_set_mode(&web_query(query), mode);
}


fn jet_jit_web_forms_new(action: i64) -> i64 {
    with_rt(|rt| {
        let action = rt.heap.clone_string(action).unwrap_or_default();
        rt.web.forms.push(web_rt::jet_web_forms_new(action));
        rt.web.forms.len() as i64
    })
}

fn jet_jit_web_forms_field(form: i64, name: i64, value_type: i64, required: i64) -> i64 {
    with_rt(|rt| {
        let form = rt
            .web
            .forms
            .get(form.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_else(|| web_rt::jet_web_forms_new(String::new()));
        let name = rt.heap.clone_string(name).unwrap_or_default();
        let value_type = rt.heap.clone_string(value_type).unwrap_or_default();
        match web_rt::jet_web_forms_field(&form, name, value_type, required != 0) {
            Ok(form) => {
                rt.web.forms.push(form);
                crate::runtime_host::alloc_jit_result(rt, true, rt.web.forms.len() as u64)
            }
            Err(error) => {
                let error = rt.heap.alloc_string(error);
                crate::runtime_host::alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_web_forms_set(form: i64, name: i64, value: i64) -> i64 {
    with_rt(|rt| {
        let form = rt
            .web
            .forms
            .get(form.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_else(|| web_rt::jet_web_forms_new(String::new()));
        let name = rt.heap.clone_string(name).unwrap_or_default();
        let value = rt.heap.clone_string(value).unwrap_or_default();
        match web_rt::jet_web_forms_set(&form, name, value) {
            Ok(()) => crate::runtime_host::alloc_jit_result(rt, true, 0),
            Err(error) => {
                let error = rt.heap.alloc_string(error);
                crate::runtime_host::alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_web_forms_blur(form: i64, name: i64) -> i64 {
    with_rt(|rt| {
        let form = rt
            .web
            .forms
            .get(form.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_else(|| web_rt::jet_web_forms_new(String::new()));
        let name = rt.heap.clone_string(name).unwrap_or_default();
        match web_rt::jet_web_forms_blur(&form, name) {
            Ok(()) => crate::runtime_host::alloc_jit_result(rt, true, 0),
            Err(error) => {
                let error = rt.heap.alloc_string(error);
                crate::runtime_host::alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_web_forms_validate(form: i64) -> i64 {
    with_rt(|rt| {
        let form = rt
            .web
            .forms
            .get(form.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_else(|| web_rt::jet_web_forms_new(String::new()));
        match web_rt::jet_web_forms_validate(&form) {
            Ok(()) => crate::runtime_host::alloc_jit_result(rt, true, 0),
            Err(error) => {
                let error = rt.heap.alloc_string(error);
                crate::runtime_host::alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_web_forms_validate_async(form: i64) -> i64 {
    with_rt(|rt| {
        let form = rt
            .web
            .forms
            .get(form.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_else(|| web_rt::jet_web_forms_new(String::new()));
        let form = web_rt::jet_web_forms_validate_async(&form);
        rt.web.forms.push(form);
        rt.web.forms.len() as i64
    })
}

fn jet_jit_web_forms_submit(form: i64) -> i64 {
    with_rt(|rt| {
        let form = rt
            .web
            .forms
            .get(form.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_else(|| web_rt::jet_web_forms_new(String::new()));
        match web_rt::jet_web_forms_submit(&form) {
            Ok(value) => {
                let value = rt.heap.alloc_string(value);
                crate::runtime_host::alloc_jit_result(rt, true, value as u64)
            }
            Err(error) => {
                let error = rt.heap.alloc_string(error);
                crate::runtime_host::alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_web_forms_no_script(form: i64) -> i64 {
    with_rt(|rt| {
        let form = rt
            .web
            .forms
            .get(form.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_else(|| web_rt::jet_web_forms_new(String::new()));
        match web_rt::jet_web_forms_no_script(&form) {
            Ok(value) => {
                let value = rt.heap.alloc_string(value);
                crate::runtime_host::alloc_jit_result(rt, true, value as u64)
            }
            Err(error) => {
                let error = rt.heap.alloc_string(error);
                crate::runtime_host::alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_web_forms_html(form: i64) -> i64 {
    with_rt(|rt| {
        let form = rt
            .web
            .forms
            .get(form.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_else(|| web_rt::jet_web_forms_new(String::new()));
        rt.heap.alloc_string(web_rt::jet_web_forms_html(&form))
    })
}

fn jet_jit_web_forms_show(form: i64) -> i64 {
    with_rt(|rt| {
        let form = rt
            .web
            .forms
            .get(form.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_else(|| web_rt::jet_web_forms_new(String::new()));
        rt.heap.alloc_string(web_rt::jet_web_forms_show(&form))
    })
}

fn web_form_input(input: i64) -> web_rt::JetWebFormInput {
    with_rt(|rt| rt.web.form_inputs.get(input.wrapping_sub(1) as usize).cloned())
        .expect("checked WebFormInput owner")
}

fn web_typed_form(form: i64) -> web_rt::JetWebFormTyped {
    with_rt(|rt| rt.web.typed_forms.get(form.wrapping_sub(1) as usize).cloned())
        .expect("checked WebFormTyped owner")
}

fn web_form_input_handle(rt: &mut crate::runtime_host::JitRuntime, input: web_rt::JetWebFormInput) -> i64 {
    rt.web.form_inputs.push(input);
    rt.web.form_inputs.len() as i64
}

fn web_typed_form_handle(rt: &mut crate::runtime_host::JitRuntime, form: web_rt::JetWebFormTyped) -> i64 {
    rt.web.typed_forms.push(form);
    rt.web.typed_forms.len() as i64
}

fn web_form_field_spec(
    rt: &crate::runtime_host::JitRuntime,
    value: i64,
) -> Result<web_rt::JetWebFormFieldSpec, String> {
    let field = |index| rt.heap.record_get_int(value, index).expect("checked form field descriptor");
    let value_type = match rt.heap.record_get_int(field(1), 0) {
        Some(0) => web_rt::JetWebFormValueType::String,
        Some(1) => web_rt::JetWebFormValueType::Int,
        Some(2) => web_rt::JetWebFormValueType::Bool,
        Some(3) => web_rt::JetWebFormValueType::Float,
        _ => panic!("checked WebFormValueType variant"),
    };
    let control = match rt.heap.record_get_int(field(5), 0) {
        Some(0) => web_rt::JetWebFormControl::Text,
        Some(1) => web_rt::JetWebFormControl::Email,
        Some(2) => web_rt::JetWebFormControl::Url,
        Some(3) => web_rt::JetWebFormControl::Password,
        Some(4) => web_rt::JetWebFormControl::Number,
        Some(5) => web_rt::JetWebFormControl::Date,
        Some(6) => web_rt::JetWebFormControl::Checkbox,
        Some(7) => web_rt::JetWebFormControl::Hidden,
        _ => panic!("checked WebFormControl variant"),
    };
    let optional = |index| {
        let (present, payload) = crate::runtime_host::jit_result_parts(rt, field(index))
            .expect("checked optional form text");
        present.then(|| rt.heap.clone_string(payload as i64).expect("checked form text"))
    };
    let mut spec = web_rt::JetWebFormFieldSpec::new(
        web_record_text(rt, value, 0),
        value_type,
        rt.heap.record_get_bool(value, 2).expect("checked required field"),
    )?
        .with_control(control)
        .with_label(web_record_text(rt, value, 4))?
        .with_wire_name(web_record_text(rt, value, 7))?;
    if let Some(default) = optional(3) {
        spec = spec.default_value(default)?;
    }
    if let Some(group) = optional(6) {
        spec = spec.in_group(group)?;
    }
    Ok(spec)
}

fn web_form_decoded_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    input: web_rt::JetWebFormDecodedInput,
) -> i64 {
    let record = rt.heap.alloc_record(3);
    let name = rt.heap.alloc_string(input.type_name);
    let values = web_map_handle(rt, input.values);
    let wire_values = web_map_handle(rt, input.wire_values);
    let _ = rt.heap.record_set_string(record, 0, name);
    let _ = rt.heap.record_set_int(record, 1, values);
    let _ = rt.heap.record_set_int(record, 2, wire_values);
    record
}

fn web_form_error_map_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    errors: std::collections::BTreeMap<String, Vec<String>>,
) -> i64 {
    let map = rt.heap.alloc_empty_map();
    for (field, messages) in errors {
        let field = rt.heap.alloc_string(field);
        let messages = web_string_list_handle(rt, messages);
        let _ = rt.heap.map_insert(map, field, messages);
    }
    map
}

fn web_form_action_error_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    error: web_rt::JetWebFormActionError,
) -> i64 {
    let record = rt.heap.alloc_record(2);
    let fields = web_form_error_map_handle(rt, error.field_errors);
    let form = web_string_list_handle(rt, error.form_errors);
    let _ = rt.heap.record_set_int(record, 0, fields);
    let _ = rt.heap.record_set_int(record, 1, form);
    record
}

fn web_form_action_error_value(
    rt: &mut crate::runtime_host::JitRuntime,
    value: i64,
) -> web_rt::JetWebFormActionError {
    let fields = rt.heap.record_get_int(value, 0).expect("checked form action field errors");
    let count = rt.heap.map_len(fields).expect("checked form action error map");
    let mut field_errors = std::collections::BTreeMap::new();
    for index in 0..count {
        let field = rt.heap.map_key_at(fields, index).expect("checked form error field");
        let field = rt.heap.clone_string(field).expect("checked form error field name");
        let messages = rt.heap.map_value_at(fields, index).expect("checked form error messages");
        field_errors.insert(field, web_string_list_value(rt, messages));
    }
    let form = rt.heap.record_get_int(value, 1).expect("checked form action errors");
    web_rt::JetWebFormActionError {
        field_errors,
        form_errors: web_string_list_value(rt, form),
    }
}

fn web_form_field_state_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    state: web_rt::JetWebFormFieldState,
) -> i64 {
    let record = rt.heap.alloc_record(7);
    let name = rt.heap.alloc_string(state.name);
    let value_type = web_enum_handle(rt, state.value_type as i64);
    let value = rt.heap.alloc_string(state.value);
    let errors = web_string_list_handle(rt, state.errors);
    let _ = rt.heap.record_set_string(record, 0, name);
    let _ = rt.heap.record_set_int(record, 1, value_type);
    let _ = rt.heap.record_set_bool(record, 2, state.required);
    let _ = rt.heap.record_set_string(record, 3, value);
    let _ = rt.heap.record_set_bool(record, 4, state.touched);
    let _ = rt.heap.record_set_bool(record, 5, state.validating);
    let _ = rt.heap.record_set_int(record, 6, errors);
    record
}

fn web_form_state_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    state: web_rt::JetWebFormState,
) -> i64 {
    let record = rt.heap.alloc_record(7);
    let status = web_enum_handle(rt, state.status as i64);
    let fields = rt.heap.alloc_empty_map();
    for (name, state) in state.fields {
        let name = rt.heap.alloc_string(name);
        let state = web_form_field_state_handle(rt, state);
        let _ = rt.heap.map_insert(fields, name, state);
    }
    let _ = rt.heap.record_set_int(record, 0, status);
    let _ = rt.heap.record_set_int(record, 1, fields);
    for (index, text) in [state.action, state.method, state.result, state.error].into_iter().enumerate() {
        let text = rt.heap.alloc_string(text);
        let _ = rt.heap.record_set_string(record, index as i64 + 2, text);
    }
    let submissions = web_int_handle(rt, state.submission_count as i64);
    let _ = rt.heap.record_set_int(record, 6, submissions);
    record
}

fn jet_jit_web_forms_input(name: i64, fields: i64) -> i64 {
    let input = with_rt(|rt| {
        let name = rt.heap.clone_string(name).expect("checked form input name");
        let fields = web_raw_list(rt, fields).expect("checked form input fields")
            .into_iter().map(|field| web_form_field_spec(rt, field))
            .collect::<Result<Vec<_>, _>>()?;
        web_rt::jet_web_forms_input(name, fields)
    });
    web_result_handle(input, web_form_input_handle)
}

fn jet_jit_web_forms_input_rename(input: i64, name: i64, wire_name: i64) -> i64 {
    let input = web_form_input(input);
    let (name, wire_name) = with_rt(|rt| (
        rt.heap.clone_string(name).expect("checked form field name"),
        rt.heap.clone_string(wire_name).expect("checked form wire name"),
    ));
    web_result_handle(web_rt::jet_web_forms_input_rename(input, name, wire_name), web_form_input_handle)
}

fn jet_jit_web_forms_input_exclude(input: i64, name: i64) -> i64 {
    let input = web_form_input(input);
    let name = with_rt(|rt| rt.heap.clone_string(name).expect("checked form field name"));
    web_result_handle(web_rt::jet_web_forms_input_exclude(input, name), web_form_input_handle)
}

fn jet_jit_web_forms_input_group(input: i64, name: i64, group: i64) -> i64 {
    let input = web_form_input(input);
    let (name, group) = with_rt(|rt| (
        rt.heap.clone_string(name).expect("checked form field name"),
        rt.heap.clone_string(group).expect("checked form group"),
    ));
    web_result_handle(web_rt::jet_web_forms_input_group(input, name, group), web_form_input_handle)
}

fn jet_jit_web_forms_input_replace(input: i64, name: i64, field: i64) -> i64 {
    let input = web_form_input(input);
    let (name, field) = with_rt(|rt| (
        rt.heap.clone_string(name).expect("checked form field name"),
        web_form_field_spec(rt, field),
    ));
    web_result_handle(
        field.and_then(|field| web_rt::jet_web_forms_input_replace(input, name, field)),
        web_form_input_handle,
    )
}

fn jet_jit_web_form(input: i64, action: i64) -> i64 {
    let input = web_form_input(input);
    let action = with_rt(|rt| rt.heap.clone_string(action).expect("checked form action"));
    let form = web_rt::jet_web_form(&input, action);
    with_rt(|rt| web_typed_form_handle(rt, form))
}

fn jet_jit_web_forms_typed(input: i64, action: i64) -> i64 {
    let input = web_form_input(input);
    let action = with_rt(|rt| rt.heap.clone_string(action).expect("checked form action"));
    web_result_handle(web_rt::jet_web_forms_typed(&input, action), web_typed_form_handle)
}

fn jet_jit_web_forms_typed_set(form: i64, name: i64, value: i64) -> i64 {
    let form = web_typed_form(form);
    let (name, value) = with_rt(|rt| (
        rt.heap.clone_string(name).expect("checked form field name"),
        rt.heap.clone_string(value).expect("checked form field value"),
    ));
    web_result_handle(web_rt::jet_web_forms_typed_set(&form, name, value), |_, ()| 0)
}

fn jet_jit_web_forms_typed_blur(form: i64, name: i64) -> i64 {
    let form = web_typed_form(form);
    let name = with_rt(|rt| rt.heap.clone_string(name).expect("checked form field name"));
    web_result_handle(web_rt::jet_web_forms_typed_blur(&form, name), |_, ()| 0)
}

fn jet_jit_web_forms_typed_validate(form: i64) -> i64 {
    web_result_handle(web_rt::jet_web_forms_typed_validate(&web_typed_form(form)), |_, ()| 0)
}

fn jet_jit_web_forms_typed_validate_async(form: i64) -> i64 {
    let validation = web_rt::jet_web_forms_typed_validate_async(&web_typed_form(form));
    with_rt(|rt| {
        rt.web.form_validations.push(Some(validation));
        rt.web.form_validations.len() as i64
    })
}

fn jet_jit_web_forms_typed_submit(form: i64) -> i64 {
    web_result_handle(web_rt::jet_web_forms_typed_submit(&web_typed_form(form)), |rt, value| rt.heap.alloc_string(value))
}

fn jet_jit_web_forms_typed_submit_async(form: i64) -> i64 {
    let submission = web_rt::jet_web_forms_typed_submit_async(&web_typed_form(form));
    with_rt(|rt| {
        rt.web.form_submissions.push(Some(submission));
        rt.web.form_submissions.len() as i64
    })
}

fn jet_jit_web_forms_typed_no_script(form: i64) -> i64 {
    web_result_handle(web_rt::jet_web_forms_typed_no_script(&web_typed_form(form)), |rt, value| rt.heap.alloc_string(value))
}

fn jet_jit_web_forms_typed_post(form: i64, body: i64) -> i64 {
    let form = web_typed_form(form);
    let body = with_rt(|rt| rt.heap.clone_string(body).expect("checked form POST body"));
    web_result_handle(web_rt::jet_web_forms_typed_post(&form, body), |rt, value| rt.heap.alloc_string(value))
}

fn jet_jit_web_forms_typed_decode_post(form: i64, body: i64) -> i64 {
    let form = web_typed_form(form);
    let body = with_rt(|rt| rt.heap.clone_string(body).expect("checked form POST body"));
    web_result_handle(web_rt::jet_web_forms_typed_decode_post(&form, body), web_form_decoded_handle)
}

fn jet_jit_web_forms_typed_html(form: i64) -> i64 {
    let html = web_rt::jet_web_forms_typed_html(&web_typed_form(form));
    with_rt(|rt| rt.heap.alloc_string(html))
}

fn jet_jit_web_forms_typed_show(form: i64) -> i64 {
    let show = web_rt::jet_web_forms_typed_show(&web_typed_form(form));
    with_rt(|rt| rt.heap.alloc_string(show))
}

fn jet_jit_web_forms_typed_state(form: i64) -> i64 {
    let state = web_rt::jet_web_forms_typed_state(&web_typed_form(form));
    with_rt(|rt| web_form_state_handle(rt, state))
}

fn jet_jit_web_forms_typed_lifecycle(form: i64) -> i64 {
    let state = web_rt::jet_web_forms_typed_lifecycle(&web_typed_form(form));
    with_rt(|rt| {
        let record = rt.heap.alloc_record(4);
        let status = web_enum_handle(rt, state.status as i64);
        let result = rt.heap.alloc_string(state.result);
        let error = rt.heap.alloc_string(state.error);
        let generation = web_int_handle(rt, state.generation as i64);
        let _ = rt.heap.record_set_int(record, 0, status);
        let _ = rt.heap.record_set_string(record, 1, result);
        let _ = rt.heap.record_set_string(record, 2, error);
        let _ = rt.heap.record_set_int(record, 3, generation);
        record
    })
}

fn jet_jit_web_forms_typed_errors(form: i64) -> i64 {
    let errors = web_rt::jet_web_forms_typed_errors(&web_typed_form(form));
    with_rt(|rt| {
        let record = rt.heap.alloc_record(2);
        let fields = web_form_error_map_handle(rt, errors.fields);
        let form = web_string_list_handle(rt, errors.form);
        let _ = rt.heap.record_set_int(record, 0, fields);
        let _ = rt.heap.record_set_int(record, 1, form);
        record
    })
}

fn jet_jit_web_forms_typed_focus(form: i64, name: i64) -> i64 {
    let form = web_typed_form(form);
    let name = with_rt(|rt| rt.heap.clone_string(name).expect("checked form field name"));
    web_result_handle(web_rt::jet_web_forms_typed_focus(&form, name), |_, ()| 0)
}

fn jet_jit_web_forms_typed_cancel(form: i64) {
    web_rt::jet_web_forms_typed_cancel(&web_typed_form(form));
}

fn jet_jit_web_forms_typed_select_field(form: i64, name: i64) -> i64 {
    let form = web_typed_form(form);
    let name = with_rt(|rt| rt.heap.clone_string(name).expect("checked form field name"));
    web_result_handle(web_rt::jet_web_forms_typed_select_field(&form, name), |rt, subscription| {
        rt.reactive.deriveds.push(crate::Reactive::DerivedSlot::typed(
            subscription,
            |state| with_rt(|rt| web_form_field_state_handle(rt, state)),
        ));
        rt.reactive.deriveds.len() as i64
    })
}

fn jet_jit_web_forms_typed_validation_wait(validation: i64) -> i64 {
    let validation = with_rt(|rt| rt.web.form_validations
        .get_mut(validation.wrapping_sub(1) as usize).and_then(Option::take))
        .expect("checked unconsumed form validation");
    web_result_handle(web_rt::jet_web_forms_typed_validation_wait(validation), |_, ()| 0)
}

fn jet_jit_web_forms_typed_validation_cancel(validation: i64) {
    with_rt(|rt| web_rt::jet_web_forms_typed_validation_cancel(
        rt.web.form_validations.get(validation.wrapping_sub(1) as usize)
            .and_then(Option::as_ref).expect("checked unconsumed form validation"),
    ));
}

fn jet_jit_web_forms_typed_submission_wait(submission: i64) -> i64 {
    let submission = with_rt(|rt| rt.web.form_submissions
        .get_mut(submission.wrapping_sub(1) as usize).and_then(Option::take))
        .expect("checked unconsumed form submission");
    web_result_handle(web_rt::jet_web_forms_typed_submission_wait(submission), |rt, value| rt.heap.alloc_string(value))
}

fn jet_jit_web_forms_typed_submission_cancel(submission: i64) {
    with_rt(|rt| web_rt::jet_web_forms_typed_submission_cancel(
        rt.web.form_submissions.get(submission.wrapping_sub(1) as usize)
            .and_then(Option::as_ref).expect("checked unconsumed form submission"),
    ));
}

fn jet_jit_web_forms_typed_set_async_validator(
    form: i64,
    name: i64,
    timing: i64,
    debounce: i64,
    validator: i64,
) -> i64 {
    let form = web_typed_form(form);
    let (epoch, validator) = Concurrency::http_callable_snapshot(validator)
        .expect("checked form validator owner");
    let (name, timing, debounce) = with_rt(|rt| {
        let timing = match rt.heap.record_get_int(timing, 0) {
            Some(0) => web_rt::JetWebFormValidationTiming::Change,
            Some(1) => web_rt::JetWebFormValidationTiming::Blur,
            Some(2) => web_rt::JetWebFormValidationTiming::Submit,
            _ => panic!("checked WebFormValidationTiming variant"),
        };
        (
            rt.heap.clone_string(name).expect("checked validated form field"),
            timing,
            web_int_value(rt, debounce) as u64,
        )
    });
    web_result_handle(
        web_rt::jet_web_forms_typed_set_async_validator(&form, name, timing, debounce, move |value| {
            Concurrency::try_with_http_jet_runtime_at(epoch, || {
                let value = with_rt(|rt| rt.heap.alloc_string(value));
                web_query_callback_for_kernel(validator, value).map(|_| ())
            }).expect("retained form validator runtime")
        }),
        |_, ()| 0,
    )
}

fn jet_jit_web_forms_typed_set_action(form: i64, action: i64) {
    let form = web_typed_form(form);
    let (epoch, action) = Concurrency::http_callable_snapshot(action)
        .expect("checked typed form action owner");
    web_rt::jet_web_forms_typed_set_action(&form, move |input| {
        Concurrency::try_with_http_jet_runtime_at(epoch, || {
            let input = with_rt(|rt| web_form_decoded_handle(rt, input));
            let result = crate::runtime_host::invoke_universal_unary(action, input)
                .expect("checked typed form action invocation");
            with_rt(|rt| {
                let (ok, value) = crate::runtime_host::jit_result_parts(rt, result)
                    .expect("checked typed form action report");
                if ok {
                    Ok(rt.heap.clone_string(value as i64).expect("checked form action result"))
                } else {
                    Err(web_form_action_error_value(rt, value as i64))
                }
            })
        }).expect("retained typed form action runtime")
    });
}

fn jet_jit_web_forms_action_error() -> i64 {
    with_rt(|rt| web_form_action_error_handle(rt, web_rt::jet_web_forms_action_error()))
}

fn jet_jit_web_forms_action_field_error(error: i64, name: i64, message: i64) -> i64 {
    let (error, name, message) = with_rt(|rt| {
        let error = web_form_action_error_value(rt, error);
        (
            error,
            rt.heap.clone_string(name).expect("checked action error field"),
            rt.heap.clone_string(message).expect("checked action field error"),
        )
    });
    let error = web_rt::jet_web_forms_action_field_error(error, name, message);
    with_rt(|rt| web_form_action_error_handle(rt, error))
}

fn jet_jit_web_forms_action_form_error(error: i64, message: i64) -> i64 {
    let (error, message) = with_rt(|rt| {
        let error = web_form_action_error_value(rt, error);
        (
            error,
            rt.heap.clone_string(message).expect("checked action form error"),
        )
    });
    let error = web_rt::jet_web_forms_action_form_error(error, message);
    with_rt(|rt| web_form_action_error_handle(rt, error))
}

fn web_table(table: i64) -> web_rt::JetWebTable<i64> {
    with_rt(|rt| {
        rt.web.tables.get(table.saturating_sub(1) as usize)
            .cloned().expect("checked WebTable handle")
    })
}

fn web_table_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    table: web_rt::JetWebTable<i64>,
) -> i64 {
    rt.web.tables.push(table);
    rt.web.tables.len() as i64
}

fn web_table_rows_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    rows: Vec<web_rt::JetWebTableRow<i64>>,
) -> i64 {
    let list = rt.heap.alloc_empty_list();
    for row in rows {
        let value = rt.heap.alloc_record(2);
        let key = rt.heap.alloc_string(row.key);
        let _ = rt.heap.record_set_string(value, 0, key);
        let _ = rt.heap.record_set_int(value, 1, row.value);
        let _ = rt.heap.list_push_int(list, value);
    }
    list
}

fn web_table_state_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    state: web_rt::JetWebTableState,
) -> i64 {
    let value = rt.heap.alloc_record(7);
    let sort = state.sort.map(|sort| {
        let value = rt.heap.alloc_record(2);
        let column = rt.heap.alloc_string(sort.column);
        let direction = web_enum_handle(rt, match sort.direction {
            web_rt::JetWebTableSortDirection::Ascending => 0,
            web_rt::JetWebTableSortDirection::Descending => 1,
        });
        let _ = rt.heap.record_set_string(value, 0, column);
        let _ = rt.heap.record_set_int(value, 1, direction);
        value
    });
    let sort = crate::runtime_host::alloc_jit_result(rt, sort.is_some(), sort.unwrap_or(0) as u64);
    let filter = state.filter.map(|filter| {
        let value = rt.heap.alloc_record(2);
        let column = rt.heap.alloc_string(filter.column);
        let text = rt.heap.alloc_string(filter.value);
        let _ = rt.heap.record_set_string(value, 0, column);
        let _ = rt.heap.record_set_string(value, 1, text);
        value
    });
    let filter = crate::runtime_host::alloc_jit_result(rt, filter.is_some(), filter.unwrap_or(0) as u64);
    let page_index = web_int_handle(rt, state.page_index);
    let page_size = web_int_handle(rt, state.page_size);
    let selected_keys = web_string_list_handle(rt, &state.selected_keys);
    let anchor = web_optional_string_handle(rt, state.selection_anchor);
    let mode = web_enum_handle(rt, match state.page_mode {
        web_rt::JetWebTablePageMode::Client => 0,
        web_rt::JetWebTablePageMode::Server => 1,
    });
    for (index, field) in [sort, filter, page_index, page_size, selected_keys, anchor, mode]
        .into_iter().enumerate()
    {
        let _ = rt.heap.record_set_int(value, index as i64, field);
    }
    value
}

fn web_table_page_value(
    rt: &mut crate::runtime_host::JitRuntime,
    page: i64,
) -> web_rt::JetWebTablePage<i64> {
    let rows = rt.heap.record_get_int(page, 0).expect("checked Table page rows");
    let keys = rt.heap.record_get_int(page, 1).expect("checked Table page keys");
    let rows = web_raw_list(rt, rows).expect("checked Table page row list");
    let row_keys = web_string_list_value(rt, keys);
    let mut counts = [0; 4];
    for (index, count) in counts.iter_mut().enumerate() {
        let value = rt.heap.record_get_int(page, index as i64 + 2)
            .expect("checked Table page count");
        *count = web_int_value(rt, value);
    }
    web_rt::JetWebTablePage {
        rows, row_keys, total_rows: counts[0], page_index: counts[1],
        page_size: counts[2], page_count: counts[3],
    }
}

fn jet_jit_web_table_new_keyed(name: i64, rows: i64, callback: i64) -> i64 {
    let callback = web_callable(callback).expect("checked Table key callback");
    let (name, rows) = with_rt(|rt| (
        rt.heap.clone_string(name).expect("checked Table name"),
        web_raw_list(rt, rows).expect("checked Table rows"),
    ));
    let table = web_rt::jet_web_table_new_keyed(name, rows, move |row| {
        web_invoke_string(callback, *row)
    });
    with_rt(|rt| web_table_handle(rt, table))
}

fn jet_jit_web_table_with_column(table: i64, column: i64) -> i64 {
    let column = with_rt(|rt| {
        rt.web.table_columns.get(column.saturating_sub(1) as usize)
            .cloned().expect("checked Table column handle")
    });
    let table = web_rt::jet_web_table_with_column(&web_table(table), column);
    with_rt(|rt| web_table_handle(rt, table))
}

fn jet_jit_web_table_with_server_page(table: i64, callback: i64) -> i64 {
    let callback = web_callable(callback).expect("checked Table page loader");
    let table = web_rt::jet_web_table_with_server_page(&web_table(table), move |state| {
        let state = with_rt(|rt| web_table_state_handle(rt, state));
        let result = crate::runtime_host::invoke_universal_unary(callback, state)
            .expect("checked Table page loader thunk");
        with_rt(|rt| {
            let (ok, value) = crate::runtime_host::jit_result_parts(rt, result)
                .expect("checked Table page loader outcome");
            if ok {
                Ok(web_table_page_value(rt, value as i64))
            } else {
                Err(rt.heap.clone_string(value as i64).expect("checked Table page loader report"))
            }
        })
    });
    with_rt(|rt| web_table_handle(rt, table))
}

fn jet_jit_web_table_state(table: i64) -> i64 {
    let state = web_rt::jet_web_table_state(&web_table(table));
    with_rt(|rt| web_table_state_handle(rt, state))
}

fn jet_jit_web_table_facts(table: i64) -> i64 {
    let facts = web_rt::jet_web_table_facts(&web_table(table));
    with_rt(|rt| rt.heap.alloc_string(facts))
}

fn jet_jit_web_table_keys(table: i64) -> i64 {
    web_result_handle(web_rt::jet_web_table_keys(&web_table(table)), |rt, keys| {
        web_string_list_handle(rt, &keys)
    })
}

fn jet_jit_web_table_page_state(table: i64) -> i64 {
    web_result_handle(web_rt::jet_web_table_page_state(&web_table(table)), |rt, page| {
        web_table_page_handle(rt, &page)
    })
}

fn jet_jit_web_table_sort_by(table: i64, column: i64, direction: i64) -> i64 {
    let (column, direction) = with_rt(|rt| (
        rt.heap.clone_string(column).expect("checked Table sort column"),
        match rt.heap.record_get_int(direction, 0) {
            Some(0) => web_rt::JetWebTableSortDirection::Ascending,
            Some(1) => web_rt::JetWebTableSortDirection::Descending,
            _ => panic!("checked Table sort direction"),
        },
    ));
    let table = web_rt::jet_web_table_sort_by(&web_table(table), column, direction);
    with_rt(|rt| web_table_handle(rt, table))
}

fn jet_jit_web_table_filter_by(table: i64, column: i64, value: i64) -> i64 {
    let (column, value) = with_rt(|rt| (
        rt.heap.clone_string(column).expect("checked Table filter column"),
        rt.heap.clone_string(value).expect("checked Table filter value"),
    ));
    let table = web_rt::jet_web_table_filter_by(&web_table(table), column, value);
    with_rt(|rt| web_table_handle(rt, table))
}

fn jet_jit_web_table_paginate(table: i64, index: i64, size: i64) -> i64 {
    let (index, size) = with_rt(|rt| (web_int_value(rt, index), web_int_value(rt, size)));
    let table = web_rt::jet_web_table_paginate(&web_table(table), index, size);
    with_rt(|rt| web_table_handle(rt, table))
}

fn jet_jit_web_table_set_rows(table: i64, rows: i64) -> i64 {
    let rows = with_rt(|rt| web_raw_list(rt, rows).expect("checked Table rows"));
    web_result_handle(web_rt::jet_web_table_set_rows(&web_table(table), rows), |_, ()| 0)
}

fn jet_jit_web_table_set_selected(table: i64, key: i64, selected: i64) -> i64 {
    let key = with_rt(|rt| rt.heap.clone_string(key).expect("checked Table selection key"));
    web_result_handle(
        web_rt::jet_web_table_set_selected(&web_table(table), key, selected != 0),
        web_table_handle,
    )
}

fn jet_jit_web_table_toggle_selection(table: i64, key: i64) -> i64 {
    let key = with_rt(|rt| rt.heap.clone_string(key).expect("checked Table selection key"));
    web_result_handle(
        web_rt::jet_web_table_toggle_selection(&web_table(table), key), web_table_handle,
    )
}

fn jet_jit_web_table_clear_selection(table: i64) -> i64 {
    let table = web_rt::jet_web_table_clear_selection(&web_table(table));
    with_rt(|rt| web_table_handle(rt, table))
}

fn jet_jit_web_table_selected_keys(table: i64) -> i64 {
    let keys = web_rt::jet_web_table_selected_keys(&web_table(table));
    with_rt(|rt| web_string_list_handle(rt, &keys))
}

fn jet_jit_web_table_selected_rows(table: i64) -> i64 {
    web_result_handle(web_rt::jet_web_table_selected_rows(&web_table(table)), web_table_rows_handle)
}

fn jet_jit_web_table_insert_row(table: i64, row: i64) -> i64 {
    web_result_handle(web_rt::jet_web_table_insert_row(&web_table(table), row), |rt, key| {
        rt.heap.alloc_string(key)
    })
}

fn jet_jit_web_table_replace_row(table: i64, key: i64, row: i64) -> i64 {
    let key = with_rt(|rt| rt.heap.clone_string(key).expect("checked Table row key"));
    web_result_handle(web_rt::jet_web_table_replace_row(&web_table(table), key, row), |_, ()| 0)
}

fn jet_jit_web_table_update_row(table: i64, key: i64, callback: i64) -> i64 {
    let key = with_rt(|rt| rt.heap.clone_string(key).expect("checked Table row key"));
    let callback = web_callable(callback).expect("checked Table row update callback");
    web_result_handle(
        web_rt::jet_web_table_update_row(&web_table(table), key, move |row| {
            web_invoke_update(callback, row);
        }),
        |_, ()| 0,
    )
}

fn jet_jit_web_table_remove_row(table: i64, key: i64) -> i64 {
    let key = with_rt(|rt| rt.heap.clone_string(key).expect("checked Table row key"));
    web_result_handle(web_rt::jet_web_table_remove_row(&web_table(table), key), |_, row| row)
}

fn jet_jit_web_table_first_page(table: i64) -> i64 {
    let table = web_rt::jet_web_table_first_page(&web_table(table));
    with_rt(|rt| web_table_handle(rt, table))
}

fn jet_jit_web_table_next_page(table: i64) -> i64 {
    let table = web_rt::jet_web_table_next_page(&web_table(table));
    with_rt(|rt| web_table_handle(rt, table))
}

fn jet_jit_web_table_last_page(table: i64) -> i64 {
    web_result_handle(web_rt::jet_web_table_last_page(&web_table(table)), web_table_handle)
}

fn jet_jit_web_table_visible_rows(table: i64, plan: i64) -> i64 {
    let plan = with_rt(|rt| web_virtual_plan_value(rt, plan).expect("checked virtual plan"));
    web_result_handle(
        web_rt::jet_web_table_visible_rows(&web_table(table), &plan), web_table_rows_handle,
    )
}

fn jet_jit_web_table_new(name: i64, rows: i64) -> i64 {
    let (name, rows) = with_rt(|rt| (
        rt.heap.clone_string(name).expect("checked Table name"),
        web_raw_list(rt, rows).expect("checked Table rows"),
    ));
    let table = web_rt::jet_web_table(name, rows);
    with_rt(|rt| web_table_handle(rt, table))
}

fn jet_jit_web_table_column(name: i64, cell_type: i64, callback: i64) -> i64 {
    let slot = web_callable(callback).expect("checked Table column callback");
    let (name, cell_type) = with_rt(|rt| {
        (
            rt.heap.clone_string(name).expect("checked Table column name"),
            rt.heap.clone_string(cell_type).expect("checked Table cell type"),
        )
    });
    let column = web_rt::jet_web_table_column(name, cell_type, move |row: &i64| {
        web_invoke_string(slot, *row)
    });
    with_rt(|rt| {
        rt.web.table_columns.push(column);
        rt.web.table_columns.len() as i64
    })
}

fn jet_jit_web_table_sort(rows: i64, descending: i64, callback: i64) -> i64 {
    let slot = web_callable(callback).expect("checked Table sort callback");
    let rows = with_rt(|rt| web_raw_list(rt, rows).expect("checked Table rows"));
    let sorted = web_rt::jet_web_table_sort(&rows, descending != 0, |row| {
        web_invoke_string(slot, *row)
    });
    with_rt(|rt| web_raw_list_handle(rt, sorted))
}

fn jet_jit_web_table_filter(rows: i64, callback: i64) -> i64 {
    let slot = web_callable(callback).expect("checked Table filter callback");
    let rows = with_rt(|rt| web_raw_list(rt, rows).expect("checked Table rows"));
    let filtered = web_rt::jet_web_table_filter(&rows, |row| {
        web_invoke_bool(slot, *row)
    });
    with_rt(|rt| web_raw_list_handle(rt, filtered))
}

fn jet_jit_web_table_page(rows: i64, page_index: i64, page_size: i64) -> i64 {
    let (rows, page_index, page_size) = with_rt(|rt| {
        (
            web_raw_list(rt, rows).expect("checked Table rows"),
            web_int_value(rt, page_index),
            web_int_value(rt, page_size),
        )
    });
    let page = web_rt::jet_web_table_page(&rows, page_index, page_size);
    with_rt(|rt| web_table_page_handle(rt, &page))
}

fn jet_jit_web_virtual_window(
    total_count: i64,
    scroll_offset: i64,
    viewport_size: i64,
    estimated_item_size: i64,
    overscan: i64,
) -> i64 {
    let values = with_rt(|rt| {
        [
            web_int_value(rt, total_count),
            web_int_value(rt, scroll_offset),
            web_int_value(rt, viewport_size),
            web_int_value(rt, estimated_item_size),
            web_int_value(rt, overscan),
        ]
    });
    let window = web_rt::jet_web_virtual_window(
        values[0], values[1], values[2], values[3], values[4],
    );
    with_rt(|rt| web_virtual_window_handle(rt, &window))
}

fn jet_jit_web_virtual_window_measured(
    total_count: i64,
    scroll_offset: i64,
    viewport_size: i64,
    fallback_item_size: i64,
    overscan: i64,
    measurements: i64,
) -> i64 {
    let (values, measurements) = with_rt(|rt| {
        (
            [
                web_int_value(rt, total_count),
                web_int_value(rt, scroll_offset),
                web_int_value(rt, viewport_size),
                web_int_value(rt, fallback_item_size),
                web_int_value(rt, overscan),
            ],
            web_raw_list(rt, measurements).unwrap_or_default(),
        )
    });
    let window = web_rt::jet_web_virtual_window_measured(
        values[0],
        values[1],
        values[2],
        values[3],
        values[4],
        &measurements,
    );
    with_rt(|rt| web_virtual_window_handle(rt, &window))
}

fn jet_jit_web_virtual_slice(rows: i64, window: i64) -> i64 {
    let (rows, window) = with_rt(|rt| {
        (
            web_raw_list(rt, rows).unwrap_or_default(),
            web_virtual_window_value(rt, window).unwrap_or_else(|| {
                web_rt::jet_web_virtual_window(0, 0, 0, 1, 0)
            }),
        )
    });
    let sliced = web_rt::jet_web_virtual_slice(&rows, &window);
    with_rt(|rt| web_raw_list_handle(rt, sliced))
}

fn jet_jit_web_virtual_indices(window: i64) -> i64 {
    let window = with_rt(|rt| {
        web_virtual_window_value(rt, window)
            .unwrap_or_else(|| web_rt::jet_web_virtual_window(0, 0, 0, 1, 0))
    });
    let indices = web_rt::jet_web_virtual_indices(&window)
        .into_iter()
        .map(|value| with_rt(|rt| web_int_handle(rt, value)))
        .collect();
    with_rt(|rt| web_raw_list_handle(rt, indices))
}

fn jet_jit_web_store_new(name: i64, initial: i64) -> i64 {
    let name = with_rt(|rt| rt.heap.clone_string(name).expect("checked Store name"));
    let store = web_rt::jet_web_store(name, initial);
    with_rt(|rt| {
        rt.web.stores.push(store);
        rt.web.stores.len() as i64
    })
}

fn jet_jit_web_store_with_history(name: i64, initial: i64, history_limit: i64) -> i64 {
    let (name, history_limit) = with_rt(|rt| (
        rt.heap.clone_string(name).expect("checked Store name"),
        web_int_value(rt, history_limit),
    ));
    let store = web_rt::jet_web_store_with_history(name, initial, history_limit);
    with_rt(|rt| {
        rt.web.stores.push(store);
        rt.web.stores.len() as i64
    })
}

fn web_store(store: i64) -> web_rt::JetWebStore<i64> {
    with_rt(|rt| {
        rt.web.stores.get(store.saturating_sub(1) as usize)
            .cloned().expect("checked WebStore handle")
    })
}

fn web_store_transaction_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    transaction: web_rt::JetWebStoreTransaction<i64>,
) -> i64 {
    let value = rt.heap.alloc_record(5);
    let generation = web_int_handle(rt, transaction.generation as i64);
    let action = rt.heap.alloc_string(transaction.action);
    let changed_fields = web_string_list_handle(rt, &transaction.changed_fields);
    let _ = rt.heap.record_set_int(value, 0, generation);
    let _ = rt.heap.record_set_string(value, 1, action);
    let _ = rt.heap.record_set_int(value, 2, changed_fields);
    let _ = rt.heap.record_set_int(value, 3, transaction.before);
    let _ = rt.heap.record_set_int(value, 4, transaction.after);
    value
}

fn web_store_event_handle(
    rt: &mut crate::runtime_host::JitRuntime,
    event: web_rt::JetWebStoreEvent,
) -> i64 {
    let value = rt.heap.alloc_record(6);
    let sequence = web_int_handle(rt, event.sequence as i64);
    let generation = web_int_handle(rt, event.generation as i64);
    let action = rt.heap.alloc_string(event.action);
    let changed_fields = web_string_list_handle(rt, &event.changed_fields);
    let cursor = web_int_handle(rt, event.cursor as i64);
    let kind = web_enum_handle(rt, match event.kind {
        web_rt::JetWebStoreEventKind::Transaction => 0,
        web_rt::JetWebStoreEventKind::Navigation => 1,
        web_rt::JetWebStoreEventKind::Rollback => 2,
    });
    let _ = rt.heap.record_set_int(value, 0, sequence);
    let _ = rt.heap.record_set_int(value, 1, generation);
    let _ = rt.heap.record_set_string(value, 2, action);
    let _ = rt.heap.record_set_int(value, 3, changed_fields);
    let _ = rt.heap.record_set_int(value, 4, cursor);
    let _ = rt.heap.record_set_int(value, 5, kind);
    value
}

fn web_optional_raw(value: Option<i64>) -> i64 {
    with_rt(|rt| match value {
        Some(value) => crate::runtime_host::alloc_jit_result(rt, true, value as u64),
        None => crate::runtime_host::alloc_jit_result(rt, false, 0),
    })
}

fn jet_jit_web_store_value(store: i64) -> i64 {
    web_rt::jet_web_store_value(&web_store(store))
}

fn jet_jit_web_store_signal(store: i64) -> i64 {
    let signal = web_rt::jet_web_store_signal(&web_store(store));
    with_rt(|rt| {
        rt.reactive.signals.push(crate::Reactive::SignalSlot::Word(signal));
        rt.reactive.signals.len() as i64
    })
}

fn jet_jit_web_store_state_signal(store: i64) -> i64 {
    let signal = web_rt::jet_web_store_state_signal(&web_store(store));
    with_rt(|rt| {
        rt.reactive.signals.push(crate::Reactive::SignalSlot::Word(signal));
        rt.reactive.signals.len() as i64
    })
}

fn jet_jit_web_store_set(store: i64, value: i64) -> i64 {
    let transaction = web_rt::jet_web_store_set(&web_store(store), value);
    with_rt(|rt| web_store_transaction_handle(rt, transaction))
}

fn jet_jit_web_store_set_state(store: i64, value: i64) -> i64 {
    let transaction = web_rt::jet_web_store_set_state(&web_store(store), value);
    with_rt(|rt| web_store_transaction_handle(rt, transaction))
}

fn jet_jit_web_store_back(store: i64) -> i64 {
    web_optional_raw(web_rt::jet_web_store_back(&web_store(store)))
}

fn jet_jit_web_store_forward(store: i64) -> i64 {
    web_optional_raw(web_rt::jet_web_store_forward(&web_store(store)))
}

fn jet_jit_web_store_jump(store: i64, generation: i64) -> i64 {
    let generation = with_rt(|rt| web_int_value(rt, generation));
    web_optional_raw(web_rt::jet_web_store_jump(&web_store(store), generation))
}

fn jet_jit_web_store_scrub(store: i64, generation: i64) -> i64 {
    let generation = with_rt(|rt| web_int_value(rt, generation));
    web_optional_raw(web_rt::jet_web_store_scrub(&web_store(store), generation))
}

fn jet_jit_web_store_restore(store: i64, generation: i64) -> i64 {
    let generation = with_rt(|rt| web_int_value(rt, generation));
    web_optional_raw(web_rt::jet_web_store_restore(&web_store(store), generation))
}

fn jet_jit_web_store_history(store: i64) -> i64 {
    let history = web_rt::jet_web_store_history(&web_store(store));
    with_rt(|rt| {
        let list = rt.heap.alloc_empty_list();
        for transaction in history {
            let value = web_store_transaction_handle(rt, transaction);
            let _ = rt.heap.list_push_int(list, value);
        }
        list
    })
}

fn jet_jit_web_store_history_at(store: i64, index: i64) -> i64 {
    let index = with_rt(|rt| web_int_value(rt, index));
    let transaction = web_rt::jet_web_store_history_at(&web_store(store), index);
    let value = transaction.map(|transaction| {
        with_rt(|rt| web_store_transaction_handle(rt, transaction))
    });
    web_optional_raw(value)
}

fn web_store_events_handle(events: Vec<web_rt::JetWebStoreEvent>) -> i64 {
    with_rt(|rt| {
        let list = rt.heap.alloc_empty_list();
        for event in events {
            let value = web_store_event_handle(rt, event);
            let _ = rt.heap.list_push_int(list, value);
        }
        list
    })
}

fn jet_jit_web_store_events(store: i64) -> i64 {
    web_store_events_handle(web_rt::jet_web_store_events(&web_store(store)))
}

fn jet_jit_web_store_events_since(store: i64, sequence: i64) -> i64 {
    let sequence = with_rt(|rt| web_int_value(rt, sequence));
    web_store_events_handle(web_rt::jet_web_store_events_since(&web_store(store), sequence))
}

fn jet_jit_web_store_clear_history(store: i64) {
    web_rt::jet_web_store_clear_history(&web_store(store));
}

fn jet_jit_web_store_set_history_limit(store: i64, limit: i64) {
    let limit = with_rt(|rt| web_int_value(rt, limit));
    web_rt::jet_web_store_set_history_limit(&web_store(store), limit);
}

fn jet_jit_web_store_history_enabled(store: i64) -> i64 {
    i64::from(web_rt::jet_web_store_history_enabled(&web_store(store)))
}

fn jet_jit_web_store_history_limit(store: i64) -> i64 {
    let value = web_rt::jet_web_store_history_limit(&web_store(store));
    with_rt(|rt| web_int_handle(rt, value))
}

fn jet_jit_web_store_cursor(store: i64) -> i64 {
    let value = web_rt::jet_web_store_cursor(&web_store(store));
    with_rt(|rt| web_int_handle(rt, value))
}

fn jet_jit_web_store_current_generation(store: i64) -> i64 {
    let value = web_rt::jet_web_store_current_generation(&web_store(store));
    with_rt(|rt| web_int_handle(rt, value))
}

fn jet_jit_web_store_facts_json(store: i64) -> i64 {
    let value = web_rt::jet_web_store_facts_json(&web_store(store));
    with_rt(|rt| rt.heap.alloc_string(value))
}

fn jet_jit_web_store_event_json(store: i64) -> i64 {
    let value = web_rt::jet_web_store_event_json(&web_store(store));
    with_rt(|rt| rt.heap.alloc_string(value))
}

fn jet_jit_web_store_inspect(store: i64) -> i64 {
    let inspection = web_rt::jet_web_store_inspect(&web_store(store));
    with_rt(|rt| {
        let value = rt.heap.alloc_record(7);
        let generation = web_int_handle(rt, inspection.generation as i64);
        let cursor = web_int_handle(rt, inspection.cursor as i64);
        let history = rt.heap.alloc_empty_list();
        for transaction in inspection.history {
            let transaction = web_store_transaction_handle(rt, transaction);
            let _ = rt.heap.list_push_int(history, transaction);
        }
        let events = rt.heap.alloc_empty_list();
        for event in inspection.events {
            let event = web_store_event_handle(rt, event);
            let _ = rt.heap.list_push_int(events, event);
        }
        let limit = web_int_handle(rt, inspection.history_limit as i64);
        let _ = rt.heap.record_set_int(value, 0, inspection.value);
        let _ = rt.heap.record_set_int(value, 1, generation);
        let _ = rt.heap.record_set_int(value, 2, cursor);
        let _ = rt.heap.record_set_int(value, 3, history);
        let _ = rt.heap.record_set_int(value, 4, events);
        let _ = rt.heap.record_set_int(value, 5, limit);
        let _ = rt.heap.record_set_bool(value, 6, inspection.history_enabled);
        value
    })
}

fn jet_jit_web_store_derived(store: i64, callback: i64) -> i64 {
    let callback = web_callable(callback).expect("checked Store derived callback");
    let derived = web_rt::jet_web_store_derived(&web_store(store), move |value| {
        crate::runtime_host::invoke_universal_unary(callback, value)
            .expect("checked Store derived callback thunk")
    });
    with_rt(|rt| {
        rt.reactive.deriveds.push(crate::Reactive::DerivedSlot::Word(derived));
        rt.reactive.deriveds.len() as i64
    })
}

fn jet_jit_web_store_selector(store: i64, callback: i64) -> i64 {
    let callback = web_callable(callback).expect("checked Store selector callback");
    let derived = web_rt::jet_web_store_selector(&web_store(store), move |value| {
        crate::runtime_host::invoke_universal_unary(callback, value)
            .expect("checked Store selector callback thunk")
    });
    with_rt(|rt| {
        rt.reactive.deriveds.push(crate::Reactive::DerivedSlot::Word(derived));
        rt.reactive.deriveds.len() as i64
    })
}

fn jet_jit_web_store_subscribe(store: i64, callback: i64) -> i64 {
    let callback = web_callable(callback).expect("checked Store subscription callback");
    let subscription = web_rt::jet_web_store_subscribe(&web_store(store), move |value| {
        let _ = crate::runtime_host::invoke_universal_unary(callback, value)
            .expect("checked Store subscription callback thunk");
    });
    with_rt(|rt| {
        rt.web.store_subscriptions.push(subscription);
        rt.web.store_subscriptions.len() as i64
    })
}

fn jet_jit_web_store_subscribe_selector(store: i64, selector: i64, callback: i64) -> i64 {
    let selector = web_callable(selector).expect("checked Store subscription selector");
    let callback = web_callable(callback).expect("checked Store subscription callback");
    let subscription = web_rt::jet_web_store_subscribe_selector(
        &web_store(store),
        move |value| crate::runtime_host::invoke_universal_unary(selector, value)
            .expect("checked Store subscription selector thunk"),
        move |value| {
            let _ = crate::runtime_host::invoke_universal_unary(callback, value)
                .expect("checked Store subscription callback thunk");
        },
    );
    with_rt(|rt| {
        rt.web.store_subscriptions.push(subscription);
        rt.web.store_subscriptions.len() as i64
    })
}

fn web_store_subscription(subscription: i64) -> web_rt::JetWebStoreSubscription {
    with_rt(|rt| {
        rt.web.store_subscriptions.get(subscription.saturating_sub(1) as usize)
            .cloned().expect("checked WebStoreSubscription handle")
    })
}

fn jet_jit_web_store_subscription_unsubscribe(subscription: i64) {
    web_rt::jet_web_store_subscription_unsubscribe(&web_store_subscription(subscription));
}

fn jet_jit_web_store_subscription_active(subscription: i64) -> i64 {
    i64::from(web_rt::jet_web_store_subscription_active(&web_store_subscription(subscription)))
}

fn web_invoke_update(callback: crate::runtime_host::JitCallableSlot, value: &mut i64) {
    // The generated thunk decodes this word into a checked typed place and
    // writes the final carrier back before this synchronous borrow ends.
    let _ = crate::runtime_host::invoke_universal_unary(callback, value as *mut i64 as i64)
        .expect("checked mutable Web callback thunk");
}

fn web_store_update_arguments(
    action: i64,
    changed_fields: i64,
    callback: i64,
) -> (String, Vec<String>, impl FnOnce(&mut i64)) {
    let callback = web_callable(callback).expect("checked Store update callback");
    let (action, fields) = with_rt(|rt| (
        rt.heap.clone_string(action).expect("checked Store action"),
        web_string_list_value(rt, changed_fields),
    ));
    (action, fields, move |value| web_invoke_update(callback, value))
}

fn jet_jit_web_store_transaction(store: i64, action: i64, fields: i64, callback: i64) -> i64 {
    let (action, fields, update) = web_store_update_arguments(action, fields, callback);
    let transaction = web_rt::jet_web_store_transaction(&web_store(store), action, fields, update);
    with_rt(|rt| web_store_transaction_handle(rt, transaction))
}

fn jet_jit_web_store_update(store: i64, action: i64, fields: i64, callback: i64) -> i64 {
    let (action, fields, update) = web_store_update_arguments(action, fields, callback);
    let transaction = web_rt::jet_web_store_update(&web_store(store), action, fields, update);
    with_rt(|rt| web_store_transaction_handle(rt, transaction))
}

fn jet_jit_web_store_batch(store: i64, action: i64, fields: i64, callback: i64) -> i64 {
    let (action, fields, update) = web_store_update_arguments(action, fields, callback);
    let transaction = web_rt::jet_web_store_batch(&web_store(store), action, fields, update);
    with_rt(|rt| web_store_transaction_handle(rt, transaction))
}

fn jet_jit_web_store_optimistic(store: i64, action: i64, fields: i64, callback: i64) -> i64 {
    let (action, fields, update) = web_store_update_arguments(action, fields, callback);
    let patch = web_rt::jet_web_store_optimistic(&web_store(store), action, fields, update);
    with_rt(|rt| {
        rt.web.store_patches.push(Some(patch));
        rt.web.store_patches.len() as i64
    })
}

fn jet_jit_web_store_patch(store: i64, action: i64, fields: i64, callback: i64) -> i64 {
    let (action, fields, update) = web_store_update_arguments(action, fields, callback);
    let patch = web_rt::jet_web_store_patch(&web_store(store), action, fields, update);
    with_rt(|rt| {
        rt.web.store_patches.push(Some(patch));
        rt.web.store_patches.len() as i64
    })
}

fn jet_jit_web_store_patch_generation(patch: i64) -> i64 {
    let generation = with_rt(|rt| {
        let patch = rt.web.store_patches.get(patch.saturating_sub(1) as usize)
            .and_then(Option::as_ref).expect("checked live WebStorePatch handle");
        web_rt::jet_web_store_patch_generation(patch)
    });
    with_rt(|rt| web_int_handle(rt, generation))
}

fn jet_jit_web_store_patch_transaction(patch: i64) -> i64 {
    let transaction = with_rt(|rt| {
        let patch = rt.web.store_patches.get(patch.saturating_sub(1) as usize)
            .and_then(Option::as_ref).expect("checked live WebStorePatch handle");
        web_rt::jet_web_store_patch_transaction(patch)
    });
    with_rt(|rt| web_store_transaction_handle(rt, transaction))
}

fn jet_jit_web_store_patch_active(patch: i64) -> i64 {
    with_rt(|rt| {
        let patch = rt.web.store_patches.get(patch.saturating_sub(1) as usize)
            .and_then(Option::as_ref).expect("checked live WebStorePatch handle");
        i64::from(web_rt::jet_web_store_patch_active(patch))
    })
}

fn web_store_take_patch(patch: i64) -> web_rt::JetWebStorePatch<i64> {
    with_rt(|rt| {
        rt.web.store_patches.get_mut(patch.saturating_sub(1) as usize)
            .and_then(Option::take).expect("checked live WebStorePatch handle")
    })
}

fn jet_jit_web_store_patch_commit(patch: i64) -> i64 {
    let transaction = web_rt::jet_web_store_patch_commit(web_store_take_patch(patch));
    with_rt(|rt| web_store_transaction_handle(rt, transaction))
}

fn jet_jit_web_store_patch_rollback(patch: i64) -> i64 {
    web_optional_raw(web_rt::jet_web_store_patch_rollback(web_store_take_patch(patch)))
}

host_fns! {
    struct WebHostFns;
    register: register_web_symbols;
    declare: declare_web_host_fns(module) {
        let cc = module.target_config().default_call_conv;

        let mut nullary = Signature::new(cc);
        nullary.returns.push(AbiParam::new(types::I64));
        let mut unary = Signature::new(cc);
        unary.params.push(AbiParam::new(types::I64));
        unary.returns.push(AbiParam::new(types::I64));
        let mut unary_void = Signature::new(cc);
        unary_void.params.push(AbiParam::new(types::I64));
        let nullary_void = Signature::new(cc);
        let mut binary_void = Signature::new(cc);
        binary_void.params.push(AbiParam::new(types::I64));
        binary_void.params.push(AbiParam::new(types::I64));
        let mut binary = Signature::new(cc);
        binary.params.push(AbiParam::new(types::I64));
        binary.params.push(AbiParam::new(types::I64));
        binary.returns.push(AbiParam::new(types::I64));
        let mut ternary = Signature::new(cc);
        for _ in 0..3 {
            ternary.params.push(AbiParam::new(types::I64));
        }
        ternary.returns.push(AbiParam::new(types::I64));
        let mut ternary_void = Signature::new(cc);
        for _ in 0..3 {
            ternary_void.params.push(AbiParam::new(types::I64));
        }
        let mut fourary = Signature::new(cc);
        for _ in 0..4 {
            fourary.params.push(AbiParam::new(types::I64));
        }
        fourary.returns.push(AbiParam::new(types::I64));
        let mut penta = Signature::new(cc);
        for _ in 0..5 {
            penta.params.push(AbiParam::new(types::I64));
        }
        penta.returns.push(AbiParam::new(types::I64));
        let mut hexa = Signature::new(cc);
        for _ in 0..6 {
            hexa.params.push(AbiParam::new(types::I64));
        }
        hexa.returns.push(AbiParam::new(types::I64));
        let mut app_method = Signature::new(cc);
        for _ in 0..7 {
            app_method.params.push(AbiParam::new(types::I64));
        }
        app_method.returns.push(AbiParam::new(types::I64));

        let mut route_decode = Signature::new(cc);
        route_decode.params.push(AbiParam::new(types::I64));
        route_decode.params.push(AbiParam::new(types::I64));
        route_decode.returns.push(AbiParam::new(types::I64));

        let mut route_encode = Signature::new(cc);
        for _ in 0..3 {
            route_encode.params.push(AbiParam::new(types::I64));
        }
        route_encode.returns.push(AbiParam::new(types::I64));

        let mut route_stringify = Signature::new(cc);
        route_stringify.params.push(AbiParam::new(types::I64));
        route_stringify.params.push(AbiParam::new(types::I64));
        route_stringify.returns.push(AbiParam::new(types::I64));

    }
    on: "jet_jit_web_on" => jet_jit_web_on: ternary_void;
    value: "jet_jit_web_value" => jet_jit_web_value: unary;
    app: "jet_jit_web_app" => jet_jit_web_app: nullary;
    page: "jet_jit_web_page" => jet_jit_web_page: binary;
    app_method: "jet_jit_web_app_method" => jet_jit_web_app_method: app_method;
    route_decode_tree: "jet_jit_web_route_decode_tree" => jet_jit_web_route_decode_tree: route_decode;
    route_encode_result: "jet_jit_web_route_encode_result" => jet_jit_web_route_encode_result: route_encode;
    route_stringify_error: "jet_jit_web_route_stringify_error" => jet_jit_web_route_stringify_error: route_stringify;
    storage_get: "jet_jit_web_storage_get" => jet_jit_web_storage_get: unary;
    storage_remove: "jet_jit_web_storage_remove" => jet_jit_web_storage_remove: unary_void;
    storage_set: "jet_jit_web_storage_set" => jet_jit_web_storage_set: binary_void;
    storage_clear: "jet_jit_web_storage_clear" => jet_jit_web_storage_clear: nullary_void;
    devserver_app: "jet_jit_devserver_app" => jet_jit_devserver_app: nullary;
    devserver_for_app: "jet_jit_devserver_for_app" => jet_jit_devserver_for_app: unary;
    devserver_html: "jet_jit_devserver_html" => jet_jit_devserver_html: binary;
    devserver_port: "jet_jit_devserver_port" => jet_jit_devserver_port: binary;
    devserver_serve: "jet_jit_devserver_serve" => jet_jit_devserver_serve: unary_void;
    // Canonical MIR Prelude rows (`core.web` `app`/`page`, `core.web.devserver`,
    // browser storage) resolve by the exact symbol the row declares; the
    // `jet_jit_*` spellings above stay for Core rows projected through
    // `CoreCallRecord::jit_symbol_candidates`.
    row_app: "jet_app" => jet_jit_web_app: nullary;
    app_sync: "jet_app_sync" => jet_jit_app_sync: binary;
    row_page: "jet_web_page" => jet_jit_web_page: binary;
    row_storage_get: "jet_web_storage_get" => jet_jit_web_storage_get: unary;
    row_storage_remove: "jet_web_storage_remove" => jet_jit_web_storage_remove: unary_void;
    row_devserver_app: "jet_devserver_app" => jet_jit_devserver_app: nullary;
    row_devserver_for_app: "jet_devserver_for_app" => jet_jit_devserver_for_app: unary;
    web_router_new: "jet_web_router_new" => jet_jit_web_router_new: nullary;
    web_router_navigate: "jet_web_router_navigate" => jet_jit_web_router_navigate: binary;
    web_router_preload: "jet_web_router_preload" => jet_jit_web_router_preload: binary;
    web_router_current: "jet_web_router_current" => jet_jit_web_router_current: unary;
    web_router_show: "jet_web_router_show" => jet_jit_web_router_show: unary;
    web_router_link: "jet_web_router_link" => jet_jit_web_router_link: fourary;
    web_router_abort: "jet_web_router_abort" => jet_jit_web_router_abort: unary;
    web_router_stale: "jet_web_router_stale" => jet_jit_web_router_stale: binary;
    web_router_invalidate: "jet_web_router_invalidate" => jet_jit_web_router_invalidate: binary;
    web_router_collect: "jet_web_router_collect" => jet_jit_web_router_collect: binary;
    web_router_cache_state: "jet_web_router_cache_state" => jet_jit_web_router_cache_state: binary;
    web_router_cache_show: "jet_web_router_cache_show" => jet_jit_web_router_cache_show: unary;
    web_query_new: "jet_web_query_new" => jet_jit_web_query_new: binary;
    web_query_live: "jet_web_query_live" => jet_jit_web_query_live: fourary;
    web_query_subscribe: "jet_web_query_subscribe" => jet_jit_web_query_subscribe: unary;
    web_query_invalidate: "jet_web_query_invalidate" => jet_jit_web_query_invalidate: unary;
    web_query_get: "jet_web_query_get" => jet_jit_web_query_get: unary;
    web_query_state: "jet_web_query_state" => jet_jit_web_query_state: unary;
    web_query_show: "jet_web_query_show" => jet_jit_web_query_show: unary;
    web_query_queue: "jet_web_query_queue" => jet_jit_web_query_queue: binary;
    web_query_refresh: "jet_web_query_refresh" => jet_jit_web_query_refresh: unary;
    web_query_facts: "jet_web_query_facts" => jet_jit_web_query_facts: unary;
    web_query_cancel: "jet_web_query_cancel" => jet_jit_web_query_cancel: unary;
    web_query_set_online: "jet_web_query_set_online" => jet_jit_web_query_set_online: binary_void;
    web_query_set_mode: "jet_web_query_set_mode" => jet_jit_web_query_set_mode: binary_void;
    web_query_mutation_state: "jet_web_query_mutation_state" => jet_jit_web_query_mutation_state: unary;
    web_query_state_signal: "jet_web_query_state_signal" => jet_jit_web_query_state_signal: unary;
    web_query_mutation_signal: "jet_web_query_mutation_signal" => jet_jit_web_query_mutation_signal: unary;
    web_query_mutate: "jet_web_query_mutate" => jet_jit_web_query_mutate: fourary;
    web_query_mutate_with_invalidations: "jet_web_query_mutate_with_invalidations" => jet_jit_web_query_mutate_with_invalidations: penta;
    web_query_retry: "jet_web_query_retry" => jet_jit_web_query_retry: binary;
    web_form: "jet_web_form" => jet_jit_web_form: binary;
    web_forms_new: "jet_web_forms_new" => jet_jit_web_forms_new: unary;
    web_forms_field: "jet_web_forms_field" => jet_jit_web_forms_field: fourary;
    web_forms_set: "jet_web_forms_set" => jet_jit_web_forms_set: ternary;
    web_forms_blur: "jet_web_forms_blur" => jet_jit_web_forms_blur: binary;
    web_forms_validate: "jet_web_forms_validate" => jet_jit_web_forms_validate: unary;
    web_forms_validate_async: "jet_web_forms_validate_async" => jet_jit_web_forms_validate_async: unary;
    web_forms_submit: "jet_web_forms_submit" => jet_jit_web_forms_submit: unary;
    web_forms_no_script: "jet_web_forms_no_script" => jet_jit_web_forms_no_script: unary;
    web_forms_html: "jet_web_forms_html" => jet_jit_web_forms_html: unary;
    web_forms_show: "jet_web_forms_show" => jet_jit_web_forms_show: unary;
    web_forms_input: "jet_web_forms_input" => jet_jit_web_forms_input: binary;
    web_forms_input_rename: "jet_web_forms_input_rename" => jet_jit_web_forms_input_rename: ternary;
    web_forms_input_exclude: "jet_web_forms_input_exclude" => jet_jit_web_forms_input_exclude: binary;
    web_forms_input_group: "jet_web_forms_input_group" => jet_jit_web_forms_input_group: ternary;
    web_forms_input_replace: "jet_web_forms_input_replace" => jet_jit_web_forms_input_replace: ternary;
    web_forms_typed: "jet_web_forms_typed" => jet_jit_web_forms_typed: binary;
    web_forms_typed_set: "jet_web_forms_typed_set" => jet_jit_web_forms_typed_set: ternary;
    web_forms_typed_set_async_validator: "jet_web_forms_typed_set_async_validator" => jet_jit_web_forms_typed_set_async_validator: penta;
    web_forms_typed_set_action: "jet_web_forms_typed_set_action" => jet_jit_web_forms_typed_set_action: binary_void;
    web_forms_typed_blur: "jet_web_forms_typed_blur" => jet_jit_web_forms_typed_blur: binary;
    web_forms_typed_validate: "jet_web_forms_typed_validate" => jet_jit_web_forms_typed_validate: unary;
    web_forms_typed_validate_async: "jet_web_forms_typed_validate_async" => jet_jit_web_forms_typed_validate_async: unary;
    web_forms_typed_submit: "jet_web_forms_typed_submit" => jet_jit_web_forms_typed_submit: unary;
    web_forms_typed_submit_async: "jet_web_forms_typed_submit_async" => jet_jit_web_forms_typed_submit_async: unary;
    web_forms_typed_no_script: "jet_web_forms_typed_no_script" => jet_jit_web_forms_typed_no_script: unary;
    web_forms_typed_post: "jet_web_forms_typed_post" => jet_jit_web_forms_typed_post: binary;
    web_forms_typed_decode_post: "jet_web_forms_typed_decode_post" => jet_jit_web_forms_typed_decode_post: binary;
    web_forms_typed_html: "jet_web_forms_typed_html" => jet_jit_web_forms_typed_html: unary;
    web_forms_typed_show: "jet_web_forms_typed_show" => jet_jit_web_forms_typed_show: unary;
    web_forms_typed_state: "jet_web_forms_typed_state" => jet_jit_web_forms_typed_state: unary;
    web_forms_typed_lifecycle: "jet_web_forms_typed_lifecycle" => jet_jit_web_forms_typed_lifecycle: unary;
    web_forms_typed_errors: "jet_web_forms_typed_errors" => jet_jit_web_forms_typed_errors: unary;
    web_forms_typed_focus: "jet_web_forms_typed_focus" => jet_jit_web_forms_typed_focus: binary;
    web_forms_typed_cancel: "jet_web_forms_typed_cancel" => jet_jit_web_forms_typed_cancel: unary_void;
    web_forms_typed_select_field: "jet_web_forms_typed_select_field" => jet_jit_web_forms_typed_select_field: binary;
    web_forms_typed_validation_wait: "jet_web_forms_typed_validation_wait" => jet_jit_web_forms_typed_validation_wait: unary;
    web_forms_typed_validation_cancel: "jet_web_forms_typed_validation_cancel" => jet_jit_web_forms_typed_validation_cancel: unary_void;
    web_forms_typed_submission_wait: "jet_web_forms_typed_submission_wait" => jet_jit_web_forms_typed_submission_wait: unary;
    web_forms_typed_submission_cancel: "jet_web_forms_typed_submission_cancel" => jet_jit_web_forms_typed_submission_cancel: unary_void;
    web_forms_action_error: "jet_web_forms_action_error" => jet_jit_web_forms_action_error: nullary;
    web_forms_action_field_error: "jet_web_forms_action_field_error" => jet_jit_web_forms_action_field_error: ternary;
    web_forms_action_form_error: "jet_web_forms_action_form_error" => jet_jit_web_forms_action_form_error: binary;
    web_table_new: "jet_web_table" => jet_jit_web_table_new: binary;
    web_table_column: "jet_web_table_column" => jet_jit_web_table_column: ternary;
    web_table_sort: "jet_web_table_sort" => jet_jit_web_table_sort: ternary;
    web_table_filter: "jet_web_table_filter" => jet_jit_web_table_filter: binary;
    web_table_page: "jet_web_table_page" => jet_jit_web_table_page: ternary;
    web_table_new_keyed: "jet_web_table_new_keyed" => jet_jit_web_table_new_keyed: ternary;
    web_table_with_column: "jet_web_table_with_column" => jet_jit_web_table_with_column: binary;
    web_table_with_server_page: "jet_web_table_with_server_page" => jet_jit_web_table_with_server_page: binary;
    web_table_state: "jet_web_table_state" => jet_jit_web_table_state: unary;
    web_table_facts: "jet_web_table_facts" => jet_jit_web_table_facts: unary;
    web_table_keys: "jet_web_table_keys" => jet_jit_web_table_keys: unary;
    web_table_page_state: "jet_web_table_page_state" => jet_jit_web_table_page_state: unary;
    web_table_sort_by: "jet_web_table_sort_by" => jet_jit_web_table_sort_by: ternary;
    web_table_filter_by: "jet_web_table_filter_by" => jet_jit_web_table_filter_by: ternary;
    web_table_paginate: "jet_web_table_paginate" => jet_jit_web_table_paginate: ternary;
    web_table_set_rows: "jet_web_table_set_rows" => jet_jit_web_table_set_rows: binary;
    web_table_set_selected: "jet_web_table_set_selected" => jet_jit_web_table_set_selected: ternary;
    web_table_toggle_selection: "jet_web_table_toggle_selection" => jet_jit_web_table_toggle_selection: binary;
    web_table_clear_selection: "jet_web_table_clear_selection" => jet_jit_web_table_clear_selection: unary;
    web_table_selected_keys: "jet_web_table_selected_keys" => jet_jit_web_table_selected_keys: unary;
    web_table_selected_rows: "jet_web_table_selected_rows" => jet_jit_web_table_selected_rows: unary;
    web_table_insert_row: "jet_web_table_insert_row" => jet_jit_web_table_insert_row: binary;
    web_table_replace_row: "jet_web_table_replace_row" => jet_jit_web_table_replace_row: ternary;
    web_table_update_row: "jet_web_table_update_row" => jet_jit_web_table_update_row: ternary;
    web_table_remove_row: "jet_web_table_remove_row" => jet_jit_web_table_remove_row: binary;
    web_table_first_page: "jet_web_table_first_page" => jet_jit_web_table_first_page: unary;
    web_table_next_page: "jet_web_table_next_page" => jet_jit_web_table_next_page: unary;
    web_table_last_page: "jet_web_table_last_page" => jet_jit_web_table_last_page: unary;
    web_table_visible_rows: "jet_web_table_visible_rows" => jet_jit_web_table_visible_rows: binary;
    web_virtual_plan: "jet_web_virtual_plan" => jet_jit_web_virtual_plan: hexa;
    web_virtual_plan_measured: "jet_web_virtual_plan_measured" => jet_jit_web_virtual_plan_measured: app_method;
    web_virtual_plan_from_sizes: "jet_web_virtual_plan_from_sizes" => jet_jit_web_virtual_plan_from_sizes: app_method;
    web_virtual_plan_measure: "jet_web_virtual_plan_measure" => jet_jit_web_virtual_plan_measure: ternary;
    web_virtual_plan_slice: "jet_web_virtual_plan_slice" => jet_jit_web_virtual_plan_slice: binary;
    web_virtual_plan_indices: "jet_web_virtual_plan_indices" => jet_jit_web_virtual_plan_indices: unary;
    web_virtual_plan_facts: "jet_web_virtual_plan_facts" => jet_jit_web_virtual_plan_facts: unary;
    web_virtual_plan_viewport: "jet_web_virtual_plan_viewport" => jet_jit_web_virtual_plan_viewport: unary;
    web_virtual_plan_viewport_state: "jet_web_virtual_plan_viewport_state" => jet_jit_web_virtual_plan_viewport_state: unary;
    web_virtual_plan_scroll_to: "jet_web_virtual_plan_scroll_to" => jet_jit_web_virtual_plan_scroll_to: binary_void;
    web_virtual_plan_resize: "jet_web_virtual_plan_resize" => jet_jit_web_virtual_plan_resize: ternary_void;
    web_virtual_plan_viewport_measure: "jet_web_virtual_plan_viewport_measure" => jet_jit_web_virtual_plan_viewport_measure: ternary_void;
    web_virtual_window: "jet_web_virtual_window" => jet_jit_web_virtual_window: penta;
    web_virtual_window_measured: "jet_web_virtual_window_measured" => jet_jit_web_virtual_window_measured: hexa;
    web_virtual_slice: "jet_web_virtual_slice" => jet_jit_web_virtual_slice: binary;
    web_virtual_indices: "jet_web_virtual_indices" => jet_jit_web_virtual_indices: unary;
    web_virtual_window_facts: "jet_web_virtual_window_facts" => jet_jit_web_virtual_window_facts: unary;
    web_store_new: "jet_web_store" => jet_jit_web_store_new: binary;
    web_store_with_history: "jet_web_store_with_history" => jet_jit_web_store_with_history: ternary;
    web_store_transaction: "jet_web_store_transaction" => jet_jit_web_store_transaction: fourary;
    web_store_value: "jet_web_store_value" => jet_jit_web_store_value: unary;
    web_store_signal: "jet_web_store_signal" => jet_jit_web_store_signal: unary;
    web_store_state_signal: "jet_web_store_state_signal" => jet_jit_web_store_state_signal: unary;
    web_store_set: "jet_web_store_set" => jet_jit_web_store_set: binary;
    web_store_set_state: "jet_web_store_set_state" => jet_jit_web_store_set_state: binary;
    web_store_back: "jet_web_store_back" => jet_jit_web_store_back: unary;
    web_store_forward: "jet_web_store_forward" => jet_jit_web_store_forward: unary;
    web_store_jump: "jet_web_store_jump" => jet_jit_web_store_jump: binary;
    web_store_scrub: "jet_web_store_scrub" => jet_jit_web_store_scrub: binary;
    web_store_restore: "jet_web_store_restore" => jet_jit_web_store_restore: binary;
    web_store_history: "jet_web_store_history" => jet_jit_web_store_history: unary;
    web_store_history_at: "jet_web_store_history_at" => jet_jit_web_store_history_at: binary;
    web_store_events: "jet_web_store_events" => jet_jit_web_store_events: unary;
    web_store_events_since: "jet_web_store_events_since" => jet_jit_web_store_events_since: binary;
    web_store_clear_history: "jet_web_store_clear_history" => jet_jit_web_store_clear_history: unary_void;
    web_store_set_history_limit: "jet_web_store_set_history_limit" => jet_jit_web_store_set_history_limit: binary_void;
    web_store_history_enabled: "jet_web_store_history_enabled" => jet_jit_web_store_history_enabled: unary;
    web_store_history_limit: "jet_web_store_history_limit" => jet_jit_web_store_history_limit: unary;
    web_store_cursor: "jet_web_store_cursor" => jet_jit_web_store_cursor: unary;
    web_store_current_generation: "jet_web_store_current_generation" => jet_jit_web_store_current_generation: unary;
    web_store_facts_json: "jet_web_store_facts_json" => jet_jit_web_store_facts_json: unary;
    web_store_event_json: "jet_web_store_event_json" => jet_jit_web_store_event_json: unary;
    web_store_inspect: "jet_web_store_inspect" => jet_jit_web_store_inspect: unary;
    web_store_derived: "jet_web_store_derived" => jet_jit_web_store_derived: binary;
    web_store_selector: "jet_web_store_selector" => jet_jit_web_store_selector: binary;
    web_store_subscribe: "jet_web_store_subscribe" => jet_jit_web_store_subscribe: binary;
    web_store_subscribe_selector: "jet_web_store_subscribe_selector" => jet_jit_web_store_subscribe_selector: ternary;
    web_store_subscription_unsubscribe: "jet_web_store_subscription_unsubscribe" => jet_jit_web_store_subscription_unsubscribe: unary_void;
    web_store_subscription_active: "jet_web_store_subscription_active" => jet_jit_web_store_subscription_active: unary;
    web_store_update: "jet_web_store_update" => jet_jit_web_store_update: fourary;
    web_store_batch: "jet_web_store_batch" => jet_jit_web_store_batch: fourary;
    web_store_optimistic: "jet_web_store_optimistic" => jet_jit_web_store_optimistic: fourary;
    web_store_patch: "jet_web_store_patch" => jet_jit_web_store_patch: fourary;
    web_store_patch_generation: "jet_web_store_patch_generation" => jet_jit_web_store_patch_generation: unary;
    web_store_patch_transaction: "jet_web_store_patch_transaction" => jet_jit_web_store_patch_transaction: unary;
    web_store_patch_active: "jet_web_store_patch_active" => jet_jit_web_store_patch_active: unary;
    web_store_patch_commit: "jet_web_store_patch_commit" => jet_jit_web_store_patch_commit: unary;
    web_store_patch_rollback: "jet_web_store_patch_rollback" => jet_jit_web_store_patch_rollback: unary;
}
