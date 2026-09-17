//! Canonical typed ambient host context for interpreter/deopt execution.
//!
//! The comptime interpreter owns callback storage. This module owns the one
//! adapter context that composes feature services and installs those callbacks
//! for a run. Feature modules register function pointers here; they do not
//! create their own thread-local hooks or reimplement MIR marshalling.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;

use jet_codegen::Comptime::{
    AmbientCoreCall, AmbientCoreClosureCall, AmbientExternCall, AmbientHandle,
    AmbientMirExternCall, AmbientMirHandle, AmbientMirHandleResult, DevSink,
};
use jet_codegen::embedded_hardware::{
    JetHardwareHost, JetHardwareReplayHost as SharedHardwareReplayHost,
};
use jet_foundation::AST::{CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::MIR::{
    MirConstKey, MirCoreClosureKind, MirForeign, MirPreludeCallId, MirRuntimeValue, MirSiteId,
    MirType,
    MirTypeKind,
};
use jet_foundation::TargetMachine::TargetHardwareFacts;
fn hardware_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3001",
        message.into(),
        "the interpreter hardware adapter rejected the checked operation".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

struct InterpreterHardwareHost {
    profile_id: String,
    replay: SharedHardwareReplayHost<TargetHardwareFacts>,
}

impl InterpreterHardwareHost {
    fn new(profile_id: impl Into<String>, facts: TargetHardwareFacts) -> Self {
        Self {
            profile_id: profile_id.into(),
            replay: SharedHardwareReplayHost::new(facts),
        }
    }

    fn dispatch(
        &mut self,
        operation: &str,
        handle: Option<i64>,
        args: Vec<MirRuntimeValue>,
        span: Span,
    ) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
        let malformed = |message: &str| Some(Err(hardware_diag(message, span)));
        match operation {
            "jet_hardware_setup" => {
                if handle.is_some() {
                    return malformed("interpreter hardware setup received a handle");
                }
                let [
                    MirRuntimeValue::String(profile),
                    MirRuntimeValue::String(setup_kind),
                    MirRuntimeValue::String(item),
                    MirRuntimeValue::Int(width_or_vector),
                    MirRuntimeValue::String(ownership_or_handler),
                ] = args.as_slice()
                else {
                    return malformed("interpreter hardware setup arguments do not match the checked ABI");
                };
                if profile != &self.profile_id {
                    return malformed("interpreter hardware setup profile disagrees with checked facts");
                }
                let status = self.replay.setup(
                    profile,
                    setup_kind,
                    item,
                    *width_or_vector,
                    ownership_or_handler,
                );
                if status == 0 {
                    Some(Ok(AmbientMirHandleResult::Value(MirRuntimeValue::Unit)))
                } else {
                    malformed("interpreter hardware setup replay rejected checked facts")
                }
            }
            "jet_hardware_register_read" => {
                if handle.is_some() {
                    return malformed("interpreter hardware register read received a handle");
                }
                let [
                    MirRuntimeValue::String(profile),
                    MirRuntimeValue::String(block),
                    MirRuntimeValue::String(register),
                    MirRuntimeValue::Int(width),
                ] = args.as_slice()
                else {
                    return malformed(
                        "interpreter hardware register read arguments do not match the checked ABI",
                    );
                };
                if profile != &self.profile_id {
                    return malformed(
                        "interpreter hardware register read profile disagrees with checked facts",
                    );
                }
                Some(Ok(AmbientMirHandleResult::Value(MirRuntimeValue::Int(
                    self.replay
                        .register_read(profile, block, register, *width),
                ))))
            }
            "jet_hardware_register_write" => {
                if handle.is_some() {
                    return malformed("interpreter hardware register write received a handle");
                }
                let [
                    MirRuntimeValue::String(profile),
                    MirRuntimeValue::String(block),
                    MirRuntimeValue::String(register),
                    MirRuntimeValue::Int(width),
                    MirRuntimeValue::Int(value),
                ] = args.as_slice()
                else {
                    return malformed(
                        "interpreter hardware register write arguments do not match the checked ABI",
                    );
                };
                if profile != &self.profile_id {
                    return malformed(
                        "interpreter hardware register write profile disagrees with checked facts",
                    );
                }
                let status = self
                    .replay
                    .register_write(profile, block, register, *width, *value);
                if status == 0 {
                    Some(Ok(AmbientMirHandleResult::Value(MirRuntimeValue::Unit)))
                } else {
                    malformed("interpreter hardware register write replay rejected checked facts")
                }
            }
            "jet_hardware_dma_start" => {
                if handle.is_some() {
                    return malformed("interpreter hardware DMA start received a handle");
                }
                let [
                    MirRuntimeValue::String(profile),
                    MirRuntimeValue::String(channel),
                    MirRuntimeValue::Int(address),
                    MirRuntimeValue::Int(bytes),
                ] = args.as_slice()
                else {
                    return malformed(
                        "interpreter hardware DMA start arguments do not match the checked ABI",
                    );
                };
                if profile != &self.profile_id {
                    return malformed(
                        "interpreter hardware DMA start profile disagrees with checked facts",
                    );
                }
                let (Ok(address), Ok(bytes)) = (u64::try_from(*address), u64::try_from(*bytes))
                else {
                    return malformed("interpreter hardware DMA start carrier has a negative address or length");
                };
                Some(Ok(AmbientMirHandleResult::Handle(
                    self.replay.dma_start(profile, channel, address, bytes),
                )))
            }
            "jet_hardware_dma_wait" => {
                let Some(token) = handle else {
                    return malformed("interpreter hardware DMA wait has no transfer token");
                };
                let [
                    MirRuntimeValue::String(profile),
                    MirRuntimeValue::String(channel),
                ] = args.as_slice()
                else {
                    return malformed(
                        "interpreter hardware DMA wait arguments do not match the checked ABI",
                    );
                };
                if profile != &self.profile_id {
                    return malformed(
                        "interpreter hardware DMA wait profile disagrees with checked facts",
                    );
                }
                Some(Ok(AmbientMirHandleResult::Value(MirRuntimeValue::Int(
                    self.replay.dma_wait(profile, channel, token),
                ))))
            }
            _ => None,
        }
    }
}
#[derive(Clone, Debug)]
struct InterpreterHttpRoute {
    method: String,
    pattern: String,
    parsed_pattern: crate::net_http_rt::JetHTTPRoutePattern,
    handler: MirRuntimeValue,
    handler_param_names: Vec<String>,
    contract_json: String,
}
#[derive(Clone, Debug, Default)]
struct InterpreterHttpRouter {
    routes: Vec<InterpreterHttpRoute>,
}

#[derive(Debug)]
struct InterpreterHttpPending {
    stream: TcpStream,
}

#[derive(Clone, Debug, Default)]
struct InterpreterHttpMux {
    routes: Vec<InterpreterHttpRoute>,
}

#[derive(Debug, Default)]
struct InterpreterHttpTransport {
    listeners: HashMap<i64, TcpListener>,
    pending: HashMap<i64, InterpreterHttpPending>,
    next_listener: i64,
    next_pending: i64,
}

fn http_mux_invocation(
    route: &InterpreterHttpRoute,
    request: &MirRuntimeValue,
) -> Result<MirRuntimeValue, String> {
    let (method, path, _body) = http_route_request(request)?;
    if !route.method.eq_ignore_ascii_case(method)
        || !crate::net_http_rt::jet_http_route_matches_path(&route.parsed_pattern, path)
    {
        return Err("HTTP mux route was selected for a non-matching request".to_string());
    }
    if route.handler_param_names.len() > 1 {
        return Err("HTTP mux handler accepts at most one request parameter".to_string());
    }
    let args = if route.handler_param_names.is_empty() {
        Vec::new()
    } else {
        vec![request.clone()]
    };
    Ok(MirRuntimeValue::Struct {
        type_name: "HTTPRouteInvocation".to_string(),
        fields: vec![
            ("handler".to_string(), route.handler.clone()),
            ("args".to_string(), MirRuntimeValue::List(args)),
        ],
    })
}

fn http_router_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2805",
        message.into(),
        "the interpreter HTTPRouter adapter rejected the checked route operation".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}
type HttpTree = crate::net_http_rt::jet_std::DataTree;

#[derive(Clone, Debug)]
struct InterpreterHttpParameter {
    name: String,
    location: String,
    required: bool,
    schema: HttpTree,
}

fn http_object(value: &HttpTree) -> Option<&[(String, HttpTree)]> {
    match value {
        HttpTree::Object(fields) => Some(fields.as_slice()),
        _ => None,
    }
}

fn http_field<'a>(object: &'a [(String, HttpTree)], name: &str) -> Option<&'a HttpTree> {
    object
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
}

fn http_text<'a>(object: &'a [(String, HttpTree)], name: &str) -> Option<&'a str> {
    match http_field(object, name)? {
        HttpTree::Text(value) | HttpTree::TypedText(value) => Some(value),
        _ => None,
    }
}

fn http_bool(object: &[(String, HttpTree)], name: &str) -> Option<bool> {
    match http_field(object, name)? {
        HttpTree::Bool(value) => Some(*value),
        _ => None,
    }
}

fn http_request_field<'a>(
    request: &'a MirRuntimeValue,
    name: &str,
) -> Option<&'a MirRuntimeValue> {
    match request {
        MirRuntimeValue::Struct { fields, .. } => {
            fields.iter().find_map(|(field, value)| (field == name).then_some(value))
        }
        _ => None,
    }
}

fn http_decode_query_component(value: &str) -> Result<String, String> {
    crate::net_http_rt::jet_http_route_decode_path_segment(&value.replace('+', " "))
        .map(|decoded| decoded.into_owned())
}
fn http_query_param(path: &str, name: &str) -> Result<Option<String>, String> {
    let Some(query) = path.split_once('?').map(|(_, query)| query) else {
        return Ok(None);
    };
    for pair in query.split('&') {
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = http_decode_query_component(raw_key)?;
        if key != name {
            continue;
        }
        let value = http_decode_query_component(raw_value)?;
        return Ok(Some(value));
    }
    Ok(None)
}

fn http_tree_runtime(value: &HttpTree) -> MirRuntimeValue {
    match value {
        HttpTree::Null => MirRuntimeValue::Unit,
        HttpTree::Bool(value) => MirRuntimeValue::Bool(*value),
        HttpTree::Int(value) => MirRuntimeValue::Int(*value),
        HttpTree::Float(value) => MirRuntimeValue::Float {
            value: *value,
            f32: false,
        },
        HttpTree::Number(value) => value
            .parse::<i64>()
            .map(MirRuntimeValue::Int)
            .or_else(|_| value.parse::<f64>().map(|value| MirRuntimeValue::Float {
                value,
                f32: false,
            }))
            .unwrap_or_else(|_| MirRuntimeValue::String(value.clone())),
        HttpTree::TypedText(value) | HttpTree::Text(value) => {
            MirRuntimeValue::String(value.clone())
        }
        HttpTree::Bytes(value) => MirRuntimeValue::Bytes(value.clone()),
        HttpTree::Array(values) => {
            MirRuntimeValue::List(values.iter().map(http_tree_runtime).collect())
        }
        HttpTree::Object(fields) => MirRuntimeValue::Struct {
            type_name: "HTTPBody".to_string(),
            fields: fields
                .iter()
                .map(|(name, value)| (name.clone(), http_tree_runtime(value)))
                .collect(),
        },
    }
}

fn http_schema_type<'a>(schema: &'a HttpTree) -> Option<&'a str> {
    http_object(schema).and_then(|object| http_text(object, "type"))
}

fn http_absent(schema: &HttpTree) -> MirRuntimeValue {
    let kind = match http_schema_type(schema) {
        Some("integer") => MirTypeKind::Int,
        Some("number") => MirTypeKind::Float,
        Some("boolean") => MirTypeKind::Bool,
        Some("array") => MirTypeKind::List(Box::new(MirType::from_kind(MirTypeKind::String))),
        Some("object") => MirTypeKind::Map {
            key: Box::new(MirType::from_kind(MirTypeKind::String)),
            value: Box::new(MirType::from_kind(MirTypeKind::String)),
        },
        _ => MirTypeKind::String,
    };
    MirRuntimeValue::Absent {
        element: MirType::from_kind(kind),
    }
}

fn http_decode_route_value(raw: &str, schema: &HttpTree) -> Result<MirRuntimeValue, String> {
    let Some(object) = http_object(schema) else {
        return Err("HTTP route parameter schema is not an object".to_string());
    };
    if let Some(nullable) = http_field(object, "nullable") {
        if matches!(nullable, HttpTree::Bool(true)) && raw == "null" {
            return Ok(MirRuntimeValue::Unit);
        }
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(HttpTree::Array(options)) = http_field(object, key) {
            for option in options {
                if let Ok(value) = http_decode_route_value(raw, option) {
                    return Ok(value);
                }
            }
            return Err("HTTP route parameter does not match its checked schema".to_string());
        }
    }
    let kind = http_schema_type(schema).unwrap_or("string");
    let tree = match kind {
        "string" => HttpTree::Text(raw.to_string()),
        "integer" => HttpTree::Int(
            raw.parse::<i64>()
                .map_err(|_| "HTTP route integer parameter is invalid".to_string())?,
        ),
        "number" => HttpTree::Float(
            raw.parse::<f64>()
                .map_err(|_| "HTTP route number parameter is invalid".to_string())?,
        ),
        "boolean" => HttpTree::Bool(match raw {
            "true" => true,
            "false" => false,
            _ => return Err("HTTP route boolean parameter is invalid".to_string()),
        }),
        "null" => {
            if raw != "null" {
                return Err("HTTP route null parameter is invalid".to_string());
            }
            HttpTree::Null
        }
        "array" | "object" => crate::net_http_rt::jet_std::parse_json_typed_datatree(raw)
            .map_err(|_| "HTTP route structured parameter is invalid JSON".to_string())?,
        _ => crate::net_http_rt::jet_std::parse_json_typed_datatree(raw)
            .unwrap_or_else(|_| HttpTree::Text(raw.to_string())),
    };
    if !crate::net_http_rt::jet_http_route_schema_matches(&tree, schema) {
        return Err("HTTP route parameter does not match its checked schema".to_string());
    }
    Ok(http_tree_runtime(&tree))
}

fn http_contract_parts(
    contract_json: &str,
) -> Result<(Vec<InterpreterHttpParameter>, Option<(bool, HttpTree)>), String> {
    let contract = crate::net_http_rt::jet_std::parse_json_typed_datatree(contract_json)
        .map_err(|_| "HTTP route contract is not valid JSON".to_string())?;
    let object = http_object(&contract)
        .ok_or_else(|| "HTTP route contract is not a JSON object".to_string())?;
    for field in [
        "method",
        "pattern",
        "path",
        "operation_id",
        "summary",
        "parameters",
        "request_body",
        "responses",
        "security",
        "provenance",
    ] {
        if http_field(object, field).is_none() {
            return Err("HTTP route contract is incomplete".to_string());
        }
    }
    let Some(HttpTree::Array(responses)) = http_field(object, "responses") else {
        return Err("HTTP route contract responses are not an array".to_string());
    };
    if responses.is_empty() {
        return Err("HTTP route contract has no responses".to_string());
    }
    if !matches!(http_field(object, "security"), Some(HttpTree::Array(_))) {
        return Err("HTTP route contract security is not an array".to_string());
    }
    match http_field(object, "request_body") {
        Some(HttpTree::Null) => {}
        Some(value) => {
            let body = http_object(value)
                .ok_or_else(|| "HTTP route request body is not an object".to_string())?;
            if http_bool(body, "required").is_none()
                || http_text(body, "content_type").is_none_or(|value| value.is_empty())
                || !http_field(body, "schema").is_some_and(|schema| http_object(schema).is_some())
            {
                return Err("HTTP route request body is incomplete".to_string());
            }
        }
        None => return Err("HTTP route contract has no request body field".to_string()),
    }
    let parameters = match http_field(object, "parameters") {
        Some(HttpTree::Array(items)) => items
            .iter()
            .map(|item| {
                let item = http_object(item)
                    .ok_or_else(|| "HTTP route contract parameter is not an object".to_string())?;
                let name = http_text(item, "name")
                    .ok_or_else(|| "HTTP route contract parameter has no name".to_string())?;
                let location = http_text(item, "in")
                    .ok_or_else(|| "HTTP route contract parameter has no location".to_string())?;
                let required = http_bool(item, "required")
                    .ok_or_else(|| "HTTP route contract parameter has no required flag".to_string())?;
                let schema = http_field(item, "schema")
                    .cloned()
                    .ok_or_else(|| "HTTP route contract parameter has no schema".to_string())?;
                Ok(InterpreterHttpParameter {
                    name: name.to_string(),
                    location: location.to_string(),
                    required,
                    schema,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        _ => return Err("HTTP route contract parameters are not an array".to_string()),
    };
    let request_body = match http_field(object, "request_body") {
        None | Some(HttpTree::Null) => None,
        Some(value) => {
            let body = http_object(value)
                .ok_or_else(|| "HTTP route request body is not an object".to_string())?;
            let required = http_bool(body, "required")
                .ok_or_else(|| "HTTP route request body has no required flag".to_string())?;

            let schema = http_field(body, "schema")
                .cloned()
                .ok_or_else(|| "HTTP route request body has no schema".to_string())?;
            Some((required, schema))
        }
    };
    Ok((parameters, request_body))
}

fn http_route_request(
    request: &MirRuntimeValue,
) -> Result<(&str, &str, &[u8]), String> {
    let method = match http_request_field(request, "method") {
        Some(MirRuntimeValue::String(value)) => value.as_str(),
        _ => return Err("HTTP route request has no method".to_string()),
    };
    let path = match http_request_field(request, "path") {
        Some(MirRuntimeValue::String(value)) => value.as_str(),
        _ => return Err("HTTP route request has no path".to_string()),
    };
    let body = match http_request_field(request, "body") {
        Some(MirRuntimeValue::Bytes(value)) => value.as_slice(),
        Some(MirRuntimeValue::String(value)) => value.as_bytes(),
        _ => return Err("HTTP route request has no body".to_string()),
    };
    Ok((method, path, body))
}
fn http_response(status: i64, body: &str) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "HTTPResponse".to_string(),
        fields: vec![
            ("status".to_string(), MirRuntimeValue::Int(status)),
            (
                "version".to_string(),
                MirRuntimeValue::String("HTTP/1.1".to_string()),
            ),
            (
                "headers".to_string(),
                MirRuntimeValue::Struct {
                    type_name: "HTTPHeaders".to_string(),
                    fields: Vec::new(),
                },
            ),
            (
                "body".to_string(),
                MirRuntimeValue::Struct {
                    type_name: "HTTPBody".to_string(),
                    fields: vec![(
                        "bytes".to_string(),
                        MirRuntimeValue::Bytes(body.as_bytes().to_vec()),
                    )],
                },
            ),
            (
                "trailers".to_string(),
                MirRuntimeValue::Struct {
                    type_name: "HTTPHeaders".to_string(),
                    fields: Vec::new(),
                },
            ),
            (
                "protocol".to_string(),
                MirRuntimeValue::String("HTTP/1.1".to_string()),
            ),
            (
                "remote_address".to_string(),
                MirRuntimeValue::String(String::new()),
            ),
            (
                "redirect_history".to_string(),
                MirRuntimeValue::List(Vec::new()),
            ),
            ("timings_ms".to_string(), MirRuntimeValue::List(Vec::new())),
            ("reused_connection".to_string(), MirRuntimeValue::Bool(false)),
            (
                "raw_content_encoding".to_string(),
                MirRuntimeValue::Absent {
                    element: MirType::from_kind(MirTypeKind::String),
                },
            ),
        ],
    }
}
fn http_route_error_response(error: &str) -> MirRuntimeValue {
    let body = if error.contains("request body") {
        "invalid request body"
    } else if error.contains("selected for a non-matching request") {
        "400 bad request"
    } else {
        "invalid route parameter"
    };
    http_response(400, body)
}



fn http_parse_request_carrier(raw: &str) -> MirRuntimeValue {
    let (method, path, headers, body) =
        crate::net_http_rt::jet_http_parse_request_carrier(raw);
    MirRuntimeValue::Struct {
        type_name: "HTTPRequest".to_string(),
        fields: vec![
            ("method".to_string(), MirRuntimeValue::String(method)),
            ("path".to_string(), MirRuntimeValue::String(path)),
            (
                "headers".to_string(),
                MirRuntimeValue::Map(
                    headers
                        .into_iter()
                        .map(|(key, value)| {
                            (MirConstKey::String(key), MirRuntimeValue::String(value))
                        })
                        .collect(),
                ),
            ),
            ("body".to_string(), MirRuntimeValue::Bytes(body)),
        ],
    }
}
fn http_read_one(stream: &mut TcpStream) -> Result<String, String> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end;
    loop {
        let count = stream.read(&mut chunk).map_err(|error| format!("HTTP read failed: {error}"))?;
        if count == 0 {
            return Err("HTTP request ended before headers".to_string());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            header_end = index + 4;
            break;
        }
        if bytes.len() > 32 * 1024 {
            return Err("HTTP request headers are too large".to_string());
        }
    }
    let head = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| "HTTP request headers are not UTF-8".to_string())?;
    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let total = header_end.saturating_add(content_length);
    while bytes.len() < total {
        let count = stream.read(&mut chunk).map_err(|error| format!("HTTP body read failed: {error}"))?;
        if count == 0 {
            return Err("HTTP request ended before its body".to_string());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    String::from_utf8(bytes).map_err(|_| "HTTP request is not UTF-8".to_string())
}

fn http_response_wire(response: &MirRuntimeValue) -> Result<Vec<u8>, String> {
    let status = match http_request_field(response, "status") {
        Some(MirRuntimeValue::Int(value)) => *value,
        _ => return Err("HTTP handler returned a response without status".to_string()),
    };
    let body = match http_request_field(response, "body") {
        Some(MirRuntimeValue::Struct { fields, .. }) => match http_field_runtime(fields, "bytes") {
            Some(MirRuntimeValue::Bytes(value)) => value.clone(),
            Some(MirRuntimeValue::String(value)) => value.as_bytes().to_vec(),
            _ => Vec::new(),
        },
        Some(MirRuntimeValue::Bytes(value)) => value.clone(),
        Some(MirRuntimeValue::String(value)) => value.as_bytes().to_vec(),
        _ => Vec::new(),
    };
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let mut wire = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    )
    .into_bytes();
    if let Some(MirRuntimeValue::Struct { fields, .. }) = http_request_field(response, "headers") {
        for (name, value) in fields {
            if let MirRuntimeValue::String(value) = value {
                wire.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
            }
        }
    }
    wire.extend_from_slice(b"\r\n");
    wire.extend_from_slice(&body);
    Ok(wire)
}

fn http_field_runtime<'a>(
    fields: &'a [(String, MirRuntimeValue)],
    name: &str,
) -> Option<&'a MirRuntimeValue> {
    fields.iter().find_map(|(field, value)| (field == name).then_some(value))
}

fn http_route_invocation(
    route: &InterpreterHttpRoute,
    request: &MirRuntimeValue,
) -> Result<MirRuntimeValue, String> {
    let (method, path, body) = http_route_request(request)?;
    if !route.method.eq_ignore_ascii_case(method)
        || !crate::net_http_rt::jet_http_route_matches_path(&route.parsed_pattern, path)
    {
        return Err("HTTP route was selected for a non-matching request".to_string());
    }
    let path_params =
        crate::net_http_rt::jet_http_route_params_path(&route.parsed_pattern, path)?;
    let (parameters, request_body) = http_contract_parts(&route.contract_json)?;
    if let Some((required, schema)) = request_body {
        if body.is_empty() {
            if required {
                return Err("HTTP route request body is required".to_string());
            }
        } else {
            let text = std::str::from_utf8(body)
                .map_err(|_| "HTTP route request body is not UTF-8".to_string())?;
            let tree = crate::net_http_rt::jet_std::parse_json_typed_datatree(text)
                .map_err(|_| "HTTP route request body is invalid JSON".to_string())?;
            if !crate::net_http_rt::jet_http_route_schema_matches(&tree, &schema) {
                return Err("HTTP route request body does not match its checked schema".to_string());
            }
        }
    }
    let mut args = Vec::with_capacity(route.handler_param_names.len());
    let mut request_seen = false;
    for name in &route.handler_param_names {
        let parameter = parameters.iter().find(|parameter| &parameter.name == name);
        let Some(parameter) = parameter else {
            if request_seen {
                return Err("HTTP route has more than one unbound handler parameter".to_string());
            }
            request_seen = true;
            args.push(request.clone());
            continue;
        };
        let raw = match parameter.location.as_str() {
            "path" => path_params.get(name).cloned(),
            "query" => http_query_param(path, name)?,
            _ => None,
        };
        let Some(raw) = raw else {
            if parameter.required {
                return Err(format!("HTTP route parameter `{name}` is missing"));
            }
            args.push(http_absent(&parameter.schema));
            continue;
        };
        let value = http_decode_route_value(&raw, &parameter.schema)?;
        args.push(if parameter.required {
            value
        } else {
            MirRuntimeValue::Present(Box::new(value))
        });
    }
    Ok(MirRuntimeValue::Struct {
        type_name: "HTTPRouteInvocation".to_string(),
        fields: vec![
            ("handler".to_string(), route.handler.clone()),
            ("args".to_string(), MirRuntimeValue::List(args)),
        ],
    })
}

/// One lazy interpreter line source remains associated with its cursor until
/// explicit close or ambient scope teardown. Each call reads at most one line.
enum InterpreterLineReader {
    Stdin,
}

impl InterpreterLineReader {
    fn stdin() -> Self {
        Self::Stdin
    }

    fn next(&mut self) -> Result<Option<String>, String> {
        if crate::fault_injection::jet_fault_should_fail("IO.Read") {
            return Err("fault injected: IO.Read".to_string());
        }
        crate::IO::term_prelude::jet_term_read_stdin_line()
            .map_err(|error| format!("read stdin: {error}"))
            .map(|line| match line {
                crate::IO::term_prelude::JetTermRead::Line(line) => Some(line),
                crate::IO::term_prelude::JetTermRead::EndOfInput => None,
            })
    }
}

fn line_reader_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the interpreter line-reader adapter rejected the checked operation".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

fn mir_absent_string() -> MirRuntimeValue {
    MirRuntimeValue::Absent {
        element: MirType::from_kind(MirTypeKind::String),
    }
}

/// Typed callback collection and resident typed handles for one interpreter
/// or deopt scope.
#[derive(Default)]
struct InterpreterAmbientState {
    core_calls: Vec<AmbientCoreCall>,
    core_closure_calls: Vec<AmbientCoreClosureCall>,
    handles: Vec<AmbientHandle>,
    extern_calls: Vec<AmbientExternCall>,
    mir_extern_calls: Vec<AmbientMirExternCall>,
    mir_handles: Vec<AmbientMirHandle>,
    line_readers: HashMap<i64, InterpreterLineReader>,
    next_line_reader: i64,
    http_routers: HashMap<i64, InterpreterHttpRouter>,
    next_http_router: i64,
    http_muxes: HashMap<i64, InterpreterHttpMux>,
    next_http_mux: i64,
    http_transport: InterpreterHttpTransport,
    hardware_host: Option<InterpreterHardwareHost>,
}
/// Typed callback collection and resident typed handles for one interpreter
/// or deopt scope.
#[derive(Clone, Default)]
pub struct InterpreterAmbientContext {
    state: Rc<RefCell<InterpreterAmbientState>>,
}

impl InterpreterAmbientContext {
    /// Register a typed plain Core-call adapter.
    pub fn register_core_call(&mut self, callback: AmbientCoreCall) {
        self.state.borrow_mut().core_calls.push(callback);
    }

    /// Register a typed closure-taking Core-call adapter.
    pub fn register_core_closure_call(&mut self, callback: AmbientCoreClosureCall) {
        self.state.borrow_mut().core_closure_calls.push(callback);
    }

    /// Register a typed receiver/handle adapter.
    pub fn register_handle(&mut self, callback: AmbientHandle) {
        self.state.borrow_mut().handles.push(callback);
    }

    /// Register a typed foreign-call adapter.
    pub fn register_extern(&mut self, callback: AmbientExternCall) {
        self.state.borrow_mut().extern_calls.push(callback);
    }

    /// Register a typed canonical MIR foreign adapter.
    pub fn register_mir_extern(&mut self, callback: AmbientMirExternCall) {
        self.state.borrow_mut().mir_extern_calls.push(callback);
    }

    /// Register a typed MIR handle operation adapter.
    pub fn register_mir_handle(&mut self, callback: AmbientMirHandle) {
        self.state.borrow_mut().mir_handles.push(callback);
    }

    /// Install the selected checked target profile for interpreter hardware
    /// operations. The replay host is scoped to this ambient run.
    pub fn register_hardware_host(
        &mut self,
        profile_id: impl Into<String>,
        facts: TargetHardwareFacts,
    ) {
        self.state.borrow_mut().hardware_host = Some(InterpreterHardwareHost::new(profile_id, facts));
    }

    fn core_call(&self, index: usize) -> Option<AmbientCoreCall> {
        self.state.borrow().core_calls.get(index).copied()
    }

    fn core_closure_call(&self, index: usize) -> Option<AmbientCoreClosureCall> {
        self.state.borrow().core_closure_calls.get(index).copied()
    }

    fn handle(&self, index: usize) -> Option<AmbientHandle> {
        self.state.borrow().handles.get(index).copied()
    }

    fn extern_call(&self, index: usize) -> Option<AmbientExternCall> {
        self.state.borrow().extern_calls.get(index).copied()
    }

    fn mir_extern_call(&self, index: usize) -> Option<AmbientMirExternCall> {
        self.state.borrow().mir_extern_calls.get(index).copied()
    }

    fn mir_handle(&self, index: usize) -> Option<AmbientMirHandle> {
        self.state.borrow().mir_handles.get(index).copied()
    }

    fn mir_hardware(
        &self,
        operation: &str,
        handle: Option<i64>,
        args: Vec<MirRuntimeValue>,
        span: Span,
    ) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
        let mut host = {
            let mut state = self.state.borrow_mut();
            state.hardware_host.take()?
        };
        let result = host.dispatch(operation, handle, args, span);
        self.state.borrow_mut().hardware_host = Some(host);
        result
    }
    fn mir_lines(
        &self,
        operation: &str,
        handle: Option<i64>,
        args: Vec<MirRuntimeValue>,
        span: Span,
    ) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
        if operation == "loop.lines.next" {
            let Some(handle) = handle else {
                return Some(Err(line_reader_diag(
                    "line iterator next has no typed reader handle",
                    span,
                )));
            };
            if !args.is_empty() {
                return Some(Err(line_reader_diag(
                    "line iterator next received unexpected arguments",
                    span,
                )));
            }
            let Some(mut reader) = self.state.borrow_mut().line_readers.remove(&handle) else {
                return Some(Err(line_reader_diag(
                    "line iterator next used an unknown interpreter handle",
                    span,
                )));
            };
            let result = reader.next();
            self.state.borrow_mut().line_readers.insert(handle, reader);
            return Some(match result {
                Ok(Some(line)) => Ok(AmbientMirHandleResult::Value(
                    MirRuntimeValue::Present(Box::new(MirRuntimeValue::String(line))),
                )),
                Ok(None) => Ok(AmbientMirHandleResult::Value(mir_absent_string())),
                Err(error) => Err(line_reader_diag(error, span)),
            });
        }
        self.state
            .borrow_mut()
            .mir_lines(operation, handle, args, span)
    }

    fn mir_http_router(
        &self,
        operation: &str,
        handle: Option<i64>,
        args: Vec<MirRuntimeValue>,
        span: Span,
    ) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
        if operation == "http_router.openapi" {
            let Some(handle) = handle else {
                return Some(Err(http_router_diag(
                    "HTTPRouter OpenAPI export has no typed router receiver",
                    span,
                )));
            };
            if !args.is_empty() {
                return Some(Err(http_router_diag(
                    "HTTPRouter OpenAPI export received unexpected arguments",
                    span,
                )));
            }
            let routes = {
                let state = self.state.borrow();
                let Some(router) = state.http_routers.get(&handle) else {
                    return Some(Err(http_router_diag(
                        "HTTPRouter OpenAPI export used an unknown interpreter handle",
                        span,
                    )));
                };
                router
                    .routes
                    .iter()
                    .map(|route| {
                        (
                            route.method.clone(),
                            route.pattern.clone(),
                            route.contract_json.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            return Some(match crate::Web::web_rt::jet_web_openapi_from_contracts(&routes) {
                Ok(document) => Ok(AmbientMirHandleResult::Value(
                    MirRuntimeValue::String(document),
                )),
                Err(error) => Err(http_router_diag(error, span)),
            });
        }
        self.state
            .borrow_mut()
            .mir_http_router(operation, handle, args, span)
    }

    fn mir_http_mux(
        &self,
        operation: &str,
        handle: Option<i64>,
        args: Vec<MirRuntimeValue>,
        span: Span,
    ) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
        self.state
            .borrow_mut()
            .mir_http_mux(operation, handle, args, span)
    }
}

impl InterpreterAmbientState {
    fn mir_lines(
        &mut self,
        operation: &str,
        handle: Option<i64>,
        args: Vec<MirRuntimeValue>,
        span: Span,
    ) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
        match operation {
            "loop.lines.init" => {
                if handle.is_some() {
                    return Some(Err(line_reader_diag(
                        "line iterator initializer received an unexpected receiver handle",
                        span,
                    )));
                }
                let [MirRuntimeValue::String(source)] = args.as_slice() else {
                    return Some(Err(line_reader_diag(
                        "line iterator initializer arguments do not match the checked ABI",
                        span,
                    )));
                };
                if source != "lines:stdin" {
                    return Some(Err(line_reader_diag(
                        "interpreter line iterator only supports the checked stdin source",
                        span,
                    )));
                }
                let next = if self.next_line_reader == 0 {
                    1
                } else {
                    self.next_line_reader
                };
                if self.line_readers.contains_key(&next) {
                    return Some(Err(line_reader_diag(
                        "interpreter line-reader handle space is exhausted",
                        span,
                    )));
                }
                self.next_line_reader = next.saturating_add(1);
                self.line_readers.insert(next, InterpreterLineReader::stdin());
                Some(Ok(AmbientMirHandleResult::Handle(next)))
            }
            "loop.lines.close" => {
                let Some(handle) = handle else {
                    return Some(Err(line_reader_diag(
                        "line iterator close has no typed reader handle",
                        span,
                    )));
                };
                if !args.is_empty() {
                    return Some(Err(line_reader_diag(
                        "line iterator close received unexpected arguments",
                        span,
                    )));
                }
                if self.line_readers.remove(&handle).is_none() {
                    return Some(Err(line_reader_diag(
                        "line iterator close used an unknown interpreter handle",
                        span,
                    )));
                }
                Some(Ok(AmbientMirHandleResult::Value(MirRuntimeValue::Unit)))
            }
            _ => None,
        }
    }

    fn mir_http_router(
        &mut self,
        operation: &str,
        handle: Option<i64>,
        args: Vec<MirRuntimeValue>,
        span: Span,
    ) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
        match operation {
            "http_router.new" => {
                if handle.is_some() || !args.is_empty() {
                    return Some(Err(http_router_diag(
                        "HTTPRouter constructor received unexpected handle arguments",
                        span,
                    )));
                }
                let next = if self.next_http_router == 0 {
                    1
                } else {
                    self.next_http_router
                };
                if self.http_routers.contains_key(&next) {
                    return Some(Err(http_router_diag(
                        "interpreter HTTPRouter handle space is exhausted",
                        span,
                    )));
                }
                self.next_http_router = next.saturating_add(1);
                self.http_routers
                    .insert(next, InterpreterHttpRouter::default());
                Some(Ok(AmbientMirHandleResult::Handle(next)))
            }
            "http_router.parse" => {
                if handle.is_some() {
                    return Some(Err(http_router_diag(
                        "HTTP request parsing received an unexpected router receiver",
                        span,
                    )));
                }
                let [MirRuntimeValue::String(raw)] = args.as_slice() else {
                    return Some(Err(http_router_diag(
                        "HTTP request parsing expects one raw request string",
                        span,
                    )));
                };
                Some(Ok(AmbientMirHandleResult::Value(
                    http_parse_request_carrier(raw),
                )))
            }
            "http_router.register" => {
                let Some(handle) = handle else {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration has no typed router receiver",
                        span,
                    )));
                };
                let [
                    MirRuntimeValue::String(method),
                    MirRuntimeValue::String(pattern),
                    handler,
                    MirRuntimeValue::String(_source_file),
                    MirRuntimeValue::Int(_source_line),
                    MirRuntimeValue::String(contract_json),
                    MirRuntimeValue::List(handler_param_names),
                ] = args.as_slice()
                else {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration arguments do not match the checked route ABI",
                        span,
                    )));
                };
                if !matches!(handler, MirRuntimeValue::Closure(_))
                    && !matches!(
                        handler,
                        MirRuntimeValue::Struct { type_name, fields }
                            if type_name == "__JetHttpHandler"
                                && fields.iter().any(|(name, value)| {
                                    name == "id" && matches!(value, MirRuntimeValue::Int(_))
                                })
                    )
                {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration handler is not a checked closure",
                        span,
                    )));
                }
                let names = match handler_param_names
                    .iter()
                    .map(|name| match name {
                        MirRuntimeValue::String(name) if !name.is_empty() => Ok(name.clone()),
                        _ => Err(http_router_diag(
                            "HTTPRouter registration handler names are not strings",
                            span,
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()
                {
                    Ok(names) => names,
                    Err(error) => return Some(Err(error)),
                };
                if names.iter().collect::<std::collections::HashSet<_>>().len() != names.len() {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration handler names contain duplicates",
                        span,
                    )));
                }
                let parsed_pattern = match crate::net_http_rt::jet_http_route_pattern(pattern) {
                    Ok(pattern) => pattern,
                    Err(error) => return Some(Err(http_router_diag(error, span))),
                };
                let Ok((parameters, _request_body)) = http_contract_parts(contract_json) else {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration contract is not a checked endpoint descriptor",
                        span,
                    )));
                };
                let Ok(contract) =
                    crate::net_http_rt::jet_std::parse_json_typed_datatree(contract_json)
                else {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration contract is not valid JSON",
                        span,
                    )));
                };
                let Some(contract_object) = http_object(&contract) else {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration contract is not an object",
                        span,
                    )));
                };
                if http_text(contract_object, "method") != Some(method.as_str())
                    || http_text(contract_object, "pattern") != Some(pattern.as_str())
                {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration contract disagrees with its route",
                        span,
                    )));
                }
                if parameters.iter().any(|parameter| {
                    !matches!(parameter.location.as_str(), "path" | "query")
                }) {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration contract has an unsupported parameter location",
                        span,
                    )));
                }
                if parameters
                    .iter()
                    .filter(|parameter| parameter.location == "path")
                    .any(|parameter| !names.iter().any(|name| name == &parameter.name))
                {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration is missing a typed path binding",
                        span,
                    )));
                }
                if names
                    .iter()
                    .filter(|name| !parameters.iter().any(|parameter| &parameter.name == *name))
                    .count()
                    > 1
                {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration has more than one unbound handler parameter",
                        span,
                    )));
                }
                let route_shape = crate::net_http_rt::jet_http_route_shape(&parsed_pattern);
                let Some(router) = self.http_routers.get_mut(&handle) else {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration used an unknown interpreter handle",
                        span,
                    )));
                };
                if router.routes.iter().any(|route| {
                    route.method.eq_ignore_ascii_case(method)
                        && crate::net_http_rt::jet_http_route_shape(&route.parsed_pattern)
                            == route_shape
                }) {
                    return Some(Err(http_router_diag(
                        "HTTPRouter registration duplicates an existing route",
                        span,
                    )));
                }
                router.routes.push(InterpreterHttpRoute {
                    method: method.clone(),
                    pattern: pattern.clone(),
                    parsed_pattern,
                    handler: handler.clone(),
                    handler_param_names: names,
                    contract_json: contract_json.clone(),
                });
                Some(Ok(AmbientMirHandleResult::Value(MirRuntimeValue::Unit)))
            }
            "http_router.dispatch" => {
                let Some(handle) = handle else {
                    return Some(Err(http_router_diag(
                        "HTTPRouter dispatch has no typed router receiver",
                        span,
                    )));
                };
                let [request] = args.as_slice() else {
                    return Some(Err(http_router_diag(
                        "HTTPRouter dispatch expects one HTTPRequest carrier",
                        span,
                    )));
                };
                let (method, path, _body) = match http_route_request(request) {
                    Ok(parts) => parts,
                    Err(error) => return Some(Err(http_router_diag(error, span))),
                };
                let Some(router) = self.http_routers.get(&handle) else {
                    return Some(Err(http_router_diag(
                        "HTTPRouter dispatch used an unknown interpreter handle",
                        span,
                    )));
                };
                if crate::net_http_rt::jet_http_route_validate_path(path).is_err() {
                    return Some(Ok(AmbientMirHandleResult::Value(http_response(
                        400,
                        "400 bad request",
                    ))));
                }
                let path_candidates = router
                    .routes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, route)| {
                        crate::net_http_rt::jet_http_route_matches_path(
                            &route.parsed_pattern,
                            path,
                        )
                        .then_some(index)
                    })
                    .collect::<Vec<_>>();
                if path_candidates.is_empty() {
                    return Some(Ok(AmbientMirHandleResult::Value(http_response(
                        404,
                        "404 not found",
                    ))));
                }
                let mut selected: Option<usize> = None;
                for index in path_candidates {
                    let route = &router.routes[index];
                    if !route.method.eq_ignore_ascii_case(method) {
                        continue;
                    }
                    let replace = selected.map_or(true, |current| {
                        crate::net_http_rt::jet_http_route_selection_cmp(
                            &route.parsed_pattern,
                            index,
                            &router.routes[current].parsed_pattern,
                            current,
                        ) == std::cmp::Ordering::Greater
                    });
                    if replace {
                        selected = Some(index);
                    }
                }
                let Some(index) = selected else {
                    return Some(Ok(AmbientMirHandleResult::Value(http_response(
                        405,
                        "405 method not allowed",
                    ))));
                };
                let route = &router.routes[index];
                Some(Ok(match http_route_invocation(route, request) {
                    Ok(invocation) => AmbientMirHandleResult::Value(invocation),
                    Err(error) => AmbientMirHandleResult::Value(http_route_error_response(&error)),
                }))
            }
            _ => None,
        }
    }
    fn mir_http_mux(
        &mut self,
        operation: &str,
        handle: Option<i64>,
        args: Vec<MirRuntimeValue>,
        span: Span,
    ) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
        match operation {
            "tcp_listener.new" => {
                let [MirRuntimeValue::String(address)] = args.as_slice() else {
                    return Some(Err(http_router_diag(
                        "TCP listener expects one address string",
                        span,
                    )));
                };
                let listener = match TcpListener::bind(address) {
                    Ok(listener) => listener,
                    Err(error) => return Some(Err(http_router_diag(format!("TCP listen failed: {error}"), span))),
                };
                let _ = listener.set_nonblocking(true);
                let next = if self.http_transport.next_listener == 0 {
                    1
                } else {
                    self.http_transport.next_listener
                };
                self.http_transport.next_listener = next.saturating_add(1).max(1);
                self.http_transport.listeners.insert(next, listener);
                Some(Ok(AmbientMirHandleResult::Handle(next)))
            }
            "tcp_listener.local_addr" => {
                let Some(handle) = handle else {
                    return Some(Err(http_router_diag("TCP listener address has no receiver", span)));
                };
                let Some(listener) = self.http_transport.listeners.get(&handle) else {
                    return Some(Err(http_router_diag("TCP listener address used an unknown handle", span)));
                };
                Some(Ok(AmbientMirHandleResult::Value(MirRuntimeValue::String(
                    listener.local_addr().map(|address| address.to_string()).unwrap_or_default(),
                ))))
            }
            "http_mux.serve_once" => {
                let Some(listener_id) = handle else {
                    return Some(Err(http_router_diag("HTTP serve has no listener handle", span)));
                };
                let [MirRuntimeValue::Int(mux_id)] = args.as_slice() else {
                    return Some(Err(http_router_diag("HTTP serve expects one mux handle", span)));
                };
                let listener = match self.http_transport.listeners.get(&listener_id) {
                    Some(listener) => match listener.try_clone() {
                        Ok(listener) => listener,
                        Err(error) => {
                            return Some(Err(http_router_diag(
                                format!("HTTP listener clone failed: {error}"),
                                span,
                            )))
                        }
                    },
                    None => return Some(Err(http_router_diag("HTTP serve used an unknown listener", span))),
                };
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                        Err(error) => return Some(Err(http_router_diag(format!("HTTP accept failed: {error}"), span))),
                    }
                };
                let raw = match http_read_one(&mut stream) {
                    Ok(raw) => raw,
                    Err(error) => return Some(Err(http_router_diag(error, span))),
                };
                let request = http_parse_request_carrier(&raw);
                let dispatch = self.mir_http_mux(
                    "http_mux.dispatch",
                    Some(*mux_id),
                    vec![request],
                    span,
                )?;
                match dispatch {
                    Ok(AmbientMirHandleResult::Value(MirRuntimeValue::Struct { mut fields, .. })) => {
                        let pending = if self.http_transport.next_pending == 0 {
                            1
                        } else {
                            self.http_transport.next_pending
                        };
                        self.http_transport.next_pending = pending.saturating_add(1).max(1);
                        fields.push(("__stream".to_string(), MirRuntimeValue::Int(pending)));
                        self.http_transport.pending.insert(pending, InterpreterHttpPending { stream });
                        Some(Ok(AmbientMirHandleResult::Value(MirRuntimeValue::Struct {
                            type_name: "HTTPRouteInvocation".to_string(),
                            fields,
                        })))
                    }
                    Ok(AmbientMirHandleResult::Value(response)) => {
                        let wire = http_response_wire(&response).unwrap_or_else(|_| {
                            http_response(500, "500 internal server error");
                            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
                        });
                        let _ = stream.write_all(&wire);
                        Some(Ok(AmbientMirHandleResult::Value(MirRuntimeValue::Unit)))
                    }
                    Ok(_) => Some(Err(http_router_diag("HTTP mux dispatch returned an invalid carrier", span))),
                    Err(error) => Some(Err(error)),
                }
            }
            "http_mux.respond" => {
                let Some(stream_id) = handle else {
                    return Some(Err(http_router_diag("HTTP response has no pending stream", span)));
                };
                let [response] = args.as_slice() else {
                    return Some(Err(http_router_diag("HTTP response expects one response carrier", span)));
                };
                let Some(mut pending) = self.http_transport.pending.remove(&stream_id) else {
                    return Some(Err(http_router_diag("HTTP response used an unknown stream", span)));
                };
                let wire = http_response_wire(response)
                    .map_err(|error| http_router_diag(error, span));
                match wire {
                    Ok(wire) => {
                        pending.stream.write_all(&wire).map_err(|error| http_router_diag(format!("HTTP write failed: {error}"), span)).ok();
                        Some(Ok(AmbientMirHandleResult::Value(MirRuntimeValue::Unit)))
                    }
                    Err(error) => Some(Err(error)),
                }
            }
            "http_mux.new" => {
                if handle.is_some() || !args.is_empty() {
                    return Some(Err(http_router_diag(
                        "HTTP mux constructor received unexpected arguments",
                        span,
                    )));
                }
                let next = if self.next_http_mux == 0 {
                    1
                } else {
                    self.next_http_mux
                };
                if self.http_muxes.contains_key(&next) {
                    return Some(Err(http_router_diag(
                        "interpreter HTTP mux handle space is exhausted",
                        span,
                    )));
                }
                self.next_http_mux = next.saturating_add(1);
                self.http_muxes.insert(next, InterpreterHttpMux::default());
                Some(Ok(AmbientMirHandleResult::Handle(next)))
            }
            "http_mux.register" => {
                let Some(handle) = handle else {
                    return Some(Err(http_router_diag(
                        "HTTP mux registration has no mux receiver",
                        span,
                    )));
                };
                let (method, pattern, handler, names) = match args.as_slice() {
                    [
                        MirRuntimeValue::String(method),
                        MirRuntimeValue::String(pattern),
                        handler,
                    ] => (method, pattern, handler, Vec::new()),
                    [
                        MirRuntimeValue::String(method),
                        MirRuntimeValue::String(pattern),
                        handler,
                        MirRuntimeValue::String(_source_file),
                        MirRuntimeValue::Int(_source_line),
                        MirRuntimeValue::String(_contract_json),
                        MirRuntimeValue::List(names),
                    ] => {
                        let names = match names
                            .iter()
                            .map(|name| match name {
                                MirRuntimeValue::String(name) if !name.is_empty() => {
                                    Ok(name.clone())
                                }
                                _ => Err(http_router_diag(
                                    "HTTP mux handler names are not strings",
                                    span,
                                )),
                            })
                            .collect::<Result<Vec<_>, _>>()
                        {
                            Ok(names) => names,
                            Err(error) => return Some(Err(error)),
                        };
                        (method, pattern, handler, names)
                    }
                    _ => {
                        return Some(Err(http_router_diag(
                            "HTTP mux registration arguments do not match the checked route ABI",
                            span,
                        )))
                    }
                };
                if !matches!(handler, MirRuntimeValue::Closure(_))
                    && !matches!(
                        handler,
                        MirRuntimeValue::Struct { type_name, fields }
                            if type_name == "__JetHttpHandler"
                                && fields.iter().any(|(name, value)| {
                                    name == "id" && matches!(value, MirRuntimeValue::Int(_))
                                })
                    )
                {
                    return Some(Err(http_router_diag(
                        "HTTP mux registration handler is not a checked closure",
                        span,
                    )));
                }
                if names.len() > 1 {
                    return Some(Err(http_router_diag(
                        "HTTP mux handler accepts at most one request parameter",
                        span,
                    )));
                }
                let parsed_pattern = match crate::net_http_rt::jet_http_route_pattern(pattern) {
                    Ok(pattern) => pattern,
                    Err(error) => return Some(Err(http_router_diag(error, span))),
                };
                let route_shape = crate::net_http_rt::jet_http_route_shape(&parsed_pattern);
                let Some(mux) = self.http_muxes.get_mut(&handle) else {
                    return Some(Err(http_router_diag(
                        "HTTP mux registration used an unknown interpreter handle",
                        span,
                    )));
                };
                if mux.routes.iter().any(|route| {
                    route.method.eq_ignore_ascii_case(method)
                        && crate::net_http_rt::jet_http_route_shape(&route.parsed_pattern)
                            == route_shape
                }) {
                    return Some(Err(http_router_diag(
                        "HTTP mux registration duplicates an existing route",
                        span,
                    )));
                }
                mux.routes.push(InterpreterHttpRoute {
                    method: method.clone(),
                    pattern: pattern.clone(),
                    parsed_pattern,
                    handler: handler.clone(),
                    handler_param_names: names,
                    contract_json: String::new(),
                });
                Some(Ok(AmbientMirHandleResult::Value(MirRuntimeValue::Unit)))
            }
            "http_mux.dispatch" => {
                let Some(handle) = handle else {
                    return Some(Err(http_router_diag(
                        "HTTP mux dispatch has no mux receiver",
                        span,
                    )));
                };
                let [request] = args.as_slice() else {
                    return Some(Err(http_router_diag(
                        "HTTP mux dispatch expects one HTTPRequest carrier",
                        span,
                    )));
                };
                let (method, path, _body) = match http_route_request(request) {
                    Ok(parts) => parts,
                    Err(error) => return Some(Err(http_router_diag(error, span))),
                };
                let Some(mux) = self.http_muxes.get(&handle) else {
                    return Some(Err(http_router_diag(
                        "HTTP mux dispatch used an unknown interpreter handle",
                        span,
                    )));
                };
                if crate::net_http_rt::jet_http_route_validate_path(path).is_err() {
                    return Some(Ok(AmbientMirHandleResult::Value(http_response(
                        400,
                        "400 bad request",
                    ))));
                }
                let path_candidates = mux
                    .routes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, route)| {
                        crate::net_http_rt::jet_http_route_matches_path(
                            &route.parsed_pattern,
                            path,
                        )
                        .then_some(index)
                    })
                    .collect::<Vec<_>>();
                if path_candidates.is_empty() {
                    return Some(Ok(AmbientMirHandleResult::Value(http_response(
                        404,
                        "404 not found",
                    ))));
                }
                let mut selected: Option<usize> = None;
                for index in path_candidates {
                    let route = &mux.routes[index];
                    if !route.method.eq_ignore_ascii_case(method) {
                        continue;
                    }
                    let replace = selected.map_or(true, |current| {
                        crate::net_http_rt::jet_http_route_selection_cmp(
                            &route.parsed_pattern,
                            index,
                            &mux.routes[current].parsed_pattern,
                            current,
                        ) == std::cmp::Ordering::Greater
                    });
                    if replace {
                        selected = Some(index);
                    }
                }
                let Some(index) = selected else {
                    return Some(Ok(AmbientMirHandleResult::Value(http_response(
                        405,
                        "405 method not allowed",
                    ))));
                };
                let route = &mux.routes[index];
                Some(Ok(match http_mux_invocation(route, request) {
                    Ok(invocation) => AmbientMirHandleResult::Value(invocation),
                    Err(error) => AmbientMirHandleResult::Value(http_route_error_response(&error)),
                }))
            }
            _ => None,
        }
    }
}


fn testing_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the interpreter testing adapter rejected the checked operation".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

fn testing_ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    if module != "core.testing" {
        return None;
    }
    match method {
        "golden" => {
            let [CtValue::Str(path), CtValue::Str(actual)] = args.as_slice() else {
                return Some(Err(testing_diag(
                    "core.testing.golden received malformed arguments",
                    span,
                )));
            };
            Some(Ok(CtValue::Bool(
                crate::testing_shared::jet_testing_golden_result(path, actual).is_ok_and(|value| value),
            )))
        }
        "fixture" => {
            let [CtValue::Str(path)] = args.as_slice() else {
                return Some(Err(testing_diag(
                    "core.testing.fixture received malformed arguments",
                    span,
                )));
            };
            Some(Ok(CtValue::Str(
                crate::testing_shared::jet_testing_fixture_result(path).unwrap_or_default(),
            )))
        }
        "temp_dir" => {
            let [CtValue::Str(prefix)] = args.as_slice() else {
                return Some(Err(testing_diag(
                    "core.testing.temp_dir received malformed arguments",
                    span,
                )));
            };
            Some(
                crate::testing_shared::jet_testing_temp_dir_path(prefix)
                    .map(CtValue::Str)
                    .map_err(|error| testing_diag(error.to_string(), span)),
            )
        }
        _ => None,
    }
}

/// Register the selected checked hardware profile for interpreter execution.
///
/// The profile is supplied by bundle facts, not inferred from a target name;
/// the host is dropped with the surrounding `with_interpreter_ambient` scope.
pub fn register_hardware_interpreter_ambient(
    context: &mut InterpreterAmbientContext,
    profile_id: impl Into<String>,
    facts: TargetHardwareFacts,
) {
    context.register_hardware_host(profile_id, facts);
}

thread_local! {
    static ACTIVE_CONTEXT: RefCell<Option<InterpreterAmbientContext>> = const { RefCell::new(None) };
}

struct ContextGuard {
    previous: Option<InterpreterAmbientContext>,
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        ACTIVE_CONTEXT.with(|slot| {
            slot.replace(self.previous.take());
        });
    }
}

/// Run `body` with one canonical typed ambient context installed.
///
/// The callback functions handed to `Comptime` are stable dispatch shims. They
/// borrow only this scope's context, preserving nested-run restoration and
/// keeping feature state out of global/per-feature ambient slots.
pub fn with_interpreter_ambient<R>(
    body: impl FnOnce(&mut InterpreterAmbientContext) -> R,
) -> R {
    crate::Process::with_interpreter_process_state(|| {
        let mut context = InterpreterAmbientContext::default();
        context.register_core_call(crate::Process::ambient_core_call);
        context.register_core_call(testing_ambient_core_call);
        context.register_mir_extern(crate::Ffi::ambient_mir_extern_call);
        context.register_mir_handle(crate::Process::ambient_mir_handle);
        let previous = ACTIVE_CONTEXT.with(|slot| slot.replace(Some(context.clone())));
        let _context_guard = ContextGuard { previous };
        jet_codegen::Comptime::with_ambient(
            Some(dispatch_core_call),
            Some(dispatch_handle),
            Some(dispatch_extern),
            || {
                jet_codegen::Comptime::with_ambient_core_closure(
                    Some(dispatch_core_closure),
                    || {
                        jet_codegen::Comptime::with_ambient_mir_handle(
                            Some(dispatch_mir_handle),
                            || {
                                jet_codegen::Comptime::with_ambient_mir_extern(
                                    Some(dispatch_mir_extern),
                                    || {
                                        body(&mut context)
                                    },
                                )
                            },
                        )
                    },
                )
            },
        )
    })
}
fn dispatch_core_closure(
    module: &str,
    method: &str,
    call: MirPreludeCallId,
    kind: MirCoreClosureKind,
    args: Vec<MirRuntimeValue>,
    closure: Option<MirRuntimeValue>,
    site: MirSiteId,
    label: &str,
    span: Span,
) -> Option<Result<MirRuntimeValue, Diagnostic>> {
    let mut index = 0;
    loop {
        let callback = ACTIVE_CONTEXT.with(|slot| {
            slot.borrow()
                .as_ref()
                .and_then(|context| context.core_closure_call(index))
        });
        let Some(callback) = callback else {
            break;
        };
        if let Some(result) = callback(
            module,
            method,
            call,
            kind.clone(),
            args.clone(),
            closure.clone(),
            site,
            label,
            span,
        ) {
            return Some(result);
        }
        index += 1;
    }
    None
}
fn dispatch_mir_handle(
    operation: &str,
    handle: Option<i64>,
    args: Vec<MirRuntimeValue>,
    span: Span,
) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
    let hardware = ACTIVE_CONTEXT
        .with(|slot| slot.borrow().as_ref().cloned())
        .and_then(|context| context.mir_hardware(operation, handle, args.clone(), span));
    if hardware.is_some() {
        return hardware;
    }
    let lines = ACTIVE_CONTEXT
        .with(|slot| slot.borrow().as_ref().cloned())
        .and_then(|context| context.mir_lines(operation, handle, args.clone(), span));
    if lines.is_some() {
        return lines;
    }
    if matches!(
        operation,
        "http_router.new"
            | "http_router.register"
            | "http_router.openapi"
            | "http_router.dispatch"
            | "http_router.parse"
    ) {
        return ACTIVE_CONTEXT
            .with(|slot| slot.borrow().as_ref().cloned())
            .and_then(|context| context.mir_http_router(operation, handle, args, span));
    }
    if matches!(
        operation,
        "http_mux.new"
            | "http_mux.register"
            | "http_mux.dispatch"
            | "http_mux.serve_once"
            | "http_mux.respond"
            | "tcp_listener.new"
            | "tcp_listener.local_addr"
    ) {
        return ACTIVE_CONTEXT
            .with(|slot| slot.borrow().as_ref().cloned())
            .and_then(|context| context.mir_http_mux(operation, handle, args, span));
    }
    let mut index = 0;
    loop {
        let callback = ACTIVE_CONTEXT.with(|slot| {
            slot.borrow()
                .as_ref()
                .and_then(|context| context.mir_handle(index))
        });
        let Some(callback) = callback else {
            break;
        };
        if let Some(result) = callback(operation, handle, args.clone(), span) {
            return Some(result);
        }
        index += 1;
    }
    None
}


fn dispatch_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    resolved_ret: Option<Type>,
    mut sink: Option<&mut DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    let mut index = 0;
    loop {
        let callback = ACTIVE_CONTEXT.with(|slot| {
            slot.borrow()
                .as_ref()
                .and_then(|context| context.core_call(index))
        });
        let Some(callback) = callback else {
            break;
        };
        if let Some(result) = callback(
            module,
            method,
            args.clone(),
            span,
            resolved_ret.clone(),
            sink.as_deref_mut(),
        ) {
            return Some(result);
        }
        index += 1;
    }
    None
}

fn dispatch_handle(
    operation: &str,
    receiver: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let mut index = 0;
    loop {
        let callback = ACTIVE_CONTEXT.with(|slot| {
            slot.borrow()
                .as_ref()
                .and_then(|context| context.handle(index))
        });
        let Some(callback) = callback else {
            break;
        };
        if let Some(result) = callback(operation, receiver, args, span) {
            return Some(result);
        }
        index += 1;
    }
    None
}

fn dispatch_extern(
    symbol: &str,
    args: Vec<CtValue>,
    span: Span,
    resolved_ret: Option<Type>,
) -> Option<Result<CtValue, Diagnostic>> {
    let mut index = 0;
    loop {
        let callback = ACTIVE_CONTEXT.with(|slot| {
            slot.borrow()
                .as_ref()
                .and_then(|context| context.extern_call(index))
        });
        let Some(callback) = callback else {
            break;
        };
        if let Some(result) = callback(symbol, args.clone(), span, resolved_ret.clone()) {
            return Some(result);
        }
        index += 1;
    }
    None
}

fn dispatch_mir_extern(
    foreign: &MirForeign,
    args: Vec<MirRuntimeValue>,
    span: Span,
) -> Option<Result<MirRuntimeValue, Diagnostic>> {
    let mut index = 0;
    loop {
        let callback = ACTIVE_CONTEXT.with(|slot| {
            slot.borrow()
                .as_ref()
                .and_then(|context| context.mir_extern_call(index))
        });
        let Some(callback) = callback else {
            break;
        };
        if let Some(result) = callback(foreign, args.clone(), span) {
            return Some(result);
        }
        index += 1;
    }
    None
}
