// D-WEBOPENAPI1=A: one typed OpenAPI projection for canonical HTTP route facts.
//
// The sema/MIR adapter supplies JetOpenApiRouteInput values, including the
// already parsed HTTP route pattern and all operation/request/response facts.
// The model validates and renders that checked graph. The router adapter below
// only marshals fields already checked by WebRouter; it never scans source or
// infers semantics from a host.

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetOpenApiHttpMethod {
    Get,
    Put,
    Post,
    Delete,
    Options,
    Head,
    Patch,
    Trace,
}

impl JetOpenApiHttpMethod {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_uppercase().as_str() {
            "GET" => Ok(Self::Get),
            "PUT" => Ok(Self::Put),
            "POST" => Ok(Self::Post),
            "DELETE" => Ok(Self::Delete),
            "OPTIONS" => Ok(Self::Options),
            "HEAD" => Ok(Self::Head),
            "PATCH" => Ok(Self::Patch),
            "TRACE" => Ok(Self::Trace),
            _ => Err(format!("unsupported OpenAPI HTTP method `{value}`")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Put => "put",
            Self::Post => "post",
            Self::Delete => "delete",
            Self::Options => "options",
            Self::Head => "head",
            Self::Patch => "patch",
            Self::Trace => "trace",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetOpenApiParameterLocation {
    Path,
    Query,
    Header,
    Cookie,
}

impl JetOpenApiParameterLocation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Query => "query",
            Self::Header => "header",
            Self::Cookie => "cookie",
        }
    }


}

#[derive(Clone, Debug, PartialEq)]
pub enum JetOpenApiSchema {
    Any,
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array(Box<JetOpenApiSchema>),
    Object {
        properties: Vec<JetOpenApiProperty>,
        additional_properties: bool,
    },
    OneOf(Vec<JetOpenApiSchema>),
    Nullable(Box<JetOpenApiSchema>),
}

impl JetOpenApiSchema {
    pub fn object(properties: Vec<JetOpenApiProperty>) -> Self {
        Self::Object {
            properties,
            additional_properties: true,
        }
    }

    pub fn closed_object(properties: Vec<JetOpenApiProperty>) -> Self {
        Self::Object {
            properties,
            additional_properties: false,
        }
    }


    fn validate_definition(&self, path: &str, errors: &mut Vec<JetOpenApiValidationError>) {
        match self {
            Self::Array(item) | Self::Nullable(item) => item.validate_definition(path, errors),
            Self::Object {
                properties,
                additional_properties: _,
            } => {
                let mut names = std::collections::BTreeSet::new();
                for property in properties {
                    if !names.insert(property.name.clone()) {
                        errors.push(JetOpenApiValidationError::new(
                            path,
                            format!("duplicate schema property `{}`", property.name),
                        ));
                    }
                    property.schema.validate_definition(
                        &format!("{path}.{}", property.name),
                        errors,
                    );
                }
            }
            Self::OneOf(items) => {
                if items.is_empty() {
                    errors.push(JetOpenApiValidationError::new(
                        path,
                        "oneOf needs at least one schema",
                    ));
                }
                for item in items {
                    item.validate_definition(path, errors);
                }
            }
            Self::Any | Self::Null | Self::Boolean | Self::Integer | Self::Number | Self::String => {}
        }
    }

    fn render_json(&self) -> String {
        match self {
            Self::Any => "{}".to_string(),
            Self::Null => "{\"type\":\"null\"}".to_string(),
            Self::Boolean => "{\"type\":\"boolean\"}".to_string(),
            Self::Integer => "{\"type\":\"integer\",\"format\":\"int64\"}".to_string(),
            Self::Number => "{\"type\":\"number\",\"format\":\"double\"}".to_string(),
            Self::String => "{\"type\":\"string\"}".to_string(),
            Self::Array(item) => format!("{{\"type\":\"array\",\"items\":{}}}", item.render_json()),
            Self::Object {
                properties,
                additional_properties,
            } => {
                let rendered = properties
                    .iter()
                    .map(|property| {
                        let mut out = format!(
                            "{}:{}",
                            jet_openapi_json_string(&property.name),
                            property.schema.render_json()
                        );
                        if let Some(default) = &property.default {
                            out.pop();
                            out.push_str(&format!(
                                ",\"default\":{}",
                                jet_std::render_datatree_json(default, false, 0)
                            ));
                            out.push('}');
                        }
                        out
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let required = properties
                    .iter()
                    .filter(|property| property.required)
                    .map(|property| jet_openapi_json_string(&property.name))
                    .collect::<Vec<_>>()
                    .join(",");
                let mut out = format!(
                    "{{\"type\":\"object\",\"properties\":{{{rendered}}},\"additionalProperties\":{additional_properties}"
                );
                if !required.is_empty() {
                    out.push_str(&format!(",\"required\":[{required}]"));
                }
                out.push('}');
                out
            }
            Self::OneOf(items) => format!(
                "{{\"oneOf\":[{}]}}",
                items
                    .iter()
                    .map(Self::render_json)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Nullable(item) => format!(
                "{{\"anyOf\":[{},{{\"type\":\"null\"}}]}}",
                item.render_json()
            ),
        }
    }

    fn validate_tree(&self, value: &jet_std::DataTree, path: &str) -> Vec<JetOpenApiValidationError> {
        match self {
            Self::Any => Vec::new(),
            Self::Null => {
                if matches!(value, jet_std::DataTree::Null) {
                    Vec::new()
                } else {
                    vec![JetOpenApiValidationError::new(path, "expected null")]
                }
            }
            Self::Boolean => {
                if matches!(value, jet_std::DataTree::Bool(_)) {
                    Vec::new()
                } else {
                    vec![JetOpenApiValidationError::new(path, "expected boolean")]
                }
            }
            Self::Integer => {
                let valid = match value {
                    jet_std::DataTree::Int(_) => true,
                    jet_std::DataTree::Number(text) => text.parse::<i64>().is_ok(),
                    _ => false,
                };
                if valid {
                    Vec::new()
                } else {
                    vec![JetOpenApiValidationError::new(path, "expected integer")]
                }
            }
            Self::Number => {
                let valid = match value {
                    jet_std::DataTree::Int(_) => true,
                    jet_std::DataTree::Float(number) => number.is_finite(),
                    jet_std::DataTree::Number(text) => text.parse::<f64>().map(|n| n.is_finite()).unwrap_or(false),
                    _ => false,
                };
                if valid {
                    Vec::new()
                } else {
                    vec![JetOpenApiValidationError::new(path, "expected number")]
                }
            }
            Self::String => {
                if matches!(value, jet_std::DataTree::Text(_) | jet_std::DataTree::TypedText(_)) {
                    Vec::new()
                } else {
                    vec![JetOpenApiValidationError::new(path, "expected string")]
                }
            }
            Self::Array(item) => match value {
                jet_std::DataTree::Array(values) => values
                    .iter()
                    .enumerate()
                    .flat_map(|(index, value)| item.validate_tree(value, &format!("{path}[{index}]")))
                    .collect(),
                _ => vec![JetOpenApiValidationError::new(path, "expected array")],
            },
            Self::Object {
                properties,
                additional_properties,
            } => {
                let jet_std::DataTree::Object(values) = value else {
                    return vec![JetOpenApiValidationError::new(path, "expected object")];
                };
                let mut errors = Vec::new();
                let mut seen = std::collections::BTreeSet::new();
                for (name, value) in values {
                    if !seen.insert(name.clone()) {
                        errors.push(JetOpenApiValidationError::new(
                            &format!("{path}.{name}"),
                            "duplicate object field",
                        ));
                        continue;
                    }
                    let Some(property) = properties.iter().find(|property| property.name == *name) else {
                        if !additional_properties {
                            errors.push(JetOpenApiValidationError::new(
                                &format!("{path}.{name}"),
                                "unknown object field",
                            ));
                        }
                        continue;
                    };
                    errors.extend(property.schema.validate_tree(value, &format!("{path}.{name}")));
                }
                for property in properties.iter().filter(|property| property.required) {
                    if !seen.contains(&property.name) {
                        errors.push(JetOpenApiValidationError::new(
                            &format!("{path}.{}", property.name),
                            "required field is missing",
                        ));
                    }
                }
                errors
            }
            Self::OneOf(items) => {
                let mut alternatives = Vec::new();
                for item in items {
                    let errors = item.validate_tree(value, path);
                    if errors.is_empty() {
                        return Vec::new();
                    }
                    alternatives.extend(errors);
                }
                if alternatives.is_empty() {
                    vec![JetOpenApiValidationError::new(path, "value matches no schema")]
                } else {
                    vec![JetOpenApiValidationError::new(path, "value matches no oneOf schema")]
                }
            }
            Self::Nullable(item) => {
                if matches!(value, jet_std::DataTree::Null) {
                    Vec::new()
                } else {
                    item.validate_tree(value, path)
                }
            }
        }
    }
}

fn jet_openapi_canonicalize_schema(schema: &mut JetOpenApiSchema) {
    match schema {
        JetOpenApiSchema::Array(item) | JetOpenApiSchema::Nullable(item) => {
            jet_openapi_canonicalize_schema(item);
        }
        JetOpenApiSchema::Object { properties, .. } => {
            properties.sort_by(|left, right| left.name.cmp(&right.name));
            for property in properties {
                jet_openapi_canonicalize_schema(&mut property.schema);
            }
        }
        JetOpenApiSchema::OneOf(items) => {
            for item in items {
                jet_openapi_canonicalize_schema(item);
            }
        }
        JetOpenApiSchema::Any
        | JetOpenApiSchema::Null
        | JetOpenApiSchema::Boolean
        | JetOpenApiSchema::Integer
        | JetOpenApiSchema::Number
        | JetOpenApiSchema::String => {}
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiProperty {
    pub name: String,
    pub schema: JetOpenApiSchema,
    pub required: bool,
    pub default: Option<jet_std::DataTree>,
}

impl JetOpenApiProperty {
    pub fn required(name: String, schema: JetOpenApiSchema) -> Self {
        Self {
            name,
            schema,
            required: true,
            default: None,
        }
    }

    pub fn optional(name: String, schema: JetOpenApiSchema, default: Option<jet_std::DataTree>) -> Self {
        Self {
            name,
            schema,
            required: false,
            default,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiParameter {
    pub location: JetOpenApiParameterLocation,
    pub name: String,
    pub schema: JetOpenApiSchema,
    pub required: bool,
    pub default: Option<jet_std::DataTree>,
    pub catch_all: bool,
}

impl JetOpenApiParameter {
    pub fn new(
        location: JetOpenApiParameterLocation,
        name: String,
        schema: JetOpenApiSchema,
    ) -> Self {
        Self {
            required: location == JetOpenApiParameterLocation::Path,
            location,
            name,
            schema,
            default: None,
            catch_all: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiRequestBody {
    pub schema: JetOpenApiSchema,
    pub required: bool,
    pub content_type: String,
}

impl JetOpenApiRequestBody {
    pub fn json(schema: JetOpenApiSchema, required: bool) -> Self {
        Self {
            schema,
            required,
            content_type: "application/json".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiResponse {
    pub status: u16,
    pub description: String,
    pub schema: Option<JetOpenApiSchema>,
    pub content_type: Option<String>,
}

impl JetOpenApiResponse {
    pub fn new(status: u16, description: String, schema: Option<JetOpenApiSchema>) -> Self {
        Self {
            status,
            description,
            content_type: schema.as_ref().map(|_| "application/json".to_string()),
            schema,
        }
    }

    pub fn ok(schema: JetOpenApiSchema) -> Self {
        Self::new(200, "OK".to_string(), Some(schema))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiSecurityRequirement {
    pub scheme: String,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiSecurityScheme {
    pub name: String,
    pub kind: String,
    pub location: Option<String>,
    pub parameter_name: Option<String>,
    pub scheme: Option<String>,
    pub bearer_format: Option<String>,
}

impl JetOpenApiSecurityScheme {
    pub fn api_key(name: String, location: String, parameter_name: String) -> Self {
        Self {
            name,
            kind: "apiKey".to_string(),
            location: Some(location),
            parameter_name: Some(parameter_name),
            scheme: None,
            bearer_format: None,
        }
    }

    pub fn http(name: String, scheme: String) -> Self {
        Self {
            name,
            kind: "http".to_string(),
            location: None,
            parameter_name: None,
            scheme: Some(scheme),
            bearer_format: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiRouteInput {
    pub method: JetOpenApiHttpMethod,
    pub pattern: JetHTTPRoutePattern,
    pub operation_id: String,
    pub summary: Option<String>,
    pub parameters: Vec<JetOpenApiParameter>,
    pub request_body: Option<JetOpenApiRequestBody>,
    pub responses: Vec<JetOpenApiResponse>,
    pub security: Vec<JetOpenApiSecurityRequirement>,
    pub provenance: String,
}

impl JetOpenApiRouteInput {
    pub fn new(
        method: JetOpenApiHttpMethod,
        pattern: JetHTTPRoutePattern,
        operation_id: String,
        parameters: Vec<JetOpenApiParameter>,
        request_body: Option<JetOpenApiRequestBody>,
        responses: Vec<JetOpenApiResponse>,
        security: Vec<JetOpenApiSecurityRequirement>,
        provenance: String,
    ) -> Self {
        Self {
            method,
            pattern,
            operation_id,
            summary: None,
            parameters,
            request_body,
            responses,
            security,
            provenance,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiRouteFact {
    pub method: JetOpenApiHttpMethod,
    pub path: String,
    pub operation_id: String,
    pub summary: Option<String>,
    pub parameters: Vec<JetOpenApiParameter>,
    pub request_body: Option<JetOpenApiRequestBody>,
    pub responses: Vec<JetOpenApiResponse>,
    pub security: Vec<JetOpenApiSecurityRequirement>,
    pub provenance: String,
    canonical_pattern: JetHTTPRoutePattern,
}

fn jet_openapi_operation_id_for_path(method: JetOpenApiHttpMethod, path: &str) -> String {
    let mut hash = 14695981039346656037u64;
    for byte in b"jet-openapi-operation-v1:" {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    for byte in method.as_str().bytes().chain(std::iter::once(b'\0')).chain(path.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("jet_{}_{hash:016x}", method.as_str().to_ascii_lowercase())
}

pub fn jet_web_openapi_operation_id(
    method: JetOpenApiHttpMethod,
    pattern: &JetHTTPRoutePattern,
) -> String {
    jet_openapi_operation_id_for_path(method, &jet_http_route_openapi_path(pattern))
}

impl JetOpenApiRouteFact {
    pub fn from_input(input: JetOpenApiRouteInput) -> Self {
        let path = jet_http_route_openapi_path(&input.pattern);
        let operation_id = if input.operation_id.is_empty() {
            jet_openapi_operation_id_for_path(input.method, &path)
        } else {
            input.operation_id
        };
        Self {
            method: input.method,
            path,
            operation_id,
            summary: input.summary,
            parameters: input.parameters,
            request_body: input.request_body,
            responses: input.responses,
            security: input.security,
            provenance: input.provenance,
            canonical_pattern: input.pattern,
        }
    }

}

fn jet_openapi_canonicalize_route(route: &mut JetOpenApiRouteFact) {
    route.parameters.sort_by(|left, right| {
        left.location
            .cmp(&right.location)
            .then_with(|| left.name.cmp(&right.name))
    });
    route
        .responses
        .sort_by(|left, right| left.status.cmp(&right.status).then_with(|| left.description.cmp(&right.description)));
    for requirement in &mut route.security {
        requirement.scopes.sort();
    }
    route.security.sort_by(|left, right| {
        left.scheme
            .cmp(&right.scheme)
            .then_with(|| left.scopes.cmp(&right.scopes))
    });
    for parameter in &mut route.parameters {
        jet_openapi_canonicalize_schema(&mut parameter.schema);
    }
    if let Some(body) = &mut route.request_body {
        jet_openapi_canonicalize_schema(&mut body.schema);
    }
    for response in &mut route.responses {
        if let Some(schema) = &mut response.schema {
            jet_openapi_canonicalize_schema(schema);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiDocument {
    pub openapi: String,
    pub title: String,
    pub version: String,
    pub routes: Vec<JetOpenApiRouteFact>,
    pub security_schemes: Vec<JetOpenApiSecurityScheme>,
}

impl JetOpenApiDocument {
    pub fn new(title: String, version: String, routes: Vec<JetOpenApiRouteFact>) -> Result<Self, String> {
        Self::with_openapi("3.1.0".to_string(), title, version, routes, Vec::new())
    }

    pub fn with_openapi(
        openapi: String,
        title: String,
        version: String,
        routes: Vec<JetOpenApiRouteFact>,
        security_schemes: Vec<JetOpenApiSecurityScheme>,
    ) -> Result<Self, String> {
        let mut routes = routes;
        for route in &mut routes {
            jet_openapi_canonicalize_route(route);
        }
        routes.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.method.cmp(&right.method))
                .then_with(|| left.operation_id.cmp(&right.operation_id))
        });
        let mut security_schemes = security_schemes;
        security_schemes.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.kind.cmp(&right.kind))
        });

        if openapi != "3.1.0" {
            return Err(format!("OpenAPI version must be `3.1.0`, got `{openapi}`"));
        }
        if title.is_empty() || version.is_empty() {
            return Err("OpenAPI info title and version must not be empty".to_string());
        }
        let mut errors = Vec::new();
        let mut route_keys = std::collections::BTreeSet::new();
        let mut operation_ids = std::collections::BTreeSet::new();
        let mut scheme_names = std::collections::BTreeSet::new();
        for scheme in &security_schemes {
            if scheme.name.is_empty() || !scheme_names.insert(scheme.name.clone()) {
                errors.push(format!("duplicate or empty security scheme `{}`", scheme.name));
            }
            match scheme.kind.as_str() {
                "apiKey" => {
                    if !matches!(scheme.location.as_deref(), Some("header" | "query" | "cookie"))
                        || scheme
                            .parameter_name
                            .as_deref()
                            .is_none_or(|value| value.is_empty())
                    {
                        errors.push(format!(
                            "apiKey security scheme `{}` needs a header, query, or cookie name",
                            scheme.name
                        ));
                    }
                }
                "http" if scheme
                    .scheme
                    .as_deref()
                    .is_none_or(|value| value.is_empty()) =>
                {
                    errors.push(format!("http security scheme `{}` needs a scheme", scheme.name));
                }
                _ => {}
            }
        }
        for route in &routes {
            let pattern = &route.canonical_pattern;
            let canonical_path = jet_http_route_openapi_path(pattern);
            if route.path != canonical_path
                || !route.path.starts_with('/')
                || route.path.contains('?')
                || route.path.contains('#')
            {
                errors.push(format!(
                    "invalid OpenAPI route path `{}` for canonical pattern `{canonical_path}`",
                    route.path
                ));
            }
            let path_names = jet_openapi_pattern_parameter_names(pattern);
            if route.operation_id.is_empty() {
                errors.push(format!("OpenAPI route `{}` needs an operationId", route.path));
            }
            if !operation_ids.insert(route.operation_id.clone()) {
                errors.push(format!("duplicate OpenAPI operationId `{}`", route.operation_id));
            }
            let key = format!("{} {}", route.method.as_str(), route.path);
            if !route_keys.insert(key.clone()) {
                errors.push(format!("duplicate OpenAPI route `{key}`"));
            }
            let mut parameter_keys = std::collections::BTreeSet::new();
            let mut declared_path_names = std::collections::BTreeSet::new();
            let mut schema_errors = Vec::new();
            for parameter in &route.parameters {
                let parameter_key = format!("{}:{}", parameter.location.as_str(), parameter.name);
                if !parameter_keys.insert(parameter_key) {
                    schema_errors.push(JetOpenApiValidationError::new(
                        "parameter",
                        format!(
                            "duplicate {} parameter `{}`",
                            parameter.location.as_str(),
                            parameter.name
                        ),
                    ));
                }
                if parameter.location == JetOpenApiParameterLocation::Path {
                    declared_path_names.insert(parameter.name.clone());
                    if !path_names.contains(&parameter.name) {
                        schema_errors.push(JetOpenApiValidationError::new(
                            &parameter.name,
                            "path parameter is not present in the route template",
                        ));
                    }
                    let is_catch_all = pattern.segments.iter().any(|segment| {
                        matches!(
                            segment,
                            JetHTTPRouteSegment::CatchAll(name) if name == &parameter.name
                        )
                    });
                    if parameter.catch_all != is_catch_all {
                        schema_errors.push(JetOpenApiValidationError::new(
                            &parameter.name,
                            "path parameter catch-all fact disagrees with the canonical route pattern",
                        ));
                    }
                }
                parameter.schema.validate_definition(&parameter.name, &mut schema_errors);
                if parameter.name.is_empty() {
                    schema_errors.push(JetOpenApiValidationError::new(
                        "parameter",
                        "parameter name must not be empty",
                    ));
                }
                if parameter.location == JetOpenApiParameterLocation::Path && !parameter.required {
                    schema_errors.push(JetOpenApiValidationError::new(
                        &parameter.name,
                        "path parameters are always required",
                    ));
                }
            }
            for path_name in path_names {
                if !declared_path_names.contains(&path_name) {
                    schema_errors.push(JetOpenApiValidationError::new(
                        &path_name,
                        "route template parameter has no parameter fact",
                    ));
                }
            }
            if let Some(body) = &route.request_body {
                body.schema.validate_definition("requestBody", &mut schema_errors);
                if body.content_type.is_empty() {
                    schema_errors.push(JetOpenApiValidationError::new(
                        "requestBody",
                        "request body content type must not be empty",
                    ));
                }
            }
            let mut response_statuses = std::collections::BTreeSet::new();
            for response in &route.responses {
                if !(100..=599).contains(&response.status) {
                    errors.push(format!("response status {} is out of range", response.status));
                }
                if !response_statuses.insert(response.status) {
                    errors.push(format!(
                        "route `{key}` declares response status {} more than once",
                        response.status
                    ));
                }
                if let Some(schema) = &response.schema {
                    schema.validate_definition("response", &mut schema_errors);
                }
            }
            if route.responses.is_empty() {
                errors.push(format!("route `{key}` must declare a response"));
            }
            for requirement in &route.security {
                if requirement.scheme.is_empty() {
                    errors.push(format!("route `{key}` has an empty security scheme"));
                } else if !scheme_names.contains(&requirement.scheme) {
                    errors.push(format!(
                        "route `{key}` references unknown security scheme `{}`",
                        requirement.scheme
                    ));
                }
            }
            errors.extend(schema_errors.into_iter().map(|error| error.to_string()));
        }
        if errors.is_empty() {
            Ok(Self {
                openapi,
                title,
                version,
                routes,
                security_schemes,
            })
        } else {
            Err(errors.join("; "))
        }
    }

    pub fn json(&self) -> String {
        let mut out = format!(
            "{{\"openapi\":{},\"info\":{{\"title\":{},\"version\":{}}},\"paths\":{{",
            jet_openapi_json_string(&self.openapi),
            jet_openapi_json_string(&self.title),
            jet_openapi_json_string(&self.version)
        );
        let mut routes = self.routes.clone();
        for route in &mut routes {
            jet_openapi_canonicalize_route(route);
        }
        routes.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.method.cmp(&right.method))
                .then_with(|| left.operation_id.cmp(&right.operation_id))
        });
        let mut security_schemes = self.security_schemes.clone();
        security_schemes.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.kind.cmp(&right.kind))
        });
        let mut paths: Vec<(String, Vec<&JetOpenApiRouteFact>)> = Vec::new();
        for route in &routes {
            if let Some((_, route_rows)) = paths.iter_mut().find(|(path, _)| path == &route.path) {
                route_rows.push(route);
            } else {
                paths.push((route.path.clone(), vec![route]));
            }
        }
        let mut path_rows = Vec::new();
        for (path, routes) in paths {
            let methods = routes
                .iter()
                .map(|route| {
                    format!(
                        "{}:{}",
                        jet_openapi_json_string(route.method.as_str()),
                        jet_openapi_route_json(route)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            path_rows.push(format!("{}:{{{methods}}}", jet_openapi_json_string(&path)));
        }
        out.push_str(&path_rows.join(","));
        out.push('}');
        if !security_schemes.is_empty() {
            out.push_str(",\"components\":{\"securitySchemes\":{");
            out.push_str(
                &security_schemes
                    .iter()
                    .map(|scheme| {
                        format!(
                            "{}:{}",
                            jet_openapi_json_string(&scheme.name),
                            jet_openapi_security_scheme_json(scheme)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(","),
            );
            out.push_str("}}");
        }
        out.push('}');
        out
    }

    pub fn validator(&self) -> JetOpenApiValidator {
        JetOpenApiValidator {
            routes: self.routes.clone(),
            security_schemes: self.security_schemes.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiValidationError {
    pub path: String,
    pub reason: String,
}

impl JetOpenApiValidationError {
    pub fn new(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for JetOpenApiValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.path.is_empty() {
            write!(formatter, "{}", self.reason)
        } else {
            write!(formatter, "{}: {}", self.path, self.reason)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiRequest {
    pub method: JetOpenApiHttpMethod,
    pub path: String,
    pub query: std::collections::BTreeMap<String, String>,
    pub headers: std::collections::BTreeMap<String, String>,
    pub cookies: std::collections::BTreeMap<String, String>,
    pub body: Option<jet_std::DataTree>,
}

impl JetOpenApiRequest {
    pub fn new(method: JetOpenApiHttpMethod, path: String) -> Self {
        Self {
            method,
            path,
            query: std::collections::BTreeMap::new(),
            headers: std::collections::BTreeMap::new(),
            cookies: std::collections::BTreeMap::new(),
            body: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetOpenApiValidator {
    routes: Vec<JetOpenApiRouteFact>,
    security_schemes: Vec<JetOpenApiSecurityScheme>,
}

impl JetOpenApiValidator {
    pub fn validate(&self, request: &JetOpenApiRequest) -> Result<(), Vec<JetOpenApiValidationError>> {
        let mut candidates = self
            .routes
            .iter()
            .enumerate()
            .filter(|(_, route)| route.method == request.method)
            .filter_map(|(index, route)| {
                jet_openapi_match_path(route, &request.path)
                    .map(|params| (index, route, params))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            jet_http_route_selection_cmp(
                &left.1.canonical_pattern,
                left.0,
                &right.1.canonical_pattern,
                right.0,
            )
            .reverse()
        });
        let Some((_, route, path_values)) = candidates.into_iter().next() else {
            return Err(vec![JetOpenApiValidationError::new(
                "route",
                format!("no OpenAPI route matches {} {}", request.method.as_str(), request.path),
            )]);
        };
        let mut errors = Vec::new();
        for parameter in &route.parameters {
            let value = match parameter.location {
                JetOpenApiParameterLocation::Path => path_values.get(&parameter.name),
                JetOpenApiParameterLocation::Query => request.query.get(&parameter.name),
                JetOpenApiParameterLocation::Header => request.headers.get(&parameter.name),
                JetOpenApiParameterLocation::Cookie => request.cookies.get(&parameter.name),
            };
            let Some(value) = value else {
                if parameter.default.is_some() {
                    continue;
                }
                if parameter.required || parameter.location == JetOpenApiParameterLocation::Path {
                    errors.push(JetOpenApiValidationError::new(
                        format!("{}.{}", parameter.location.as_str(), parameter.name),
                        "required parameter is missing",
                    ));
                }
                continue;
            };
            if let Err(reason) = jet_openapi_validate_parameter_value(&parameter.schema, value) {
                errors.push(JetOpenApiValidationError::new(
                    format!("{}.{}", parameter.location.as_str(), parameter.name),
                    reason,
                ));
            }
        }
        if let Some(body) = &route.request_body {
            match request.body.as_ref() {
                Some(value) => errors.extend(body.schema.validate_tree(value, "body")),
                None if body.required => errors.push(JetOpenApiValidationError::new(
                    "body",
                    "required request body is missing",
                )),
                None => {}
            }
        }
        if !route.security.is_empty()
            && !route.security.iter().any(|requirement| {
                self.security_schemes
                    .iter()
                    .find(|scheme| scheme.name == requirement.scheme)
                    .is_some_and(|scheme| jet_openapi_security_present(scheme, request))
            })
        {
            errors.push(JetOpenApiValidationError::new(
                "security",
                "no declared security requirement is satisfied",
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn jet_openapi_validate_parameter_value(schema: &JetOpenApiSchema, value: &str) -> Result<(), String> {
    match schema {
        JetOpenApiSchema::Any | JetOpenApiSchema::String => Ok(()),
        JetOpenApiSchema::Boolean => match value {
            "true" | "false" | "1" | "0" => Ok(()),
            _ => Err("expected boolean parameter".to_string()),
        },
        JetOpenApiSchema::Integer => value
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| "expected integer parameter".to_string()),
        JetOpenApiSchema::Number => value
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(|_| ())
            .ok_or_else(|| "expected finite number parameter".to_string()),
        JetOpenApiSchema::Null => Err("null is not a valid parameter value".to_string()),
        JetOpenApiSchema::Nullable(item) => {
            if value == "null" {
                Ok(())
            } else {
                jet_openapi_validate_parameter_value(item, value)
            }
        }
        JetOpenApiSchema::Array(item) => value
            .split(',')
            .enumerate()
            .try_for_each(|(_, item_value)| jet_openapi_validate_parameter_value(item, item_value)),
        JetOpenApiSchema::Object { .. } | JetOpenApiSchema::OneOf(_) => Ok(()),
    }
}

fn jet_openapi_security_present(
    scheme: &JetOpenApiSecurityScheme,
    request: &JetOpenApiRequest,
) -> bool {
    match scheme.kind.as_str() {
        "apiKey" => match (scheme.location.as_deref(), scheme.parameter_name.as_deref()) {
            (Some("header"), Some(name)) => request.headers.contains_key(name),
            (Some("query"), Some(name)) => request.query.contains_key(name),
            (Some("cookie"), Some(name)) => request.cookies.contains_key(name),
            _ => false,
        },
        "http" | "oauth2" | "openIdConnect" => request
            .headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("authorization")),
        _ => false,
    }
}


fn jet_openapi_pattern_parameter_names(
    pattern: &JetHTTPRoutePattern,
) -> std::collections::BTreeSet<String> {
    pattern
        .segments
        .iter()
        .filter_map(|segment| match segment {
            JetHTTPRouteSegment::Param(name) | JetHTTPRouteSegment::CatchAll(name) => {
                Some(name.clone())
            }
            JetHTTPRouteSegment::Static(_) => None,
        })
        .collect()
}

fn jet_openapi_match_path(
    route: &JetOpenApiRouteFact,
    actual: &str,
) -> Option<std::collections::BTreeMap<String, String>> {
    let path = jet_http_route_path(actual).ok()?;
    jet_http_route_match(&route.canonical_pattern, &path)
}
fn jet_openapi_route_json(route: &JetOpenApiRouteFact) -> String {
    let mut out = format!("{{\"operationId\":{}", jet_openapi_json_string(&route.operation_id));
    if let Some(summary) = &route.summary {
        out.push_str(&format!(",\"summary\":{}", jet_openapi_json_string(summary)));
    }
    let parameters = &route.parameters;
    if !parameters.is_empty() {
        out.push_str(",\"parameters\":[");
        out.push_str(
            &parameters
                .iter()
                .map(jet_openapi_parameter_json)
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push(']');
    }
    if let Some(body) = &route.request_body {
        out.push_str(",\"requestBody\":{\"required\":");
        out.push_str(if body.required { "true" } else { "false" });
        out.push_str(",\"content\":{");
        out.push_str(&jet_openapi_json_string(&body.content_type));
        out.push_str(":{\"schema\":");
        out.push_str(&body.schema.render_json());
        out.push_str("}}}");
    }
    let responses = &route.responses;
    out.push_str(",\"responses\":{");
    out.push_str(
        &responses
            .iter()
            .map(|response| {
                let mut rendered = format!(
                    "{}:{{\"description\":{}",
                    jet_openapi_json_string(&response.status.to_string()),
                    jet_openapi_json_string(&response.description)
                );
                if let (Some(schema), Some(content_type)) = (&response.schema, &response.content_type) {
                    rendered.push_str(",\"content\":{");
                    rendered.push_str(&jet_openapi_json_string(content_type));
                    rendered.push_str(":{\"schema\":");
                    rendered.push_str(&schema.render_json());
                    rendered.push_str("}}");
                }
                rendered.push('}');
                rendered
            })
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push('}');
    if !route.security.is_empty() {
        let security = &route.security;
        out.push_str(",\"security\":[");
        out.push_str(
            &security
                .iter()
                .map(|requirement| {
                    let scopes = &requirement.scopes;
                    format!(
                        "{{{}:[{}]}}",
                        jet_openapi_json_string(&requirement.scheme),
                        scopes
                            .iter()
                            .map(|scope| jet_openapi_json_string(scope))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                })
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push(']');
    }
    out.push('}');
    out
}

fn jet_openapi_parameter_json(parameter: &JetOpenApiParameter) -> String {
    let mut out = format!(
        "{{\"name\":{},\"in\":{},\"required\":{},\"schema\":{}",
        jet_openapi_json_string(&parameter.name),
        jet_openapi_json_string(parameter.location.as_str()),
        parameter.required || parameter.location == JetOpenApiParameterLocation::Path,
        parameter.schema.render_json()
    );
    if let Some(default) = &parameter.default {
        out.push_str(&format!(",\"default\":{}", jet_std::render_datatree_json(default, false, 0)));
    }
    out.push('}');
    out
}

fn jet_openapi_security_scheme_json(scheme: &JetOpenApiSecurityScheme) -> String {
    let mut out = format!("{{\"type\":{}", jet_openapi_json_string(&scheme.kind));
    if let Some(location) = &scheme.location {
        out.push_str(&format!(",\"in\":{}", jet_openapi_json_string(location)));
    }
    if let Some(name) = &scheme.parameter_name {
        out.push_str(&format!(",\"name\":{}", jet_openapi_json_string(name)));
    }
    if let Some(value) = &scheme.scheme {
        out.push_str(&format!(",\"scheme\":{}", jet_openapi_json_string(value)));
    }
    if let Some(value) = &scheme.bearer_format {
        out.push_str(&format!(",\"bearerFormat\":{}", jet_openapi_json_string(value)));
    }
    out.push('}');
    out
}


fn jet_openapi_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// Convert the typed route graph emitted by the sema/MIR adapter into the
/// projection model. The resulting document canonicalizes identity and order.
pub fn jet_web_openapi_route_facts(
    inputs: Vec<JetOpenApiRouteInput>,
) -> Vec<JetOpenApiRouteFact> {
    inputs
        .into_iter()
        .map(JetOpenApiRouteFact::from_input)
        .collect()
}

pub fn jet_web_openapi_document_from_routes(
    inputs: Vec<JetOpenApiRouteInput>,
    title: String,
    version: String,
    security_schemes: Vec<JetOpenApiSecurityScheme>,
) -> Result<JetOpenApiDocument, String> {
    JetOpenApiDocument::with_openapi(
        "3.1.0".to_string(),
        title,
        version,
        jet_web_openapi_route_facts(inputs),
        security_schemes,
    )
}

pub fn jet_web_openapi_from_routes(
    inputs: Vec<JetOpenApiRouteInput>,
    title: String,
    version: String,
    security_schemes: Vec<JetOpenApiSecurityScheme>,
) -> Result<String, String> {
    Ok(jet_web_openapi_document_from_routes(inputs, title, version, security_schemes)?.json())
}

pub fn jet_web_openapi_validator_from_routes(
    inputs: Vec<JetOpenApiRouteInput>,
    title: String,
    version: String,
    security_schemes: Vec<JetOpenApiSecurityScheme>,
) -> Result<JetOpenApiValidator, String> {
    Ok(jet_web_openapi_document_from_routes(inputs, title, version, security_schemes)?.validator())
}

pub fn jet_web_openapi_validate_routes(
    inputs: Vec<JetOpenApiRouteInput>,
    title: String,
    version: String,
    security_schemes: Vec<JetOpenApiSecurityScheme>,
    request: &JetOpenApiRequest,
) -> Result<(), Vec<JetOpenApiValidationError>> {
    match jet_web_openapi_validator_from_routes(inputs, title, version, security_schemes) {
        Ok(validator) => validator.validate(request),
        Err(error) => Err(vec![JetOpenApiValidationError::new("document", error)]),
    }
}

fn jet_openapi_contract_object(
    value: &jet_std::DataTree,
) -> Result<&[(String, jet_std::DataTree)], String> {
    match value {
        jet_std::DataTree::Object(fields) => Ok(fields),
        _ => Err("OpenAPI route contract must be an object".to_string()),
    }
}

fn jet_openapi_contract_field<'a>(
    object: &'a [(String, jet_std::DataTree)],
    name: &str,
) -> Result<&'a jet_std::DataTree, String> {
    object
        .iter()
        .find_map(|(field_name, value)| (field_name == name).then_some(value))
        .ok_or_else(|| format!("OpenAPI route contract is missing `{name}`"))
}

fn jet_openapi_contract_text(
    object: &[(String, jet_std::DataTree)],
    name: &str,
) -> Result<String, String> {
    match jet_openapi_contract_field(object, name)? {
        jet_std::DataTree::Text(value) | jet_std::DataTree::TypedText(value) => Ok(value.clone()),
        _ => Err(format!("OpenAPI route contract field `{name}` must be text")),
    }
}

fn jet_openapi_contract_bool(
    object: &[(String, jet_std::DataTree)],
    name: &str,
) -> Result<bool, String> {
    match jet_openapi_contract_field(object, name)? {
        jet_std::DataTree::Bool(value) => Ok(*value),
        _ => Err(format!("OpenAPI route contract field `{name}` must be boolean")),
    }
}

fn jet_openapi_contract_optional_text(
    object: &[(String, jet_std::DataTree)],
    name: &str,
) -> Result<Option<String>, String> {
    match jet_openapi_contract_field(object, name)? {
        jet_std::DataTree::Null => Ok(None),
        jet_std::DataTree::Text(value) | jet_std::DataTree::TypedText(value) => {
            Ok(Some(value.clone()))
        }
        _ => Err(format!(
            "OpenAPI route contract field `{name}` must be text or null"
        )),
    }
}

fn jet_openapi_contract_array<'a>(
    object: &'a [(String, jet_std::DataTree)],
    name: &str,
) -> Result<&'a [jet_std::DataTree], String> {
    match jet_openapi_contract_field(object, name)? {
        jet_std::DataTree::Array(values) => Ok(values),
        _ => Err(format!("OpenAPI route contract field `{name}` must be an array")),
    }
}

fn jet_openapi_contract_integer(
    object: &[(String, jet_std::DataTree)],
    name: &str,
) -> Result<i64, String> {
    match jet_openapi_contract_field(object, name)? {
        jet_std::DataTree::Int(value) => Ok(*value),
        jet_std::DataTree::Number(value) => value
            .parse::<i64>()
            .map_err(|_| format!("OpenAPI route contract field `{name}` is out of range")),
        _ => Err(format!("OpenAPI route contract field `{name}` must be an integer")),
    }
}

fn jet_openapi_contract_schema(
    value: &jet_std::DataTree,
) -> Result<JetOpenApiSchema, String> {
    let object = jet_openapi_contract_object(value)?;
    let Some(kind) = jet_openapi_contract_field(object, "type").ok() else {
        if let Some(any_of) = jet_openapi_contract_field(object, "anyOf").ok() {
            let jet_std::DataTree::Array(items) = any_of else {
                return Err("OpenAPI schema `anyOf` must be an array".to_string());
            };
            if items.len() == 2 && matches!(items[1], jet_std::DataTree::Object(_)) {
                let null_schema = jet_openapi_contract_schema(&items[1])?;
                if matches!(null_schema, JetOpenApiSchema::Null) {
                    return Ok(JetOpenApiSchema::Nullable(Box::new(
                        jet_openapi_contract_schema(&items[0])?,
                    )));
                }
            }
        }
        if let Some(one_of) = jet_openapi_contract_field(object, "oneOf").ok() {
            let jet_std::DataTree::Array(items) = one_of else {
                return Err("OpenAPI schema `oneOf` must be an array".to_string());
            };
            return Ok(JetOpenApiSchema::OneOf(
                items
                    .iter()
                    .map(jet_openapi_contract_schema)
                    .collect::<Result<Vec<_>, _>>()?,
            ));
        }
        return Ok(JetOpenApiSchema::Any);
    };
    let (jet_std::DataTree::Text(kind) | jet_std::DataTree::TypedText(kind)) = kind else {
        return Err("OpenAPI schema `type` must be text".to_string());
    };
    match kind.as_str() {
        "null" => Ok(JetOpenApiSchema::Null),
        "boolean" => Ok(JetOpenApiSchema::Boolean),
        "integer" => Ok(JetOpenApiSchema::Integer),
        "number" => Ok(JetOpenApiSchema::Number),
        "string" => Ok(JetOpenApiSchema::String),
        "array" => Ok(JetOpenApiSchema::Array(Box::new(
            jet_openapi_contract_schema(jet_openapi_contract_field(object, "items")?)?,
        ))),
        "object" => {
            let properties = match jet_openapi_contract_field(object, "properties")? {
                jet_std::DataTree::Object(fields) => fields,
                _ => {
                    return Err("OpenAPI object schema `properties` must be an object".to_string())
                }
            };
            let required = match jet_openapi_contract_field(object, "required").ok() {
                None => Vec::new(),
                Some(jet_std::DataTree::Array(values)) => values
                    .iter()
                    .map(|value| match value {
                        jet_std::DataTree::Text(name) | jet_std::DataTree::TypedText(name) => {
                            Ok(name.clone())
                        }
                        _ => Err("OpenAPI schema `required` names must be text".to_string()),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                Some(_) => return Err("OpenAPI schema `required` must be an array".to_string()),
            };
            let additional_properties =
                match jet_openapi_contract_field(object, "additionalProperties")? {
                    jet_std::DataTree::Bool(value) => *value,
                    _ => {
                        return Err(
                            "OpenAPI object schema `additionalProperties` must be boolean"
                                .to_string(),
                        )
                    }
                };
            let properties = properties
                .iter()
                .map(|(name, value)| {
                    Ok(JetOpenApiProperty {
                        name: name.clone(),
                        schema: jet_openapi_contract_schema(value)?,
                        required: required.iter().any(|required| required == name),
                        default: None,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(JetOpenApiSchema::Object {
                properties,
                additional_properties,
            })
        }
        other => Err(format!("unsupported OpenAPI schema type `{other}`")),
    }
}

fn jet_openapi_contract_parameter(
    value: &jet_std::DataTree,
) -> Result<JetOpenApiParameter, String> {
    let object = jet_openapi_contract_object(value)?;
    let location = match jet_openapi_contract_text(object, "in")?.as_str() {
        "path" => JetOpenApiParameterLocation::Path,
        "query" => JetOpenApiParameterLocation::Query,
        "header" => JetOpenApiParameterLocation::Header,
        "cookie" => JetOpenApiParameterLocation::Cookie,
        other => return Err(format!("unsupported OpenAPI parameter location `{other}`")),
    };
    Ok(JetOpenApiParameter {
        location,
        name: jet_openapi_contract_text(object, "name")?,
        schema: jet_openapi_contract_schema(jet_openapi_contract_field(object, "schema")?)?,
        required: jet_openapi_contract_bool(object, "required")?,
        default: None,
        catch_all: jet_openapi_contract_bool(object, "catch_all")?,
    })
}

fn jet_openapi_contract_request_body(
    value: &jet_std::DataTree,
) -> Result<Option<JetOpenApiRequestBody>, String> {
    let jet_std::DataTree::Null = value else {
        let object = jet_openapi_contract_object(value)?;
        return Ok(Some(JetOpenApiRequestBody {
            required: jet_openapi_contract_bool(object, "required")?,
            content_type: jet_openapi_contract_text(object, "content_type")?,
            schema: jet_openapi_contract_schema(jet_openapi_contract_field(object, "schema")?)?,
        }));
    };
    Ok(None)
}

fn jet_openapi_contract_response(
    value: &jet_std::DataTree,
) -> Result<JetOpenApiResponse, String> {
    let object = jet_openapi_contract_object(value)?;
    let status = jet_openapi_contract_integer(object, "status")?;
    let status = u16::try_from(status)
        .map_err(|_| "OpenAPI response status is outside the u16 range".to_string())?;
    let schema = match jet_openapi_contract_field(object, "schema")? {
        jet_std::DataTree::Null => None,
        schema => Some(jet_openapi_contract_schema(schema)?),
    };
    Ok(JetOpenApiResponse {
        status,
        description: jet_openapi_contract_text(object, "description")?,
        schema,
        content_type: jet_openapi_contract_optional_text(object, "content_type")?,
    })
}

fn jet_openapi_contract_security(
    value: &jet_std::DataTree,
) -> Result<JetOpenApiSecurityRequirement, String> {
    let object = jet_openapi_contract_object(value)?;
    let scopes = jet_openapi_contract_array(object, "scopes")?
        .iter()
        .map(|scope| match scope {
            jet_std::DataTree::Text(scope) | jet_std::DataTree::TypedText(scope) => {
                Ok(scope.clone())
            }
            _ => Err("OpenAPI security scopes must be text".to_string()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(JetOpenApiSecurityRequirement {
        scheme: jet_openapi_contract_text(object, "scheme")?,
        scopes,
    })
}

fn jet_openapi_contract_input(
    contract: &str,
) -> Result<JetOpenApiRouteInput, String> {
    let value = jet_std::parse_json_strict(contract)
        .map_err(|error| format!("invalid checked OpenAPI route contract: {}", error.reason))?;
    let object = jet_openapi_contract_object(&value)?;
    let method = JetOpenApiHttpMethod::parse(&jet_openapi_contract_text(object, "method")?)?;
    let pattern_text = jet_openapi_contract_text(object, "pattern")?;
    let pattern = jet_http_route_pattern(&pattern_text)?;
    let expected_path = jet_http_route_openapi_path(&pattern);
    let path = jet_openapi_contract_text(object, "path")?;
    if path != expected_path {
        return Err("checked OpenAPI route path does not match its route pattern".to_string());
    }
    let parameters = jet_openapi_contract_array(object, "parameters")?
        .iter()
        .map(jet_openapi_contract_parameter)
        .collect::<Result<Vec<_>, _>>()?;
    let responses = jet_openapi_contract_array(object, "responses")?
        .iter()
        .map(jet_openapi_contract_response)
        .collect::<Result<Vec<_>, _>>()?;
    if responses.is_empty() {
        return Err("checked OpenAPI route contract has no responses".to_string());
    }
    let security = jet_openapi_contract_array(object, "security")?
        .iter()
        .map(jet_openapi_contract_security)
        .collect::<Result<Vec<_>, _>>()?;
    let mut input = JetOpenApiRouteInput::new(
        method,
        pattern,
        jet_openapi_contract_text(object, "operation_id")?,
        parameters,
        jet_openapi_contract_request_body(jet_openapi_contract_field(object, "request_body")?)?,
        responses,
        security,
        jet_openapi_contract_text(object, "provenance")?,
    );
    input.summary = jet_openapi_contract_optional_text(object, "summary")?;
    Ok(input)
}

/// Project interpreter-held HTTPRouter route facts through the same checked
/// contract parser and OpenAPI renderer as native HTTPRouter routes.
pub fn jet_web_openapi_from_contracts(
    routes: &[(String, String, String)],
) -> Result<String, String> {
    let mut inputs = Vec::with_capacity(routes.len());
    for (method, pattern, contract_json) in routes {
        if contract_json.is_empty() {
            return Err(format!(
                "E2805: route `{} {}` has no checked OpenAPI contract",
                method, pattern
            ));
        }
        let input = jet_openapi_contract_input(contract_json)?;
        if !input.method.as_str().eq_ignore_ascii_case(method) {
            return Err(format!(
                "E2805: checked OpenAPI method `{}` does not match route method `{}`",
                input.method.as_str(),
                method
            ));
        }
        let route_pattern = jet_http_route_pattern(pattern)?;
        if input.pattern != route_pattern {
            return Err(format!(
                "E2805: checked OpenAPI pattern does not match route pattern `{}`",
                pattern
            ));
        }
        inputs.push(input);
    }
    jet_web_openapi_from_routes(
        inputs,
        "Jet HTTP Router".to_string(),
        "1".to_string(),
        Vec::new(),
    )
}

fn jet_openapi_router_inputs(
    router: &JetHTTPRouter,
) -> Result<Vec<JetOpenApiRouteInput>, String> {
    let mut inputs = Vec::with_capacity(router.routes.len());
    for route in &router.routes {
        if route.contract_json.is_empty() {
            return Err(format!(
                "E2805: route `{} {}` has no checked OpenAPI contract",
                route.method, route.template
            ));
        }
        let input = jet_openapi_contract_input(&route.contract_json)?;
        if !input.method.as_str().eq_ignore_ascii_case(&route.method) {
            return Err(format!(
                "E2805: checked OpenAPI method `{}` does not match route method `{}`",
                input.method.as_str(),
                route.method
            ));
        }
        let route_pattern = jet_http_route_pattern(&route.template)?;
        if input.pattern != route_pattern {
            return Err(format!(
                "E2805: checked OpenAPI pattern does not match route pattern `{}`",
                route.template
            ));
        }
        inputs.push(input);
    }
    Ok(inputs)
}

/// Generate OpenAPI from HTTPRouter's checked route facts. Visual WebRouter
/// routes are intentionally not accepted: they have no HTTP contract.
pub fn jet_web_openapi(router: &JetHTTPRouter) -> String {
    let inputs = match jet_openapi_router_inputs(router) {
        Ok(inputs) => inputs,
        Err(error) => jet_runtime_stop("E2805", "", 0, &error),
    };
    match jet_web_openapi_from_routes(
        inputs,
        "Jet HTTP Router".to_string(),
        "1".to_string(),
        Vec::new(),
    ) {
        Ok(document) => document,
        Err(error) => jet_runtime_stop("E2805", "", 0, &error),
    }
}
