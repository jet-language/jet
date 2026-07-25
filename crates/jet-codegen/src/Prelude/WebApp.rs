// D-WEBAPP1=D / D-WEBAUTHOR1=D: `core.web.app` — statically known application
// builder value. Sema owns the typed graph; this runtime handle records the
// same edges for `facts_json()` / serve dogfood. Std-only (I6).

mod jet_webapp_impl {
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Clone, Default)]
    struct JetWebAppState {
        routes: Vec<(String, String, String)>,
        actions: Vec<(String, String, String)>,
        mounts: Vec<(String, String)>,
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
    pub struct JetWebApp {
        state: Rc<RefCell<JetWebAppState>>,
    }

    pub fn jet_web_app() -> JetWebApp {
        JetWebApp {
            state: Rc::new(RefCell::new(JetWebAppState {
                render: "csr".to_string(),
                hydration: "dev-overlay".to_string(),
                ..JetWebAppState::default()
            })),
        }
    }

    #[derive(Clone)]
    pub struct JetWebPage {
        pub title: String,
        pub body: String,
    }

    pub fn jet_web_page(title: String, body: String) -> JetWebPage {
        JetWebPage { title, body }
    }

    impl JetWebApp {
        pub fn route(&self, path: String, handler: String) -> JetWebApp {
            let render = self.state.borrow().render.clone();
            self.state
                .borrow_mut()
                .routes
                .push((path, handler, render));
            self.clone()
        }

        pub fn page(&self, path: String, handler: String) -> JetWebApp {
            self.route(path, handler)
        }

        pub fn layout(&self, path: String, handler: String) -> JetWebApp {
            self.route(path, handler)
        }

        pub fn action(&self, name: String, handler: String) -> JetWebApp {
            self.state
                .borrow_mut()
                .actions
                .push((name, handler, "action".to_string()));
            self.clone()
        }

        pub fn form(&self, name: String, handler: String) -> JetWebApp {
            self.state
                .borrow_mut()
                .actions
                .push((name, handler, "form".to_string()));
            self.clone()
        }

        pub fn data(&self, name: String, handler: String) -> JetWebApp {
            self.state
                .borrow_mut()
                .actions
                .push((name, handler, "data".to_string()));
            self.clone()
        }

        pub fn mount(&self, prefix: String, handler: String) -> JetWebApp {
            self.state.borrow_mut().mounts.push((prefix, handler));
            self.clone()
        }

        pub fn routes(&self, root: String) -> JetWebApp {
            self.state.borrow_mut().routes_from.push(root);
            self.clone()
        }

        pub fn csr(&self) -> JetWebApp {
            self.state.borrow_mut().render = "csr".to_string();
            self.clone()
        }
        pub fn ssr(&self) -> JetWebApp {
            self.state.borrow_mut().render = "ssr".to_string();
            self.clone()
        }
        pub fn ssg(&self) -> JetWebApp {
            self.state.borrow_mut().render = "ssg".to_string();
            self.clone()
        }
        pub fn stream(&self) -> JetWebApp {
            self.state.borrow_mut().render = "stream".to_string();
            self.clone()
        }
        pub fn streaming(&self) -> JetWebApp {
            self.stream()
        }
        pub fn island(&self) -> JetWebApp {
            self.state.borrow_mut().render = "island".to_string();
            self.clone()
        }
        pub fn hydration_dev(&self) -> JetWebApp {
            self.state.borrow_mut().hydration = "dev-overlay".to_string();
            self.clone()
        }
        pub fn hydration_release(&self) -> JetWebApp {
            self.state.borrow_mut().hydration = "release-keep-server".to_string();
            self.clone()
        }
        pub fn security(&self, policy: String) -> JetWebApp {
            self.state.borrow_mut().security.push(policy);
            self.clone()
        }
        pub fn assets(&self, path: String) -> JetWebApp {
            self.state.borrow_mut().assets.push(path);
            self.clone()
        }
        pub fn split(&self, name: String) -> JetWebApp {
            self.state.borrow_mut().split.push(name);
            self.clone()
        }
        pub fn code_split(&self, name: String) -> JetWebApp {
            self.split(name)
        }
        pub fn cache(&self, policy: String) -> JetWebApp {
            self.state.borrow_mut().cache.push(policy);
            self.clone()
        }
        pub fn a11y(&self, policy: String) -> JetWebApp {
            self.state.borrow_mut().a11y.push(policy);
            self.clone()
        }
        pub fn adapter(&self, name: String) -> JetWebApp {
            self.state.borrow_mut().adapters.push(name);
            self.clone()
        }

        pub fn facts_json(&self) -> String {
            let s = self.state.borrow();
            let mut out = String::from("{\n");
            out.push_str(&format!("  \"hydration\": \"{}\",\n", s.hydration));
            out.push_str("  \"shared_tir\": true,\n");
            out.push_str("  \"routes\": [\n");
            for (i, (path, handler, render)) in s.routes.iter().enumerate() {
                out.push_str(&format!(
                    "    {{\"path\": \"{path}\", \"handler\": \"{handler}\", \"render\": \"{render}\"}}"
                ));
                if i + 1 != s.routes.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("  ],\n");
            out.push_str("  \"actions\": [\n");
            for (i, (name, handler, kind)) in s.actions.iter().enumerate() {
                out.push_str(&format!(
                    "    {{\"name\": \"{name}\", \"handler\": \"{handler}\", \"kind\": \"{kind}\"}}"
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

        pub fn serve(&self) {
            // Production dogfood hook: print facts then exit. Real HTTP serve
            // composes with core.http / jet dev; this keeps the builder runnable
            // without claiming a second server stack.
            println!("{}", self.facts_json());
        }
    }
}

pub use jet_webapp_impl::{jet_web_app, jet_web_page, JetWebApp, JetWebPage};
