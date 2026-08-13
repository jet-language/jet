//! D-WEBAPP1=D / D-WEBAUTHOR1=D: one statically known full-stack application graph.
//!
//! Sema evaluates the WebApp-returning `fn run` builder chain into this typed graph. Runtime
//! registration outside a declared `.mount` is a compile diagnostic. Optional
//! `.routes(from:)` conventions expand only when the builder opts in.

use std::collections::BTreeMap;

/// How a route renders (CSR / SSR / SSG / streaming / island). Bodies stay
/// executable TIR — the mode is a graph fact, not a second IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebRenderMode {
    Csr,
    Ssr,
    Ssg,
    Stream,
    Island,
}

impl WebRenderMode {
    pub fn as_str(self) -> &'static str {
        match self {
            WebRenderMode::Csr => "csr",
            WebRenderMode::Ssr => "ssr",
            WebRenderMode::Ssg => "ssg",
            WebRenderMode::Stream => "stream",
            WebRenderMode::Island => "island",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "csr" => Some(WebRenderMode::Csr),
            "ssr" => Some(WebRenderMode::Ssr),
            "ssg" => Some(WebRenderMode::Ssg),
            "stream" | "streaming" => Some(WebRenderMode::Stream),
            "island" => Some(WebRenderMode::Island),
            _ => None,
        }
    }
}

/// One route / page / layout entry in the application graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAppRoute {
    pub path: String,
    pub handler: String,
    pub render: WebRenderMode,
    /// Source file that contributed this entry (`builder` or a convention path).
    pub provenance: String,
    pub span_start: usize,
    pub span_end: usize,
}

/// Server action / form / data dependency registered on the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAppAction {
    pub name: String,
    pub handler: String,
    pub kind: String,
    pub provenance: String,
    pub span_start: usize,
    pub span_end: usize,
}

/// Declared dynamic mount: prefix / effects / security stay static facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAppMount {
    pub prefix: String,
    pub handler: String,
    pub effects: Vec<String>,
    pub security: Vec<String>,
    pub provenance: String,
    pub span_start: usize,
    pub span_end: usize,
}

/// Opt-in file-routing root from `.routes(from: "…")`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAppRoutesFrom {
    pub root: String,
    pub span_start: usize,
    pub span_end: usize,
}

/// Whole-app security / asset / split / cache / a11y / adapter facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebAppPolicy {
    pub security: Vec<String>,
    pub assets: Vec<String>,
    pub split: Vec<String>,
    pub cache: Vec<String>,
    pub a11y: Vec<String>,
    pub adapters: Vec<String>,
}

/// One sema-known application graph (D-WEBAPP1=D).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebAppGraph {
    pub entry_file: String,
    pub routes: Vec<WebAppRoute>,
    pub actions: Vec<WebAppAction>,
    pub mounts: Vec<WebAppMount>,
    pub routes_from: Vec<WebAppRoutesFrom>,
    pub policy: WebAppPolicy,
    /// Hydration law: `dev-overlay` | `release-keep-server`.
    pub hydration: String,
    /// True when every body remains executable TIR (no VDOM / second IR).
    pub shared_tir: bool,
}

impl WebAppGraph {
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n");
        out.push_str(&format!("  \"entry\": {},\n", json_str(&self.entry_file)));
        out.push_str(&format!("  \"hydration\": {},\n", json_str(&self.hydration)));
        out.push_str(&format!("  \"shared_tir\": {},\n", self.shared_tir));
        out.push_str("  \"routes\": [\n");
        for (i, route) in self.routes.iter().enumerate() {
            out.push_str("    {\n");
            out.push_str(&format!("      \"path\": {},\n", json_str(&route.path)));
            out.push_str(&format!("      \"handler\": {},\n", json_str(&route.handler)));
            out.push_str(&format!(
                "      \"render\": {},\n",
                json_str(route.render.as_str())
            ));
            out.push_str(&format!(
                "      \"provenance\": {}\n",
                json_str(&route.provenance)
            ));
            out.push_str("    }");
            if i + 1 != self.routes.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");
        out.push_str("  \"actions\": [\n");
        for (i, action) in self.actions.iter().enumerate() {
            out.push_str("    {\n");
            out.push_str(&format!("      \"name\": {},\n", json_str(&action.name)));
            out.push_str(&format!("      \"handler\": {},\n", json_str(&action.handler)));
            out.push_str(&format!("      \"kind\": {},\n", json_str(&action.kind)));
            out.push_str(&format!(
                "      \"provenance\": {}\n",
                json_str(&action.provenance)
            ));
            out.push_str("    }");
            if i + 1 != self.actions.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");
        out.push_str("  \"mounts\": [\n");
        for (i, mount) in self.mounts.iter().enumerate() {
            out.push_str("    {\n");
            out.push_str(&format!("      \"prefix\": {},\n", json_str(&mount.prefix)));
            out.push_str(&format!("      \"handler\": {},\n", json_str(&mount.handler)));
            out.push_str(&format!(
                "      \"effects\": {},\n",
                json_str_list(&mount.effects)
            ));
            out.push_str(&format!(
                "      \"security\": {},\n",
                json_str_list(&mount.security)
            ));
            out.push_str(&format!(
                "      \"provenance\": {}\n",
                json_str(&mount.provenance)
            ));
            out.push_str("    }");
            if i + 1 != self.mounts.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");
        out.push_str("  \"routes_from\": [\n");
        for (i, root) in self.routes_from.iter().enumerate() {
            out.push_str(&format!("    {}", json_str(&root.root)));
            if i + 1 != self.routes_from.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");
        out.push_str("  \"policy\": {\n");
        out.push_str(&format!(
            "    \"security\": {},\n",
            json_str_list(&self.policy.security)
        ));
        out.push_str(&format!(
            "    \"assets\": {},\n",
            json_str_list(&self.policy.assets)
        ));
        out.push_str(&format!(
            "    \"split\": {},\n",
            json_str_list(&self.policy.split)
        ));
        out.push_str(&format!(
            "    \"cache\": {},\n",
            json_str_list(&self.policy.cache)
        ));
        out.push_str(&format!(
            "    \"a11y\": {},\n",
            json_str_list(&self.policy.a11y)
        ));
        out.push_str(&format!(
            "    \"adapters\": {}\n",
            json_str_list(&self.policy.adapters)
        ));
        out.push_str("  }\n");
        out.push('}');
        out
    }

    /// Human explain lines for `jet explain --web-graph`.
    pub fn explain_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("web application graph — {}", self.entry_file));
        lines.push(format!(
            "hydration: {} (shared TIR: {})",
            self.hydration, self.shared_tir
        ));
        for route in &self.routes {
            lines.push(format!(
                "  {} [{}] -> {} ({})",
                route.path,
                route.render.as_str(),
                route.handler,
                route.provenance
            ));
        }
        for action in &self.actions {
            lines.push(format!(
                "  action {} ({}) -> {} ({})",
                action.name, action.kind, action.handler, action.provenance
            ));
        }
        for mount in &self.mounts {
            lines.push(format!(
                "  mount {} -> {} effects={} security={} ({})",
                mount.prefix,
                mount.handler,
                mount.effects.join(","),
                mount.security.join(","),
                mount.provenance
            ));
        }
        for root in &self.routes_from {
            lines.push(format!("  routes(from: {})", root.root));
        }
        if !self.policy.security.is_empty() {
            lines.push(format!("  security: {}", self.policy.security.join(", ")));
        }
        if !self.policy.assets.is_empty() {
            lines.push(format!("  assets: {}", self.policy.assets.join(", ")));
        }
        if !self.policy.split.is_empty() {
            lines.push(format!("  split: {}", self.policy.split.join(", ")));
        }
        if !self.policy.cache.is_empty() {
            lines.push(format!("  cache: {}", self.policy.cache.join(", ")));
        }
        if !self.policy.a11y.is_empty() {
            lines.push(format!("  a11y: {}", self.policy.a11y.join(", ")));
        }
        if !self.policy.adapters.is_empty() {
            lines.push(format!("  adapters: {}", self.policy.adapters.join(", ")));
        }
        lines
    }

    pub fn route_index(&self) -> BTreeMap<String, usize> {
        let mut map = BTreeMap::new();
        for (i, route) in self.routes.iter().enumerate() {
            map.insert(route.path.clone(), i);
        }
        map
    }
}

fn json_str(value: &str) -> String {
    let mut out = String::from('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_str_list(values: &[String]) -> String {
    let mut out = String::from('[');
    for (i, value) in values.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&json_str(value));
    }
    out.push(']');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_shape() {
        let mut graph = WebAppGraph::default();
        graph.entry_file = "app.jet".into();
        graph.hydration = "dev-overlay".into();
        graph.shared_tir = true;
        graph.routes.push(WebAppRoute {
            path: "/".into(),
            handler: "home".into(),
            render: WebRenderMode::Csr,
            provenance: "builder".into(),
            span_start: 0,
            span_end: 1,
        });
        let json = graph.to_json();
        assert!(json.contains("\"path\": \"/\""));
        assert!(json.contains("\"render\": \"csr\""));
        assert!(json.contains("\"shared_tir\": true"));
    }
}
