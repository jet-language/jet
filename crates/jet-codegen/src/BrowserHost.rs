//! Tier-0 adapter for the canonical generated Browser runtime (#772).
//!
//! The included files are also emitted verbatim for AOT. This module adds only
//! the CtValue handle table needed by the structured TIR evaluator.
#![allow(dead_code)]

use crate::AST::Type;
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
    pub struct JSONError {
        pub line: i64,
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum JSON {
        Null,
        Boolean(bool),
        Number(f64),
        Text(String),
        Array(Vec<JSON>),
        Object(std::collections::BTreeMap<String, JSON>),
    }

    include!("Prelude/CoreLib/JetStd/JSONCodec.rs");
}

include!("Prelude/CoreLib/Top/WsClient.rs");
include!("Prelude/CoreLib/Top/Browser.rs");

#[derive(Clone)]
enum BrowserHostValue {
    Browser(JetBrowser),
    Context(JetBrowserContext),
    Page(JetBrowserPage),
    Frame(JetBrowserFrame),
    Locator(JetBrowserLocator),
    Intercept(JetBrowserIntercept),
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

fn int_arg(args: &[CtValue], index: usize, span: Span) -> Result<i64, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Int(value)) => Ok(*value),
        _ => Err(host_error("Int argument", span)),
    }
}

fn event_ct(event: JetBrowserEvent) -> CtValue {
    CtValue::Struct {
        type_name: "BrowserEvent".to_string(),
        fields: vec![
            ("method".to_string(), CtValue::Str(event.method)),
            ("request_id".to_string(), CtValue::Str(event.request_id)),
            (
                "request_method".to_string(),
                CtValue::Str(event.request_method),
            ),
            ("url_hash".to_string(), CtValue::Str(event.url_hash)),
            ("is_blocked".to_string(), CtValue::Bool(event.is_blocked)),
            ("status_code".to_string(), CtValue::Int(event.status_code)),
            ("download_id".to_string(), CtValue::Str(event.download_id)),
            (
                "suggested_filename_hash".to_string(),
                CtValue::Str(event.suggested_filename_hash),
            ),
        ],
    }
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

fn locked_ct(value: JetBrowserLocked) -> CtValue {
    CtValue::Struct {
        type_name: "BrowserLocked".to_string(),
        fields: vec![
            ("engine".to_string(), CtValue::Str(value.engine)),
            ("version".to_string(), CtValue::Str(value.version)),
            ("binary".to_string(), CtValue::Str(value.binary)),
            ("protocol".to_string(), CtValue::Str(value.protocol)),
            ("size".to_string(), CtValue::Int(value.size)),
            ("output_hash".to_string(), CtValue::Str(value.output_hash)),
        ],
    }
}

fn locked_value(value: &CtValue, span: Span) -> Result<JetBrowserLocked, Diagnostic> {
    match (
        field(value, "BrowserLocked", "engine"),
        field(value, "BrowserLocked", "version"),
        field(value, "BrowserLocked", "binary"),
        field(value, "BrowserLocked", "protocol"),
        field(value, "BrowserLocked", "size"),
        field(value, "BrowserLocked", "output_hash"),
    ) {
        (
            Some(CtValue::Str(engine)),
            Some(CtValue::Str(version)),
            Some(CtValue::Str(binary)),
            Some(CtValue::Str(protocol)),
            Some(CtValue::Int(size)),
            Some(CtValue::Str(output_hash)),
        ) => Ok(JetBrowserLocked {
            engine: engine.clone(),
            version: version.clone(),
            binary: binary.clone(),
            protocol: protocol.clone(),
            size: *size,
            output_hash: output_hash.clone(),
        }),
        _ => Err(host_error("locked", span)),
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
        "locked" => result(jet_browser_locked(&string_arg(&args, 0, span)?), locked_ct),
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
                event_ct,
            ),
            (BrowserHostValue::Browser(browser), "add_intercept") => result(
                jet_browser_add_intercept(browser, &string_arg(args, 0, span)?),
                |intercept| store("BrowserIntercept", BrowserHostValue::Intercept(intercept)),
            ),
            (BrowserHostValue::Browser(browser), "add_intercept_url") => result(
                jet_browser_add_intercept_url(
                    browser,
                    &string_arg(args, 0, span)?,
                    &string_arg(args, 1, span)?,
                ),
                |intercept| store("BrowserIntercept", BrowserHostValue::Intercept(intercept)),
            ),
            (BrowserHostValue::Browser(browser), "continue_request") => result(
                jet_browser_continue_request(browser, &string_arg(args, 0, span)?),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Browser(browser), "fail_request") => result(
                jet_browser_fail_request(browser, &string_arg(args, 0, span)?),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Browser(browser), "fulfill_request") => result(
                jet_browser_fulfill_request(
                    browser,
                    &string_arg(args, 0, span)?,
                    int_arg(args, 1, span)?,
                    &string_arg(args, 2, span)?,
                ),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Browser(browser), "allow_downloads") => result(
                jet_browser_allow_downloads(browser, &string_arg(args, 0, span)?),
                |_| CtValue::Unit,
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
            (BrowserHostValue::Context(context), "tab") => result(
                jet_browser_context_tab(context),
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
            (BrowserHostValue::Page(page), "get_by_text") => store(
                "BrowserLocator",
                BrowserHostValue::Locator(jet_browser_page_get_by_text(
                    page,
                    &string_arg(args, 0, span)?,
                )),
            ),
            (BrowserHostValue::Page(page), "get_by_label") => store(
                "BrowserLocator",
                BrowserHostValue::Locator(jet_browser_page_get_by_label(
                    page,
                    &string_arg(args, 0, span)?,
                )),
            ),
            (BrowserHostValue::Page(page), "get_by_placeholder") => store(
                "BrowserLocator",
                BrowserHostValue::Locator(jet_browser_page_get_by_placeholder(
                    page,
                    &string_arg(args, 0, span)?,
                )),
            ),
            (BrowserHostValue::Page(page), "get_by_test_id") => store(
                "BrowserLocator",
                BrowserHostValue::Locator(jet_browser_page_get_by_test_id(
                    page,
                    &string_arg(args, 0, span)?,
                )),
            ),
            (BrowserHostValue::Page(page), "get_by_css") => store(
                "BrowserLocator",
                BrowserHostValue::Locator(jet_browser_page_get_by_css(
                    page,
                    &string_arg(args, 0, span)?,
                )),
            ),
            (BrowserHostValue::Page(page), "main_frame") => result(
                jet_browser_page_main_frame(page),
                |frame| store("BrowserFrame", BrowserHostValue::Frame(frame)),
            ),
            (BrowserHostValue::Page(page), "frames") => result(
                jet_browser_page_frames(page),
                |frames| {
                    CtValue::List(
                        frames
                            .into_iter()
                            .map(|frame| store("BrowserFrame", BrowserHostValue::Frame(frame)))
                            .collect(),
                    )
                },
            ),
            (BrowserHostValue::Page(page), "close") => {
                result(jet_browser_page_close(page), |_| CtValue::Unit)
            }
            (BrowserHostValue::Page(page), "screenshot") => {
                result(jet_browser_page_screenshot(page), CtValue::Str)
            }
            (BrowserHostValue::Page(page), "pdf") => {
                result(jet_browser_page_pdf(page), CtValue::Str)
            }
            (BrowserHostValue::Page(page), "set_cookie") => result(
                jet_browser_page_set_cookie(
                    page,
                    &string_arg(args, 0, span)?,
                    &string_arg(args, 1, span)?,
                    &string_arg(args, 2, span)?,
                ),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Page(page), "cookie") => result(
                jet_browser_page_cookie(page, &string_arg(args, 0, span)?),
                |value| match value {
                    Some(text) => CtValue::Some(Box::new(CtValue::Str(text))),
                    None => CtValue::None(Type::String),
                },
            ),
            (BrowserHostValue::Page(page), "clear_cookies") => {
                result(jet_browser_page_clear_cookies(page), |_| CtValue::Unit)
            }
            (BrowserHostValue::Page(page), "storage_get") => result(
                jet_browser_page_storage_get(
                    page,
                    &string_arg(args, 0, span)?,
                    &string_arg(args, 1, span)?,
                ),
                |value| match value {
                    Some(text) => CtValue::Some(Box::new(CtValue::Str(text))),
                    None => CtValue::None(Type::String),
                },
            ),
            (BrowserHostValue::Page(page), "storage_set") => result(
                jet_browser_page_storage_set(
                    page,
                    &string_arg(args, 0, span)?,
                    &string_arg(args, 1, span)?,
                    &string_arg(args, 2, span)?,
                ),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Page(page), "storage_clear") => result(
                jet_browser_page_storage_clear(page, &string_arg(args, 0, span)?),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Frame(frame), "close") => {
                result(jet_browser_frame_close(frame), |_| CtValue::Unit)
            }
            (BrowserHostValue::Locator(locator), "wait") => result(
                jet_browser_locator_wait(
                    locator,
                    timeout_value(args.first().ok_or_else(|| host_error("timeout", span))?, span)?,
                ),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Locator(locator), "wait_gone") => result(
                jet_browser_locator_wait_gone(
                    locator,
                    timeout_value(args.first().ok_or_else(|| host_error("timeout", span))?, span)?,
                ),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Locator(locator), "click") => {
                result(jet_browser_locator_click(locator), |_| CtValue::Unit)
            }
            (BrowserHostValue::Locator(locator), "hover") => {
                result(jet_browser_locator_hover(locator), |_| CtValue::Unit)
            }
            (BrowserHostValue::Locator(locator), "fill") => result(
                jet_browser_locator_fill(locator, &string_arg(args, 0, span)?),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Locator(locator), "press") => result(
                jet_browser_locator_press(locator, &string_arg(args, 0, span)?),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Locator(locator), "set_files") => result(
                jet_browser_locator_set_files(locator, &string_arg(args, 0, span)?),
                |_| CtValue::Unit,
            ),
            (BrowserHostValue::Intercept(intercept), "remove") => {
                result(jet_browser_intercept_remove(intercept), |_| CtValue::Unit)
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
        ("BrowserEvent", "request_id" | "request_method" | "url_hash" | "download_id"
            | "suggested_filename_hash") => field(recv, kind, method).cloned(),
        ("BrowserEvent", "is_blocked") => field(recv, kind, "is_blocked").cloned(),
        ("BrowserEvent", "status_code") => field(recv, kind, "status_code").cloned(),
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
        ("BrowserLocked", "engine" | "version" | "binary" | "protocol") => {
            field(recv, kind, method).cloned()
        }
        ("BrowserLocked", "verify") => {
            let locked = locked_value(recv, span)?;
            Some(result(jet_browser_locked_verify(&locked), |_| CtValue::Unit))
        }
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
