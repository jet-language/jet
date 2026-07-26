//! Tier-0 adapter for the canonical generated Browser runtime (#772).
//!
//! The included files are also emitted verbatim for AOT. This module adds only
//! the CtValue handle table needed by the structured TIR evaluator.
#![allow(dead_code)]

use crate::Comptime::CtValue;
use crate::Diagnostics::{Diagnostic, Span};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

trait JetShow {
    fn jet_show(&self) -> String;
}

fn jet_deadline_remaining_ms() -> Option<i64> {
    None
}

mod jet_std {
    #[derive(Clone, Debug, PartialEq)]
    pub enum Json {
        Null,
        Boolean(bool),
        Number(f64),
        Text(String),
        Array(Vec<Json>),
        Object(std::collections::BTreeMap<String, Json>),
    }

    #[derive(Clone, Debug)]
    pub struct JsonError;

    pub fn parse_json_strict(text: &str) -> Result<Json, JsonError> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            pos: 0,
        };
        let value = parser.value()?;
        parser.ws();
        (parser.pos == parser.bytes.len())
            .then_some(value)
            .ok_or(JsonError)
    }

    pub fn render_json(value: &Json, _pretty: bool, _depth: usize) -> String {
        match value {
            Json::Null => "null".to_string(),
            Json::Boolean(value) => value.to_string(),
            Json::Number(value) => format!("{value:?}"),
            Json::Text(value) => quote(value),
            Json::Array(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(|value| render_json(value, false, 0))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Json::Object(fields) => format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(name, value)| {
                        format!("{}:{}", quote(name), render_json(value, false, 0))
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }

    fn quote(text: &str) -> String {
        let mut out = String::from("\"");
        for ch in text.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\u{0008}' => out.push_str("\\b"),
                '\u{000c}' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                ch if ch < '\u{0020}' => out.push_str(&format!("\\u{:04x}", ch as u32)),
                ch => out.push(ch),
            }
        }
        out.push('"');
        out
    }

    struct Parser<'a> {
        bytes: &'a [u8],
        pos: usize,
    }

    impl Parser<'_> {
        fn ws(&mut self) {
            while self
                .bytes
                .get(self.pos)
                .is_some_and(|byte| byte.is_ascii_whitespace())
            {
                self.pos += 1;
            }
        }

        fn value(&mut self) -> Result<Json, JsonError> {
            self.ws();
            match self.bytes.get(self.pos).copied() {
                Some(b'n') => self.literal(b"null", Json::Null),
                Some(b't') => self.literal(b"true", Json::Boolean(true)),
                Some(b'f') => self.literal(b"false", Json::Boolean(false)),
                Some(b'"') => self.string().map(Json::Text),
                Some(b'[') => self.array(),
                Some(b'{') => self.object(),
                Some(b'-' | b'0'..=b'9') => self.number(),
                _ => Err(JsonError),
            }
        }

        fn literal(&mut self, text: &[u8], value: Json) -> Result<Json, JsonError> {
            if self.bytes.get(self.pos..self.pos + text.len()) == Some(text) {
                self.pos += text.len();
                Ok(value)
            } else {
                Err(JsonError)
            }
        }

        fn string(&mut self) -> Result<String, JsonError> {
            if self.bytes.get(self.pos) != Some(&b'"') {
                return Err(JsonError);
            }
            self.pos += 1;
            let mut out = String::new();
            while let Some(byte) = self.bytes.get(self.pos).copied() {
                self.pos += 1;
                match byte {
                    b'"' => return Ok(out),
                    b'\\' => {
                        let escaped = self.bytes.get(self.pos).copied().ok_or(JsonError)?;
                        self.pos += 1;
                        match escaped {
                            b'"' => out.push('"'),
                            b'\\' => out.push('\\'),
                            b'/' => out.push('/'),
                            b'b' => out.push('\u{0008}'),
                            b'f' => out.push('\u{000c}'),
                            b'n' => out.push('\n'),
                            b'r' => out.push('\r'),
                            b't' => out.push('\t'),
                            b'u' => {
                                let end = self.pos.checked_add(4).ok_or(JsonError)?;
                                let digits = std::str::from_utf8(
                                    self.bytes.get(self.pos..end).ok_or(JsonError)?,
                                )
                                .map_err(|_| JsonError)?;
                                let code =
                                    u32::from_str_radix(digits, 16).map_err(|_| JsonError)?;
                                out.push(char::from_u32(code).ok_or(JsonError)?);
                                self.pos = end;
                            }
                            _ => return Err(JsonError),
                        }
                    }
                    0..=31 => return Err(JsonError),
                    _ => {
                        let start = self.pos - 1;
                        let text =
                            std::str::from_utf8(&self.bytes[start..]).map_err(|_| JsonError)?;
                        let ch = text.chars().next().ok_or(JsonError)?;
                        out.push(ch);
                        self.pos = start + ch.len_utf8();
                    }
                }
            }
            Err(JsonError)
        }

        fn number(&mut self) -> Result<Json, JsonError> {
            let start = self.pos;
            while self.bytes.get(self.pos).is_some_and(|byte| {
                matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
            }) {
                self.pos += 1;
            }
            let text =
                std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| JsonError)?;
            let value = text.parse::<f64>().map_err(|_| JsonError)?;
            if !value.is_finite() {
                return Err(JsonError);
            }
            Ok(Json::Number(value))
        }

        fn array(&mut self) -> Result<Json, JsonError> {
            self.pos += 1;
            let mut values = Vec::new();
            loop {
                self.ws();
                if self.bytes.get(self.pos) == Some(&b']') {
                    self.pos += 1;
                    return Ok(Json::Array(values));
                }
                values.push(self.value()?);
                self.ws();
                match self.bytes.get(self.pos) {
                    Some(b',') => self.pos += 1,
                    Some(b']') => {}
                    _ => return Err(JsonError),
                }
            }
        }

        fn object(&mut self) -> Result<Json, JsonError> {
            self.pos += 1;
            let mut fields = std::collections::BTreeMap::new();
            loop {
                self.ws();
                if self.bytes.get(self.pos) == Some(&b'}') {
                    self.pos += 1;
                    return Ok(Json::Object(fields));
                }
                let name = self.string()?;
                self.ws();
                if self.bytes.get(self.pos) != Some(&b':') {
                    return Err(JsonError);
                }
                self.pos += 1;
                let value = self.value()?;
                if fields.insert(name, value).is_some() {
                    return Err(JsonError);
                }
                self.ws();
                match self.bytes.get(self.pos) {
                    Some(b',') => self.pos += 1,
                    Some(b'}') => {}
                    _ => return Err(JsonError),
                }
            }
        }
    }
}

include!("Prelude/CoreLib/Top/WsClient.rs");
include!("Prelude/CoreLib/Top/Browser.rs");

#[derive(Clone)]
enum BrowserHostValue {
    Browser(JetBrowser),
    Context(JetBrowserContext),
    Page(JetBrowserPage),
    Locator(JetBrowserLocator),
    Protocol(JetBrowserProtocol),
}

thread_local! {
    static VALUES: RefCell<HashMap<i64, BrowserHostValue>> = RefCell::new(HashMap::new());
    static NEXT_VALUE: Cell<i64> = const { Cell::new(1) };
}

fn browser_error(error: JetBrowserError) -> CtValue {
    CtValue::Struct {
        type_name: "BrowserError".to_string(),
        fields: vec![("kind".to_string(), CtValue::Str(error.kind.to_string()))],
    }
}

fn result<T>(value: Result<T, JetBrowserError>, map: impl FnOnce(T) -> CtValue) -> CtValue {
    match value {
        Ok(value) => CtValue::ResOk(Box::new(map(value))),
        Err(error) => CtValue::ResErr(Box::new(browser_error(error))),
    }
}

fn store(type_name: &str, value: BrowserHostValue) -> CtValue {
    let id = NEXT_VALUE.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    });
    VALUES.with(|values| {
        values.borrow_mut().insert(id, value);
    });
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![("__browser_host_id".to_string(), CtValue::Int(id))],
    }
}

fn handle_id(value: &CtValue, type_name: &str, span: Span) -> Result<i64, Diagnostic> {
    let CtValue::Struct {
        type_name: actual,
        fields,
    } = value
    else {
        return Err(host_error(type_name, span));
    };
    if actual != type_name {
        return Err(host_error(type_name, span));
    }
    fields
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("__browser_host_id", CtValue::Int(id)) => Some(*id),
            _ => None,
        })
        .ok_or_else(|| host_error(type_name, span))
}

fn field<'a>(value: &'a CtValue, type_name: &str, name: &str) -> Option<&'a CtValue> {
    match value {
        CtValue::Struct {
            type_name: actual,
            fields,
        } if actual == type_name => fields
            .iter()
            .find_map(|(field, value)| (field == name).then_some(value)),
        _ => None,
    }
}

fn host_error(what: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        format!("Browser {what} can't run in tier 0"),
        "the Browser value did not come from the active tier-0 session".to_string(),
        "create and use the Browser value in one `jet dev` iteration".to_string(),
        Some(span),
    )
}

fn string_arg(args: &[CtValue], index: usize, span: Span) -> Result<String, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Str(value)) => Ok(value.clone()),
        _ => Err(host_error("String argument", span)),
    }
}

fn timeout_value(value: &CtValue, span: Span) -> Result<JetBrowserTimeout, Diagnostic> {
    match field(value, "BrowserTimeout", "milliseconds") {
        Some(CtValue::Int(milliseconds)) => Ok(JetBrowserTimeout {
            milliseconds: *milliseconds,
        }),
        _ => Err(host_error("timeout", span)),
    }
}

fn profile_value(value: &CtValue, span: Span) -> Result<JetBrowserProfile, Diagnostic> {
    match (
        field(value, "BrowserProfile", "name"),
        field(value, "BrowserProfile", "version"),
    ) {
        (Some(CtValue::Str(name)), Some(CtValue::Str(version))) => Ok(JetBrowserProfile {
            name: name.clone(),
            version: version.clone(),
        }),
        _ => Err(host_error("profile", span)),
    }
}

fn profile_ct(value: JetBrowserProfile) -> CtValue {
    CtValue::Struct {
        type_name: "BrowserProfile".to_string(),
        fields: vec![
            ("name".to_string(), CtValue::Str(value.name)),
            ("version".to_string(), CtValue::Str(value.version)),
        ],
    }
}

fn timeout_ct(value: JetBrowserTimeout) -> CtValue {
    CtValue::Struct {
        type_name: "BrowserTimeout".to_string(),
        fields: vec![("milliseconds".to_string(), CtValue::Int(value.milliseconds))],
    }
}

pub(crate) fn eval_core_call(
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    Ok(match method {
        "profile" => result(
            jet_browser_profile(&string_arg(&args, 0, span)?),
            profile_ct,
        ),
        "timeout" => {
            let milliseconds = match args.first() {
                Some(CtValue::Int(value)) => *value,
                _ => return Err(host_error("timeout argument", span)),
            };
            result(jet_browser_timeout(milliseconds), timeout_ct)
        }
        "connect" => result(
            jet_browser_connect(&string_arg(&args, 0, span)?),
            |browser| store("Browser", BrowserHostValue::Browser(browser)),
        ),
        "connect_profile" => {
            let endpoint = string_arg(&args, 0, span)?;
            let profile = profile_value(args.get(1).ok_or_else(|| host_error("profile", span))?, span)?;
            let timeout = timeout_value(args.get(2).ok_or_else(|| host_error("timeout", span))?, span)?;
            result(
                jet_browser_connect_profile(&endpoint, &profile, timeout),
                |browser| store("Browser", BrowserHostValue::Browser(browser)),
            )
        }
        _ => return Err(host_error(method, span)),
    })
}

pub(crate) fn eval_method(
    kind: &str,
    method: &str,
    recv: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let id = handle_id(recv, kind, span)?;
    let value = VALUES
        .with(|values| values.borrow().get(&id).cloned())
        .ok_or_else(|| host_error(kind, span))?;
    Ok(match (&value, method) {
            (BrowserHostValue::Browser(browser), "capabilities") => {
                let caps = jet_browser_capabilities(browser);
                CtValue::Struct {
                    type_name: "BrowserCapabilities".to_string(),
                    fields: vec![
                        ("bidi".to_string(), CtValue::Bool(caps.bidi)),
                        ("cdp".to_string(), CtValue::Bool(caps.cdp)),
                        ("profile".to_string(), CtValue::Str(caps.profile)),
                    ],
                }
            }
            (BrowserHostValue::Browser(browser), "context") => result(
                jet_browser_context(browser),
                |context| store("BrowserContext", BrowserHostValue::Context(context)),
            ),
            (BrowserHostValue::Browser(browser), "subscribe") => result(
                jet_browser_subscribe(browser, &string_arg(args, 0, span)?),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Browser(browser), "next_event") => result(
                jet_browser_next_event(
                    browser,
                    timeout_value(args.first().ok_or_else(|| host_error("timeout", span))?, span)?,
                ),
                |event| CtValue::Struct {
                    type_name: "BrowserEvent".to_string(),
                    fields: vec![("method".to_string(), CtValue::Str(event.method))],
                },
            ),
            (BrowserHostValue::Browser(browser), "protocol") => result(
                jet_browser_protocol(browser, &string_arg(args, 0, span)?),
                |protocol| store("BrowserProtocol", BrowserHostValue::Protocol(protocol)),
            ),
            (BrowserHostValue::Browser(browser), "trace") => {
                let trace = jet_browser_trace(browser);
                CtValue::Struct {
                    type_name: "BrowserTrace".to_string(),
                    fields: vec![(
                        "entries".to_string(),
                        CtValue::List(trace.entries.into_iter().map(CtValue::Str).collect()),
                    )],
                }
            }
            (BrowserHostValue::Browser(browser), "close") => {
                result(jet_browser_close(browser), |_| CtValue::Unit)
            }
            (BrowserHostValue::Context(context), "page") => result(
                jet_browser_context_page(context),
                |page| store("BrowserPage", BrowserHostValue::Page(page)),
            ),
            (BrowserHostValue::Context(context), "close") => {
                result(jet_browser_context_close(context), |_| CtValue::Unit)
            }
            (BrowserHostValue::Page(page), "goto") => result(
                jet_browser_page_goto(page, &string_arg(args, 0, span)?),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Page(page), "get_by_role") => store(
                "BrowserLocator",
                BrowserHostValue::Locator(jet_browser_page_get_by_role(
                    page,
                    &string_arg(args, 0, span)?,
                    &string_arg(args, 1, span)?,
                )),
            ),
            (BrowserHostValue::Page(page), "close") => {
                result(jet_browser_page_close(page), |_| CtValue::Unit)
            }
            (BrowserHostValue::Locator(locator), "wait") => result(
                jet_browser_locator_wait(
                    locator,
                    timeout_value(args.first().ok_or_else(|| host_error("timeout", span))?, span)?,
                ),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Locator(locator), "click") => {
                result(jet_browser_locator_click(locator), |_| CtValue::Unit)
            }
            (BrowserHostValue::Protocol(protocol), "send") => result(
                jet_browser_protocol_send(
                    protocol,
                    &string_arg(args, 0, span)?,
                    &string_arg(args, 1, span)?,
                ),
                CtValue::Str,
            ),
            _ => return Err(host_error(method, span)),
    })
}

pub(crate) fn eval_value_method(
    kind: &str,
    method: &str,
    recv: &CtValue,
    span: Span,
) -> Result<Option<CtValue>, Diagnostic> {
    let value = match (kind, method) {
        ("BrowserEvent", "kind") => field(recv, kind, "method").cloned(),
        ("BrowserCapabilities", "bidi" | "cdp" | "profile") => {
            field(recv, kind, method).cloned()
        }
        ("BrowserTrace", "entry_count") => field(recv, kind, "entries").and_then(|value| {
            if let CtValue::List(entries) = value {
                Some(CtValue::Int(entries.len() as i64))
            } else {
                None
            }
        }),
        ("BrowserTrace", "summary") => field(recv, kind, "entries").and_then(|value| {
            if let CtValue::List(entries) = value {
                Some(CtValue::Str(
                    entries
                        .iter()
                        .filter_map(|entry| match entry {
                            CtValue::Str(entry) => Some(entry.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(","),
                ))
            } else {
                None
            }
        }),
        ("BrowserTrace", "redacted") => field(recv, kind, "entries").and_then(|value| {
            let CtValue::List(entries) = value else {
                return None;
            };
            let entries = entries
                .iter()
                .map(|entry| match entry {
                    CtValue::Str(entry) => Some(entry.clone()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            Some(CtValue::Bool(jet_browser_trace_redacted(&JetBrowserTrace {
                entries,
            })))
        }),
        _ => None,
    };
    if value.is_none() {
        return Err(host_error(method, span));
    }
    Ok(value)
}

pub(crate) fn clear() {
    VALUES.with(|values| values.borrow_mut().clear());
    NEXT_VALUE.with(|next| next.set(1));
}
