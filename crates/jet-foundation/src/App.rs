//! D-WEBAPP1=D / D-WEBAUTHOR1=D / D-APP-UNIFY1=B: one statically known full-stack application graph.
//!
//! Sema evaluates the App-returning `fn run` builder chain into this typed graph. Runtime
//! registration outside a declared `.mount` is a compile diagnostic. Optional
//! `.routes(from:)` conventions expand only when the builder opts in.

use std::collections::BTreeMap;
pub use crate::AppMiddleware::{AppServerMiddleware, APP_SERVER_FUNCTION_MIDDLEWARE};

/// How a route renders (CSR / SSR / SSG / streaming / island). Bodies stay
/// executable TIR — the mode is a graph fact, not a second IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRenderMode {
    Csr,
    Ssr,
    Ssg,
    Stream,
    Island,
}

/// D-APP-UNIFY1=B: target capability requirements live with the App contract,
/// not in an execution engine. `None` means the builder operation is valid on
/// every App target; the returned target is the narrow target class required by
/// a target-sensitive operation.
pub fn app_capability_target(method: &str) -> Option<&'static str> {
    match method {
        "csr" | "island" | "hydration_dev" | "hydration_release" => Some("JS"),
        "serve" | "serve_on" => Some("Native"),
        _ => None,
    }
}

/// D-APP-UNIFY1=B: a broad `Web` build includes the JS browser edge. Native
/// App capabilities remain valid for an OS-native target, but not freestanding
/// or a web partition.
pub fn app_target_supports(required: &str, target: &str) -> bool {
    match required {
        "JS" => matches!(target, "Web" | "JS"),
        "Native" => matches!(target, "Native" | "OS"),
        _ => false,
    }
}

impl AppRenderMode {
    pub fn as_str(self) -> &'static str {
        match self {
            AppRenderMode::Csr => "csr",
            AppRenderMode::Ssr => "ssr",
            AppRenderMode::Ssg => "ssg",
            AppRenderMode::Stream => "stream",
            AppRenderMode::Island => "island",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "csr" => Some(AppRenderMode::Csr),
            "ssr" => Some(AppRenderMode::Ssr),
            "ssg" => Some(AppRenderMode::Ssg),
            "stream" | "streaming" => Some(AppRenderMode::Stream),
            "island" => Some(AppRenderMode::Island),
            _ => None,
        }
    }
}

/// Compiler-derived rendering mode for one route. Unlike [`AppRenderMode`],
/// this is not a builder spelling: it is the backend-neutral fact produced
/// from effect, data, and interactivity metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRenderFactMode {
    Static,
    Server,
    ServerStream,
    ServerIsland,
    Client,
}

impl AppRenderFactMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Server => "server",
            Self::ServerStream => "server+stream",
            Self::ServerIsland => "server+island",
            Self::Client => "client",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "static" | "Static" | "ssg" => Some(Self::Static),
            "server" | "Server" | "ssr" => Some(Self::Server),
            "server+stream" | "ServerStream" | "Stream" | "stream" | "streaming" => {
                Some(Self::ServerStream)
            }
            "server+island" | "ServerIsland" | "Island" | "island" => {
                Some(Self::ServerIsland)
            }
            "client" | "Client" | "csr" => Some(Self::Client),
            _ => None,
        }
    }

    pub fn from_legacy(mode: AppRenderMode) -> Self {
        match mode {
            AppRenderMode::Csr => Self::Client,
            AppRenderMode::Ssr => Self::Server,
            AppRenderMode::Ssg => Self::Static,
            AppRenderMode::Stream => Self::ServerStream,
            AppRenderMode::Island => Self::ServerIsland,
        }
    }
}

/// Route cache policy as a compiler fact. The web backend may interpret the
/// fact, but it does not choose it or infer a second policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCacheFact {
    Unspecified,
    Immutable,
    Revalidate,
    Private,
}

impl Default for AppCacheFact {
    fn default() -> Self {
        Self::Unspecified
    }
}

impl AppCacheFact {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Immutable => "immutable",
            Self::Revalidate => "revalidate",
            Self::Private => "private",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "immutable" | "static" => Some(Self::Immutable),
            "revalidate" | "stream" => Some(Self::Revalidate),
            "private" | "no-store" => Some(Self::Private),
            _ => None,
        }
    }
}
/// Stable identifier for one route's pending/streaming boundary.
///
/// The ID is a compiler fact. Renderers and hosts may wrap it for their own
/// transport, but they must not derive a second route identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct AppPendingBoundaryId(pub u64);

impl AppPendingBoundaryId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Canonical hydration scheduling fact for one resumable island.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum AppHydrationTrigger {
    Immediate,
    ClientLoad,
    ClientIdle,
    ClientVisible,
    Manual,
}

impl Default for AppHydrationTrigger {
    fn default() -> Self {
        Self::Immediate
    }
}

impl AppHydrationTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::ClientLoad => "client-load",
            Self::ClientIdle => "client-idle",
            Self::ClientVisible => "client-visible",
            Self::Manual => "manual",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "immediate" => Some(Self::Immediate),
            "client-load" => Some(Self::ClientLoad),
            "client-idle" => Some(Self::ClientIdle),
            "client-visible" => Some(Self::ClientVisible),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

/// One value carried across an island resume boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppResumeCapture {
    pub name: String,
    pub ty: String,
    pub serializable: bool,
}

/// Explicit, backend-neutral state payload for one resumable island.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppResumePayload {
    pub captures: Vec<AppResumeCapture>,
    pub serializable: bool,
}

/// Identity and resume payload for one interactive route island.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppIslandFact {
    pub identity: String,
    pub hydration_trigger: AppHydrationTrigger,
    pub resume_payload: AppResumePayload,
}

/// All route rendering facts consumed by tooling and web codegen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRenderFacts {
    pub mode: AppRenderFactMode,
    pub reason: Vec<String>,
    pub effects: Vec<String>,
    pub interactive: bool,
    pub loader: Option<String>,
    pub form: Option<String>,
    pub cache: AppCacheFact,
    pub override_mode: Option<AppRenderFactMode>,
    pub pending_boundary_id: Option<AppPendingBoundaryId>,
    pub island: Option<AppIslandFact>,
}


impl Default for AppRenderFacts {
    fn default() -> Self {
        Self {
            mode: AppRenderFactMode::Static,
            reason: Vec::new(),
            effects: Vec::new(),
            interactive: false,
            loader: None,
            form: None,
            cache: AppCacheFact::Unspecified,
            override_mode: None,
            pending_boundary_id: None,
            island: None,
        }
    }
}

impl AppRenderFacts {
    pub fn with_legacy_override(mode: AppRenderMode) -> Self {
        Self {
            mode: AppRenderFactMode::from_legacy(mode),
            override_mode: Some(AppRenderFactMode::from_legacy(mode)),
            reason: vec![format!("explicit render mode `{}`", mode.as_str())],
            ..Self::default()
        }
    }
}

/// Stable identity for a route island. The source span is deliberately not
/// included, so harmless edits do not replace the resumable client identity.
pub fn stable_island_identity(path: &str, handler: &str) -> String {
    let key = format!("jet:island:v1:{path}\0{handler}");
    format!("island-{}", &crate::SHA256::sha256_hex(key.as_bytes())[..24])
}

/// Stable identity for one route's pending boundary. The route path and
/// handler are the canonical inputs, so source-span edits do not churn it.
pub fn stable_pending_boundary_id(path: &str, handler: &str) -> AppPendingBoundaryId {
    let key = format!("jet:pending-boundary:v1:{path}\0{handler}");
    let digest = crate::SHA256::sha256(key.as_bytes());
    AppPendingBoundaryId(u64::from_be_bytes(
        digest[..8].try_into().expect("SHA-256 has at least eight bytes"),
    ))
}

/// One typed input carried by a checked route contract. `ty` is the source
/// type spelling; `codec` is the shared wire codec selected by sema. Keeping
/// both means renderers can display the user type without re-inferring it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRouteField {
    pub name: String,
    pub ty: String,
    pub codec: String,
    pub required: bool,
    pub default: Option<String>,
}

/// Compiler-owned loader facts. A backend may schedule or marshal this
/// callback, but it must not invent a second cache key or dependency model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppRouteLoader {
    pub handler: String,
    pub data_type: String,
    pub dependency: String,
    pub preload: bool,
    pub cache_identity: String,
}

/// Boundary names attached to one route identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppRouteBoundaries {
    pub pending: Option<String>,
    pub not_found: Option<String>,
    pub error: Option<String>,
}

/// Stable identity shared by navigation, preload, abort, and boundary facts.
pub fn stable_route_identity(path: &str, handler: &str) -> String {
    let key = format!("jet:route:v1:{path}\0{handler}");
    format!("route-{}", &crate::SHA256::sha256_hex(key.as_bytes())[..24])
}

/// One route / page / layout entry in the application graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRoute {
    pub path: String,
    pub handler: String,
    /// Stable identity shared by all route lifecycle operations.
    pub route_identity: String,
    /// Typed path inputs and their sema-selected codecs.
    pub path_params: Vec<AppRouteField>,
    /// Typed search inputs. Named records use the JSON-first codec.
    pub search_params: Vec<AppRouteField>,
    pub search_codec: String,
    pub loader: Option<AppRouteLoader>,
    pub boundaries: AppRouteBoundaries,
    /// Human-readable precedence fact; matching remains shared with HTTP.
    pub precedence: String,
    /// Legacy builder spelling retained for existing graph consumers.
    pub render: AppRenderMode,
    /// Compiler-derived rendering/data/interactivity facts. Web codegen reads
    /// this field rather than re-deciding route policy.
    pub render_facts: AppRenderFacts,
    /// Source file that contributed this entry (`builder` or a convention path).
    pub provenance: String,
    pub span_start: usize,
    pub span_end: usize,
}


/// One typed server boundary registered on the application graph.
///
/// The input/output/error names are compiler-selected wire shapes.  Runtime
/// adapters use the same endpoint and policy facts; they must not reconstruct
/// a second signature from a callback value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppAction {
    pub name: String,
    pub handler: String,
    pub kind: String,
    pub preload: bool,
    pub input_type: String,
    pub output_type: String,
    pub error_type: String,
    pub endpoint: String,
    pub method: String,
    pub csrf: String,
    pub effects: Vec<String>,
    pub middleware: Vec<String>,
    pub provenance: String,
    pub span_start: usize,
    pub span_end: usize,
}


/// Declared dynamic mount: prefix / effects / security stay static facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMount {
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
pub struct AppRoutesFrom {
    pub root: String,
    pub span_start: usize,
    pub span_end: usize,
}

/// Whole-app security / asset / split / cache / a11y / adapter facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppPolicy {
    pub security: Vec<String>,
    pub assets: Vec<String>,
    pub split: Vec<String>,
    pub cache: Vec<String>,
    pub a11y: Vec<String>,
    pub adapters: Vec<String>,
}

/// Sema-owned identity and dependency facts for a first-party web query.
///
/// `key` is the explicit cache identity from the call site. `footprint` is
/// dependency metadata, never a second identity. `source` names the checked
/// function that declared the query so graph/devtools consumers can join the
/// fact without re-walking runtime code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppQueryFact {
    pub key: String,
    pub footprint: String,
    pub source: String,
    pub kind: String,
}
/// Sema-owned local Store identity/history facts; state payload never enters the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStoreFact {
    pub name: String,
    pub source: String,
    pub kind: String,
    pub history_limit: String,
}
/// Sema-owned schema and boundary facts for one struct-derived form.
///
/// This is metadata only.  Field values and validation payloads remain in the
/// form runtime; graph consumers receive the model/action contract and the
/// fields needed to build a development inspector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppFormFieldFact {
    pub name: String,
    pub ty: String,
    pub required: bool,
    pub default: Option<String>,
    pub label: String,
    pub control: String,
    pub wire_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppFormFact {
    pub name: String,
    pub model: String,
    pub action: String,
    pub input_type: String,
    pub output_type: String,
    pub error_type: String,
    pub endpoint: String,
    pub method: String,
    pub csrf: String,
    pub effects: Vec<String>,
    pub fields: Vec<AppFormFieldFact>,
    pub middleware: Vec<String>,
    pub provenance: String,
    pub span_start: usize,
    pub span_end: usize,
}


/// One sema-known application graph (D-WEBAPP1=D).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppGraph {
    pub entry_file: String,
    pub routes: Vec<AppRoute>,
    pub actions: Vec<AppAction>,
    pub forms: Vec<AppFormFact>,
    pub queries: Vec<AppQueryFact>,
    pub stores: Vec<AppStoreFact>,
    pub mounts: Vec<AppMount>,
    pub routes_from: Vec<AppRoutesFrom>,
    pub policy: AppPolicy,
    /// Hydration law: `dev-overlay` | `release-keep-server`.
    pub hydration: String,
    /// True when every body remains executable TIR (no VDOM / second IR).
    pub shared_tir: bool,
}

impl AppGraph {
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n");
        out.push_str(&format!("  \"entry\": {},\n", json_str(&self.entry_file)));
        out.push_str(&format!(
            "  \"hydration\": {},\n",
            json_str(&self.hydration)
        ));
        out.push_str(&format!("  \"shared_tir\": {},\n", self.shared_tir));
        out.push_str("  \"routes\": [\n");
        for (i, route) in self.routes.iter().enumerate() {
            out.push_str("    {\n");
            out.push_str(&format!("      \"path\": {},\n", json_str(&route.path)));
            out.push_str(&format!(
                "      \"handler\": {},\n",
                json_str(&route.handler)
            ));
            out.push_str(&format!(
                "      \"route_identity\": {},\n",
                json_str(&route.route_identity)
            ));
            out.push_str(&format!(
                "      \"path_params\": {},\n",
                json_route_fields(&route.path_params)
            ));
            out.push_str(&format!(
                "      \"search_params\": {},\n",
                json_route_fields(&route.search_params)
            ));
            out.push_str(&format!(
                "      \"search_codec\": {},\n",
                json_str(&route.search_codec)
            ));
            out.push_str(&format!(
                "      \"loader\": {},\n",
                json_route_loader(route.loader.as_ref())
            ));
            out.push_str(&format!(
                "      \"boundaries\": {},\n",
                json_route_boundaries(&route.boundaries)
            ));
            out.push_str(&format!(
                "      \"precedence\": {},\n",
                json_str(&route.precedence)
            ));
            out.push_str(&format!(
                "      \"render\": {},\n",
                json_str(route.render.as_str())
            ));
            out.push_str("      \"render_facts\": {\n");
            out.push_str(&format!(
                "        \"mode\": {},\n",
                json_str(route.render_facts.mode.as_str())
            ));
            out.push_str(&format!(
                "        \"reason\": {},\n",
                json_str_list(&route.render_facts.reason)
            ));
            out.push_str(&format!(
                "        \"effects\": {},\n",
                json_str_list(&route.render_facts.effects)
            ));
            out.push_str(&format!(
                "        \"interactive\": {},\n",
                route.render_facts.interactive
            ));
            out.push_str(&format!(
                "        \"loader\": {},\n",
                json_optional_str(route.render_facts.loader.as_deref())
            ));
            out.push_str(&format!(
                "        \"form\": {},\n",
                json_optional_str(route.render_facts.form.as_deref())
            ));
            out.push_str(&format!(
                "        \"cache\": {},\n",
                json_str(route.render_facts.cache.as_str())
            ));
            out.push_str(&format!(
                "        \"override\": {},\n",
                route
                    .render_facts
                    .override_mode
                    .map(|mode| json_str(mode.as_str()))
                    .unwrap_or_else(|| "null".to_string())
            ));
            out.push_str(&format!(
                "        \"pending_boundary_id\": {},\n",
                route
                    .render_facts
                    .pending_boundary_id
                    .map(|id| id.get().to_string())
                    .unwrap_or_else(|| "null".to_string())
            ));
            match &route.render_facts.island {
                Some(island) => {
                    out.push_str("        \"island\": {\n");
                    out.push_str(&format!(
                        "          \"identity\": {},\n",
                        json_str(&island.identity)
                    ));
                    out.push_str(&format!(
                        "          \"hydration_trigger\": {},\n",
                        json_str(island.hydration_trigger.as_str())
                    ));
                    out.push_str("          \"resume_payload\": {\n");
                    out.push_str(&format!(
                        "            \"serializable\": {},\n",
                        island.resume_payload.serializable
                    ));
                    out.push_str("            \"captures\": [\n");
                    for (capture_idx, capture) in
                        island.resume_payload.captures.iter().enumerate()
                    {
                        out.push_str(&format!(
                            "              {{\"name\": {}, \"type\": {}, \"serializable\": {}}}",
                            json_str(&capture.name),
                            json_str(&capture.ty),
                            capture.serializable
                        ));
                        if capture_idx + 1 != island.resume_payload.captures.len() {
                            out.push(',');
                        }
                        out.push('\n');
                    }
                    out.push_str("            ]\n");
                    out.push_str("          }\n");
                    out.push_str("        }\n");
                }
                None => out.push_str("        \"island\": null\n"),
            }
            out.push_str("      },\n");
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
            out.push_str(&format!(
                "      \"handler\": {},\n",
                json_str(&action.handler)
            ));
            out.push_str(&format!("      \"kind\": {},\n", json_str(&action.kind)));
            out.push_str(&format!("      \"preload\": {},\n", action.preload));
            out.push_str(&format!(
                "      \"input\": {},\n",
                json_str(&action.input_type)
            ));
            out.push_str(&format!(
                "      \"output\": {},\n",
                json_str(&action.output_type)
            ));
            out.push_str(&format!(
                "      \"error\": {},\n",
                json_str(&action.error_type)
            ));
            out.push_str(&format!(
                "      \"endpoint\": {},\n",
                json_str(&action.endpoint)
            ));
            out.push_str(&format!("      \"method\": {},\n", json_str(&action.method)));
            out.push_str(&format!("      \"csrf\": {},\n", json_str(&action.csrf)));
            out.push_str(&format!(
                "      \"effects\": {},\n",
                json_str_list(&action.effects)
            ));
            out.push_str(&format!(
                "      \"middleware\": {},\n",
                json_str_list(&action.middleware)
            ));
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
        out.push_str("  \"forms\": [\n");
        for (i, form) in self.forms.iter().enumerate() {
            let fields = form
                .fields
                .iter()
                .map(|field| {
                    format!(
                        "{{\"name\":{},\"ty\":{},\"required\":{},\"default\":{},\"label\":{},\"control\":{},\"wire_name\":{}}}",
                        json_str(&field.name),
                        json_str(&field.ty),
                        field.required,
                        field
                            .default
                            .as_deref()
                            .map(json_str)
                            .unwrap_or_else(|| "null".to_string()),
                        json_str(&field.label),
                        json_str(&field.control),
                        json_str(&field.wire_name),
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            out.push_str(&format!(
                "    {{\"name\":{},\"model\":{},\"action\":{},\"input\":{},\"output\":{},\"error\":{},\"endpoint\":{},\"method\":{},\"csrf\":{},\"effects\":{},\"fields\":[{}],\"middleware\":{},\"provenance\":{}}}",
                json_str(&form.name),
                json_str(&form.model),
                json_str(&form.action),
                json_str(&form.input_type),
                json_str(&form.output_type),
                json_str(&form.error_type),
                json_str(&form.endpoint),
                json_str(&form.method),
                json_str(&form.csrf),
                json_str_list(&form.effects),
                fields,
                json_str_list(&form.middleware),
                json_str(&form.provenance),
            ));
            if i + 1 != self.forms.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");
        out.push_str("  \"queries\": [\n");
        for (i, query) in self.queries.iter().enumerate() {
            out.push_str("    {\n");
            out.push_str(&format!("      \"key\": {},\n", json_str(&query.key)));
            out.push_str(&format!(
                "      \"footprint\": {},\n",
                json_str(&query.footprint)
            ));
            out.push_str(&format!("      \"source\": {},\n", json_str(&query.source)));
            out.push_str(&format!("      \"kind\": {}\n", json_str(&query.kind)));
            out.push_str("    }");
            if i + 1 != self.queries.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");
        out.push_str("  \"stores\": [\n");
        for (i, store) in self.stores.iter().enumerate() {
            out.push_str(&format!("    {{\n      \"name\": {},\n      \"source\": {},\n      \"kind\": {},\n      \"history_limit\": {}\n    }}", json_str(&store.name), json_str(&store.source), json_str(&store.kind), json_str(&store.history_limit)));
            if i + 1 != self.stores.len() { out.push(','); }
            out.push('\n');
        }
        out.push_str("  ],\n");
        out.push_str("  \"mounts\": [\n");
        for (i, mount) in self.mounts.iter().enumerate() {
            out.push_str("    {\n");
            out.push_str(&format!("      \"prefix\": {},\n", json_str(&mount.prefix)));
            out.push_str(&format!(
                "      \"handler\": {},\n",
                json_str(&mount.handler)
            ));
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
                route.render_facts.mode.as_str(),
                route.handler,
                route.provenance
            ));
            let path_params = route
                .path_params
                .iter()
                .map(|field| format!("{}:{}", field.name, field.ty))
                .collect::<Vec<_>>()
                .join(",");
            let search_params = route
                .search_params
                .iter()
                .map(|field| format!("{}:{}", field.name, field.ty))
                .collect::<Vec<_>>()
                .join(",");
            let loader = route
                .loader
                .as_ref()
                .map(|loader| {
                    format!(
                        " loader={} data={} preload={} cache={}",
                        loader.handler, loader.data_type, loader.preload, loader.cache_identity
                    )
                })
                .unwrap_or_default();
            lines.push(format!(
                "    route-contract: identity={} precedence={} path=[{}] search=[{}] codec={}{} boundaries=pending:{} not_found:{} error:{}",
                route.route_identity,
                route.precedence,
                path_params,
                search_params,
                route.search_codec,
                loader,
                route.boundaries.pending.as_deref().unwrap_or("-"),
                route.boundaries.not_found.as_deref().unwrap_or("-"),
                route.boundaries.error.as_deref().unwrap_or("-"),
            ));
            let reason = if route.render_facts.reason.is_empty() {
                "unspecified".to_string()
            } else {
                route.render_facts.reason.join("; ")
            };
            let pending = route
                .render_facts
                .pending_boundary_id
                .map(|id| format!(" pending_boundary_id={}", id.get()))
                .unwrap_or_default();
            let island = route
                .render_facts
                .island
                .as_ref()
                .map(|island| {
                    format!(
                        " island={} hydration={}",
                        island.identity,
                        island.hydration_trigger.as_str()
                    )
                })
                .unwrap_or_default();
            lines.push(format!(
                "    render-facts: mode={} reason={} effects=[{}] cache={}{}{}",
                route.render_facts.mode.as_str(),
                reason,
                route.render_facts.effects.join(","),
                route.render_facts.cache.as_str(),
                pending,
                island
            ));
        }
        for action in &self.actions {
            lines.push(format!(
                "  action {} ({}) -> {} input={} output={} error={} endpoint={} method={} csrf={} effects=[{}] middleware=[{}] ({})",
                action.name,
                action.kind,
                action.handler,
                action.input_type,
                action.output_type,
                action.error_type,
                action.endpoint,
                action.method,
                action.csrf,
                action.effects.join(","),
                action.middleware.join(","),
                action.provenance
            ));
        }
        for form in &self.forms {
            let fields = form
                .fields
                .iter()
                .map(|field| {
                    format!(
                        "{}:{}{}",
                        field.name,
                        field.ty,
                        if field.required { "" } else { "?" }
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            lines.push(format!(
                "  form {} model={} action={} input={} output={} error={} endpoint={} method={} csrf={} fields=[{}] effects=[{}] middleware=[{}] ({})",
                form.name,
                form.model,
                form.action,
                form.input_type,
                form.output_type,
                form.error_type,
                form.endpoint,
                form.method,
                form.csrf,
                fields,
                form.effects.join(","),
                form.middleware.join(","),
                form.provenance
            ));
        }
        for query in &self.queries {
            lines.push(format!(
                "  query {} ({}) footprint={} source={}",
                query.key, query.kind, query.footprint, query.source
            ));
        }
        for store in &self.stores {
            lines.push(format!("  store {} ({}) history_limit={} source={}", store.name, store.kind, store.history_limit, store.source));
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
fn json_optional_str(value: Option<&str>) -> String {
    value
        .map(json_str)
        .unwrap_or_else(|| "null".to_string())
}

fn json_route_fields(fields: &[AppRouteField]) -> String {
    let values = fields
        .iter()
        .map(|field| {
            format!(
                "{{\"name\":{},\"type\":{},\"codec\":{},\"required\":{},\"default\":{}}}",
                json_str(&field.name),
                json_str(&field.ty),
                json_str(&field.codec),
                field.required,
                json_optional_str(field.default.as_deref())
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

fn json_route_loader(loader: Option<&AppRouteLoader>) -> String {
    loader
        .map(|loader| {
            format!(
                "{{\"handler\":{},\"data_type\":{},\"dependency\":{},\"preload\":{},\"cache_identity\":{}}}",
                json_str(&loader.handler),
                json_str(&loader.data_type),
                json_str(&loader.dependency),
                loader.preload,
                json_str(&loader.cache_identity)
            )
        })
        .unwrap_or_else(|| "null".to_string())
}

fn json_route_boundaries(boundaries: &AppRouteBoundaries) -> String {
    format!(
        "{{\"pending\":{},\"not_found\":{},\"error\":{}}}",
        json_optional_str(boundaries.pending.as_deref()),
        json_optional_str(boundaries.not_found.as_deref()),
        json_optional_str(boundaries.error.as_deref())
    )
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
        let mut graph = AppGraph::default();
        graph.entry_file = "app.jet".into();
        graph.hydration = "dev-overlay".into();
        graph.shared_tir = true;
        graph.routes.push(AppRoute {
            path: "/".into(),
            handler: "home".into(),
            route_identity: stable_route_identity("/", "home"),
            path_params: Vec::new(),
            search_params: Vec::new(),
            search_codec: "query".into(),
            loader: None,
            boundaries: AppRouteBoundaries::default(),
            precedence: "static".into(),
            render: AppRenderMode::Csr,
            render_facts: AppRenderFacts::default(),
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
