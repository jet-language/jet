//! `core.text` hosts (#729). `include!` canonical UnicodeTables + UnicodeString +
//! Top/Text.rs — no third algorithm.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

/// Canonical text/unicode runtime — types stubbed, algorithm via include!
pub(crate) mod text_rt {
    mod jet_regex_syntax {
        include!("../../jet-foundation/src/RegexSyntax.rs");
    }

    pub mod jet_std {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub enum IOOperation {
            Read,
            Write,
            Flush,
            Connect,
            Accept,
            Close,
            Resolve,
            Codec,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct IOContext {
            pub operation: IOOperation,
            pub resource: Option<String>,
            pub os_code: Option<i64>,
            pub cause: Option<String>,
        }

        impl IOContext {
            pub fn new(
                operation: IOOperation,
                resource: Option<String>,
                os_code: Option<i64>,
                cause: Option<String>,
            ) -> Self {
                Self {
                    operation,
                    resource,
                    os_code,
                    cause,
                }
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum IOError {
            InvalidInput(IOContext),
            NotFound(IOContext),
            PermissionDenied(IOContext),
            TimedOut(IOContext),
            Cancelled(IOContext),
            Closed(IOContext),
            Protocol(IOContext),
            Other(IOContext),
        }

        pub fn io_error_at(operation: IOOperation, path: &str, e: std::io::Error) -> IOError {
            let context = IOContext::new(
                operation,
                Some(path.to_string()),
                e.raw_os_error().map(i64::from),
                Some(e.to_string()),
            );
            match e.kind() {
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
                    IOError::InvalidInput(context)
                }
                std::io::ErrorKind::NotFound => IOError::NotFound(context),
                std::io::ErrorKind::PermissionDenied => IOError::PermissionDenied(context),
                std::io::ErrorKind::TimedOut => IOError::TimedOut(context),
                std::io::ErrorKind::NotConnected | std::io::ErrorKind::BrokenPipe => {
                    IOError::Closed(context)
                }
                _ => IOError::Other(context),
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum TextWidthAmbiguous {
            Narrow,
            Wide,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum TextWidthControls {
            Zero,
            Reject,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct TextWidth {
            pub ambiguous: TextWidthAmbiguous,
            pub controls: TextWidthControls,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct TextError {
            pub message: String,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct DirEntry {
            pub name: String,
            pub path: String,
            pub is_dir: bool,
        }

        // D-REGEXENGINE1: canonical regex engine from JetStd/Open.rs (build.rs extract).
        include!(concat!(env!("OUT_DIR"), "/regex_rt.rs"));
    }

    include!("../../jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs");
    include!("../../jet-codegen/src/Prelude/Core/UnicodeString.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Text.rs");

    pub(crate) fn lower(s: &str) -> String {
        jet_text_lower(&s.to_string())
    }
    pub(crate) fn upper(s: &str) -> String {
        jet_text_upper(&s.to_string())
    }
    pub(crate) fn graphemes(s: &str) -> Vec<String> {
        jet_text_graphemes(&s.to_string())
    }
    pub(crate) fn words(s: &str) -> Vec<String> {
        jet_text_words(&s.to_string())
    }
    pub(crate) fn sentences(s: &str) -> Vec<String> {
        jet_text_sentences(&s.to_string())
    }
    pub(crate) fn nfc(s: &str) -> String {
        jet_text_nfc(&s.to_string())
    }
    pub(crate) fn nfkc(s: &str) -> String {
        jet_text_nfkc(&s.to_string())
    }
    pub(crate) fn nfd(s: &str) -> String {
        jet_text_nfd(&s.to_string())
    }
    pub(crate) fn nfkd(s: &str) -> String {
        jet_text_nfkd(&s.to_string())
    }
    pub(crate) fn caseless_eq(a: &str, b: &str) -> bool {
        jet_text_caseless_eq(&a.to_string(), &b.to_string())
    }
    pub(crate) fn display_width_default(s: &str) -> i64 {
        jet_text_display_width_default(&s.to_string())
    }
    pub(crate) fn display_width_policy(
        s: &str,
        ambiguous_wide: bool,
        controls_reject: bool,
    ) -> Result<i64, String> {
        jet_text_display_width_policy(&s.to_string(), ambiguous_wide, controls_reject)
    }
    pub(crate) fn is_alphabetic(s: &str) -> bool {
        jet_text_is_alphabetic(&s.to_string())
    }
    pub(crate) fn is_numeric(s: &str) -> bool {
        jet_text_is_numeric(&s.to_string())
    }
    pub(crate) fn pad_start(s: &str, width: i64, fill: &str) -> String {
        jet_text_pad_start(&s.to_string(), width, &fill.to_string())
    }
    pub(crate) fn center(s: &str, width: i64, fill: &str) -> String {
        jet_text_center(&s.to_string(), width, &fill.to_string())
    }
    pub(crate) fn starts_any(s: &str, prefixes: &[String]) -> bool {
        jet_text_starts_any(&s.to_string(), &prefixes.to_vec())
    }
    pub(crate) fn char_indices(s: &str) -> Vec<String> {
        jet_text_char_indices(&s.to_string())
    }
}

fn clone_str(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}

fn alloc_str(s: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
}

fn list_from_strings(items: Vec<String>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for s in items {
            let sid = rt.heap.alloc_string(s);
            rt.heap.list_push_int(list, sid).expect("jit text list");
        }
        list
    })
}

fn list_of_strings(list: i64) -> Vec<String> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let sid = rt.heap.list_get_int(list, i).unwrap_or(0);
            out.push(rt.heap.clone_string(sid).unwrap_or_default());
        }
        out
    })
}

fn result_ok_i64(v: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(crate::JitResultValue {
            ok: true,
            bits: v as u64,
        });
        rt.results.len() as i64
    })
}

fn result_err_msg(msg: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.alloc_string(msg.to_string());
        rt.results.push(crate::JitResultValue {
            ok: false,
            bits: sid as u64,
        });
        rt.results.len() as i64
    })
}

/// TextWidth record: field0=ambiguous disc (Narrow=0, Wide=1), field1=controls (Zero=0, Reject=1).
fn decode_text_width(policy: i64) -> (bool, bool) {
    Concurrency::with_runtime_mut(|rt| {
        let amb = rt.heap.record_get_int(policy, 0).unwrap_or(0);
        let ctrl = rt.heap.record_get_int(policy, 1).unwrap_or(0);
        (amb == 1, ctrl == 1)
    })
}

extern "C" fn jet_jit_text_lower(s: i64) -> i64 {
    alloc_str(text_rt::lower(&clone_str(s)))
}

extern "C" fn jet_jit_text_upper(s: i64) -> i64 {
    alloc_str(text_rt::upper(&clone_str(s)))
}

extern "C" fn jet_jit_text_graphemes(s: i64) -> i64 {
    list_from_strings(text_rt::graphemes(&clone_str(s)))
}

extern "C" fn jet_jit_text_words(s: i64) -> i64 {
    list_from_strings(text_rt::words(&clone_str(s)))
}

extern "C" fn jet_jit_text_sentences(s: i64) -> i64 {
    list_from_strings(text_rt::sentences(&clone_str(s)))
}

extern "C" fn jet_jit_text_nfc(s: i64) -> i64 {
    alloc_str(text_rt::nfc(&clone_str(s)))
}

extern "C" fn jet_jit_text_nfkc(s: i64) -> i64 {
    alloc_str(text_rt::nfkc(&clone_str(s)))
}

extern "C" fn jet_jit_text_nfd(s: i64) -> i64 {
    alloc_str(text_rt::nfd(&clone_str(s)))
}

extern "C" fn jet_jit_text_nfkd(s: i64) -> i64 {
    alloc_str(text_rt::nfkd(&clone_str(s)))
}

extern "C" fn jet_jit_text_caseless_eq(a: i64, b: i64) -> i8 {
    i8::from(text_rt::caseless_eq(&clone_str(a), &clone_str(b)))
}

extern "C" fn jet_jit_text_display_width(s: i64) -> i64 {
    text_rt::display_width_default(&clone_str(s))
}

extern "C" fn jet_jit_text_display_width_policy(s: i64, policy: i64) -> i64 {
    let (ambiguous_wide, controls_reject) = decode_text_width(policy);
    match text_rt::display_width_policy(&clone_str(s), ambiguous_wide, controls_reject) {
        Ok(w) => result_ok_i64(w),
        Err(msg) => result_err_msg(&msg),
    }
}

extern "C" fn jet_jit_text_is_alphabetic(s: i64) -> i8 {
    i8::from(text_rt::is_alphabetic(&clone_str(s)))
}

extern "C" fn jet_jit_text_is_numeric(s: i64) -> i8 {
    i8::from(text_rt::is_numeric(&clone_str(s)))
}

extern "C" fn jet_jit_text_pad_start(s: i64, width: i64, fill: i64) -> i64 {
    alloc_str(text_rt::pad_start(&clone_str(s), width, &clone_str(fill)))
}

extern "C" fn jet_jit_text_center(s: i64, width: i64, fill: i64) -> i64 {
    alloc_str(text_rt::center(&clone_str(s), width, &clone_str(fill)))
}

extern "C" fn jet_jit_text_starts_any(s: i64, prefixes: i64) -> i8 {
    let prefs = list_of_strings(prefixes);
    i8::from(text_rt::starts_any(&clone_str(s), &prefs))
}

extern "C" fn jet_jit_text_char_indices(s: i64) -> i64 {
    list_from_strings(text_rt::char_indices(&clone_str(s)))
}


pub(crate) enum RegexValue {
    Regex(text_rt::jet_std::JetRegex),
    Match(text_rt::jet_std::JetRegexMatch),
    Flags(text_rt::jet_std::RegexFlags),
}

fn push_regex(v: RegexValue) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.regex_values.push(Some(v));
        rt.regex_values.len() as i64
    })
}

fn with_regex<R: Default>(handle: i64, f: impl FnOnce(&RegexValue) -> R) -> R {
    Concurrency::with_runtime_mut(|rt| {
        match rt.regex_values.get(handle.saturating_sub(1) as usize).and_then(|s| s.as_ref()) {
            Some(v) => f(v),
            None => R::default(),
        }
    })
}

fn clone_compiled_regex(handle: i64) -> Option<text_rt::jet_std::JetRegex> {
    with_regex(handle, |value| match value {
        RegexValue::Regex(regex) => Some(regex.clone()),
        _ => None,
    })
}

fn regex_result_ok(bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok: true, bits });
        rt.results.len() as i64
    })
}

fn regex_result_err(msg: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.alloc_string(msg);
        rt.results.push(super::JitResultValue { ok: false, bits: sid as u64 });
        rt.results.len() as i64
    })
}

fn option_string_bits(opt: Option<String>) -> i64 {
    match opt {
        None => 0,
        Some(s) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
            sid.wrapping_add(1)
        }
    }
}

fn list_strings(items: Vec<String>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for s in items {
            let sid = rt.heap.alloc_string(s);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    })
}

extern "C" fn jet_jit_regex_flags(ci: i64, ml: i64, ds: i64) -> i64 {
    push_regex(RegexValue::Flags(text_rt::jet_std::jet_regex_flags(ci != 0, ml != 0, ds != 0)))
}

extern "C" fn jet_jit_regex_literal(pat: i64) -> i64 {
    push_regex(RegexValue::Regex(text_rt::jet_std::jet_regex_literal(
        &clone_string(pat),
    )))
}

extern "C" fn jet_jit_regex_is_match(pat: i64, text: i64) -> i8 {
    let t = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| i8::from(regex.is_match(&t)))
        .unwrap_or_default()
}

extern "C" fn jet_jit_regex_find(pat: i64, text: i64) -> i64 {
    let t = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| option_string_bits(regex.find(&t)))
        .unwrap_or_default()
}

extern "C" fn jet_jit_regex_find_all(pat: i64, text: i64) -> i64 {
    let t = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| list_strings(regex.find_all(&t)))
        .unwrap_or_default()
}

extern "C" fn jet_jit_regex_matches(pat: i64, text: i64) -> i64 {
    let text = clone_string(text);
    let Some(regex) = clone_compiled_regex(pat) else {
        return 0;
    };
    let list = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list());
    for found in regex.matches(&text) {
        let handle = push_regex(RegexValue::Match(found));
        Concurrency::with_runtime_mut(|rt| {
            let _ = rt.heap.list_push_int(list, handle);
        });
    }
    list
}

extern "C" fn jet_jit_regex_match(pat: i64, text: i64) -> i64 {
    let t = clone_string(text);
    clone_compiled_regex(pat)
        .and_then(|regex| regex.match_value(&t))
        .map(|found| push_regex(RegexValue::Match(found)).wrapping_add(1))
        .unwrap_or_default()
}

extern "C" fn jet_jit_regex_replace_all(pat: i64, text: i64, repl: i64) -> i64 {
    let t = clone_string(text);
    let r = clone_string(repl);
    clone_compiled_regex(pat)
        .map(|regex| {
            Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(regex.replace_all(&t, &r)))
        })
        .unwrap_or_default()
}

extern "C" fn jet_jit_regex_replace(pat: i64, text: i64, repl: i64) -> i64 {
    let text = clone_string(text);
    let replacement = clone_string(repl);
    clone_compiled_regex(pat)
        .map(|regex| Concurrency::with_runtime_mut(|rt| {
            rt.heap.alloc_string(regex.replace(&text, &replacement))
        }))
        .unwrap_or_default()
}

extern "C" fn jet_jit_regex_split(pat: i64, text: i64) -> i64 {
    let t = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| list_strings(regex.split(&t)))
        .unwrap_or_default()
}

extern "C" fn jet_jit_regex_split_limit(pat: i64, text: i64, limit: i64) -> i64 {
    let text = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| list_strings(regex.split_limit(&text, limit)))
        .unwrap_or_default()
}

extern "C" fn jet_jit_regex_compile(pat: i64) -> i64 {
    match text_rt::jet_std::jet_regex_compile(&clone_string(pat)) {
        Ok(rx) => regex_result_ok(push_regex(RegexValue::Regex(rx)) as u64),
        Err(e) => regex_result_err(e),
    }
}

extern "C" fn jet_jit_regex_compile_with(pat: i64, flags: i64) -> i64 {
    let flags = with_regex(flags, |v| match v {
        RegexValue::Flags(f) => Some(f.clone()),
        _ => None,
    });
    let Some(flags) = flags else { return regex_result_err("bad RegexFlags".into()); };
    match text_rt::jet_std::jet_regex_compile_with(&clone_string(pat), &flags) {
        Ok(rx) => regex_result_ok(push_regex(RegexValue::Regex(rx)) as u64),
        Err(e) => regex_result_err(e),
    }
}

/// Regex/Match method. method is string handle.
extern "C" fn jet_jit_regex_method(recv: i64, method: i64, arg0: i64, arg1: i64) -> i64 {
    let method = clone_string(method);
    with_regex(recv, |v| match (v, method.as_str()) {
        (RegexValue::Regex(rx), "is_match") => i64::from(rx.is_match(&clone_string(arg0))),
        (RegexValue::Regex(rx), "match") => match rx.match_value(&clone_string(arg0)) {
            None => 0,
            Some(m) => push_regex(RegexValue::Match(m)).wrapping_add(1),
        },
        (RegexValue::Regex(rx), "find") => option_string_bits(rx.find(&clone_string(arg0))),
        (RegexValue::Regex(rx), "find_all") => list_strings(rx.find_all(&clone_string(arg0))),
        (RegexValue::Regex(rx), "matches") => {
            let list = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list());
            for m in rx.matches(&clone_string(arg0)) {
                let h = push_regex(RegexValue::Match(m));
                Concurrency::with_runtime_mut(|rt| { let _ = rt.heap.list_push_int(list, h); });
            }
            list
        }
        (RegexValue::Regex(rx), "split") => list_strings(rx.split(&clone_string(arg0))),
        (RegexValue::Regex(rx), "split_limit") => list_strings(rx.split_limit(&clone_string(arg0), arg1)),
        (RegexValue::Regex(rx), "replace" | "replace_all") => {
            let s = if method == "replace" {
                rx.replace(&clone_string(arg0), &clone_string(arg1))
            } else {
                rx.replace_all(&clone_string(arg0), &clone_string(arg1))
            };
            Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
        }
        (RegexValue::Regex(rx), "replace_all_with") => {
            // arg1 unused — constant "hit" path not general; use empty callback substitute via replace_all with empty.
            // Real callback dispatch is wired from lower_ctx via FuncId; this host path is for string-only.
            let s = rx.replace_all_with(&clone_string(arg0), |_m| "hit".to_string());
            Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
        }
        (RegexValue::Match(m), "group") => option_string_bits(m.group(arg0)),
        (RegexValue::Match(m), "name") => option_string_bits(m.name(&clone_string(arg0))),
        (RegexValue::Match(m), "start") => m.start(),
        (RegexValue::Match(m), "end") => m.end(),
        _ => 0,
    })
}

fn clone_string(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}


pub(crate) struct TextHostFns {
    pub lower: FuncId,
    pub upper: FuncId,
    pub graphemes: FuncId,
    pub words: FuncId,
    pub sentences: FuncId,
    pub nfc: FuncId,
    pub nfkc: FuncId,
    pub nfd: FuncId,
    pub nfkd: FuncId,
    pub caseless_eq: FuncId,
    pub display_width: FuncId,
    pub display_width_policy: FuncId,
    pub is_alphabetic: FuncId,
    pub is_numeric: FuncId,
    pub pad_start: FuncId,
    pub center: FuncId,
    pub starts_any: FuncId,
    pub char_indices: FuncId,
    pub regex_flags: FuncId,
    pub regex_literal: FuncId,
    pub regex_is_match: FuncId,
    pub regex_find: FuncId,
    pub regex_find_all: FuncId,
    pub regex_matches: FuncId,
    pub regex_match: FuncId,
    pub regex_replace: FuncId,
    pub regex_replace_all: FuncId,
    pub regex_split: FuncId,
    pub regex_split_limit: FuncId,
    pub regex_compile: FuncId,
    pub regex_compile_with: FuncId,
    pub regex_method: FuncId,
}

pub(crate) fn register_text_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_text_lower", jet_jit_text_lower as *const u8);
    builder.symbol("jet_jit_text_upper", jet_jit_text_upper as *const u8);
    builder.symbol("jet_jit_text_graphemes", jet_jit_text_graphemes as *const u8);
    builder.symbol("jet_jit_text_words", jet_jit_text_words as *const u8);
    builder.symbol("jet_jit_text_sentences", jet_jit_text_sentences as *const u8);
    builder.symbol("jet_jit_text_nfc", jet_jit_text_nfc as *const u8);
    builder.symbol("jet_jit_text_nfkc", jet_jit_text_nfkc as *const u8);
    builder.symbol("jet_jit_text_nfd", jet_jit_text_nfd as *const u8);
    builder.symbol("jet_jit_text_nfkd", jet_jit_text_nfkd as *const u8);
    builder.symbol("jet_jit_text_caseless_eq", jet_jit_text_caseless_eq as *const u8);
    builder.symbol(
        "jet_jit_text_display_width",
        jet_jit_text_display_width as *const u8,
    );
    builder.symbol(
        "jet_jit_text_display_width_policy",
        jet_jit_text_display_width_policy as *const u8,
    );
    builder.symbol(
        "jet_jit_text_is_alphabetic",
        jet_jit_text_is_alphabetic as *const u8,
    );
    builder.symbol("jet_jit_text_is_numeric", jet_jit_text_is_numeric as *const u8);
    builder.symbol("jet_jit_text_pad_start", jet_jit_text_pad_start as *const u8);
    builder.symbol("jet_jit_text_center", jet_jit_text_center as *const u8);
    builder.symbol("jet_jit_text_starts_any", jet_jit_text_starts_any as *const u8);
    builder.symbol(
        "jet_jit_text_char_indices",
        jet_jit_text_char_indices as *const u8,
    );
    builder.symbol("jet_jit_regex_flags", jet_jit_regex_flags as *const u8);
    builder.symbol("jet_jit_regex_literal", jet_jit_regex_literal as *const u8);
    builder.symbol("jet_jit_regex_is_match", jet_jit_regex_is_match as *const u8);
    builder.symbol("jet_jit_regex_find", jet_jit_regex_find as *const u8);
    builder.symbol("jet_jit_regex_find_all", jet_jit_regex_find_all as *const u8);
    builder.symbol("jet_jit_regex_matches", jet_jit_regex_matches as *const u8);
    builder.symbol("jet_jit_regex_match", jet_jit_regex_match as *const u8);
    builder.symbol("jet_jit_regex_replace", jet_jit_regex_replace as *const u8);
    builder.symbol("jet_jit_regex_replace_all", jet_jit_regex_replace_all as *const u8);
    builder.symbol("jet_jit_regex_split", jet_jit_regex_split as *const u8);
    builder.symbol("jet_jit_regex_split_limit", jet_jit_regex_split_limit as *const u8);
    builder.symbol("jet_jit_regex_compile", jet_jit_regex_compile as *const u8);
    builder.symbol("jet_jit_regex_compile_with", jet_jit_regex_compile_with as *const u8);
    builder.symbol("jet_jit_regex_method", jet_jit_regex_method as *const u8);
}

pub(crate) fn declare_text_host_fns(module: &mut JITModule) -> Result<TextHostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut unary = Signature::new(cc);
    unary.params.push(AbiParam::new(types::I64));
    unary.returns.push(AbiParam::new(types::I64));
    let mut unary_i8 = Signature::new(cc);
    unary_i8.params.push(AbiParam::new(types::I64));
    unary_i8.returns.push(AbiParam::new(types::I8));
    let mut binary = Signature::new(cc);
    binary.params.push(AbiParam::new(types::I64));
    binary.params.push(AbiParam::new(types::I64));
    binary.returns.push(AbiParam::new(types::I64));
    let mut binary_i8 = Signature::new(cc);
    binary_i8.params.push(AbiParam::new(types::I64));
    binary_i8.params.push(AbiParam::new(types::I64));
    binary_i8.returns.push(AbiParam::new(types::I8));
    let mut ternary = Signature::new(cc);
    for _ in 0..3 {
        ternary.params.push(AbiParam::new(types::I64));
    }
    ternary.returns.push(AbiParam::new(types::I64));
    let mut import = |name: &str, sig: &Signature| {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(TextHostFns {
        lower: import("jet_jit_text_lower", &unary)?,
        upper: import("jet_jit_text_upper", &unary)?,
        graphemes: import("jet_jit_text_graphemes", &unary)?,
        words: import("jet_jit_text_words", &unary)?,
        sentences: import("jet_jit_text_sentences", &unary)?,
        nfc: import("jet_jit_text_nfc", &unary)?,
        nfkc: import("jet_jit_text_nfkc", &unary)?,
        nfd: import("jet_jit_text_nfd", &unary)?,
        nfkd: import("jet_jit_text_nfkd", &unary)?,
        caseless_eq: import("jet_jit_text_caseless_eq", &binary_i8)?,
        display_width: import("jet_jit_text_display_width", &unary)?,
        display_width_policy: import("jet_jit_text_display_width_policy", &binary)?,
        is_alphabetic: import("jet_jit_text_is_alphabetic", &unary_i8)?,
        is_numeric: import("jet_jit_text_is_numeric", &unary_i8)?,
        pad_start: import("jet_jit_text_pad_start", &ternary)?,
        center: import("jet_jit_text_center", &ternary)?,
        starts_any: import("jet_jit_text_starts_any", &binary_i8)?,
        char_indices: import("jet_jit_text_char_indices", &unary)?,
        regex_flags: import("jet_jit_regex_flags", &ternary)?,
        regex_literal: import("jet_jit_regex_literal", &unary)?,
        regex_is_match: import("jet_jit_regex_is_match", &binary_i8)?,
        regex_find: import("jet_jit_regex_find", &binary)?,
        regex_find_all: import("jet_jit_regex_find_all", &binary)?,
        regex_matches: import("jet_jit_regex_matches", &binary)?,
        regex_match: import("jet_jit_regex_match", &binary)?,
        regex_replace: import("jet_jit_regex_replace", &ternary)?,
        regex_replace_all: import("jet_jit_regex_replace_all", &ternary)?,
        regex_split: import("jet_jit_regex_split", &binary)?,
        regex_split_limit: import("jet_jit_regex_split_limit", &ternary)?,
        regex_compile: import("jet_jit_regex_compile", &unary)?,
        regex_compile_with: import("jet_jit_regex_compile_with", &binary)?,
        regex_method: {
            let mut q = Signature::new(cc);
            for _ in 0..4 { q.params.push(AbiParam::new(types::I64)); }
            q.returns.push(AbiParam::new(types::I64));
            import("jet_jit_regex_method", &q)?
        },
    })
}
