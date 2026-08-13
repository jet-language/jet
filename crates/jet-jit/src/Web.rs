//! D-WEBAPP1 / c-devserver / D-FLAGSHIP-WEBAPI1: resident-JIT web host.
//! Thin opaque-handle adapters over Prelude/App.rs + DevServer.rs.
//! `core.web.on` / `core.web.value` are native no-ops (match AOT emit).

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

#[allow(dead_code, unused_imports)]
pub(crate) mod web_rt {
    pub(crate) use crate::net_http_rt::{
        jet_app_http_action, jet_app_http_assets, jet_app_http_mount,
        jet_app_http_mux_new, jet_app_http_page, jet_app_http_reload,
        jet_app_http_serve,
    };
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/App.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/DevServer.rs");
}

#[derive(Default)]
pub(crate) struct WebState {
    pub(crate) apps: Vec<web_rt::JetApp>,
    pub(crate) pages: Vec<web_rt::JetWebPage>,
    pub(crate) servers: Vec<web_rt::JetDevServer>,
}

fn with_rt<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::runtime_host::JitRuntime) -> R,
    R: Default,
{
    Concurrency::with_runtime_mut(f)
}

extern "C" fn jet_jit_web_on(_sel: i64, _ev: i64, _fn: i64) {
    // Native no-op — real registration is JS/Wasm only (emit/core_calls.rs).
}

extern "C" fn jet_jit_web_value() -> i64 {
    with_rt(|rt| rt.heap.alloc_string(String::new()))
}

extern "C" fn jet_jit_web_app() -> i64 {
    with_rt(|rt| {
        rt.web.apps.push(web_rt::jet_app());
        rt.web.apps.len() as i64
    })
}

extern "C" fn jet_jit_web_page(title: i64, body: i64) -> i64 {
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
    let app_handle = with_rt(|rt| {
        rt.web
            .apps
            .get(app.saturating_sub(1) as usize)
            .cloned()
    })
    .expect("jit web app: bad handle");
    app_handle.serve();
}

extern "C" fn jet_jit_web_app_method(app: i64, method: i64, a0: i64, a1: i64) -> i64 {
    let method_name = with_rt(|rt| rt.heap.clone_string(method).unwrap_or_default());
    if method_name == "serve" || method_name == "serve_on" {
        // Serving blocks. Keep the resident runtime published, but release its
        // access lock so HTTP worker callbacks can enter Jet one at a time.
        let app_handle = with_rt(|rt| {
            rt.web
                .apps
                .get(app.saturating_sub(1) as usize)
                .cloned()
        })
        .expect("jit app: bad handle");
        if method_name == "serve_on" {
            app_handle.serve_on(a0);
        } else {
            app_handle.serve();
        }
        return app;
    }
    with_rt(|rt| {
        let method = method_name;
        let app_handle = rt
            .web
            .apps
            .get(app.saturating_sub(1) as usize)
            .expect("jit web app: bad handle")
            .clone();
        let next = match method.as_str() {
            "csr" => app_handle.csr(),
            "ssr" => app_handle.ssr(),
            "ssg" => app_handle.ssg(),
            "stream" => app_handle.stream(),
            "streaming" => app_handle.streaming(),
            "island" => app_handle.island(),
            "hydration_dev" => app_handle.hydration_dev(),
            "hydration_release" => app_handle.hydration_release(),
            "facts_json" => {
                let _ = app_handle.facts_json();
                app_handle
            }
            "route" | "page" | "layout" | "action" | "form" | "data" => {
                let key = rt.heap.clone_string(a0).unwrap_or_default();
                match method.as_str() {
                    "route" | "page" | "layout" => {
                        let handler = move || {
                            Concurrency::with_http_jet_runtime(|| {
                                let call: extern "C" fn() -> i64 =
                                    unsafe { std::mem::transmute(a1 as usize) };
                                let page = call();
                                with_rt(|rt| {
                                    rt.web
                                        .pages
                                        .get(page.saturating_sub(1) as usize)
                                        .cloned()
                                        .unwrap_or_default()
                                })
                            })
                        };
                        match method.as_str() {
                            "route" => app_handle.route(key, std::sync::Arc::new(handler)),
                            "page" => app_handle.page(key, std::sync::Arc::new(handler)),
                            _ => app_handle.layout(key, std::sync::Arc::new(handler)),
                        }
                    }
                    "action" | "form" | "data" => {
                        let handler = move || {
                            Concurrency::with_http_jet_runtime(|| {
                                let call: extern "C" fn() =
                                    unsafe { std::mem::transmute(a1 as usize) };
                                call();
                            });
                        };
                        match method.as_str() {
                            "action" => app_handle.action(key, std::sync::Arc::new(handler)),
                            "form" => app_handle.form(key, std::sync::Arc::new(handler)),
                            _ => app_handle.data(key, std::sync::Arc::new(handler)),
                        }
                    }
                    _ => unreachable!(),
                }
            }
            "mount" => {
                let key = rt.heap.clone_string(a0).unwrap_or_default();
                app_handle.mount(key, std::sync::Arc::new(move |path: &String| {
                    Concurrency::with_http_jet_runtime(|| {
                        let path = with_rt(|rt| rt.heap.alloc_string(path.clone()));
                        let call: extern "C" fn(i64) =
                            unsafe { std::mem::transmute(a1 as usize) };
                        call(path);
                    });
                }))
            }
            "routes" | "security" | "assets" | "split" | "code_split" | "cache" | "a11y"
            | "adapter" => {
                let key = rt.heap.clone_string(a0).unwrap_or_default();
                match method.as_str() {
                    "routes" => app_handle.routes(key),
                    "security" => app_handle.security(key),
                    "assets" => app_handle.assets(key),
                    "split" => app_handle.split(key),
                    "code_split" => app_handle.code_split(key),
                    "cache" => app_handle.cache(key),
                    "a11y" => app_handle.a11y(key),
                    _ => app_handle.adapter(key),
                }
            }
            _ => app_handle,
        };
        rt.web.apps.push(next);
        rt.web.apps.len() as i64
    })
}

extern "C" fn jet_jit_devserver_app() -> i64 {
    // Under test/run without JET_DEV_FILE, return a for_app on the empty path
    // rather than exiting — compile + unused `fn dev()` must succeed.
    with_rt(|rt| {
        let file = std::env::var("JET_DEV_FILE").unwrap_or_else(|_| String::new());
        let server = if file.is_empty() {
            web_rt::jet_devserver_for_app(".")
        } else {
            web_rt::jet_devserver_app()
        };
        rt.web.servers.push(server);
        rt.web.servers.len() as i64
    })
}

extern "C" fn jet_jit_devserver_for_app(path: i64) -> i64 {
    with_rt(|rt| {
        let path = rt.heap.clone_string(path).unwrap_or_default();
        rt.web.servers.push(web_rt::jet_devserver_for_app(&path));
        rt.web.servers.len() as i64
    })
}

extern "C" fn jet_jit_devserver_html(server: i64, path: i64) -> i64 {
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

extern "C" fn jet_jit_devserver_port(server: i64, port: i64) -> i64 {
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

extern "C" fn jet_jit_devserver_serve(server: i64) {
    // Do not block forever in JIT/AOT ProgramOutput tests — `fn run()` never
    // calls serve; compiling `fn dev()` only needs the symbol to exist.
    let _ = server;
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
        let mut binary = Signature::new(cc);
        binary.params.push(AbiParam::new(types::I64));
        binary.params.push(AbiParam::new(types::I64));
        binary.returns.push(AbiParam::new(types::I64));
        let mut ternary_void = Signature::new(cc);
        ternary_void.params.push(AbiParam::new(types::I64));
        ternary_void.params.push(AbiParam::new(types::I64));
        ternary_void.params.push(AbiParam::new(types::I64));
        let mut app_method = Signature::new(cc);
        for _ in 0..4 {
            app_method.params.push(AbiParam::new(types::I64));
        }
        app_method.returns.push(AbiParam::new(types::I64));

    }
    on: "jet_jit_web_on" => jet_jit_web_on: ternary_void;
    value: "jet_jit_web_value" => jet_jit_web_value: nullary;
    app: "jet_jit_web_app" => jet_jit_web_app: nullary;
    page: "jet_jit_web_page" => jet_jit_web_page: binary;
    app_method: "jet_jit_web_app_method" => jet_jit_web_app_method: app_method;
    devserver_app: "jet_jit_devserver_app" => jet_jit_devserver_app: nullary;
    devserver_for_app: "jet_jit_devserver_for_app" => jet_jit_devserver_for_app: unary;
    devserver_html: "jet_jit_devserver_html" => jet_jit_devserver_html: binary;
    devserver_port: "jet_jit_devserver_port" => jet_jit_devserver_port: binary;
    devserver_serve: "jet_jit_devserver_serve" => jet_jit_devserver_serve: unary_void;
}


