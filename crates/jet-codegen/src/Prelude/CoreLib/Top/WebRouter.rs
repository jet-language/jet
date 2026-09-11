// D-DX-SUITE1=C: first-party Router state on the shared reactive primitive.
//
// This file owns route matching, typed path/search decoding, preloading, and
// navigation boundaries. A host only renders the resulting state; it does not
// parse URLs or maintain a second router.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const JET_WEB_ROUTER_MAX_URL: usize = 16 * 1024;
const JET_WEB_ROUTER_MAX_ROUTES: usize = 1024;
const JET_WEB_ROUTER_MAX_PARAMS: usize = 128;
const JET_WEB_ROUTER_MAX_PRELOADED: usize = 256;
const JET_WEB_ROUTER_MAX_DATA: usize = 4 * 1024 * 1024;

fn jet_web_router_bound_data(value: String, fallback: &str) -> String {
    if value.len() <= JET_WEB_ROUTER_MAX_DATA {
        value
    } else {
        fallback.to_string()
    }
}
fn jet_web_router_identity(pattern: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in pattern.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("route-{hash:016x}")
}
fn jet_web_router_cache_key(route: &JetWebRoute, navigation: &JetWebNavigation) -> String {
    let mut key = route.identity.clone();
    key.push_str("|p");
    for (name, value) in &navigation.params {
        key.push('|');
        key.push_str(&name.len().to_string());
        key.push(':');
        key.push_str(name);
        key.push('=');
        key.push_str(&value.len().to_string());
        key.push(':');
        key.push_str(value);
    }
    key.push_str("|s");
    for (name, value) in &navigation.search {
        key.push('|');
        key.push_str(&name.len().to_string());
        key.push(':');
        key.push_str(name);
        key.push('=');
        key.push_str(&value.len().to_string());
        key.push(':');
        key.push_str(value);
    }
    key
}
fn jet_web_router_json_string(value: &str) -> String {
    format!("{value:?}")
}

fn jet_web_router_json_array(values: &[String]) -> String {
    let encoded = values
        .iter()
        .map(|value| jet_web_router_json_string(value))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{encoded}]")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebRouterValueType {
    String,
    Int,
    Bool,
    Float,
    Json,
}

impl JetWebRouterValueType {
    fn name(self) -> &'static str {
        match self {
            Self::String => "String",
            Self::Int => "Int",
            Self::Bool => "Bool",
            Self::Float => "Float",
            Self::Json => "JSON",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebRouterField {
    pub name: String,
    pub value_type: JetWebRouterValueType,
    pub required: bool,
    pub default: Option<String>,
}

impl JetWebRouterField {
    pub fn new(name: String, value_type: JetWebRouterValueType) -> Self {
        Self {
            name,
            value_type,
            required: true,
            default: None,
        }
    }

    pub fn optional(mut self, default: Option<String>) -> Self {
        self.required = false;
        self.default = default;
        self
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebRouterSearchCodec {
    Query,
    Json,
}

impl JetWebRouterSearchCodec {
    pub fn name(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Json => "json",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebRouterCacheStatus {
    Fresh,
    Stale,
    Invalidated,
    Collected,
}

impl JetWebRouterCacheStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Invalidated => "invalidated",
            Self::Collected => "collected",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebRouterCacheState {
    pub identity: String,
    pub status: JetWebRouterCacheStatus,
    pub data: String,
    pub dependencies: Vec<String>,
    pub generation: u64,
}




fn jet_web_router_decode(value: &str, plus_as_space: bool) -> Result<String, String> {
    let normalized = if plus_as_space && value.as_bytes().contains(&b'+') {
        std::borrow::Cow::Owned(value.replace('+', " "))
    } else {
        std::borrow::Cow::Borrowed(value)
    };
    let decoded = jet_std::jet_url_percent_decode_str(normalized.as_ref())?;
    if decoded.chars().any(|character| character.is_control()) {
        return Err("URL component contains a control character".to_string());
    }
    Ok(decoded)
}


fn jet_web_router_parse_pattern(path: &str) -> Result<JetHTTPRoutePattern, String> {
    if path.len() > JET_WEB_ROUTER_MAX_URL {
        return Err(format!(
            "E2805: invalid web route `{path}`: route exceeds the {}-byte limit",
            JET_WEB_ROUTER_MAX_URL
        ));
    }
    jet_http_route_parse(path)
}


fn jet_web_router_split_url(url: &str) -> Result<(&str, &str), String> {
    if url.is_empty() || url.len() > JET_WEB_ROUTER_MAX_URL || !url.starts_with('/') {
        return Err("navigation URL must be an absolute path beginning with `/`".to_string());
    }
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    if path.contains('#') || query.contains('#') {
        return Err("URL fragments are not accepted by the server navigation contract".to_string());
    }
    Ok((path, query))
}

fn jet_web_router_match(
    pattern: &JetHTTPRoutePattern,
    path: &str,
) -> Result<Option<BTreeMap<String, String>>, String> {
    let values = jet_http_route_path(path)?;
    Ok(jet_http_route_match(pattern, &values))
}
fn jet_web_router_parse_search(
    query: &str,
    codec: JetWebRouterSearchCodec,
) -> Result<BTreeMap<String, String>, String> {
    match codec {
        JetWebRouterSearchCodec::Query => jet_web_router_parse_query(query),
        JetWebRouterSearchCodec::Json => jet_web_router_parse_json_search(query),
    }
}

fn jet_web_router_parse_json_search(
    query: &str,
) -> Result<BTreeMap<String, String>, String> {
    if query.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut encoded = None;
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = jet_web_router_decode(raw_key, true)?;
        if key != "search" {
            return Err(format!(
                "JSON search codec accepts only the `search` query parameter, got `{key}`"
            ));
        }
        if encoded.replace(jet_web_router_decode(raw_value, true)?).is_some() {
            return Err("JSON search parameter `search` is repeated".to_string());
        }
    }
    let Some(encoded) = encoded else {
        return Ok(BTreeMap::new());
    };
    let value = jet_std::parse_json_strict(&encoded)
        .map_err(|error| format!("invalid JSON search value: {}", error.reason))?;
    let jet_std::DataTree::Object(entries) = value else {
        return Err("JSON search value must be an object".to_string());
    };
    if entries.len() > JET_WEB_ROUTER_MAX_PARAMS {
        return Err("navigation has too many search parameters".to_string());
    }
    entries
        .into_iter()
        .map(|(key, value)| {
            if !jet_http_route_name(&key) {
                return Err(format!("search parameter `{key}` has an invalid name"));
            }
            Ok((key, jet_web_router_json_value_text(&value)?))
        })
        .collect()
}

fn jet_web_router_json_value_text(value: &jet_std::DataTree) -> Result<String, String> {
    match value {
        jet_std::DataTree::Text(text) | jet_std::DataTree::TypedText(text) => Ok(text.clone()),
        jet_std::DataTree::Bool(value) => Ok(value.to_string()),
        jet_std::DataTree::Int(value) => Ok(value.to_string()),
        jet_std::DataTree::Float(value) if value.is_finite() => Ok(value.to_string()),
        jet_std::DataTree::Float(_) => Err("JSON search number must be finite".to_string()),
        jet_std::DataTree::Number(value) => Ok(value.clone()),
        jet_std::DataTree::Null
        | jet_std::DataTree::Array(_)
        | jet_std::DataTree::Object(_)
        | jet_std::DataTree::Bytes(_) => Ok(jet_std::render_json(value, false, 0)),
    }
}


fn jet_web_router_parse_query(query: &str) -> Result<BTreeMap<String, String>, String> {
    let mut values = BTreeMap::new();
    if query.is_empty() {
        return Ok(values);
    }
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = jet_web_router_decode(raw_key, true)?;
        if !jet_http_route_name(&key) {
            return Err(format!("search parameter `{key}` has an invalid name"));
        }
        if values.contains_key(&key) {
            return Err(format!("search parameter `{key}` is repeated"));
        }
        let value = jet_web_router_decode(raw_value, true)?;
        values.insert(key, value);
        if values.len() > JET_WEB_ROUTER_MAX_PARAMS {
            return Err("navigation has too many search parameters".to_string());
        }
    }
    Ok(values)
}

fn jet_web_router_parse_value(
    field: &JetWebRouterField,
    raw: Option<&String>,
) -> Result<Option<String>, String> {
    let Some(raw) = raw.or(field.default.as_ref()) else {
        if field.required {
            return Err(format!("missing required parameter `{}`", field.name));
        }
        return Ok(None);
    };
    let normalized = match field.value_type {
        JetWebRouterValueType::String => raw.clone(),
        JetWebRouterValueType::Int => raw
            .parse::<i64>()
            .map(|value| value.to_string())
            .map_err(|_| format!("parameter `{}` expects Int, got `{raw}`", field.name))?,
        JetWebRouterValueType::Bool => match raw.as_str() {
            "true" | "1" => "true".to_string(),
            "false" | "0" => "false".to_string(),
            _ => {
                return Err(format!(
                    "parameter `{}` expects Bool, got `{raw}`",
                    field.name
                ));
            }
        },
        JetWebRouterValueType::Float => {
            let value = raw
                .parse::<f64>()
                .map_err(|_| format!("parameter `{}` expects Float, got {raw}", field.name))?;
            if !value.is_finite() {
                return Err(format!("parameter `{}` expects a finite Float", field.name));
            }
            value.to_string()
        }
        JetWebRouterValueType::Json => {
            jet_std::parse_json_strict(raw)
                .map_err(|error| format!("parameter `{}` expects JSON: {}", field.name, error.reason))?;
            raw.clone()
        }
    };
    Ok(Some(normalized))
}
fn jet_web_router_json_value_for_field(
    field: &JetWebRouterField,
    raw: &str,
) -> Result<jet_std::DataTree, String> {
    match field.value_type {
        JetWebRouterValueType::String => Ok(jet_std::DataTree::Text(raw.to_string())),
        JetWebRouterValueType::Int => raw
            .parse::<i64>()
            .map(jet_std::DataTree::Int)
            .map_err(|_| format!("parameter `{}` expects Int, got {raw}", field.name)),
        JetWebRouterValueType::Bool => match raw {
            "true" => Ok(jet_std::DataTree::Bool(true)),
            "false" => Ok(jet_std::DataTree::Bool(false)),
            _ => Err(format!("parameter `{}` expects Bool, got {raw}", field.name)),
        },
        JetWebRouterValueType::Float => raw
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(jet_std::DataTree::Float)
            .ok_or_else(|| format!("parameter `{}` expects Float, got {raw}", field.name)),
        JetWebRouterValueType::Json => jet_std::parse_json_strict(raw)
            .map_err(|error| format!("parameter `{}` expects JSON: {}", field.name, error.reason)),
    }
}

fn jet_web_router_typed_values(
    fields: &[JetWebRouterField],
    values: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut typed = BTreeMap::new();
    for field in fields {
        let value = jet_web_router_parse_value(field, values.get(&field.name))?;
        if let Some(value) = value {
            typed.insert(field.name.clone(), value);
        }
    }
    Ok(typed)
}
 
fn jet_web_router_validate_search(
    fields: &[JetWebRouterField],
    values: &BTreeMap<String, String>,
) -> Result<(), String> {
    for key in values.keys() {
        if !fields.iter().any(|field| field.name == *key) {
            return Err(format!("search parameter `{key}` is not declared for this route"));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebNavigationStatus {
    Idle,
    Preloading,
    Pending,
    Ready,
    Error,

    Aborted,
}

fn jet_web_router_validate_fields(
    pattern: &JetHTTPRoutePattern,
    params: &[JetWebRouterField],
    search: &[JetWebRouterField],
) -> Result<(), String> {
    let mut path_names = std::collections::BTreeSet::new();
    for segment in &pattern.segments {
        match segment {
            JetHTTPRouteSegment::Param(name) | JetHTTPRouteSegment::CatchAll(name) => {
                path_names.insert(name.clone());
            }
            JetHTTPRouteSegment::Static(_) => {}
        }
    }
    let mut declared = std::collections::BTreeSet::new();
    for field in params {
        if !jet_http_route_name(&field.name) {
            return Err(format!("route parameter `{}` has an invalid name", field.name));
        }
        if !path_names.contains(&field.name) {
            return Err(format!(
                "route parameter `{}` is not present in the route pattern",
                field.name
            ));
        }
        if field.required && field.default.is_some() {
            return Err(format!(
                "required route parameter `{}` cannot have a default",
                field.name
            ));
        }
        if let Some(default) = field.default.as_ref() {
            jet_web_router_parse_value(field, Some(default))?;
        }
        if !declared.insert(field.name.clone()) {
            return Err(format!("route parameter `{}` is declared more than once", field.name));
        }
    }
    for name in &path_names {
        if !declared.contains(name) {
            return Err(format!("route pattern parameter `{name}` has no typed declaration"));
        }
    }
    for field in search {
        if !jet_http_route_name(&field.name) {
            return Err(format!("search parameter `{}` has an invalid name", field.name));
        }
        if field.required && field.default.is_some() {
            return Err(format!(
                "required search parameter `{}` cannot have a default",
                field.name
            ));
        }
        if let Some(default) = field.default.as_ref() {
            jet_web_router_parse_value(field, Some(default))?;
        }
        if !declared.insert(field.name.clone()) {
            return Err(format!("parameter `{}` is declared more than once", field.name));
        }
    }
    Ok(())
}

impl JetWebNavigationStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Preloading => "preloading",
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Error => "error",
            Self::Aborted => "aborted",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebRouteBoundaries {
    pub pending: Option<JetWebBoundaryId>,
    pub error: Option<JetWebBoundaryId>,
    pub identity: Option<JetWebIslandIdentity>,
    pub hydration_trigger: Option<JetWebHydrationTrigger>,
}

impl Default for JetWebRouteBoundaries {
    fn default() -> Self {
        Self {
            pending: None,
            error: None,
            identity: None,
            hydration_trigger: None,
        }
    }
}

impl JetWebRouteBoundaries {
    /// Validate and retain compiler-derived boundary identities.  A route
    /// cannot attach a boundary ID without the exact source/build identity
    /// that the shared WebPending kernel uses for stream and hydration checks.
    pub fn new(
        pending_boundary_id: Option<String>,
        error_boundary_id: Option<String>,
        identity: Option<JetWebIslandIdentity>,
        hydration_trigger: Option<JetWebHydrationTrigger>,
    ) -> Result<Self, String> {
        let pending = pending_boundary_id
            .map(JetWebBoundaryId::new)
            .transpose()
            .map_err(|error| error.to_string())?;
        let error = error_boundary_id
            .map(JetWebBoundaryId::new)
            .transpose()
            .map_err(|error| error.to_string())?;
        if (pending.is_some() || error.is_some() || hydration_trigger.is_some())
            && identity.is_none()
        {
            return Err("web route boundary IDs and hydration require an island identity".to_string());
        }
        if let Some(identity) = identity.as_ref() {
            identity.validate().map_err(|error| error.to_string())?;
        }
        Ok(Self {
            pending,
            error,
            identity,
            hydration_trigger,
        })
    }

    pub fn pending(
        boundary_id: String,
        identity: JetWebIslandIdentity,
        hydration_trigger: JetWebHydrationTrigger,
    ) -> Result<Self, String> {
        Self::new(
            Some(boundary_id.clone()),
            Some(boundary_id),
            Some(identity),
            Some(hydration_trigger),
        )
    }
}
struct JetWebRoute {
    pattern_text: String,
    pattern: JetHTTPRoutePattern,
    identity: String,
    params: Vec<JetWebRouterField>,
    search: Vec<JetWebRouterField>,
    search_codec: JetWebRouterSearchCodec,
    loader: JetWebRouteLoader,
    pending: Option<JetWebPendingBoundary>,
    error: Option<JetWebErrorBoundary>,
    boundaries: JetWebRouteBoundaries,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetWebNavigation {
    pub url: String,
    pub route: String,
    pub params: BTreeMap<String, String>,
    pub search: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct JetWebNavigationState {
    pub status: JetWebNavigationStatus,
    pub current: Option<JetWebNavigation>,
    pub data: String,
    pub error: String,
    pub pending_boundary_id: Option<JetWebBoundaryId>,
    pub error_boundary_id: Option<JetWebBoundaryId>,
    pub island_identity: Option<JetWebIslandIdentity>,
    pub hydration_trigger: Option<JetWebHydrationTrigger>,
}

impl Default for JetWebNavigationState {
    fn default() -> Self {
        Self {
            status: JetWebNavigationStatus::Idle,
            current: None,
            data: String::new(),
            error: String::new(),
            pending_boundary_id: None,
            error_boundary_id: None,
            island_identity: None,
            hydration_trigger: None,
        }
    }
}

type JetWebRouteLoader = Arc<dyn Fn(&JetWebNavigation) -> Result<String, String> + Send + Sync>;
type JetWebPendingBoundary = Arc<dyn Fn(&JetWebNavigation) -> String + Send + Sync>;
type JetWebErrorBoundary = Arc<dyn Fn(&JetWebNavigation, &str) -> String + Send + Sync>;
type JetWebNotFoundBoundary = Arc<dyn Fn(&str) -> String + Send + Sync>;

fn jet_web_router_navigation_state(
    status: JetWebNavigationStatus,
    current: Option<JetWebNavigation>,
    data: String,
    error: String,
    boundaries: &JetWebRouteBoundaries,
) -> JetWebNavigationState {
    JetWebNavigationState {
        status,
        current,
        data,
        error,
        pending_boundary_id: boundaries.pending.clone(),
        error_boundary_id: boundaries.error.clone(),
        island_identity: boundaries.identity.clone(),
        hydration_trigger: boundaries.hydration_trigger,
    }
}

#[derive(Clone)]
struct JetWebRouterInner {
    routes: Vec<JetWebRoute>,
    cache: BTreeMap<String, JetWebRouterCacheState>,
    cache_generation: u64,
    not_found: Option<JetWebNotFoundBoundary>,
    state: jet_std::JetSignal<JetWebNavigationState>,
    next_navigation_id: u64,
    active_navigation: Option<(u64, Arc<AtomicBool>)>,
}

impl Clone for JetWebRoute {
    fn clone(&self) -> Self {
        Self {
            pattern_text: self.pattern_text.clone(),
            pattern: self.pattern.clone(),
            identity: self.identity.clone(),
            params: self.params.clone(),
            search: self.search.clone(),
            search_codec: self.search_codec,
            loader: self.loader.clone(),
            pending: self.pending.clone(),
            error: self.error.clone(),
            boundaries: self.boundaries.clone(),
        }
    }
}

#[derive(Clone)]
pub struct JetWebRouter {
    inner: Arc<Mutex<JetWebRouterInner>>,
}

impl JetWebRouter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(JetWebRouterInner {
                routes: Vec::new(),
                cache: BTreeMap::new(),
                cache_generation: 0,
                not_found: None,
                state: jet_std::JetSignal::new(JetWebNavigationState::default()),
                next_navigation_id: 0,
                active_navigation: None,
            })),
        }
    }
    fn start_navigation(
        &self,
    ) -> Result<
        (
            u64,
            Arc<AtomicBool>,
            jet_std::JetSignal<JetWebNavigationState>,
            JetWebNavigationState,
        ),
        String,
    > {
        let mut state = self.inner.lock().map_err(|_| "router is unavailable".to_string())?;
        if let Some((_, previous)) = state.active_navigation.take() {
            previous.store(true, Ordering::Release);
            let mut superseded = state.state.get();
            superseded.status = JetWebNavigationStatus::Aborted;
            superseded.error = "navigation superseded".to_string();
            state.state.set(superseded);
        }
        state.next_navigation_id = state
            .next_navigation_id
            .checked_add(1)
            .ok_or_else(|| "router navigation id space exhausted".to_string())?;
        let id = state.next_navigation_id;
        let token = Arc::new(AtomicBool::new(false));
        let signal = state.state.clone();
        let previous = state.state.get();
        state.active_navigation = Some((id, token.clone()));
        Ok((id, token, signal, previous))
    }

    fn navigation_active(&self, id: u64, token: &Arc<AtomicBool>) -> bool {
        if token.load(Ordering::Acquire) {
            return false;
        }
        self.inner
            .lock()
            .map(|state| {
                state
                    .active_navigation
                    .as_ref()
                    .is_some_and(|(active_id, active_token)| {
                        *active_id == id && Arc::ptr_eq(active_token, token)
                    })
            })
            .unwrap_or(false)
    }

    fn finish_navigation(&self, id: u64, token: &Arc<AtomicBool>) {
        if let Ok(mut state) = self.inner.lock() {
            if state
                .active_navigation
                .as_ref()
                .is_some_and(|(active_id, active_token)| {
                    *active_id == id && Arc::ptr_eq(active_token, token)
                })
            {
                state.active_navigation = None;
            }
        }
    }
    fn set_resolution_error(&self, error: &str) {
        if let Ok(state) = self.inner.lock() {
            let mut current = state.state.get();
            let rendered = state
                .not_found
                .as_ref()
                .map(|boundary| boundary(error))
                .unwrap_or_else(|| error.to_string());
            current.status = JetWebNavigationStatus::Error;
            current.data = jet_web_router_bound_data(rendered, error);
            current.error = error.to_string();
            state.state.set(current);
        }
    }

    fn cache_value(&self, key: &str) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|state| state.cache.get(key).cloned())
            .filter(|entry| entry.status == JetWebRouterCacheStatus::Fresh)
            .map(|entry| entry.data)
    }

    fn cache_store(
        &self,
        key: String,
        identity: String,
        status: JetWebRouterCacheStatus,
        data: String,
        dependencies: Vec<String>,
    ) -> Result<(), String> {
        let mut state = self.inner.lock().map_err(|_| "router is unavailable".to_string())?;
        state.cache_generation = state
            .cache_generation
            .checked_add(1)
            .ok_or_else(|| "router cache generation exhausted".to_string())?;
        let generation = state.cache_generation;
        if state.cache.len() >= JET_WEB_ROUTER_MAX_PRELOADED && !state.cache.contains_key(&key) {
            if let Some(oldest) = state.cache.keys().next().cloned() {
                state.cache.remove(&oldest);
            }
        }
        state.cache.insert(
            key,
            JetWebRouterCacheState {
                identity,
                status,
                data,
                dependencies,
                generation,
            },
        );
        Ok(())
    }


    /// Abort the currently running preload or loader.  Synchronous loaders
    /// observe this before publication; a concurrent loader cannot overwrite a
    /// newer navigation.
    pub fn abort(&self) -> bool {
        let Some((_, token, signal, mut current)) = self.start_navigation_abort() else {
            return false;
        };
        token.store(true, Ordering::Release);
        current.status = JetWebNavigationStatus::Aborted;
        current.error = "navigation aborted".to_string();
        signal.set(current);
        true
    }

    fn start_navigation_abort(
        &self,
    ) -> Option<(u64, Arc<AtomicBool>, jet_std::JetSignal<JetWebNavigationState>, JetWebNavigationState)> {
        let mut state = self.inner.lock().ok()?;
        let (id, token) = state.active_navigation.take()?;
        let signal = state.state.clone();
        let current = state.state.get();
        Some((id, token, signal, current))
    }


    pub fn route<F>(
        &self,
        pattern: String,
        params: Vec<JetWebRouterField>,
        search: Vec<JetWebRouterField>,
        loader: F,
    ) -> Result<Self, String>
    where
        F: Fn(&JetWebNavigation) -> Result<String, String> + Send + Sync + 'static,
    {
        self.route_with_boundaries(
            pattern, params, search, loader,
            None::<fn(&JetWebNavigation) -> String>,
            None::<fn(&JetWebNavigation, &str) -> String>,
        )
    }

    pub fn route_with_search_codec<F>(
        &self,
        pattern: String,
        params: Vec<JetWebRouterField>,
        search: Vec<JetWebRouterField>,
        search_codec: JetWebRouterSearchCodec,
        loader: F,
    ) -> Result<Self, String>
    where
        F: Fn(&JetWebNavigation) -> Result<String, String> + Send + Sync + 'static,
    {
        self.route_with_facts_codec(
            pattern,
            params,
            search,
            search_codec,
            loader,
            None::<fn(&JetWebNavigation) -> String>,
            None::<fn(&JetWebNavigation, &str) -> String>,
            JetWebRouteBoundaries::default(),
        )
    }

    pub fn route_with_boundaries<F, P, E>(
        &self,
        pattern: String,
        params: Vec<JetWebRouterField>,
        search: Vec<JetWebRouterField>,
        loader: F,
        pending: Option<P>,
        error: Option<E>,
    ) -> Result<Self, String>
    where
        F: Fn(&JetWebNavigation) -> Result<String, String> + Send + Sync + 'static,
        P: Fn(&JetWebNavigation) -> String + Send + Sync + 'static,
        E: Fn(&JetWebNavigation, &str) -> String + Send + Sync + 'static,
    {
        self.route_with_facts_codec(
            pattern,
            params,
            search,
            JetWebRouterSearchCodec::Query,
            loader,
            pending,
            error,
            JetWebRouteBoundaries::default(),
        )
    }

    /// Register one route with compiler-derived pending/error boundary facts.
    /// The exact IDs and island identity are retained for the shared
    /// WebPending registry; callbacks remain renderer projections.
    pub fn route_with_facts<F, P, E>(
        &self,
        pattern: String,
        params: Vec<JetWebRouterField>,
        search: Vec<JetWebRouterField>,
        loader: F,
        pending: Option<P>,
        error: Option<E>,
        boundaries: JetWebRouteBoundaries,
    ) -> Result<Self, String>
    where
        F: Fn(&JetWebNavigation) -> Result<String, String> + Send + Sync + 'static,
        P: Fn(&JetWebNavigation) -> String + Send + Sync + 'static,
        E: Fn(&JetWebNavigation, &str) -> String + Send + Sync + 'static,
    {
        self.route_with_facts_codec(
            pattern,
            params,
            search,
            JetWebRouterSearchCodec::Query,
            loader,
            pending,
            error,
            boundaries,
        )
    }

    pub fn route_with_facts_codec<F, P, E>(
        &self,
        pattern: String,
        params: Vec<JetWebRouterField>,
        search: Vec<JetWebRouterField>,
        search_codec: JetWebRouterSearchCodec,
        loader: F,
        pending: Option<P>,
        error: Option<E>,
        boundaries: JetWebRouteBoundaries,
    ) -> Result<Self, String>
    where
        F: Fn(&JetWebNavigation) -> Result<String, String> + Send + Sync + 'static,
        P: Fn(&JetWebNavigation) -> String + Send + Sync + 'static,
        E: Fn(&JetWebNavigation, &str) -> String + Send + Sync + 'static,
    {
        if params.len() + search.len() > JET_WEB_ROUTER_MAX_PARAMS {
            return Err("route declares too many typed parameters".to_string());
        }
        let parsed = jet_web_router_parse_pattern(&pattern)?;
        jet_web_router_validate_fields(&parsed, &params, &search)?;
        let mut state = self.inner.lock().map_err(|_| "router is unavailable".to_string())?;
        if state.routes.len() >= JET_WEB_ROUTER_MAX_ROUTES {
            return Err("router route table is full".to_string());
        }
        if state.routes.iter().any(|route| route.pattern_text == pattern) {
            return Err(format!("duplicate route `{pattern}`"));
        }
        if let Some(identity) = boundaries.identity.as_ref() {
            if let Some(boundary_id) = boundaries.pending.as_ref() {
                jet_web_stream_register(boundary_id.as_str().to_string(), identity.clone())
                    .map_err(|error| error.to_string())?;
            }
            if boundaries.error.as_ref() != boundaries.pending.as_ref() {
                if let Some(boundary_id) = boundaries.error.as_ref() {
                    jet_web_stream_register(boundary_id.as_str().to_string(), identity.clone())
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        let identity = jet_web_router_identity(&pattern);
        state.routes.push(JetWebRoute {
            pattern_text: pattern,
            pattern: parsed,
            identity,
            params,
            search,
            search_codec,
            loader: Arc::new(loader),
            pending: pending.map(|handler| Arc::new(handler) as JetWebPendingBoundary),
            error: error.map(|handler| Arc::new(handler) as JetWebErrorBoundary),
            boundaries,
        });
        Ok(self.clone())
    }

    pub fn not_found<F>(&self, handler: F) -> Result<Self, String>
    where
        F: Fn(&str) -> String + Send + Sync + 'static,
    {
        self.inner
            .lock()
            .map_err(|_| "router is unavailable".to_string())?
            .not_found = Some(Arc::new(handler));
        Ok(self.clone())
    }
    fn resolve(&self, url: &str) -> Result<(JetWebRoute, JetWebNavigation), String> {
        let (path, query) = jet_web_router_split_url(url)?;
        let state = self.inner.lock().map_err(|_| "router is unavailable".to_string())?;
        let mut selected: Option<(usize, JetWebRoute, JetWebNavigation)> = None;
        for (index, route) in state.routes.iter().enumerate() {
            let Some(raw_params) = jet_web_router_match(&route.pattern, path)? else {
                continue;
            };
            let raw_search = jet_web_router_parse_search(query, route.search_codec)?;
            let params = jet_web_router_typed_values(&route.params, &raw_params)?;
            jet_web_router_validate_search(&route.search, &raw_search)?;
            let search = jet_web_router_typed_values(&route.search, &raw_search)?;
            let candidate = (
                index,
                route.clone(),
                JetWebNavigation {
                    url: url.to_string(),
                    route: route.pattern_text.clone(),
                    params,
                    search,
                },
            );
            if selected
                .as_ref()
                .map(|current| {
                    jet_http_route_selection_cmp(
                        &candidate.1.pattern,
                        candidate.0,
                        &current.1.pattern,
                        current.0,
                    ) == std::cmp::Ordering::Greater
                })
                .unwrap_or(true)
            {
                selected = Some(candidate);
            }
        }
        selected
            .map(|(_, route, navigation)| (route, navigation))
            .ok_or_else(|| format!("no web route matches `{url}`"))
    }

    pub fn preload(&self, url: String) -> Result<JetWebNavigation, String> {
        let (route, navigation) = match self.resolve(&url) {
            Ok(value) => value,
            Err(error) => {
                self.set_resolution_error(&error);
                return Err(error);
            }
        };
        let cache_key = jet_web_router_cache_key(&route, &navigation);
        if self.cache_value(&cache_key).is_some() {
            return Ok(navigation);
        }
        let (id, token, signal, previous) = self.start_navigation()?;
        signal.set(jet_web_router_navigation_state(
            JetWebNavigationStatus::Preloading,
            previous.current.clone(),
            previous.data.clone(),
            String::new(),
            &route.boundaries,
        ));
        let loaded = (route.loader)(&navigation);
        if !self.navigation_active(id, &token) {
            return Err(if token.load(Ordering::Acquire) {
                "navigation aborted".to_string()
            } else {
                "navigation superseded".to_string()
            });
        }
        let loaded = match loaded {
            Ok(data) if data.len() <= JET_WEB_ROUTER_MAX_DATA => Ok(data),
            Ok(_) => Err("preloaded route data is too large".to_string()),
            Err(error) => Err(error),
        };
        match loaded {
            Ok(data) => {
                let _ = self.cache_store(
                    cache_key,
                    route.identity.clone(),
                    JetWebRouterCacheStatus::Fresh,
                    data,
                    vec![route.identity.clone()],
                );
                signal.set(jet_web_router_navigation_state(
                    if previous.current.is_some() {
                        JetWebNavigationStatus::Ready
                    } else {
                        JetWebNavigationStatus::Idle
                    },
                    previous.current,
                    previous.data,
                    String::new(),
                    &route.boundaries,
                ));
                self.finish_navigation(id, &token);
                Ok(navigation)
            }
            Err(error) => {
                let _ = self.cache_store(
                    cache_key,
                    route.identity,
                    JetWebRouterCacheStatus::Invalidated,
                    String::new(),
                    Vec::new(),
                );
                signal.set(jet_web_router_navigation_state(
                    if previous.current.is_some() {
                        JetWebNavigationStatus::Ready
                    } else {
                        JetWebNavigationStatus::Idle
                    },
                    previous.current,
                    previous.data,
                    error.clone(),
                    &route.boundaries,
                ));
                self.finish_navigation(id, &token);
                Err(error)
            }
        }
    }

    pub fn navigate(&self, url: String) -> Result<JetWebNavigation, String> {
        let (route, navigation) = match self.resolve(&url) {
            Ok(value) => value,
            Err(error) => {
                self.set_resolution_error(&error);
                return Err(error);
            }
        };
        let cache_key = jet_web_router_cache_key(&route, &navigation);
        let (id, token, signal, _) = self.start_navigation()?;
        let cached = self.cache_value(&cache_key);
        let from_cache = cached.is_some();
        let pending_data = route
            .pending
            .as_ref()
            .map(|pending| pending(&navigation))
            .unwrap_or_default();
        signal.set(jet_web_router_navigation_state(
            JetWebNavigationStatus::Pending,
            Some(navigation.clone()),
            pending_data,
            String::new(),
            &route.boundaries,
        ));
        let loaded = cached.map(Ok).unwrap_or_else(|| (route.loader)(&navigation));
        if !self.navigation_active(id, &token) {
            return Err(if token.load(Ordering::Acquire) {
                "navigation aborted".to_string()
            } else {
                "navigation superseded".to_string()
            });
        }
        let loaded = match loaded {
            Ok(data) if data.len() <= JET_WEB_ROUTER_MAX_DATA => Ok(data),
            Ok(_) => Err("route loader data is too large".to_string()),
            Err(error) => Err(error),
        };
        match loaded {
            Ok(data) => {
                if !from_cache {
                    let _ = self.cache_store(
                        cache_key,
                        route.identity.clone(),
                        JetWebRouterCacheStatus::Fresh,
                        data.clone(),
                        vec![route.identity.clone()],
                    );
                }
                signal.set(jet_web_router_navigation_state(
                    JetWebNavigationStatus::Ready,
                    Some(navigation.clone()),
                    data,
                    String::new(),
                    &route.boundaries,
                ));
                self.finish_navigation(id, &token);
                Ok(navigation)
            }
            Err(error) => {
                let rendered = jet_web_router_bound_data(
                    route
                        .error
                        .as_ref()
                        .map(|boundary| boundary(&navigation, &error))
                        .unwrap_or_else(|| error.clone()),
                    &error,
                );
                let _ = self.cache_store(
                    cache_key,
                    route.identity,
                    JetWebRouterCacheStatus::Invalidated,
                    String::new(),
                    Vec::new(),
                );
                signal.set(jet_web_router_navigation_state(
                    JetWebNavigationStatus::Error,
                    Some(navigation.clone()),
                    rendered,
                    error.clone(),
                    &route.boundaries,
                ));
                self.finish_navigation(id, &token);
                Err(error)
            }
        }
    }
    pub fn stale(&self, identity: String) -> i64 {
        self.update_cache_status(&identity, JetWebRouterCacheStatus::Stale)
    }

    pub fn invalidate(&self, identity: String) -> i64 {
        self.update_cache_status(&identity, JetWebRouterCacheStatus::Invalidated)
    }

    fn update_cache_status(&self, identity: &str, status: JetWebRouterCacheStatus) -> i64 {
        let Ok(mut state) = self.inner.lock() else {
            return 0;
        };
        let mut updated = 0;
        for entry in state.cache.values_mut() {
            if entry.identity == identity {
                entry.status = status;
                updated += 1;
            }
        }
        updated
    }

    pub fn collect(&self, identity: String) -> i64 {
        let Ok(mut state) = self.inner.lock() else {
            return 0;
        };
        let keys = state
            .cache
            .iter()
            .filter(|(_, entry)| entry.identity == identity)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let count = keys.len() as i64;
        for key in keys {
            state.cache.remove(&key);
        }
        count
    }

    pub fn cache_state(&self, identity: String) -> JetWebRouterCacheState {
        self.inner
            .lock()
            .ok()
            .and_then(|state| {
                state
                    .cache
                    .values()
                    .find(|entry| entry.identity == identity)
                    .cloned()
                    .or_else(|| state.cache.get(&identity).cloned())
            })
            .unwrap_or(JetWebRouterCacheState {
                identity,
                status: JetWebRouterCacheStatus::Collected,
                data: String::new(),
                dependencies: Vec::new(),
                generation: 0,
            })
    }

    pub fn cache_show(&self) -> String {
        let Ok(state) = self.inner.lock() else {
            return "RouterCache(unavailable)".to_string();
        };
        let entries = state
            .cache
            .iter()
            .map(|(key, entry)| {
                format!(
                    "{{\"key\":{},\"identity\":{},\"status\":{},\"generation\":{},\"dependencies\":{},\"data_bytes\":{}}}",
                    jet_web_router_json_string(key),
                    jet_web_router_json_string(&entry.identity),
                    jet_web_router_json_string(entry.status.name()),
                    entry.generation,
                    jet_web_router_json_array(&entry.dependencies),
                    entry.data.len(),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"generation\":{},\"entries\":[{}]}}", state.cache_generation, entries)
    }



    pub fn current(&self) -> JetWebNavigationState {
        self.inner
            .lock()
            .map(|state| state.state.get())
            .unwrap_or_else(|_| JetWebNavigationState {
                status: JetWebNavigationStatus::Error,
                current: None,
                data: String::new(),
                error: "router is unavailable".to_string(),
                pending_boundary_id: None,
                error_boundary_id: None,
                island_identity: None,
                hydration_trigger: None,
            })
    }

    pub fn state_signal(&self) -> jet_std::JetSignal<JetWebNavigationState> {
        self.inner
            .lock()
            .map(|state| state.state.clone())
            .unwrap_or_else(|_| jet_std::JetSignal::new(JetWebNavigationState {
                status: JetWebNavigationStatus::Error,
                current: None,
                data: String::new(),
                error: "router is unavailable".to_string(),
                pending_boundary_id: None,
                error_boundary_id: None,
                island_identity: None,
                hydration_trigger: None,
            }))
    }

    pub fn show(&self) -> String {
        let state = self.current();
        let route = state
            .current
            .as_ref()
            .map(|current| current.route.as_str())
            .unwrap_or("-");
        let pending = state
            .pending_boundary_id
            .as_ref()
            .map(|boundary| boundary.as_str())
            .unwrap_or("-");
        let error_boundary = state
            .error_boundary_id
            .as_ref()
            .map(|boundary| boundary.as_str())
            .unwrap_or("-");
        let hydration = state
            .hydration_trigger
            .map(JetWebHydrationTrigger::as_str)
            .unwrap_or("-");
        format!(
            "Router(status={},route={},url={},params={},search={},pending_boundary={},error_boundary={},hydration={},error={})",
            state.status.name(),
            route,
            state.current.as_ref().map(|current| current.url.as_str()).unwrap_or("-"),
            state.current.as_ref().map(|current| current.params.len()).unwrap_or(0),
            state.current.as_ref().map(|current| current.search.len()).unwrap_or(0),
            pending,
            error_boundary,
            hydration,
            state.error
        )
    }

    pub fn link(
        &self,
        pattern: String,
        params: BTreeMap<String, String>,
        search: BTreeMap<String, String>,
    ) -> Result<String, String> {
        let state = self.inner.lock().map_err(|_| "router is unavailable".to_string())?;
        let route = state
            .routes
            .iter()
            .find(|route| route.pattern_text == pattern)
            .ok_or_else(|| format!("no web route named `{pattern}`"))?;
        let typed_params = jet_web_router_typed_values(&route.params, &params)?;
        let typed_search = jet_web_router_typed_values(&route.search, &search)?;
        let mut path = String::new();
        path.push('/');
        for (index, segment) in route.pattern.segments.iter().enumerate() {
            if index > 0 {
                path.push('/');
            }
            match segment {
                JetHTTPRouteSegment::Static(value) => {
                    path.push_str(&jet_http_route_encode_segment(value, false));
                }
                JetHTTPRouteSegment::Param(name) => path.push_str(&jet_http_route_encode_segment(
                    typed_params.get(name).ok_or_else(|| format!("missing `{name}`"))?,
                    false,
                )),
                JetHTTPRouteSegment::CatchAll(name) => path.push_str(&jet_http_route_encode_segment(
                    typed_params.get(name).ok_or_else(|| format!("missing `{name}`"))?,
                    true,
                )),
            }
        }
        if path.is_empty() {
            path.push('/');
        }
        if !typed_search.is_empty() {
            path.push('?');
            match route.search_codec {
                JetWebRouterSearchCodec::Query => {
                    for (index, (key, value)) in typed_search.iter().enumerate() {
                        if index > 0 {
                            path.push('&');
                        }
                        path.push_str(&jet_http_route_encode_component(key, true, false));
                        path.push('=');
                        path.push_str(&jet_http_route_encode_component(value, true, false));
                    }
                }
                JetWebRouterSearchCodec::Json => {
                    let entries = typed_search
                        .iter()
                        .map(|(key, value)| {
                            let field = route
                                .search
                                .iter()
                                .find(|field| field.name == *key)
                                .ok_or_else(|| format!("search parameter `{key}` is not declared"))?;
                            Ok((key.clone(), jet_web_router_json_value_for_field(field, value)?))
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    let encoded = jet_std::render_json(
                        &jet_std::DataTree::Object(entries),
                        false,
                        0,
                    );
                    path.push_str("search=");
                    path.push_str(&jet_http_route_encode_component(&encoded, true, false));
                }
            }
        }
        Ok(path)
    }
}
/// Register one checked route in the canonical visual-router graph.  The
/// compiler supplies typed fields and the loader callback; this adapter only
/// forwards to the existing route registration seam.
pub fn jet_web_router_route<F>(
    router: &JetWebRouter,
    pattern: String,
    params: Vec<JetWebRouterField>,
    search: Vec<JetWebRouterField>,
    loader: F,
) -> Result<JetWebRouter, String>
where
    F: Fn(&JetWebNavigation) -> Result<String, String> + Send + Sync + 'static,
{
    router.route(pattern, params, search, loader)
}

/// Register a route whose typed search state uses the explicit JSON-first or
/// query-string codec selected by the checked route facts.
pub fn jet_web_router_route_with_search_codec<F>(
    router: &JetWebRouter,
    pattern: String,
    params: Vec<JetWebRouterField>,
    search: Vec<JetWebRouterField>,
    search_codec: JetWebRouterSearchCodec,
    loader: F,
) -> Result<JetWebRouter, String>
where
    F: Fn(&JetWebNavigation) -> Result<String, String> + Send + Sync + 'static,
{
    router.route_with_search_codec(pattern, params, search, search_codec, loader)
}

/// Install the one not-found projection used by all visual-router hosts.
pub fn jet_web_router_not_found<F>(
    router: &JetWebRouter,
    handler: F,
) -> Result<JetWebRouter, String>
where
    F: Fn(&str) -> String + Send + Sync + 'static,
{
    router.not_found(handler)
}

pub fn jet_web_router_stale(router: &JetWebRouter, identity: String) -> i64 {
    router.stale(identity)
}

pub fn jet_web_router_invalidate(router: &JetWebRouter, identity: String) -> i64 {
    router.invalidate(identity)
}

pub fn jet_web_router_collect(router: &JetWebRouter, identity: String) -> i64 {
    router.collect(identity)
}

pub fn jet_web_router_cache_state(
    router: &JetWebRouter,
    identity: String,
) -> JetWebRouterCacheState {
    router.cache_state(identity)
}

pub fn jet_web_router_cache_show(router: &JetWebRouter) -> String {
    router.cache_show()
}

impl Default for JetWebRouter {
    fn default() -> Self {
        Self::new()
    }
}

pub fn jet_web_router_new() -> JetWebRouter {
    JetWebRouter::new()
}

pub fn jet_web_router_navigate(
    router: &JetWebRouter,
    url: String,
) -> Result<JetWebNavigation, String> {
    router.navigate(url)
}

pub fn jet_web_router_preload(
    router: &JetWebRouter,
    url: String,
) -> Result<JetWebNavigation, String> {
    router.preload(url)
}

pub fn jet_web_router_current(router: &JetWebRouter) -> JetWebNavigationState {
    router.current()
}

pub fn jet_web_router_show(router: &JetWebRouter) -> String {
    router.show()
}

pub fn jet_web_router_abort(router: &JetWebRouter) -> bool {
    router.abort()
}

pub fn jet_web_router_link(
    router: &JetWebRouter,
    pattern: String,
    params: BTreeMap<String, String>,
    search: BTreeMap<String, String>,
) -> Result<String, String> {
    router.link(pattern, params, search)
}
