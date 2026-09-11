// D-WEBAPP1=D / D-WEBAUTHOR1=D / D-DX-ROUTER1=A: `core.web.app` — statically
// known application builder value. Sema owns the typed graph; this runtime
// handle records the same edges and feeds them, once, into the canonical
// WebRouter navigation kernel and the typed server-function boundary.
// Std-only (I6).

mod jet_app_impl {
    use std::any::type_name;
    use std::sync::{Arc, Mutex};
    use super::jet_std;

    /// One checked page handler: typed route inputs come from the router's
    /// navigation and the route's loader data.
    type PageHandler = Arc<
        dyn Fn(&super::JetWebNavigation, &str) -> Result<JetWebPage, String> + Send + Sync,
    >;
    /// One checked route boundary (`pending`/`not_found`/`error`).
    type BoundaryHandler = Arc<dyn Fn() -> Result<JetWebPage, String> + Send + Sync>;
    /// One checked loader: typed route inputs in, encoded route data out.
    type LoaderHandler =
        Arc<dyn Fn(&super::JetWebNavigation) -> Result<String, String> + Send + Sync>;
    type ActionHandler = Option<super::JetWebServerFunction>;
    type MountHandler = Arc<dyn Fn(&String) + Send + Sync>;

    /// The one wire rendering of a checked handler failure.  The default
    /// callable carrier (`Err`) renders its report; every Codable domain is
    /// JSON; a proven-unreachable domain has no values.
    pub trait JetAppFailure {
        fn into_app_failure(self) -> String;
    }

    impl<E: super::__jet_Encode> JetAppFailure for E {
        fn into_app_failure(self) -> String {
            super::jet_enc_json_to_string(&self)
        }
    }

    impl JetAppFailure for super::JetErr {
        fn into_app_failure(self) -> String {
            self.to_string()
        }
    }

    impl JetAppFailure for std::convert::Infallible {
        fn into_app_failure(self) -> String {
            match self {}
        }
    }

    fn jet_app_invalid(message: String) -> ! {
        super::jet_runtime_stop("E3001", "", 0, &message)
    }

    fn jet_app_server_function<I, O, E, F>(
        name: String,
        source_id: String,
        handler: F,
    ) -> super::JetWebServerFunction
    where
        I: super::__jet_Decode + 'static,
        O: super::__jet_Encode + 'static,
        E: JetAppFailure + 'static,
        F: Fn(I) -> Result<O, E> + Send + Sync + 'static,
    {
        let input_type = type_name::<I>().to_string();
        let output_type = type_name::<O>().to_string();
        let error_type = type_name::<E>().to_string();
        let dispatch: super::JetWebServerFnHandler = Arc::new(move |_context, body| {
            let body = body.to_string();
            let input = super::jet_enc_json_decode::<I>(&body).map_err(|errors| {
                super::JetWebServerFnError::InvalidInput(format!("{errors:?}"))
            })?;
            match handler(input) {
                Ok(output) => Ok(super::jet_enc_json_to_string(&output)),
                Err(error) => Err(super::JetWebServerFnError::Handler {
                    code: "application_error".to_string(),
                    message: error.into_app_failure(),
                }),
            }
        });
        super::jet_web_server_fn_typed(
            name,
            source_id,
            "POST".to_string(),
            input_type,
            output_type,
            error_type,
            dispatch,
        )
        .unwrap_or_else(|error| jet_app_invalid(format!("invalid app server function: {error:?}")))
    }

    fn jet_app_unit_server_function<O, E, F>(
        name: String,
        source_id: String,
        handler: F,
    ) -> super::JetWebServerFunction
    where
        O: super::__jet_Encode + 'static,
        E: JetAppFailure + 'static,
        F: Fn() -> Result<O, E> + Send + Sync + 'static,
    {
        let output_type = type_name::<O>().to_string();
        let error_type = type_name::<E>().to_string();
        let dispatch: super::JetWebServerFnHandler = Arc::new(move |_context, _body| {
            match handler() {
                Ok(output) => Ok(super::jet_enc_json_to_string(&output)),
                Err(error) => Err(super::JetWebServerFnError::Handler {
                    code: "application_error".to_string(),
                    message: error.into_app_failure(),
                }),
            }
        });
        super::jet_web_server_fn_typed(
            name,
            source_id,
            "POST".to_string(),
            "Unit".to_string(),
            output_type,
            error_type,
            dispatch,
        )
        .unwrap_or_else(|error| jet_app_invalid(format!("invalid app server function: {error:?}")))
    }


    /// A checked `action`/`form`/`data` handler.  The compiler hands every
    /// handler over as the send-safe borrow callback, so the typed input is
    /// decoded here once from the shared JSON wire and borrowed by the user
    /// function.
    pub trait JetAppActionHandler {
        fn into_server_function(self, name: String) -> super::JetWebServerFunction;
    }

    impl<I, O, E> JetAppActionHandler for Arc<dyn Fn(&I) -> Result<O, E> + Send + Sync + 'static>
    where
        I: super::__jet_Decode + 'static,
        O: super::__jet_Encode + 'static,
        E: JetAppFailure + 'static,
    {
        fn into_server_function(self, name: String) -> super::JetWebServerFunction {
            jet_app_server_function::<I, O, E, _>(
                name.clone(),
                name,
                move |input| self(&input),
            )
        }
    }

    impl<O, E> JetAppActionHandler for Arc<dyn Fn() -> Result<O, E> + Send + Sync + 'static>
    where
        O: super::__jet_Encode + 'static,
        E: JetAppFailure + 'static,
    {
        fn into_server_function(self, name: String) -> super::JetWebServerFunction {
            jet_app_unit_server_function::<O, E, _>(
                name.clone(),
                name,
                move || self(),
            )
        }
    }

    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub enum JetAppRenderMode {
        #[default]
        Csr,
        Ssr,
        Ssg,
        Stream,
        Island,
    }

    impl JetAppRenderMode {
        pub const fn as_str(self) -> &'static str {
            match self {
                Self::Csr => "csr",
                Self::Ssr => "ssr",
                Self::Ssg => "ssg",
                Self::Stream => "stream",
                Self::Island => "island",
            }
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum JetAppHydrationTrigger {
        Immediate,
        ClientLoad,
        ClientIdle,
        ClientVisible,
        Manual,
    }

    impl JetAppHydrationTrigger {
        pub const fn as_str(self) -> &'static str {
            match self {
                Self::Immediate => "immediate",
                Self::ClientLoad => "client-load",
                Self::ClientIdle => "client-idle",
                Self::ClientVisible => "client-visible",
                Self::Manual => "manual",
            }
        }
    }

    /// Compiler-derived facts carried by one runtime route registration.
    ///
    /// The route owns this value.  A later route cannot inherit a mutable
    /// application-wide render choice, and hosts only project these facts.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct JetAppRouteField {
        pub name: String,
        pub ty: String,
        pub codec: String,
        pub required: bool,
        pub default: Option<String>,
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct JetAppLoaderFacts {
        pub handler: String,
        pub data_type: String,
        pub dependency: String,
        pub preload: bool,
        pub cache_identity: String,
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct JetAppRouteFacts {
        pub route_identity: String,
        pub path_params: Vec<JetAppRouteField>,
        pub search_params: Vec<JetAppRouteField>,
        pub search_codec: String,
        pub loader_contract: Option<JetAppLoaderFacts>,
        pub pending_boundary: Option<String>,
        pub not_found_boundary: Option<String>,
        pub error_boundary: Option<String>,
        pub mode: JetAppRenderMode,
        pub reason: Vec<String>,
        pub effects: Vec<String>,
        pub interactive: bool,
        pub loader: Option<String>,
        pub form: Option<String>,
        pub cache: String,
        pub override_mode: Option<JetAppRenderMode>,
        pub pending_boundary_id: Option<u64>,
        pub island_identity: Option<String>,
        pub hydration_trigger: Option<JetAppHydrationTrigger>,
    }

    impl JetAppRouteFacts {
        pub fn with_mode(mode: JetAppRenderMode) -> Self {
            Self {
                mode,
                reason: vec![format!("explicit render mode `{}`", mode.as_str())],
                override_mode: Some(mode),
                ..Self::default()
            }
        }

        fn json(&self) -> String {
            let reason = self
                .reason
                .iter()
                .map(|value| jet_app_json_string(value))
                .collect::<Vec<_>>()
                .join(",");
            let effects = self
                .effects
                .iter()
                .map(|value| jet_app_json_string(value))
                .collect::<Vec<_>>()
                .join(",");
            let path_params = self
                .path_params
                .iter()
                .map(jet_app_route_field_json)
                .collect::<Vec<_>>()
                .join(",");
            let search_params = self
                .search_params
                .iter()
                .map(jet_app_route_field_json)
                .collect::<Vec<_>>()
                .join(",");
            let loader = self
                .loader
                .as_deref()
                .map(jet_app_json_string)
                .unwrap_or_else(|| "null".to_string());
            let loader_contract = self
                .loader_contract
                .as_ref()
                .map(jet_app_loader_facts_json)
                .unwrap_or_else(|| "null".to_string());
            let form = self
                .form
                .as_deref()
                .map(jet_app_json_string)
                .unwrap_or_else(|| "null".to_string());
            let override_mode = self
                .override_mode
                .map(|mode| jet_app_json_string(mode.as_str()))
                .unwrap_or_else(|| "null".to_string());
            let pending_boundary_id = self
                .pending_boundary_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "null".to_string());
            let pending = self
                .pending_boundary
                .as_deref()
                .map(jet_app_json_string)
                .unwrap_or_else(|| "null".to_string());
            let not_found = self
                .not_found_boundary
                .as_deref()
                .map(jet_app_json_string)
                .unwrap_or_else(|| "null".to_string());
            let error = self
                .error_boundary
                .as_deref()
                .map(jet_app_json_string)
                .unwrap_or_else(|| "null".to_string());
            let island = self
                .island_identity
                .as_deref()
                .map(|identity| {
                    let trigger = self
                        .hydration_trigger
                        .unwrap_or(JetAppHydrationTrigger::Immediate);
                    format!(
                        "{{\"identity\":{},\"hydration_trigger\":{},\"resume_payload\":{{\"captures\":[],\"serializable\":false}}}}",
                        jet_app_json_string(identity),
                        jet_app_json_string(trigger.as_str()),
                    )
                })
                .unwrap_or_else(|| "null".to_string());
            format!(
                "{{\"route_identity\":{},\"path_params\":[{}],\"search_params\":[{}],\"search_codec\":{},\"loader_contract\":{},\"pending_boundary\":{},\"not_found_boundary\":{},\"error_boundary\":{},\"mode\":{},\"reason\":[{}],\"effects\":[{}],\"interactive\":{},\"loader\":{},\"form\":{},\"cache\":{},\"override_mode\":{},\"pending_boundary_id\":{},\"island\":{}}}",
                jet_app_json_string(&self.route_identity),
                path_params,
                search_params,
                jet_app_json_string(if self.search_codec.is_empty() { "query" } else { &self.search_codec }),
                loader_contract,
                pending,
                not_found,
                error,
                jet_app_json_string(self.mode.as_str()),
                reason,
                effects,
                self.interactive,
                loader,
                form,
                jet_app_json_string(if self.cache.is_empty() { "unspecified" } else { &self.cache }),
                override_mode,
                pending_boundary_id,
                island,
            )
        }
    }

    fn jet_app_route_field_json(field: &JetAppRouteField) -> String {
        let default = field
            .default
            .as_deref()
            .map(jet_app_json_string)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"name\":{},\"ty\":{},\"codec\":{},\"required\":{},\"default\":{}}}",
            jet_app_json_string(&field.name),
            jet_app_json_string(&field.ty),
            jet_app_json_string(&field.codec),
            field.required,
            default,
        )
    }

    fn jet_app_loader_facts_json(loader: &JetAppLoaderFacts) -> String {
        format!(
            "{{\"handler\":{},\"data_type\":{},\"dependency\":{},\"preload\":{},\"cache_identity\":{}}}",
            jet_app_json_string(&loader.handler),
            jet_app_json_string(&loader.data_type),
            jet_app_json_string(&loader.dependency),
            loader.preload,
            jet_app_json_string(&loader.cache_identity),
        )
    }

    fn jet_app_route_identity(path: &str) -> String {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in path.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("route-{hash:016x}")
    }

    fn jet_app_json_string(value: &str) -> String {
        let mut out = String::with_capacity(value.len() + 2);
        out.push('"');
        for character in value.chars() {
            match character {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                character if (character as u32) < 0x20 => {
                    out.push_str(&format!("\\u{:04x}", character as u32));
                }
                character => out.push(character),
            }
        }
        out.push('"');
        out
    }

    fn jet_app_html_attr(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        for character in value.chars() {
            match character {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                character => out.push(character),
            }
        }
        out
    }

    // ── Typed route inputs ──────────────────────────────────────────────
    //
    // Sema binds every handler parameter to one route input: a path
    // parameter by name, the one search record (or a scalar search field),
    // or the route's loader data.  The compiler serializes that binding next
    // to the handler so the runtime decodes each position from the checked
    // navigation, never from a positional guess.

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct JetAppSearchField {
        pub name: String,
        pub ty: String,
        pub required: bool,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum JetAppRouteBinding {
        /// `p:<name>=<Type>` — one dynamic path segment.
        Path { name: String, ty: String },
        /// `s=<codec>:<name>=<Type>,…` — the search record (`json`) or one
        /// scalar search field (`query`).
        Search {
            codec: String,
            fields: Vec<JetAppSearchField>,
            scalar: Option<String>,
        },
        /// `d` — the route's loader data.
        Data,
    }

    impl JetAppRouteBinding {
        fn label(&self) -> String {
            match self {
                Self::Path { name, .. } => format!("path parameter `{name}`"),
                Self::Search { scalar: Some(name), .. } => format!("search field `{name}`"),
                Self::Search { .. } => "search record".to_string(),
                Self::Data => "loader data".to_string(),
            }
        }
    }

    fn jet_app_router_value_type(ty: &str) -> super::JetWebRouterValueType {
        match ty {
            "Int" => super::JetWebRouterValueType::Int,
            "Bool" => super::JetWebRouterValueType::Bool,
            "Float" => super::JetWebRouterValueType::Float,
            "String" | "Text" => super::JetWebRouterValueType::String,
            _ => super::JetWebRouterValueType::Json,
        }
    }

    fn jet_app_search_field(item: &str) -> Result<JetAppSearchField, String> {
        let (name, ty) = item
            .split_once('=')
            .ok_or_else(|| format!("invalid search field binding `{item}`"))?;
        let (ty, required) = match ty.strip_suffix('?') {
            Some(ty) => (ty, false),
            None => (ty, true),
        };
        if name.is_empty() || ty.is_empty() {
            return Err(format!("invalid search field binding `{item}`"));
        }
        Ok(JetAppSearchField {
            name: name.to_string(),
            ty: ty.to_string(),
            required,
        })
    }

    /// Parse the compiler-serialized binding.  The grammar is owned by the
    /// TIR App lowering; a mismatch is a compiler invariant failure.
    pub fn jet_app_route_binding(spec: &str) -> Vec<JetAppRouteBinding> {
        spec.split(';')
            .filter(|item| !item.is_empty())
            .map(|item| -> Result<JetAppRouteBinding, String> {
                if item == "d" {
                    return Ok(JetAppRouteBinding::Data);
                }
                if let Some(rest) = item.strip_prefix("p:") {
                    let (name, ty) = rest
                        .split_once('=')
                        .ok_or_else(|| format!("invalid path binding `{item}`"))?;
                    if name.is_empty() || ty.is_empty() {
                        return Err(format!("invalid path binding `{item}`"));
                    }
                    return Ok(JetAppRouteBinding::Path {
                        name: name.to_string(),
                        ty: ty.to_string(),
                    });
                }
                if let Some(rest) = item.strip_prefix("s=") {
                    let (codec, fields) = rest
                        .split_once(':')
                        .ok_or_else(|| format!("invalid search binding `{item}`"))?;
                    let fields = fields
                        .split(',')
                        .filter(|field| !field.is_empty())
                        .map(jet_app_search_field)
                        .collect::<Result<Vec<_>, _>>()?;
                    let scalar = match codec {
                        "json" => None,
                        "query" if fields.len() == 1 => Some(fields[0].name.clone()),
                        _ => return Err(format!("invalid search binding `{item}`")),
                    };
                    return Ok(JetAppRouteBinding::Search {
                        codec: codec.to_string(),
                        fields,
                        scalar,
                    });
                }
                Err(format!("invalid route input binding `{item}`"))
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|error| jet_app_invalid(format!("invalid app route binding: {error}")))
    }

    /// One scalar search field as its wire tree.  A field the binding marks
    /// optional (`T?`) is `Null` when the URL omits it, so the typed decoder
    /// yields the absent value; a required field must be present.
    fn jet_app_scalar_search_tree(
        navigation: &super::JetWebNavigation,
        name: &str,
        fields: &[JetAppSearchField],
    ) -> Result<jet_std::DataTree, String> {
        match navigation.search.get(name) {
            Some(value) => Ok(jet_std::DataTree::Text(value.clone())),
            None if fields.iter().any(|field| field.name == name && !field.required) => {
                Ok(jet_std::DataTree::Null)
            }
            None => Err(format!(
                "route `{}` has no search field `{name}`",
                navigation.route
            )),
        }
    }

    /// The decoded navigation one handler invocation reads from.
    pub struct JetAppRouteInputs<'a> {
        binding: &'a [JetAppRouteBinding],
        navigation: &'a super::JetWebNavigation,
        data: &'a str,
    }

    impl JetAppRouteInputs<'_> {
        /// The checked wire value for one bound input, before typed decoding.
        /// Resident tiers (JIT/interpreter) marshal this tree into their own
        /// runtime values with the same typed decoder they use for JSON.
        pub fn tree(&self, index: usize) -> Result<jet_std::DataTree, String> {
            let route = &self.navigation.route;
            let Some(binding) = self.binding.get(index) else {
                return Err(format!("route `{route}` input {index} has no checked binding"));
            };
            Ok(match binding {
                JetAppRouteBinding::Path { name, .. } => jet_std::DataTree::Text(
                    self.navigation
                        .params
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("route `{route}` has no path parameter `{name}`"))?,
                ),
                JetAppRouteBinding::Search {
                    scalar: Some(name),
                    fields,
                    ..
                } => jet_app_scalar_search_tree(&self.navigation, name, fields)?,
                JetAppRouteBinding::Search { .. } => jet_std::DataTree::Object(
                    self.navigation
                        .search
                        .iter()
                        .map(|(name, value)| (name.clone(), jet_std::DataTree::Text(value.clone())))
                        .collect(),
                ),
                // Loader data is JSON text; the tier's JSON decoder owns it.
                JetAppRouteBinding::Data => jet_std::DataTree::TypedText(self.data.to_string()),
            })
        }

        pub fn arity(&self) -> usize {
            self.binding.len()
        }

        pub fn decode<T: super::__jet_Decode>(&self, index: usize) -> Result<T, String> {
            let route = &self.navigation.route;
            let Some(binding) = self.binding.get(index) else {
                return Err(format!("route `{route}` input {index} has no checked binding"));
            };
            let tree = match binding {
                JetAppRouteBinding::Path { name, .. } => jet_std::DataTree::Text(
                    self.navigation
                        .params
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("route `{route}` has no path parameter `{name}`"))?,
                ),
                JetAppRouteBinding::Search {
                    scalar: Some(name),
                    fields,
                    ..
                } => jet_app_scalar_search_tree(&self.navigation, name, fields)?,
                JetAppRouteBinding::Search { .. } => jet_std::DataTree::Object(
                    self.navigation
                        .search
                        .iter()
                        .map(|(name, value)| (name.clone(), jet_std::DataTree::Text(value.clone())))
                        .collect(),
                ),
                JetAppRouteBinding::Data => {
                    // Loader data crosses the one JSON wire; the shared
                    // decoder owns parse-error rendering on every tier.
                    return super::jet_enc_json_decode::<T>(&self.data.to_string()).map_err(
                        |errors| format!("route `{route}` loader data: {errors:?}"),
                    );
                }
            };
            T::jet_decode(&tree)
                .map_err(|errors| format!("route `{route}` {}: {errors:?}", binding.label()))
        }
    }

    /// A checked route callback (page handler, boundary, or loader).  The
    /// compiler hands every handler over as the send-safe borrow callback, so
    /// one impl per arity covers every scalar/record parameter shape.
    pub trait JetAppRouteCallback<R>: Send + Sync + 'static {
        fn arity(&self) -> usize;
        fn call_route(&self, inputs: &JetAppRouteInputs<'_>) -> Result<R, String>;
    }

    /// The one bridge for resident tiers whose user values are runtime
    /// handles: the App supplies each bound input as its checked wire tree
    /// (`JetAppRouteInputs::tree`) and the tier marshals it with its own
    /// typed decoder before invoking the user callable.
    pub struct JetAppTreeRouteCallback<R> {
        arity: usize,
        call: Arc<dyn Fn(&[jet_std::DataTree]) -> Result<R, String> + Send + Sync + 'static>,
    }

    impl<R> JetAppTreeRouteCallback<R> {
        pub fn new(
            arity: usize,
            call: Arc<dyn Fn(&[jet_std::DataTree]) -> Result<R, String> + Send + Sync + 'static>,
        ) -> Self {
            Self { arity, call }
        }
    }

    impl<R: 'static> JetAppRouteCallback<R> for JetAppTreeRouteCallback<R> {
        fn arity(&self) -> usize {
            self.arity
        }
        fn call_route(&self, inputs: &JetAppRouteInputs<'_>) -> Result<R, String> {
            let trees = (0..self.arity)
                .map(|index| inputs.tree(index))
                .collect::<Result<Vec<_>, _>>()?;
            (self.call)(&trees)
        }
    }

    impl<R: 'static> JetAppRouteCallback<R> for Arc<dyn Fn() -> R + Send + Sync + 'static> {
        fn arity(&self) -> usize {
            0
        }
        fn call_route(&self, _inputs: &JetAppRouteInputs<'_>) -> Result<R, String> {
            Ok(self())
        }
    }

    impl<A, R> JetAppRouteCallback<R> for Arc<dyn Fn(&A) -> R + Send + Sync + 'static>
    where
        A: super::__jet_Decode + 'static,
        R: 'static,
    {
        fn arity(&self) -> usize {
            1
        }
        fn call_route(&self, inputs: &JetAppRouteInputs<'_>) -> Result<R, String> {
            let a = inputs.decode::<A>(0)?;
            Ok(self(&a))
        }
    }

    impl<A, B, R> JetAppRouteCallback<R> for Arc<dyn Fn(&A, &B) -> R + Send + Sync + 'static>
    where
        A: super::__jet_Decode + 'static,
        B: super::__jet_Decode + 'static,
        R: 'static,
    {
        fn arity(&self) -> usize {
            2
        }
        fn call_route(&self, inputs: &JetAppRouteInputs<'_>) -> Result<R, String> {
            let a = inputs.decode::<A>(0)?;
            let b = inputs.decode::<B>(1)?;
            Ok(self(&a, &b))
        }
    }

    impl<A, B, C, R> JetAppRouteCallback<R>
        for Arc<dyn Fn(&A, &B, &C) -> R + Send + Sync + 'static>
    where
        A: super::__jet_Decode + 'static,
        B: super::__jet_Decode + 'static,
        C: super::__jet_Decode + 'static,
        R: 'static,
    {
        fn arity(&self) -> usize {
            3
        }
        fn call_route(&self, inputs: &JetAppRouteInputs<'_>) -> Result<R, String> {
            let a = inputs.decode::<A>(0)?;
            let b = inputs.decode::<B>(1)?;
            let c = inputs.decode::<C>(2)?;
            Ok(self(&a, &b, &c))
        }
    }

    /// A page handler result: the page itself or its declared failure.
    pub trait JetAppPageOutput {
        fn into_page(self) -> Result<JetWebPage, String>;
    }

    impl JetAppPageOutput for JetWebPage {
        fn into_page(self) -> Result<JetWebPage, String> {
            Ok(self)
        }
    }

    impl<E: JetAppFailure> JetAppPageOutput for Result<JetWebPage, E> {
        fn into_page(self) -> Result<JetWebPage, String> {
            self.map_err(JetAppFailure::into_app_failure)
        }
    }

    fn jet_app_page_handler<F, R>(handler: F, binding: Vec<JetAppRouteBinding>) -> PageHandler
    where
        F: JetAppRouteCallback<R>,
        R: JetAppPageOutput,
    {
        if binding.len() != handler.arity() {
            jet_app_invalid(format!(
                "app route handler `{}` takes {} inputs but the checked binding has {}",
                type_name::<F>(),
                handler.arity(),
                binding.len()
            ));
        }
        Arc::new(move |navigation, data| {
            let inputs = JetAppRouteInputs {
                binding: &binding,
                navigation,
                data,
            };
            handler.call_route(&inputs)?.into_page()
        })
    }

    fn jet_app_boundary_handler<F, R>(handler: F) -> BoundaryHandler
    where
        F: JetAppRouteCallback<R>,
        R: JetAppPageOutput,
    {
        if handler.arity() != 0 {
            jet_app_invalid(format!(
                "app boundary handler `{}` must take no inputs",
                type_name::<F>()
            ));
        }
        let navigation = super::JetWebNavigation {
            url: String::new(),
            route: String::new(),
            params: Default::default(),
            search: Default::default(),
        };
        Arc::new(move || {
            let inputs = JetAppRouteInputs {
                binding: &[],
                navigation: &navigation,
                data: "",
            };
            handler.call_route(&inputs)?.into_page()
        })
    }

    fn jet_app_loader_binding_check(identity: &str, arity: usize, binding: &[JetAppRouteBinding]) {
        if binding.len() != arity {
            jet_app_invalid(format!(
                "app loader `{identity}` takes {arity} inputs but the checked binding has {}",
                binding.len()
            ));
        }
        if binding.iter().any(|binding| matches!(binding, JetAppRouteBinding::Data)) {
            jet_app_invalid(format!("app loader `{identity}` cannot consume its own route data"));
        }
    }

    fn jet_app_loader_handler<L, V, E>(
        loader: L,
        binding: Vec<JetAppRouteBinding>,
    ) -> LoaderHandler
    where
        L: JetAppRouteCallback<Result<V, E>>,
        V: super::__jet_Encode + 'static,
        E: JetAppFailure + 'static,
    {
        jet_app_loader_binding_check(type_name::<L>(), loader.arity(), &binding);
        Arc::new(move |navigation| {
            let inputs = JetAppRouteInputs {
                binding: &binding,
                navigation,
                data: "",
            };
            match loader.call_route(&inputs)? {
                Ok(value) => Ok(super::jet_enc_json_to_string(&value)),
                Err(error) => Err(error.into_app_failure()),
            }
        })
    }

    /// Resident tiers encode the loader value with their own typed encoder;
    /// the App stores that JSON text unchanged as the route data.
    fn jet_app_encoded_loader_handler<L>(
        loader: L,
        binding: Vec<JetAppRouteBinding>,
    ) -> LoaderHandler
    where
        L: JetAppRouteCallback<Result<String, String>>,
    {
        jet_app_loader_binding_check(type_name::<L>(), loader.arity(), &binding);
        Arc::new(move |navigation| {
            let inputs = JetAppRouteInputs {
                binding: &binding,
                navigation,
                data: "",
            };
            loader.call_route(&inputs)?
        })
    }

    fn jet_app_binding_facts(
        binding: &[JetAppRouteBinding],
    ) -> (Vec<JetAppRouteField>, Vec<JetAppRouteField>, String) {
        let mut path_params = Vec::new();
        let mut search_params = Vec::new();
        let mut search_codec = "query".to_string();
        for entry in binding {
            match entry {
                JetAppRouteBinding::Path { name, ty } => path_params.push(JetAppRouteField {
                    name: name.clone(),
                    ty: ty.clone(),
                    codec: "path".to_string(),
                    required: true,
                    default: None,
                }),
                JetAppRouteBinding::Search { codec, fields, .. } => {
                    search_codec = codec.clone();
                    search_params.extend(fields.iter().map(|field| JetAppRouteField {
                        name: field.name.clone(),
                        ty: field.ty.clone(),
                        codec: codec.clone(),
                        required: field.required,
                        default: None,
                    }));
                }
                JetAppRouteBinding::Data => {}
            }
        }
        (path_params, search_params, search_codec)
    }

    #[derive(Clone)]
    struct JetAppRoute {
        path: String,
        handler: PageHandler,
        handler_identity: String,
        binding: Vec<JetAppRouteBinding>,
        facts: JetAppRouteFacts,
        loader: Option<LoaderHandler>,
        pending: Option<BoundaryHandler>,
        error: Option<BoundaryHandler>,
    }

    #[derive(Default)]
    struct JetAppState {
        routes: Vec<JetAppRoute>,
        actions: Vec<(String, ActionHandler, String, bool)>,
        mounts: Vec<(String, MountHandler, Vec<String>, Vec<String>)>,
        routes_from: Vec<String>,
        not_found: Option<BoundaryHandler>,
        security: Vec<String>,
        assets: Vec<String>,
        split: Vec<String>,
        cache: Vec<String>,
        a11y: Vec<String>,
        adapters: Vec<String>,
        hydration: String,
        /// The one navigation kernel built from the routes above.  Any later
        /// registration rebuilds it, so the router never diverges from the
        /// graph.
        router: Option<super::JetWebRouter>,
    }

    #[derive(Clone)]
    pub struct JetApp {
        state: Arc<Mutex<JetAppState>>,
        route_facts: Arc<Mutex<JetAppRouteFacts>>,
    }

    pub fn jet_app() -> JetApp {
        JetApp {
            state: Arc::new(Mutex::new(JetAppState {
                hydration: "dev-overlay".to_string(),
                ..JetAppState::default()
            })),
            route_facts: Arc::new(Mutex::new(JetAppRouteFacts::default())),
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

    /// One served page: the HTTP status and the page after the route's
    /// navigation, loader, and boundaries ran.
    #[derive(Clone)]
    pub struct JetAppResponse {
        pub status: i64,
        pub page: JetWebPage,
        pub navigation: super::JetWebNavigationState,
    }

    impl JetApp {
        fn lock(&self) -> std::sync::MutexGuard<'_, JetAppState> {
            self.state.lock().unwrap_or_else(|error| error.into_inner())
        }

        fn register_route<F, R>(
            &self,
            path: String,
            handler: F,
            binding: Vec<JetAppRouteBinding>,
        ) -> JetApp
        where
            F: JetAppRouteCallback<R>,
            R: JetAppPageOutput,
        {
            let handler_identity = type_name::<F>().to_string();
            let handler = jet_app_page_handler(handler, binding.clone());
            let mut facts = self
                .route_facts
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            if facts.route_identity.is_empty() {
                facts.route_identity =
                    jet_app_route_identity(&format!("{path}\0{handler_identity}"));
            }
            let (path_params, search_params, search_codec) = jet_app_binding_facts(&binding);
            facts.path_params = path_params;
            facts.search_params = search_params;
            facts.search_codec = search_codec;
            let mut state = self.lock();
            state.routes.push(JetAppRoute {
                path,
                handler,
                handler_identity,
                binding,
                facts,
                loader: None,
                pending: None,
                error: None,
            });
            state.router = None;
            drop(state);
            self.clone()
        }

        pub fn route<F, R>(
            &self,
            path: String,
            handler: F,
            binding: Vec<JetAppRouteBinding>,
        ) -> JetApp
        where
            F: JetAppRouteCallback<R>,
            R: JetAppPageOutput,
        {
            self.register_route(path, handler, binding)
        }

        pub fn page<F, R>(
            &self,
            path: String,
            handler: F,
            binding: Vec<JetAppRouteBinding>,
        ) -> JetApp
        where
            F: JetAppRouteCallback<R>,
            R: JetAppPageOutput,
        {
            self.register_route(path, handler, binding)
        }

        pub fn layout<F, R>(
            &self,
            path: String,
            handler: F,
            binding: Vec<JetAppRouteBinding>,
        ) -> JetApp
        where
            F: JetAppRouteCallback<R>,
            R: JetAppPageOutput,
        {
            self.register_route(path, handler, binding)
        }

        /// Attach a boundary to the route registered immediately before it.
        /// Sema diagnoses a boundary with no preceding route (E2810); the
        /// runtime graph keeps the same attachment rule.
        fn attach_boundary(
            &self,
            handler: BoundaryHandler,
            identity: String,
            attach: impl FnOnce(&mut JetAppRoute, BoundaryHandler, String),
        ) -> JetApp {
            let mut state = self.lock();
            let Some(route) = state.routes.last_mut() else {
                jet_app_invalid(format!(
                    "app boundary `{identity}` has no preceding route"
                ));
            };
            attach(route, handler, identity);
            state.router = None;
            drop(state);
            self.clone()
        }

        pub fn pending<F, R>(&self, handler: F) -> JetApp
        where
            F: JetAppRouteCallback<R>,
            R: JetAppPageOutput,
        {
            let identity = type_name::<F>().to_string();
            self.attach_boundary(
                jet_app_boundary_handler(handler),
                identity,
                |route, handler, identity| {
                    route.pending = Some(handler);
                    route.facts.pending_boundary = Some(identity);
                },
            )
        }

        pub fn error<F, R>(&self, handler: F) -> JetApp
        where
            F: JetAppRouteCallback<R>,
            R: JetAppPageOutput,
        {
            let identity = type_name::<F>().to_string();
            self.attach_boundary(
                jet_app_boundary_handler(handler),
                identity,
                |route, handler, identity| {
                    route.error = Some(handler);
                    route.facts.error_boundary = Some(identity);
                },
            )
        }

        /// The not-found boundary answers every unmatched navigation, so it is
        /// one application-wide projection; the route it follows records the
        /// boundary identity in its facts.

        pub fn not_found<F, R>(&self, handler: F) -> JetApp
        where
            F: JetAppRouteCallback<R>,
            R: JetAppPageOutput,
        {
            let mut state = self.lock();
            state.not_found = Some(jet_app_boundary_handler(handler));
            drop(state);
            self.clone()
        }

        fn register_action<F>(&self, name: String, handler: F, kind: &str) -> JetApp
        where
            F: JetAppActionHandler,
        {
            let function = handler.into_server_function(name.clone());
            let mut state = self.lock();
            if let Some(route) = state.routes.last_mut() {
                if matches!(kind, "form" | "action") && route.facts.form.is_none() {
                    route.facts.form = Some(name.clone());
                }
            }
            state
                .actions
                .push((name, Some(function), kind.to_string(), false));
            drop(state);
            self.clone()
        }

        pub fn action<F>(&self, name: String, handler: F) -> JetApp
        where
            F: JetAppActionHandler,
        {
            self.register_action(name, handler, "action")
        }

        pub fn form<F>(&self, name: String, handler: F) -> JetApp
        where
            F: JetAppActionHandler,
        {
            self.register_action(name, handler, "form")
        }

        pub fn data<F>(&self, name: String, handler: F) -> JetApp
        where
            F: JetAppActionHandler,
        {
            self.register_action(name, handler, "data")
        }

        /// Attach a loader to the route registered immediately before it.
        /// Sema checks that the named route handler is that route's handler
        /// and that the loader inputs are the route's typed inputs; the
        /// runtime graph keeps the same attachment rule.
        pub fn loader<L, V, E>(
            &self,
            loader: L,
            binding: Vec<JetAppRouteBinding>,
            preload: bool,
        ) -> JetApp
        where
            L: JetAppRouteCallback<Result<V, E>>,
            V: super::__jet_Encode + 'static,
            E: JetAppFailure + 'static,
        {
            let loader_identity = type_name::<L>().to_string();
            let data_type = type_name::<V>().to_string();
            let loader = jet_app_loader_handler(loader, binding);
            self.attach_loader(loader, loader_identity, data_type, preload)
        }

        /// Attach a loader whose callback already produced the encoded JSON
        /// route data (resident JIT/interpreter tiers).
        pub fn loader_encoded<L>(
            &self,
            loader: L,
            binding: Vec<JetAppRouteBinding>,
            data_type: String,
            preload: bool,
        ) -> JetApp
        where
            L: JetAppRouteCallback<Result<String, String>>,
        {
            let loader_identity = type_name::<L>().to_string();
            let loader = jet_app_encoded_loader_handler(loader, binding);
            self.attach_loader(loader, loader_identity, data_type, preload)
        }

        fn attach_loader(
            &self,
            loader: LoaderHandler,
            loader_identity: String,
            data_type: String,
            preload: bool,
        ) -> JetApp {
            let mut state = self.lock();
            let Some(route) = state.routes.last_mut() else {
                jet_app_invalid(format!("app loader `{loader_identity}` has no preceding route"));
            };
            let cache_identity = route.facts.route_identity.clone();
            let dependency = route.handler_identity.clone();
            route.facts.loader = Some(loader_identity.clone());
            route.facts.loader_contract = Some(JetAppLoaderFacts {
                handler: loader_identity,
                data_type,
                dependency: dependency.clone(),
                preload,
                cache_identity,
            });
            route.loader = Some(loader);
            state
                .actions
                .push((dependency, None, "loader".to_string(), preload));
            state.router = None;
            drop(state);
            self.clone()
        }

        fn register_mount<F, R>(
            &self,
            prefix: String,
            handler: F,
            effects: Vec<String>,
            security: Vec<String>,
        ) -> JetApp
        where
            F: Fn(&String) -> R + Send + Sync + 'static,
            R: 'static,
        {
            let handler: MountHandler = Arc::new(move |path| {
                let _ = handler(path);
            });
            self.lock().mounts.push((prefix, handler, effects, security));
            self.clone()
        }

        pub fn mount<F, R>(&self, prefix: String, handler: F) -> JetApp
        where
            F: Fn(&String) -> R + Send + Sync + 'static,
            R: 'static,
        {
            self.register_mount(prefix, handler, Vec::new(), Vec::new())
        }

        pub fn mount_with_effect<F, R>(
            &self,
            prefix: String,
            handler: F,
            effect: String,
        ) -> JetApp
        where
            F: Fn(&String) -> R + Send + Sync + 'static,
            R: 'static,
        {
            self.register_mount(prefix, handler, vec![effect], Vec::new())
        }

        pub fn mount_with_policy<F, R>(
            &self,
            prefix: String,
            handler: F,
            effect: String,
            policy: String,
        ) -> JetApp
        where
            F: Fn(&String) -> R + Send + Sync + 'static,
            R: 'static,
        {
            self.register_mount(prefix, handler, vec![effect], vec![policy])
        }

        pub fn routes(&self, root: String) -> JetApp {
            self.lock().routes_from.push(root);
            self.clone()
        }

        fn set_route_mode(&self, mode: JetAppRenderMode) -> JetApp {
            let mut facts = self
                .route_facts
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            facts.mode = mode;
            facts.reason = vec![format!("explicit render mode `{}`", mode.as_str())];
            facts.override_mode = Some(mode);
            self.clone()
        }

        pub fn csr(&self) -> JetApp {
            self.set_route_mode(JetAppRenderMode::Csr)
        }
        pub fn ssr(&self) -> JetApp {
            self.set_route_mode(JetAppRenderMode::Ssr)
        }
        pub fn ssg(&self) -> JetApp {
            self.set_route_mode(JetAppRenderMode::Ssg)
        }
        pub fn stream(&self) -> JetApp {
            self.set_route_mode(JetAppRenderMode::Stream)
        }
        pub fn streaming(&self) -> JetApp {
            self.stream()
        }
        pub fn island(&self) -> JetApp {
            self.set_route_mode(JetAppRenderMode::Island)
        }
        pub fn hydration_dev(&self) -> JetApp {
            self.lock().hydration = "dev-overlay".to_string();
            self.clone()
        }
        pub fn hydration_release(&self) -> JetApp {
            self.lock().hydration = "release-keep-server".to_string();
            self.clone()
        }
        pub fn security(&self, policy: String) -> JetApp {
            self.lock().security.push(policy);
            self.clone()
        }
        pub fn assets(&self, path: String) -> JetApp {
            self.lock().assets.push(path);
            self.clone()
        }
        pub fn split(&self, name: String) -> JetApp {
            self.lock().split.push(name);
            self.clone()
        }
        pub fn code_split(&self, name: String) -> JetApp {
            self.split(name)
        }
        pub fn cache(&self, policy: String) -> JetApp {
            self.lock().cache.push(policy);
            self.clone()
        }
        pub fn a11y(&self, policy: String) -> JetApp {
            self.lock().a11y.push(policy);
            self.clone()
        }
        pub fn adapter(&self, name: String) -> JetApp {
            self.lock().adapters.push(name);
            self.clone()
        }

        pub fn facts_json(&self) -> String {
            let s = self.lock();
            let mut out = String::from("{\n");
            out.push_str(&format!(
                "  \"hydration\": {},\n",
                jet_app_json_string(&s.hydration)
            ));
            out.push_str("  \"shared_tir\": true,\n");
            out.push_str("  \"routes\": [\n");
            for (i, route) in s.routes.iter().enumerate() {
                out.push_str(&format!(
                    "    {{\"path\": {}, \"handler\": {}, \"provenance\": \"runtime\", \"render_facts\": {}}}",
                    jet_app_json_string(&route.path),
                    jet_app_json_string(&route.handler_identity),
                    route.facts.json(),
                ));
                if i + 1 != s.routes.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("  ],\n");
            out.push_str("  \"actions\": [\n");
            for (i, (name, function, kind, preload)) in s.actions.iter().enumerate() {
                let server_facts = function
                    .as_ref()
                    .map(|_| {
                        let endpoint = format!("/actions/{name}");
                        let middleware = super::jet_app_middleware::APP_SERVER_FUNCTION_MIDDLEWARE
                            .iter()
                            .map(|kind| jet_app_json_string(kind.as_str()))
                            .collect::<Vec<_>>()
                            .join(",");
                        format!(
                            "{{\"name\":{},\"endpoint\":{},\"method\":\"POST\",\"csrf\":\"same-origin\",\"middleware\":[{}]}}",
                            jet_app_json_string(name),
                            jet_app_json_string(&endpoint),
                            middleware,
                        )
                    })
                    .unwrap_or_else(|| "null".to_string());
                out.push_str(&format!(
                    "    {{\"name\": {}, \"handler\": \"callable\", \"kind\": {}, \"preload\": {}, \"server_function\": {}}}",
                    jet_app_json_string(name),
                    jet_app_json_string(kind),
                    preload,
                    server_facts,
                ));
                if i + 1 != s.actions.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("  ],\n");
            out.push_str("  \"queries\": ");
            out.push_str(&super::jet_web_query_facts_json());
            out.push_str(",\n");
            out.push_str(&format!(
                "  \"security\": {:?},\n  \"assets\": {:?},\n  \"split\": {:?},\n  \"cache\": {:?},\n  \"a11y\": {:?},\n  \"adapters\": {:?}\n",
                s.security, s.assets, s.split, s.cache, s.a11y, s.adapters
            ));
            out.push('}');
            out
        }

        /// Build the canonical navigation kernel from the route graph.  Every
        /// route registers exactly once with its typed path/search fields,
        /// loader, and pending/error boundaries; the router owns matching,
        /// precedence, loader execution, caching, preload, and abort.
        fn build_router(state: &JetAppState) -> super::JetWebRouter {
            let mut router = super::JetWebRouter::new();
            for route in &state.routes {
                let mut params = Vec::new();
                let mut search = Vec::new();
                let mut codec = super::JetWebRouterSearchCodec::Query;
                for binding in &route.binding {
                    match binding {
                        JetAppRouteBinding::Path { name, ty } => params.push(
                            super::JetWebRouterField::new(
                                name.clone(),
                                jet_app_router_value_type(ty),
                            ),
                        ),
                        JetAppRouteBinding::Search {
                            codec: search_codec,
                            fields,
                            scalar,
                        } => {
                            if search_codec == "json" {
                                codec = super::JetWebRouterSearchCodec::Json;
                            }
                            // The router validates declared names and value
                            // shapes; the typed decoder owns required fields
                            // and defaults of a search record.
                            search.extend(fields.iter().map(|field| {
                                let router_field = super::JetWebRouterField::new(
                                    field.name.clone(),
                                    jet_app_router_value_type(&field.ty),
                                );
                                if scalar.is_some() && field.required {
                                    router_field
                                } else {
                                    router_field.optional(None)
                                }
                            }));
                        }
                        JetAppRouteBinding::Data => {}
                    }
                }
                let loader = route.loader.clone();
                let loader = move |navigation: &super::JetWebNavigation| match &loader {
                    Some(loader) => loader(navigation),
                    None => Ok(String::new()),
                };
                let pending = route.pending.clone().map(|pending| {
                    move |_navigation: &super::JetWebNavigation| -> String {
                        pending().map(|page| page.body).unwrap_or_else(|error| error)
                    }
                });
                let error = route.error.clone().map(|error| {
                    move |_navigation: &super::JetWebNavigation, message: &str| -> String {
                        error()
                            .map(|page| page.body)
                            .unwrap_or_else(|_| message.to_string())
                    }
                });
                router = router
                    .route_with_facts_codec(
                        route.path.clone(),
                        params,
                        search,
                        codec,
                        loader,
                        pending,
                        error,
                        super::JetWebRouteBoundaries::default(),
                    )
                    .unwrap_or_else(|error| {
                        jet_app_invalid(format!(
                            "app route `{}` is not a valid router route: {error}",
                            route.path
                        ))
                    });
            }
            if let Some(not_found) = state.not_found.clone() {
                router = router
                    .not_found(move |message: &str| {
                        not_found()
                            .map(|page| page.body)
                            .unwrap_or_else(|_| message.to_string())
                    })
                    .unwrap_or_else(|error| {
                        jet_app_invalid(format!("app not-found boundary is invalid: {error}"))
                    });
            }
            router
        }

        /// The application's navigation kernel.  Built once per graph state;
        /// preloaded and cached route data survive across navigations.
        pub fn router(&self) -> super::JetWebRouter {
            let mut state = self.lock();
            if state.router.is_none() {
                let router = Self::build_router(&state);
                state.router = Some(router);
            }
            state
                .router
                .clone()
                .expect("app router is built above")
        }

        /// Serve one navigation through the route graph: resolve the route,
        /// run its loader (or reuse the router cache), decode the typed
        /// inputs, and render the page or the matching boundary.
        pub fn respond(&self, url: &str) -> JetAppResponse {
            let router = self.router();
            let route = router.navigate(url.to_string());
            let navigation = router.current();
            let (status, page) = match route {
                Ok(current) => {
                    let handler = self
                        .lock()
                        .routes
                        .iter()
                        .find(|route| route.path == current.route)
                        .map(|route| (route.handler.clone(), route.error.clone()));
                    match handler {
                        Some((handler, error)) => match handler(&current, &navigation.data) {
                            Ok(page) => (200, page),
                            Err(message) => (
                                500,
                                Self::boundary_page(error.as_ref(), "Route error", message),
                            ),
                        },
                        None => (
                            500,
                            JetWebPage {
                                title: "Route error".to_string(),
                                body: format!(
                                    "route `{}` is not registered on the app graph",
                                    current.route
                                ),
                            },
                        ),
                    }
                }
                Err(message) => {
                    let error_boundary = navigation.current.as_ref().and_then(|current| {
                        self.lock()
                            .routes
                            .iter()
                            .find(|route| route.path == current.route)
                            .and_then(|route| route.error.clone())
                    });
                    let status = if navigation.current.is_some() {
                        500
                    } else if message.starts_with("no web route matches") {
                        404
                    } else {
                        400
                    };
                    let page = if status == 404 {
                        let not_found = self.lock().not_found.clone();
                        Self::boundary_page(not_found.as_ref(), "Not found", message)
                    } else {
                        Self::boundary_page(error_boundary.as_ref(), "Route error", message)
                    };
                    (status, page)
                }
            };
            JetAppResponse {
                status,
                page,
                navigation,
            }
        }

        fn boundary_page(
            boundary: Option<&BoundaryHandler>,
            title: &str,
            message: String,
        ) -> JetWebPage {
            match boundary.map(|boundary| boundary()) {
                Some(Ok(page)) => page,
                Some(Err(boundary_error)) => JetWebPage {
                    title: title.to_string(),
                    body: format!("{message}\n{boundary_error}"),
                },
                None => JetWebPage {
                    title: title.to_string(),
                    body: message,
                },
            }
        }
        /// Development-only server-function facts.  The projection is
        /// deliberately metadata-only: handler results, errors, auth values,
        /// and request payloads never enter the HTML document.
        fn server_function_dev_panel(&self) -> String {
            let state = self.lock();
            let mut rows = Vec::new();
            for (name, function, kind, _) in &state.actions {
                if function.is_none() {
                    continue;
                }
                let endpoint = format!("/actions/{name}");
                let middleware = super::jet_app_middleware::APP_SERVER_FUNCTION_MIDDLEWARE
                    .iter()
                    .map(|kind| jet_app_json_string(kind.as_str()))
                    .collect::<Vec<_>>()
                    .join(",");
                let facts = format!(
                    "{{\"name\":{},\"endpoint\":{},\"method\":\"POST\",\"csrf\":\"same-origin\",\"middleware\":[{}]}}",
                    jet_app_json_string(name),
                    jet_app_json_string(&endpoint),
                    middleware,
                )
                .replace('<', "\\u003c");
                rows.push(format!(
                    "<li data-jet-server-function=\"{}\" data-jet-kind=\"{}\"><code>{}</code><pre>{}</pre></li>",
                    jet_app_html_attr(name),
                    jet_app_html_attr(kind),
                    jet_app_html_attr(name),
                    facts,
                ));
            }
            if rows.is_empty() {
                return String::new();
            }
            format!(
                "<aside id=\"jet-dev-server-functions\" data-jet-devtools=\"server-functions\"><h2>Server functions</h2><ul>{}</ul></aside>",
                rows.join("")
            )
        }

        /// Development-only query lifecycle facts. The query prelude owns the
        /// registry so this panel can expose state without exposing payloads.
        fn query_dev_panel(&self) -> String {
            super::jet_web_query_dev_panel()
        }

        /// Development-only form values and validation lifecycle. The form
        /// runtime owns this projection; release responses never call it.
        fn form_dev_panel(&self) -> String {
            super::jet_web_form_dev_panel()
        }


        fn html(&self, response: &JetAppResponse, dev: bool) -> String {
            let reload = if dev {
                r#"<script>
const source = new EventSource("/__jet/reload");
source.onmessage = () => location.reload();
</script>"#
            } else {
                ""
            };
            // Every successful route publishes its identity and navigation
            // status; the body is the encoded loader data, or `null` when the
            // route declares no loader, so the browser adapter resumes the
            // same navigation state either way.
            let route_data = match &response.navigation.current {
                Some(current) if response.status == 200 => {
                    let route_data_body = if response.navigation.data.is_empty() {
                        "null".to_string()
                    } else {
                        response.navigation.data.replace('<', "\\u003c")
                    };
                    format!(
                        "<script type=\"application/json\" id=\"jet-route-data\" data-route=\"{}\" data-url=\"{}\" data-status=\"{}\">{}</script>",
                        jet_app_html_attr(&current.route),
                        jet_app_html_attr(&current.url),
                        response.navigation.status.name(),
                        route_data_body,
                    )
                }
                _ => String::new(),
            };
            let server_functions = if dev {
                self.server_function_dev_panel()
            } else {
                String::new()
            };
            let queries = if dev {
                self.query_dev_panel()
            } else {
                String::new()
            };
            let forms = if dev {
                self.form_dev_panel()
            } else {
                String::new()
            };
            format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title></head><body>{}{}{}{}{}{}</body></html>",
                response.page.title,
                response.page.body,
                route_data,
                reload,
                server_functions,
                queries,
                forms,
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
            let dev = std::env::var_os("JET_APP_DEV").is_some();
            let port = u16::try_from(port)
                .unwrap_or_else(|_| jet_app_invalid(format!("web app port {port} is out of range")));
            self.serve_port(port, dev);
        }

        fn http_mux(&self, dev: bool) -> super::JetHTTPMux {
            let mux = super::jet_app_http_mux_new();
            let state = self.lock();
            for route in &state.routes {
                let app = self.clone();
                super::jet_app_http_page_response(&mux, &route.path, move |request| {
                    let response = app.respond(&super::jet_app_http_request_url(request));
                    (response.status, app.html(&response, dev))
                });
            }
            for (_, function, _, _) in &state.actions {
                if let Some(function) = function {
                    super::jet_web_server_fn_register_http(&mux, function.clone());
                }
            }
            for (prefix, handler, _, _) in &state.mounts {
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
            mux
        }

        pub(crate) fn console_http_mux(&self) -> super::JetHTTPMux {
            self.http_mux(false)
        }

        fn serve_port(&self, port: u16, dev: bool) {
            super::jet_app_http_serve(self.http_mux(dev), port, dev);
        }
    }

    // Native resident runs have no browser storage. Keep the same explicit
    // no-op contract as the AOT core.web.storage surface.
    pub fn jet_web_storage_get(_key: &String) -> Option<String> {
        None
    }

    pub fn jet_web_storage_remove(_key: &String) {}

    pub fn jet_web_storage_set(_key: &String, _value: &String) {}

    pub fn jet_web_storage_clear() {}

    pub fn jet_app_route<F, R>(app: &JetApp, path: String, handler: F, binding: String) -> JetApp
    where
        F: JetAppRouteCallback<R>,
        R: JetAppPageOutput,
    {
        app.route(path, handler, jet_app_route_binding(&binding))
    }

    pub fn jet_app_page<F, R>(app: &JetApp, path: String, handler: F, binding: String) -> JetApp
    where
        F: JetAppRouteCallback<R>,
        R: JetAppPageOutput,
    {
        app.page(path, handler, jet_app_route_binding(&binding))
    }

    pub fn jet_app_layout<F, R>(app: &JetApp, path: String, handler: F, binding: String) -> JetApp
    where
        F: JetAppRouteCallback<R>,
        R: JetAppPageOutput,
    {
        app.layout(path, handler, jet_app_route_binding(&binding))
    }

    /// `.loader(route, loader)`: the route handler value only names the
    /// route in source; sema proves it is the handler of the route registered
    /// immediately before, which is where the loader attaches.
    pub fn jet_app_loader<T, L, V, E>(
        app: &JetApp,
        _route: T,
        loader: L,
        binding: String,
    ) -> JetApp
    where
        T: 'static,
        L: JetAppRouteCallback<Result<V, E>>,
        V: super::__jet_Encode + 'static,
        E: JetAppFailure + 'static,
    {
        app.loader(loader, jet_app_route_binding(&binding), false)
    }

    pub fn jet_app_loader_preload<T, L, V, E>(
        app: &JetApp,
        _route: T,
        loader: L,
        preload: bool,
        binding: String,
    ) -> JetApp
    where
        T: 'static,
        L: JetAppRouteCallback<Result<V, E>>,
        V: super::__jet_Encode + 'static,
        E: JetAppFailure + 'static,
    {
        app.loader(loader, jet_app_route_binding(&binding), preload)
    }

    pub fn jet_app_pending<F, R>(app: &JetApp, handler: F) -> JetApp
    where
        F: JetAppRouteCallback<R>,
        R: JetAppPageOutput,
    {
        app.pending(handler)
    }

    pub fn jet_app_not_found<F, R>(app: &JetApp, handler: F) -> JetApp
    where
        F: JetAppRouteCallback<R>,
        R: JetAppPageOutput,
    {
        app.not_found(handler)
    }

    pub fn jet_app_error<F, R>(app: &JetApp, handler: F) -> JetApp
    where
        F: JetAppRouteCallback<R>,
        R: JetAppPageOutput,
    {
        app.error(handler)
    }

    pub fn jet_app_action<F>(app: &JetApp, name: String, handler: F) -> JetApp
    where
        F: JetAppActionHandler,
    {
        app.action(name, handler)
    }

    pub fn jet_app_form<F>(app: &JetApp, name: String, handler: F) -> JetApp
    where
        F: JetAppActionHandler,
    {
        app.form(name, handler)
    }

    pub fn jet_app_data<F>(app: &JetApp, name: String, handler: F) -> JetApp
    where
        F: JetAppActionHandler,
    {
        app.data(name, handler)
    }

    pub fn jet_app_mount<F, R>(app: &JetApp, prefix: String, handler: F) -> JetApp
    where
        F: Fn(&String) -> R + Send + Sync + 'static,
        R: 'static,
    {
        app.mount(prefix, handler)
    }

    pub fn jet_app_mount_with_effect<F, R>(
        app: &JetApp,
        prefix: String,
        handler: F,
        effect: String,
    ) -> JetApp
    where
        F: Fn(&String) -> R + Send + Sync + 'static,
        R: 'static,
    {
        app.mount_with_effect(prefix, handler, effect)
    }

    pub fn jet_app_mount_with_policy<F, R>(
        app: &JetApp,
        prefix: String,
        handler: F,
        effect: String,
        policy: String,
    ) -> JetApp
    where
        F: Fn(&String) -> R + Send + Sync + 'static,
        R: 'static,
    {
        app.mount_with_policy(prefix, handler, effect, policy)
    }

    pub fn jet_app_routes(app: &JetApp, root: String) -> JetApp {
        app.routes(root)
    }

    pub fn jet_app_security(app: &JetApp, policy: String) -> JetApp {
        app.security(policy)
    }

    pub fn jet_app_assets(app: &JetApp, path: String) -> JetApp {
        app.assets(path)
    }

    pub fn jet_app_split(app: &JetApp, name: String) -> JetApp {
        app.split(name)
    }

    pub fn jet_app_code_split(app: &JetApp, name: String) -> JetApp {
        app.code_split(name)
    }

    pub fn jet_app_cache(app: &JetApp, policy: String) -> JetApp {
        app.cache(policy)
    }

    pub fn jet_app_a11y(app: &JetApp, policy: String) -> JetApp {
        app.a11y(policy)
    }

    pub fn jet_app_adapter(app: &JetApp, name: String) -> JetApp {
        app.adapter(name)
    }

    pub fn jet_app_csr(app: &JetApp) -> JetApp {
        app.csr()
    }

    pub fn jet_app_ssr(app: &JetApp) -> JetApp {
        app.ssr()
    }

    pub fn jet_app_ssg(app: &JetApp) -> JetApp {
        app.ssg()
    }

    pub fn jet_app_stream(app: &JetApp) -> JetApp {
        app.stream()
    }

    pub fn jet_app_streaming(app: &JetApp) -> JetApp {
        app.streaming()
    }

    pub fn jet_app_island(app: &JetApp) -> JetApp {
        app.island()
    }

    pub fn jet_app_hydration_dev(app: &JetApp) -> JetApp {
        app.hydration_dev()
    }

    pub fn jet_app_hydration_release(app: &JetApp) -> JetApp {
        app.hydration_release()
    }

    pub fn jet_app_facts_json(app: &JetApp) -> String {
        app.facts_json()
    }

    pub fn jet_app_serve(app: &JetApp) {
        app.serve()
    }

    pub fn jet_app_serve_with_port(app: &JetApp, port: i64) {
        app.serve_on(port)
    }

    pub fn jet_app_serve_on(app: &JetApp, port: i64) {
        app.serve_on(port)
    }

    pub fn jet_web_virtual_window_facts(window: &super::JetWebVirtualWindow) -> String {
        window.facts_json()
    }

}
pub use jet_app_impl::{
    jet_app, jet_app_a11y, jet_app_action, jet_app_adapter, jet_app_assets, jet_app_cache,
    jet_app_code_split, jet_app_csr, jet_app_data, jet_app_error, jet_app_facts_json,
    jet_app_form, jet_app_hydration_dev, jet_app_hydration_release, jet_app_island,
    jet_app_layout, jet_app_not_found, jet_app_page, jet_app_pending, jet_app_route,
    jet_app_route_binding, jet_app_routes, jet_app_security, jet_app_serve, jet_app_serve_on,
    jet_app_split, jet_app_ssg, jet_app_ssr, jet_app_stream, jet_app_streaming, jet_web_page,
    JetApp, JetAppRouteBinding, JetAppTreeRouteCallback, JetWebPage,
};
