// D-WEBAPP1=D / D-WEBAUTHOR1=D: `core.web.app` — statically known application
// builder value. Sema owns the typed graph; this runtime handle records the
// same edges for `facts_json()` / serve dogfood. Std-only (I6).

mod jet_app_impl {
    use std::sync::{Arc, Mutex};

    type PageHandler = Arc<dyn Fn() -> JetWebPage + Send + Sync>;
    type ActionHandler = Arc<dyn Fn() + Send + Sync>;
    type MountHandler = Arc<dyn Fn(&String) + Send + Sync>;

    #[derive(Default)]
    struct JetAppState {
        routes: Vec<(String, PageHandler, String)>,
        actions: Vec<(String, ActionHandler, String)>,
        mounts: Vec<(String, MountHandler)>,
        routes_from: Vec<String>,
        security: Vec<String>,
        assets: Vec<String>,
        split: Vec<String>,
        cache: Vec<String>,
        a11y: Vec<String>,
        adapters: Vec<String>,
        render: String,
        hydration: String,
    }

    #[derive(Clone)]
    pub struct JetApp {
        state: Arc<Mutex<JetAppState>>,
    }

    pub fn jet_app() -> JetApp {
        JetApp {
            state: Arc::new(Mutex::new(JetAppState {
                render: "csr".to_string(),
                hydration: "dev-overlay".to_string(),
                ..JetAppState::default()
            })),
        }
    }

    #[derive(Clone, Default)]
    pub struct JetWebPage {
        pub title: String,
        pub body: String,
    }

    pub fn jet_web_page(title: String, body: String) -> JetWebPage {
        JetWebPage { title, body }
    }

    impl JetApp {
        pub fn route(&self, path: String, handler: PageHandler) -> JetApp {
            let mut state = self.state.lock().unwrap();
            let render = state.render.clone();
            state.routes.push((path, handler, render));
            self.clone()
        }

        pub fn page(&self, path: String, handler: PageHandler) -> JetApp {
            self.route(path, handler)
        }

        pub fn layout(&self, path: String, handler: PageHandler) -> JetApp {
            self.route(path, handler)
        }

        pub fn action(&self, name: String, handler: ActionHandler) -> JetApp {
            self.state.lock().unwrap().actions.push((
                name,
                handler,
                "action".to_string(),
            ));
            self.clone()
        }

        pub fn form(&self, name: String, handler: ActionHandler) -> JetApp {
            self.state.lock().unwrap().actions.push((
                name,
                handler,
                "form".to_string(),
            ));
            self.clone()
        }

        pub fn data(&self, name: String, handler: ActionHandler) -> JetApp {
            self.state.lock().unwrap().actions.push((
                name,
                handler,
                "data".to_string(),
            ));
            self.clone()
        }

        pub fn mount(&self, prefix: String, handler: MountHandler) -> JetApp {
            self.state
                .lock()
                .unwrap()
                .mounts
                .push((prefix, handler));
            self.clone()
        }

        pub fn routes(&self, root: String) -> JetApp {
            self.state.lock().unwrap().routes_from.push(root);
            self.clone()
        }

        pub fn csr(&self) -> JetApp {
            self.state.lock().unwrap().render = "csr".to_string();
            self.clone()
        }
        pub fn ssr(&self) -> JetApp {
            self.state.lock().unwrap().render = "ssr".to_string();
            self.clone()
        }
        pub fn ssg(&self) -> JetApp {
            self.state.lock().unwrap().render = "ssg".to_string();
            self.clone()
        }
        pub fn stream(&self) -> JetApp {
            self.state.lock().unwrap().render = "stream".to_string();
            self.clone()
        }
        pub fn streaming(&self) -> JetApp {
            self.stream()
        }
        pub fn island(&self) -> JetApp {
            self.state.lock().unwrap().render = "island".to_string();
            self.clone()
        }
        pub fn hydration_dev(&self) -> JetApp {
            self.state.lock().unwrap().hydration = "dev-overlay".to_string();
            self.clone()
        }
        pub fn hydration_release(&self) -> JetApp {
            self.state.lock().unwrap().hydration = "release-keep-server".to_string();
            self.clone()
        }
        pub fn security(&self, policy: String) -> JetApp {
            self.state.lock().unwrap().security.push(policy);
            self.clone()
        }
        pub fn assets(&self, path: String) -> JetApp {
            self.state.lock().unwrap().assets.push(path);
            self.clone()
        }
        pub fn split(&self, name: String) -> JetApp {
            self.state.lock().unwrap().split.push(name);
            self.clone()
        }
        pub fn code_split(&self, name: String) -> JetApp {
            self.split(name)
        }
        pub fn cache(&self, policy: String) -> JetApp {
            self.state.lock().unwrap().cache.push(policy);
            self.clone()
        }
        pub fn a11y(&self, policy: String) -> JetApp {
            self.state.lock().unwrap().a11y.push(policy);
            self.clone()
        }
        pub fn adapter(&self, name: String) -> JetApp {
            self.state.lock().unwrap().adapters.push(name);
            self.clone()
        }

        pub fn facts_json(&self) -> String {
            let s = self.state.lock().unwrap();
            let mut out = String::from("{\n");
            out.push_str(&format!("  \"hydration\": \"{}\",\n", s.hydration));
            out.push_str("  \"shared_tir\": true,\n");
            out.push_str("  \"routes\": [\n");
            for (i, (path, _, render)) in s.routes.iter().enumerate() {
                out.push_str(&format!(
                    "    {{\"path\": \"{path}\", \"handler\": \"callable\", \"render\": \"{render}\"}}"
                ));
                if i + 1 != s.routes.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("  ],\n");
            out.push_str("  \"actions\": [\n");
            for (i, (name, _, kind)) in s.actions.iter().enumerate() {
                out.push_str(&format!(
                    "    {{\"name\": \"{name}\", \"handler\": \"callable\", \"kind\": \"{kind}\"}}"
                ));
                if i + 1 != s.actions.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("  ],\n");
            out.push_str(&format!(
                "  \"security\": {:?},\n  \"assets\": {:?},\n  \"split\": {:?},\n  \"cache\": {:?},\n  \"a11y\": {:?},\n  \"adapters\": {:?}\n",
                s.security, s.assets, s.split, s.cache, s.a11y, s.adapters
            ));
            out.push('}');
            out
        }

        fn html(page: JetWebPage, dev: bool) -> String {
            let reload = if dev {
                r#"<script>
const source = new EventSource("/__jet/reload");
source.onmessage = () => location.reload();
</script>"#
            } else {
                ""
            };
            format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title></head><body>{}{}</body></html>",
                page.title, page.body, reload
            )
        }

        pub fn serve(&self) {
            let dev = std::env::var_os("JET_APP_DEV").is_some();
            let port = std::env::var("JET_APP_PORT")
                .ok()
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(8080);
            self.serve_port(port, dev);
        }

        pub fn serve_on(&self, port: i64) {
            let port = u16::try_from(port)
                .unwrap_or_else(|_| panic!("app port must be between 0 and 65535"));
            self.serve_port(port, false);
        }

        fn serve_port(&self, port: u16, dev: bool) {
            let mux = super::jet_app_http_mux_new();
            let state = self.state.lock().unwrap();
            for (path, handler, _) in &state.routes {
                let handler = handler.clone();
                super::jet_app_http_page(&mux, path, move || Self::html(handler(), dev));
            }
            for (name, handler, _) in &state.actions {
                let path = if name.starts_with('/') {
                    name.clone()
                } else {
                    format!("/actions/{name}")
                };
                let handler = handler.clone();
                super::jet_app_http_action(&mux, &path, move || handler());
            }
            for (prefix, handler) in &state.mounts {
                let handler = handler.clone();
                super::jet_app_http_mount(&mux, prefix, move |path| handler(path));
            }
            for root in &state.assets {
                super::jet_app_http_assets(&mux, root);
            }
            if dev {
                super::jet_app_http_reload(&mux);
            }
            drop(state);

            super::jet_app_http_serve(mux, port, dev);
        }
    }
}

pub use jet_app_impl::{jet_app, jet_web_page, JetApp, JetWebPage};
