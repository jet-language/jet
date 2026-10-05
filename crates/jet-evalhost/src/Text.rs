//! `core.text` hosts (#729). `include!` canonical UnicodeTables + UnicodeString +
//! Top/Text.rs — no third algorithm.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use crate::Marshal::{alloc_string, clone_string, result_err_msg};
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;

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

        impl IOError {
            pub fn other(
                operation: IOOperation,
                resource: Option<String>,
                cause: impl ToString,
            ) -> Self {
                Self::Other(IOContext::new(
                    operation,
                    resource,
                    None,
                    Some(cause.to_string()),
                ))
            }
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

        // D-REGEXENGINE1: canonical regex engine from JetStd/Regex.rs (build.rs extract).
        #[allow(unused_imports)]
        pub use jet_foundation::Outcome::*;
        include!(concat!(env!("OUT_DIR"), "/regex_rt.rs"));
    }

    #[allow(unused_imports)]
    pub use jet_unicode::*;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/Core/UnicodeString.rs");
    include!("../../jet-codegen/src/Prelude/Core/Ascii.rs");
    use crate::fault_injection::jet_fault_should_fail;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Text.rs");

    pub(crate) fn lower(s: &str) -> String {
        jet_text_lower(&s.to_string())
    }
    pub(crate) fn upper(s: &str) -> String {
        jet_text_upper(&s.to_string())
    }
    pub(crate) fn ascii_lower(s: &str) -> String {
        jet_text_ascii_lower(&s.to_string())
    }
    pub(crate) fn casefold(s: &str) -> String {
        jet_text_casefold(&s.to_string())
    }
    pub(crate) fn unicode_scalar_count(s: &str) -> i64 {
        jet_text_unicode_scalar_count(&s.to_string())
    }
    pub(crate) fn unicode_byte_count(s: &str) -> i64 {
        jet_text_unicode_byte_count(&s.to_string())
    }
    pub(crate) fn rsplitn(s: &str, pat: &str, n: i64) -> Vec<String> {
        jet_text_rsplitn(&s.to_string(), &pat.to_string(), n)
    }
    pub(crate) fn splitn(s: &str, pat: &str, n: i64) -> Vec<String> {
        jet_text_splitn(&s.to_string(), &pat.to_string(), n)
    }
    pub(crate) fn ascii_upper(s: &str) -> String {
        jet_text_ascii_upper(&s.to_string())
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
    pub(crate) fn is_whitespace(s: &str) -> bool {
        jet_text_is_whitespace(&s.to_string())
    }
    pub(crate) fn is_ascii(s: &str) -> bool {
        jet_text_unicode_is_ascii(&s.to_string())
    }
    pub(crate) fn trim_start(s: &str) -> String {
        jet_text_trim_start(&s.to_string())
    }
    pub(crate) fn trim(s: &str) -> String {
        jet_text_trim(&s.to_string())
    }

    pub(crate) fn trim_end(s: &str) -> String {
        jet_text_trim_end(&s.to_string())
    }
    pub(crate) fn pad_start(s: &str, width: i64, fill: &str) -> String {
        jet_text_pad_start(
            &s.to_string(),
            jet_foundation::Numeric::JetInt::from_i64(width),
            &fill.to_string(),
        )
    }
    pub(crate) fn pad_end(s: &str, width: i64, fill: &str) -> String {
        jet_text_pad_end(
            &s.to_string(),
            jet_foundation::Numeric::JetInt::from_i64(width),
            &fill.to_string(),
        )
    }
    pub(crate) fn index_of(s: &str, needle: &str) -> Option<i64> {
        jet_unicode_index_of(&s.to_string(), &needle.to_string())
    }
    pub(crate) fn count(s: &str, needle: &str) -> i64 {
        jet_unicode_count(&s.to_string(), &needle.to_string())
    }
    pub(crate) fn title(s: &str) -> String {
        jet_text_title(&s.to_string())
    }
    pub(crate) fn is_lower(s: &str) -> bool {
        jet_text_is_lower(&s.to_string())
    }
    pub(crate) fn is_upper(s: &str) -> bool {
        jet_text_is_upper(&s.to_string())
    }
    pub(crate) fn capitalize(s: &str) -> String {
        jet_text_capitalize(&s.to_string())
    }
    pub(crate) fn swapcase(s: &str) -> String {
        jet_text_swapcase(&s.to_string())
    }
    pub(crate) fn remove_prefix(s: &str, prefix: &str) -> String {
        jet_text_remove_prefix(&s.to_string(), &prefix.to_string())
    }
    pub(crate) fn remove_suffix(s: &str, suffix: &str) -> String {
        jet_text_remove_suffix(&s.to_string(), &suffix.to_string())
    }
    pub(crate) fn compare(a: &str, b: &str) -> i64 {
        jet_text_compare(a, b)
    }
    pub(crate) fn reverse(s: &str) -> String {
        jet_text_reverse(&s.to_string())
    }
    pub(crate) fn normalize_nfc(s: &str) -> String {
        jet_text_normalize_nfc(&s.to_string())
    }
    pub(crate) fn last_index_of(s: &str, needle: &str) -> Option<i64> {
        jet_unicode_last_index_of(&s.to_string(), &needle.to_string())
    }
    pub(crate) fn split_once(s: &str, separator: &str) -> Option<(String, String)> {
        jet_unicode_split_once(&s.to_string(), &separator.to_string()).ok()
    }
    pub(crate) fn cut_last(s: &str, separator: &str) -> Option<(String, String)> {
        jet_unicode_cut_last(&s.to_string(), &separator.to_string()).ok()
    }
    pub(crate) fn center(s: &str, width: i64, fill: &str) -> String {
        jet_text_center(
            &s.to_string(),
            jet_foundation::Numeric::JetInt::from_i64(width),
            &fill.to_string(),
        )
    }
    pub(crate) fn starts_any(s: &str, prefixes: &[String]) -> bool {
        jet_text_starts_any(&s.to_string(), &prefixes.to_vec())
    }
    pub(crate) fn ends_any(s: &str, suffixes: &[String]) -> bool {
        jet_text_ends_any(&s.to_string(), &suffixes.to_vec())
    }
    pub(crate) fn char_indices(s: &str) -> Vec<String> {
        jet_text_char_indices(&s.to_string())
    }
    pub(crate) fn inspect(s: &str) -> Vec<String> {
        jet_text_inspect(&s.to_string())
    }
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
fn list_from_bytes(bytes: &[u8]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for &byte in bytes {
            rt.heap
                .list_push_int(list, i64::from(byte))
                .expect("jit text byte list");
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

/// TextWidth record: field0=ambiguous disc (Narrow=0, Wide=1), field1=controls (Zero=0, Reject=1).
fn decode_text_width(policy: i64) -> (bool, bool) {
    Concurrency::with_runtime_mut(|rt| {
        let amb = rt.heap.record_get_int(policy, 0).unwrap_or(0);
        let ctrl = rt.heap.record_get_int(policy, 1).unwrap_or(0);
        (amb == 1, ctrl == 1)
    })
}

fn jet_jit_text_lower(s: i64) -> i64 {
    alloc_string(text_rt::lower(&clone_string(s)))
}

fn jet_jit_text_upper(s: i64) -> i64 {
    alloc_string(text_rt::upper(&clone_string(s)))
}

fn jet_jit_text_ascii_lower(s: i64) -> i64 {
    alloc_string(text_rt::ascii_lower(&clone_string(s)))
}

fn jet_jit_text_ascii_upper(s: i64) -> i64 {
    alloc_string(text_rt::ascii_upper(&clone_string(s)))
}

fn jet_jit_text_casefold(s: i64) -> i64 {
    alloc_string(text_rt::casefold(&clone_string(s)))
}

fn jet_jit_text_unicode_scalar_count(s: i64) -> i64 {
    text_rt::unicode_scalar_count(&clone_string(s))
}

fn jet_jit_text_unicode_byte_count(s: i64) -> i64 {
    text_rt::unicode_byte_count(&clone_string(s))
}
fn jet_jit_text_rsplitn(s: i64, pat: i64, n: i64) -> i64 {
    list_from_strings(text_rt::rsplitn(
        &clone_string(s),
        &clone_string(pat),
        n,
    ))
}
fn jet_jit_text_splitn(s: i64, pat: i64, n: i64) -> i64 {
    list_from_strings(text_rt::splitn(
        &clone_string(s),
        &clone_string(pat),
        n,
    ))
}
fn jet_jit_text_parse_split(s: i64, separator: i64) -> i64 {
    list_from_strings(text_rt::jet_text_parse_split(
        &clone_string(s),
        &clone_string(separator),
    ))
}
fn jet_jit_text_parse_rsplit(s: i64, separator: i64, maxsplit: i64) -> i64 {
    list_from_strings(text_rt::jet_text_parse_rsplit(
        &clone_string(s),
        &clone_string(separator),
        maxsplit,
    ))
}
fn jet_jit_text_parse_split_once(s: i64, separator: i64) -> i64 {
    let (found, before, after) = text_rt::jet_text_parse_split_once(
        &clone_string(s),
        &clone_string(separator),
    );
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(3);
        let before_id = rt.heap.alloc_string(before);
        let after_id = rt.heap.alloc_string(after);
        let _ = rt.heap.record_set_bool(record, 0, found);
        let _ = rt.heap.record_set_string(record, 1, before_id);
        let _ = rt.heap.record_set_string(record, 2, after_id);
        record
    })
}
fn jet_jit_text_parse_partition(s: i64, separator: i64) -> i64 {
    let (head, sep, tail) = text_rt::jet_text_parse_partition(
        &clone_string(s),
        &clone_string(separator),
    );
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(3);
        let head_id = rt.heap.alloc_string(head);
        let sep_id = rt.heap.alloc_string(sep);
        let tail_id = rt.heap.alloc_string(tail);
        let _ = rt.heap.record_set_string(record, 0, head_id);
        let _ = rt.heap.record_set_string(record, 1, sep_id);
        let _ = rt.heap.record_set_string(record, 2, tail_id);
        record
    })
}
fn jet_jit_text_parse_rpartition(s: i64, separator: i64) -> i64 {
    let (head, sep, tail) = text_rt::jet_text_parse_rpartition(
        &clone_string(s),
        &clone_string(separator),
    );
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(3);
        let head_id = rt.heap.alloc_string(head);
        let sep_id = rt.heap.alloc_string(sep);
        let tail_id = rt.heap.alloc_string(tail);
        let _ = rt.heap.record_set_string(record, 0, head_id);
        let _ = rt.heap.record_set_string(record, 1, sep_id);
        let _ = rt.heap.record_set_string(record, 2, tail_id);
        record
    })
}
fn jet_jit_text_parse_split_ws(s: i64) -> i64 {
    list_from_strings(text_rt::jet_text_parse_split_ws(&clone_string(s)))
}
fn jet_jit_text_parse_bool(s: i64) -> i64 {
    text_rt::jet_text_parse_bool(&clone_string(s))
        .map_or(0, |value| i64::from(value) + 1)
}
fn jet_jit_text_parse_int(s: i64) -> i64 {
    text_rt::jet_text_parse_int(&clone_string(s)).map_or(0, |value| value.wrapping_add(1))
}
fn jet_jit_text_parse_int_base(s: i64, base: i64) -> i64 {
    text_rt::jet_text_parse_int_base(&clone_string(s), base)
        .map_or(0, |value| value.wrapping_add(1))
}
fn jet_jit_text_wrap(s: i64, width: i64) -> i64 {
    list_from_strings(text_rt::jet_text_wrap(&clone_string(s), width))
}
fn jet_jit_text_fill(s: i64, width: i64) -> i64 {
    alloc_string(text_rt::jet_text_fill(&clone_string(s), width))
}
fn jet_jit_text_indent_with(s: i64, prefix: i64, predicate_nonblank: i64) -> i64 {
    alloc_string(text_rt::jet_text_indent_with(
        &clone_string(s),
        &clone_string(prefix),
        predicate_nonblank != 0,
    ))
}
fn jet_jit_text_wrap_paragraphs(s: i64, width: i64) -> i64 {
    alloc_string(text_rt::jet_text_wrap_paragraphs(&clone_string(s), width))
}
fn jet_jit_text_hanging_indent(s: i64, first: i64, rest: i64, width: i64) -> i64 {
    alloc_string(text_rt::jet_text_hanging_indent(
        &clone_string(s),
        &clone_string(first),
        &clone_string(rest),
        width,
    ))
}
fn jet_jit_text_shorten(s: i64, width: i64, placeholder: i64) -> i64 {
    alloc_string(text_rt::jet_text_shorten(
        &clone_string(s),
        width,
        &clone_string(placeholder),
    ))
}
fn jet_jit_text_indent(s: i64, prefix: i64) -> i64 {
    alloc_string(text_rt::jet_text_indent(
        &clone_string(s),
        &clone_string(prefix),
    ))
}
fn jet_jit_text_dedent(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_dedent(&clone_string(s)))
}
fn jet_jit_text_expand_tabs(s: i64, tabsize: i64) -> i64 {
    alloc_string(text_rt::jet_text_expand_tabs(&clone_string(s), tabsize))
}
fn jet_jit_text_html_escape(s: i64, quote: i64) -> i64 {
    alloc_string(text_rt::jet_text_html_escape(
        &clone_string(s),
        quote != 0,
    ))
}
fn jet_jit_text_html_escape_quoted(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_html_escape_quoted(&clone_string(s)))
}
fn jet_jit_text_html_escape_text(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_html_escape_text(&clone_string(s)))
}
fn jet_jit_text_html_unescape(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_html_unescape(&clone_string(s)))
}
fn jet_jit_text_html_strip_tags(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_html_strip_tags(&clone_string(s)))
}
fn jet_jit_text_html_unescape_and_strip(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_html_unescape_and_strip(&clone_string(s)))
}
fn jet_jit_text_parse_float(s: i64) -> i64 {
    text_rt::jet_text_parse_float(&clone_string(s))
        .map_or(0, |value| value.to_bits().wrapping_add(1) as i64)
}
fn jet_jit_text_parse_kv(s: i64, separator: i64) -> i64 {
    let (key, ok, value) = text_rt::jet_text_parse_kv(
        &clone_string(s),
        &clone_string(separator),
    );
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(3);
        let key_id = rt.heap.alloc_string(key);
        let value_id = rt.heap.alloc_string(value);
        let _ = rt.heap.record_set_string(record, 0, key_id);
        let _ = rt.heap.record_set_bool(record, 1, ok);
        let _ = rt.heap.record_set_string(record, 2, value_id);
        record
    })
}
fn jet_jit_text_parse_find(s: i64, needle: i64) -> i64 {
    text_rt::jet_text_parse_find(&clone_string(s), &clone_string(needle))
}
fn jet_jit_text_parse_rfind(s: i64, needle: i64) -> i64 {
    text_rt::jet_text_parse_rfind(&clone_string(s), &clone_string(needle))
}
fn jet_jit_text_parse_count(s: i64, needle: i64) -> i64 {
    text_rt::jet_text_parse_count(&clone_string(s), &clone_string(needle))
}
fn jet_jit_text_parse_replace_n(s: i64, old: i64, new: i64, count: i64) -> i64 {
    alloc_string(text_rt::jet_text_parse_replace_n(
        &clone_string(s),
        &clone_string(old),
        &clone_string(new),
        count,
    ))
}
fn jet_jit_text_parse_join(parts: i64, separator: i64) -> i64 {
    alloc_string(text_rt::jet_text_parse_join(
        &list_of_strings(parts),
        &clone_string(separator),
    ))
}
fn jet_jit_text_parse_splitlines(s: i64, keepends: i64) -> i64 {
    list_from_strings(text_rt::jet_text_parse_splitlines(
        &clone_string(s),
        keepends != 0,
    ))
}
fn jet_jit_text_parse_capwords(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_parse_capwords(&clone_string(s)))
}
fn jet_jit_text_parse_find_from(s: i64, needle: i64, start: i64) -> i64 {
    text_rt::jet_text_parse_find_from(&clone_string(s), &clone_string(needle), start)
}
fn jet_jit_text_parse_rfind_from(s: i64, needle: i64, end: i64) -> i64 {
    text_rt::jet_text_parse_rfind_from(&clone_string(s), &clone_string(needle), end)
}
fn jet_jit_text_parse_index(s: i64, needle: i64) -> i64 {
    text_rt::jet_text_parse_index(&clone_string(s), &clone_string(needle))
}
fn jet_jit_text_parse_contains(s: i64, needle: i64) -> i8 {
    i8::from(text_rt::jet_text_parse_contains(
        &clone_string(s),
        &clone_string(needle),
    ))
}
fn jet_jit_text_parse_lstrip(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_parse_lstrip(&clone_string(s)))
}
fn jet_jit_text_parse_rstrip(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_parse_rstrip(&clone_string(s)))
}
fn jet_jit_text_parse_strip(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_parse_strip(&clone_string(s)))
}
fn jet_jit_text_parse_escape_c(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_parse_escape_c(&clone_string(s)))
}
fn jet_jit_text_parse_unescape_c(s: i64) -> i64 {
    alloc_string(text_rt::jet_text_parse_unescape_c(&clone_string(s)))
}

fn jet_jit_string_lines(s: i64) -> i64 {
    let text = clone_string(s);
    list_from_strings(
        text_rt::jet_text_line_views(&text)
            .into_iter()
            .map(str::to_owned)
            .collect(),
    )
}

fn jet_jit_text_graphemes(s: i64) -> i64 {
    list_from_strings(text_rt::graphemes(&clone_string(s)))
}

fn jet_jit_text_words(s: i64) -> i64 {
    list_from_strings(text_rt::words(&clone_string(s)))
}

fn jet_jit_text_sentences(s: i64) -> i64 {
    list_from_strings(text_rt::sentences(&clone_string(s)))
}

/// Byte spans of `parts` inside `source`, or the reason a part escaped it.
/// Computed on a shared borrow so the caller can trap on `rt` afterwards.
fn view_spans<'a, P: AsRef<[u8]> + 'a>(
    source: &'a str,
    parts: impl IntoIterator<Item = P>,
    kind: &'static str,
) -> Result<Vec<(usize, usize)>, String> {
    let base = source.as_ptr() as usize;
    let mut spans = Vec::new();
    for part in parts {
        let part = part.as_ref();
        let Some(start) = (part.as_ptr() as usize).checked_sub(base) else {
            return Err(format!("{kind} view escaped its source string"));
        };
        let Some(end) = start.checked_add(part.len()) else {
            return Err(format!("{kind} view range overflow"));
        };
        if end > source.len() {
            return Err(format!("{kind} view escaped its source string"));
        }
        spans.push((start, end));
    }
    Ok(spans)
}

fn alloc_text_views(
    rt: &mut crate::JitRuntime,
    text: i64,
    spans: Result<Vec<(usize, usize)>, String>,
    bytes: bool,
) -> i64 {
    let spans = match spans {
        Ok(spans) => spans,
        Err(reason) => {
            rt.set_trap(&reason);
            return 0;
        }
    };
    let out = rt.heap.alloc_empty_list();
    for (start, end) in spans {
        let slot = rt.view_slots.len();
        rt.view_slots.push(crate::runtime_host::JitViewSlot::String {
            owner: text,
            start,
            end,
            bytes,
        });
        rt.heap
            .list_push_int(out, crate::runtime_host::view_handle(slot))
            .expect("JIT text view list");
    }
    out
}

fn alloc_text_string_views<F>(text: i64, make: F) -> i64
where
    F: for<'a> FnOnce(&'a str) -> text_rt::JetViewIter<'a, &'a str>,
{
    Concurrency::with_runtime_mut(|rt| {
        let spans = rt
            .heap
            .get_string(text)
            .ok_or_else(|| "invalid String handle for text view".to_string())
            .and_then(|source| view_spans(source, make(source), "text"));
        alloc_text_views(rt, text, spans, false)
    })
}

fn alloc_text_byte_views<F>(text: i64, make: F) -> i64
where
    F: for<'a> FnOnce(&'a str) -> text_rt::JetViewIter<'a, &'a [u8]>,
{
    Concurrency::with_runtime_mut(|rt| {
        let spans = rt
            .heap
            .get_string(text)
            .ok_or_else(|| "invalid String handle for byte view".to_string())
            .and_then(|source| view_spans(source, make(source), "byte"));
        alloc_text_views(rt, text, spans, true)
    })
}

fn jet_jit_text_grapheme_views(text: i64) -> i64 {
    alloc_text_string_views(text, text_rt::jet_text_grapheme_views)
}

fn jet_jit_text_word_views(text: i64) -> i64 {
    alloc_text_string_views(text, text_rt::jet_text_word_views)
}

fn jet_jit_text_line_views(text: i64) -> i64 {
    alloc_text_string_views(text, text_rt::jet_text_line_views)
}

fn jet_jit_text_byte_views(text: i64) -> i64 {
    alloc_text_byte_views(text, text_rt::jet_text_byte_views)
}

fn jet_jit_text_nfc(s: i64) -> i64 {
    alloc_string(text_rt::nfc(&clone_string(s)))
}

fn jet_jit_text_nfkc(s: i64) -> i64 {
    alloc_string(text_rt::nfkc(&clone_string(s)))
}

fn jet_jit_text_nfd(s: i64) -> i64 {
    alloc_string(text_rt::nfd(&clone_string(s)))
}

fn jet_jit_text_nfkd(s: i64) -> i64 {
    alloc_string(text_rt::nfkd(&clone_string(s)))
}

fn jet_jit_text_caseless_eq(a: i64, b: i64) -> i8 {
    i8::from(text_rt::caseless_eq(&clone_string(a), &clone_string(b)))
}

fn jet_jit_text_display_width(s: i64) -> i64 {
    text_rt::display_width_default(&clone_string(s))
}

fn jet_jit_text_display_width_policy(s: i64, policy: i64) -> i64 {
    let (ambiguous_wide, controls_reject) = decode_text_width(policy);
    match text_rt::display_width_policy(&clone_string(s), ambiguous_wide, controls_reject) {
        Ok(w) => result_ok_i64(w),
        Err(msg) => result_err_msg(&msg),
    }
}

fn jet_jit_text_is_alphabetic(s: i64) -> i8 {
    i8::from(text_rt::is_alphabetic(&clone_string(s)))
}

fn jet_jit_text_is_numeric(s: i64) -> i8 {
    i8::from(text_rt::is_numeric(&clone_string(s)))
}

fn jet_jit_text_is_whitespace(s: i64) -> i8 {
    i8::from(text_rt::is_whitespace(&clone_string(s)))
}

fn jet_jit_text_is_ascii(s: i64) -> i8 {
    i8::from(text_rt::is_ascii(&clone_string(s)))
}

fn jet_jit_text_is_lower(s: i64) -> i8 {
    i8::from(text_rt::is_lower(&clone_string(s)))
}

fn jet_jit_text_is_upper(s: i64) -> i8 {
    i8::from(text_rt::is_upper(&clone_string(s)))
}

fn jet_jit_text_capitalize(s: i64) -> i64 {
    alloc_string(text_rt::capitalize(&clone_string(s)))
}

fn jet_jit_text_swapcase(s: i64) -> i64 {
    alloc_string(text_rt::swapcase(&clone_string(s)))
}
fn jet_jit_text_remove_prefix(s: i64, prefix: i64) -> i64 {
    alloc_string(text_rt::remove_prefix(
        &clone_string(s),
        &clone_string(prefix),
    ))
}

fn jet_jit_text_remove_suffix(s: i64, suffix: i64) -> i64 {
    alloc_string(text_rt::remove_suffix(
        &clone_string(s),
        &clone_string(suffix),
    ))
}

fn jet_jit_text_compare(a: i64, b: i64) -> i64 {
    text_rt::compare(&clone_string(a), &clone_string(b))
}

fn jet_jit_text_reverse(s: i64) -> i64 {
    alloc_string(text_rt::reverse(&clone_string(s)))
}


fn jet_jit_text_trim_start(s: i64) -> i64 {
    alloc_string(text_rt::trim_start(&clone_string(s)))
}

fn jet_jit_text_trim(s: i64) -> i64 {
    alloc_string(text_rt::trim(&clone_string(s)))
}

fn jet_jit_text_trim_end(s: i64) -> i64 {
    alloc_string(text_rt::trim_end(&clone_string(s)))
}

fn jet_jit_text_pad_start(s: i64, width: i64, fill: i64) -> i64 {
    alloc_string(text_rt::pad_start(
        &clone_string(s),
        width,
        &clone_string(fill),
    ))
}

fn jet_jit_text_pad_end(s: i64, width: i64, fill: i64) -> i64 {
    alloc_string(text_rt::pad_end(
        &clone_string(s),
        width,
        &clone_string(fill),
    ))
}

fn jet_jit_text_index_of(s: i64, needle: i64) -> i64 {
    text_rt::index_of(&clone_string(s), &clone_string(needle))
        .map_or(0, |index| index.wrapping_add(1))
}
fn jet_jit_text_last_index_of(s: i64, needle: i64) -> i64 {
    text_rt::last_index_of(&clone_string(s), &clone_string(needle))
        .map_or(0, |index| index.wrapping_add(1))
}


fn jet_jit_text_count(s: i64, needle: i64) -> i64 {
    text_rt::count(&clone_string(s), &clone_string(needle))
}

fn jet_jit_text_title(s: i64) -> i64 {
    alloc_string(text_rt::title(&clone_string(s)))
}

/// #1476 StringMethod dispatcher. method ids mirror lower_ctx match.
/// Returns i64; bool methods use 0/1 and are narrowed to i8 by the caller.
fn jet_jit_string_method(recv: i64, method: i64, arg0: i64) -> i64 {
    let s = clone_string(recv);
    match method {
        0 => {
            text_rt::last_index_of(&s, &clone_string(arg0)).map_or(0, |index| index.wrapping_add(1))
        }
        1 => i64::from(text_rt::is_lower(&s)),
        2 => i64::from(text_rt::is_upper(&s)),
        3 => alloc_string(text_rt::capitalize(&s)),
        4 => alloc_string(text_rt::swapcase(&s)),
        5 => alloc_string(text_rt::remove_prefix(&s, &clone_string(arg0))),
        6 => alloc_string(text_rt::remove_suffix(&s, &clone_string(arg0))),
        7 => text_rt::compare(&s, &clone_string(arg0)),
        8 => i64::from(s == clone_string(arg0)),
        9 => alloc_string(s),
        10 => alloc_string(text_rt::reverse(&s)),
        11 => alloc_string(text_rt::normalize_nfc(&s)),
        _ => 0,
    }
}

fn jet_jit_text_split_once(s: i64, separator: i64) -> i64 {
    let Some((before, after)) = text_rt::split_once(&clone_string(s), &clone_string(separator))
    else {
        return 0;
    };
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(2);
        let before_id = rt.heap.alloc_string(before);
        let after_id = rt.heap.alloc_string(after);
        let _ = rt.heap.record_set_string(record, 0, before_id);
        let _ = rt.heap.record_set_string(record, 1, after_id);
        record.wrapping_add(1)
    })
}

fn jet_jit_text_cut_last(s: i64, separator: i64) -> i64 {
    let Some((before, after)) = text_rt::cut_last(&clone_string(s), &clone_string(separator))
    else {
        return 0;
    };
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(2);
        let before_id = rt.heap.alloc_string(before);
        let after_id = rt.heap.alloc_string(after);
        let _ = rt.heap.record_set_string(record, 0, before_id);
        let _ = rt.heap.record_set_string(record, 1, after_id);
        record.wrapping_add(1)
    })
}

fn jet_jit_text_center(s: i64, width: i64, fill: i64) -> i64 {
    alloc_string(text_rt::center(
        &clone_string(s),
        width,
        &clone_string(fill),
    ))
}

fn jet_jit_text_starts_any(s: i64, prefixes: i64) -> i8 {
    let prefs = list_of_strings(prefixes);
    i8::from(text_rt::starts_any(&clone_string(s), &prefs))
}

fn jet_jit_text_ends_any(s: i64, suffixes: i64) -> i8 {
    let suffixes = list_of_strings(suffixes);
    i8::from(text_rt::ends_any(&clone_string(s), &suffixes))
}

fn jet_jit_text_rindex(s: i64, needle: i64) -> i64 {
    clone_string(s)
        .rfind(clone_string(needle).as_str())
        .map(|index| index as i64)
        .unwrap_or(-1)
}

fn jet_jit_text_encode(s: i64) -> i64 {
    let text = clone_string(s);
    list_from_bytes(text.as_bytes())
}

fn jet_jit_text_isdecimal(s: i64) -> i8 {
    let text = clone_string(s);
    i8::from(!text.is_empty() && text.chars().all(|character| character.is_ascii_digit()))
}

fn jet_jit_text_isnumeric(s: i64) -> i8 {
    let text = clone_string(s);
    i8::from(!text.is_empty() && text.chars().all(char::is_numeric))
}

fn jet_jit_text_isalnum(s: i64) -> i8 {
    let text = clone_string(s);
    i8::from(!text.is_empty() && text.chars().all(char::is_alphanumeric))
}

fn jet_jit_text_isdigit(s: i64) -> i8 {
    let text = clone_string(s);
    i8::from(!text.is_empty() && text.chars().all(|character| character.is_ascii_digit()))
}

fn jet_jit_text_isidentifier(s: i64) -> i8 {
    let text = clone_string(s);
    let mut chars = text.chars();
    let valid = match chars.next() {
        Some(first) => {
            (first == '_' || first.is_alphabetic())
                && chars.all(|character| character == '_' || character.is_alphanumeric())
        }
        None => false,
    };
    i8::from(valid)
}

fn jet_jit_text_istitle(s: i64) -> i8 {
    let text = clone_string(s);
    let mut has_cased = false;
    let mut expect_upper = true;
    let mut valid = true;
    for character in text.chars() {
        if character.is_uppercase() {
            has_cased = true;
            expect_upper = false;
        } else if character.is_lowercase() {
            has_cased = true;
            if expect_upper {
                valid = false;
                break;
            }
        } else if character.is_alphabetic() {
            valid = false;
            break;
        } else {
            expect_upper = true;
        }
    }
    i8::from(valid && has_cased)
}

fn jet_jit_text_isprintable(s: i64) -> i8 {
    i8::from(clone_string(s).chars().all(|character| !character.is_control()))
}

fn jet_jit_text_expandtabs(s: i64, tabsize: i64) -> i64 {
    let text = clone_string(s);
    let width = tabsize.max(0);
    let mut column = 0i64;
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\t' if width > 0 => {
                let spaces = width - column.rem_euclid(width);
                out.extend(std::iter::repeat(' ').take(spaces as usize));
                column += spaces;
            }
            '\t' => {}
            '\n' | '\r' => {
                out.push(character);
                column = 0;
            }
            _ => {
                out.push(character);
                column += 1;
            }
        }
    }
    alloc_string(out)
}
fn jet_jit_text_zfill(s: i64, width: i64) -> i64 {
    let text = clone_string(s);
    let length = text.chars().count() as i64;
    if width <= length {
        return alloc_string(text);
    }
    let (sign, body) = if let Some(rest) = text.strip_prefix('+') {
        ("+", rest)
    } else if let Some(rest) = text.strip_prefix('-') {
        ("-", rest)
    } else {
        ("", text.as_str())
    };
    alloc_string(format!("{sign}{}{}", "0".repeat((width - length) as usize), body))
}
fn jet_jit_text_char_indices(s: i64) -> i64 {
    list_from_strings(text_rt::char_indices(&clone_string(s)))
}

fn jet_jit_text_inspect(s: i64) -> i64 {
    list_from_strings(text_rt::inspect(&clone_string(s)))
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
        match rt
            .regex_values
            .get(handle.saturating_sub(1) as usize)
            .and_then(|s| s.as_ref())
        {
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
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: sid as u64,
        });
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
fn option_string_result(
    value: text_rt::jet_std::JetOutcome<String, text_rt::jet_std::JetAbsent>,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| match value {
        Ok(text) => {
            let sid = rt.heap.alloc_string(text);
            crate::runtime_host::alloc_jit_result(rt, true, sid as u64)
        }
        Err(_) => crate::runtime_host::alloc_jit_result(rt, false, 0),
    })
}


fn option_int_bits(opt: Option<i64>) -> i64 {
    opt.map_or(0, |value| value.wrapping_add(1))
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
fn list_nested_strings(items: Vec<Vec<String>>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let outer = rt.heap.alloc_empty_list();
        for pair in items {
            let inner = rt.heap.alloc_empty_list();
            for s in pair {
                let sid = rt.heap.alloc_string(s);
                let _ = rt.heap.list_push_int(inner, sid);
            }
            let _ = rt.heap.list_push_int(outer, inner);
        }
        outer
    })
}

fn jet_jit_regex_string_property(
    handle: i64,
    property: impl FnOnce(&text_rt::jet_std::JetRegex) -> String,
) -> i64 {
    with_regex(handle, |value| match value {
        RegexValue::Regex(regex) => Some(property(regex)),
        _ => None,
    })
    .map(alloc_string)
    .unwrap_or_default()
}

fn jet_jit_regex_pattern(handle: i64) -> i64 {
    jet_jit_regex_string_property(handle, |regex| regex.pattern())
}

fn jet_jit_regex_source(handle: i64) -> i64 {
    jet_jit_regex_string_property(handle, |regex| regex.source())
}

fn jet_jit_regex_flags_property(handle: i64) -> i64 {
    jet_jit_regex_string_property(handle, |regex| regex.flags())
}

fn jet_jit_regex_options(handle: i64) -> i64 {
    jet_jit_regex_string_property(handle, |regex| regex.options())
}

fn jet_jit_regex_names(handle: i64) -> i64 {
    with_regex(handle, |value| match value {
        RegexValue::Regex(regex) => Some(regex.names()),
        _ => None,
    })
    .map(list_strings)
    .unwrap_or_default()
}

fn jet_jit_regex_flags(ci: i64, ml: i64, ds: i64) -> i64 {
    push_regex(RegexValue::Flags(text_rt::jet_std::jet_regex_flags(
        ci != 0,
        ml != 0,
        ds != 0,
    )))
}

fn jet_jit_regex_literal(pat: i64) -> i64 {
    push_regex(RegexValue::Regex(text_rt::jet_std::jet_regex_literal(
        &clone_string(pat),
    )))
}

fn jet_jit_regex_is_match(pat: i64, text: i64) -> i8 {
    let t = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| i8::from(regex.is_match(&t)))
        .unwrap_or_default()
}

fn jet_jit_regex_full_match(pat: i64, text: i64) -> i8 {
    let t = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| i8::from(regex.full_match(&t)))
        .unwrap_or_default()
}

fn jet_jit_regex_find(pat: i64, text: i64) -> i64 {
    let t = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| option_string_bits(regex.find(&t).ok()))
        .unwrap_or_default()
}

fn jet_jit_regex_find_all(pat: i64, text: i64) -> i64 {
    let t = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| list_strings(regex.find_all(&t)))
        .unwrap_or_default()
}

fn jet_jit_regex_matches(pat: i64, text: i64) -> i64 {
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
fn jet_jit_regex_count(pat: i64, text: i64) -> i64 {
    let text = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| regex.count(&text))
        .unwrap_or_default()
}

fn jet_jit_regex_replace_method(pat: i64, text: i64, repl: i64) -> i64 {
    jet_jit_regex_replace(pat, repl, text)
}

fn jet_jit_regex_replace_first_method(pat: i64, text: i64, repl: i64) -> i64 {
    jet_jit_regex_replace_first(pat, repl, text)
}

fn regex_replacement_callback(
    callback: i64,
) -> Option<crate::runtime_host::JitCallableSlot> {
    Concurrency::with_runtime_mut(|rt| {
        let Some(slot) = crate::runtime_host::jit_callable_parts(rt, callback) else {
            rt.set_host_fault("JIT Regex replacement callback is invalid");
            return None;
        };
        if slot.raw_unary.is_none() || slot.raw_pair.is_some() || slot.raw_many.is_some() {
            rt.set_host_fault(
                "JIT Regex replacement callback has no unary universal thunk",
            );
            return None;
        }
        Some(slot)
    })
}

fn regex_replacement_callback_trapped() -> bool {
    Concurrency::with_runtime_mut(|rt| crate::runtime_host::runtime_stop_pending(rt))
}

fn jet_jit_regex_replace_all_with(pat: i64, text: i64, callback: i64) -> i64 {
    let Some(callback) = regex_replacement_callback(callback) else {
        return 0;
    };
    let text = clone_string(text);
    let Some(regex) = clone_compiled_regex(pat) else {
        return 0;
    };
    match regex.replace_all_with_result(&text, |matched| {
        let match_handle = push_regex(RegexValue::Match(matched));
        let Some(replacement) =
            crate::runtime_host::invoke_universal_unary(callback, match_handle)
        else {
            return Err("JIT Regex replacement callback has no unary universal thunk".to_string());
        };
        if regex_replacement_callback_trapped() {
            return Err(String::new());
        }
        Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(replacement))
            .ok_or_else(|| {
                "JIT Regex replacement callback returned an invalid String carrier".to_string()
            })
    }) {
        Ok(replaced) => alloc_string(replaced),
        Err(message) => {
            Concurrency::with_runtime_mut(|rt| {
                if !crate::runtime_host::runtime_stop_pending(rt) {
                    rt.set_host_fault(if message.is_empty() {
                        "JIT Regex replacement callback stopped".to_string()
                    } else {
                        message
                    });
                }
            });
            0
        }
    }
}

fn jet_jit_regex_match(pat: i64, text: i64) -> i64 {
    let t = clone_string(text);
    clone_compiled_regex(pat)
        .and_then(|regex| regex.match_value(&t).ok())
        .map(|found| push_regex(RegexValue::Match(found)).wrapping_add(1))
        .unwrap_or_default()
}
fn jet_jit_regex_match_start(handle: i64) -> i64 {
    with_regex(handle, |value| match value {
        RegexValue::Match(matched) => matched.start(),
        _ => 0,
    })
}

fn jet_jit_regex_match_end(handle: i64) -> i64 {
    with_regex(handle, |value| match value {
        RegexValue::Match(matched) => matched.end(),
        _ => 0,
    })
}

fn jet_jit_regex_match_named_captures(handle: i64) -> i64 {
    with_regex(handle, |value| match value {
        RegexValue::Match(matched) => Some(matched.named_captures()),
        _ => None,
    })
    .map(list_nested_strings)
    .unwrap_or_default()
}

fn jet_jit_regex_match_group(handle: i64, group: i64) -> i64 {
    with_regex(handle, |value| match value {
        RegexValue::Match(matched) => Some(matched.clone()),
        _ => None,
    })
    .map(|matched| option_string_bits(matched.group(group).ok()))
    .unwrap_or_default()
}

fn jet_jit_regex_match_name(handle: i64, name: i64) -> i64 {
    let name = clone_string(name);
    with_regex(handle, |value| match value {
        RegexValue::Match(matched) => Some(matched.clone()),
        _ => None,
    })
    .map(|matched| option_string_bits(matched.name(&name).ok()))
    .unwrap_or_default()
}

fn jet_jit_regex_match_group_start(handle: i64, group: i64) -> i64 {
    with_regex(handle, |value| match value {
        RegexValue::Match(matched) => Some(matched.clone()),
        _ => None,
    })
    .map(|matched| option_int_bits(matched.group_start(group).ok()))
    .unwrap_or_default()
}

fn jet_jit_regex_match_group_end(handle: i64, group: i64) -> i64 {
    with_regex(handle, |value| match value {
        RegexValue::Match(matched) => Some(matched.clone()),
        _ => None,
    })
    .map(|matched| option_int_bits(matched.group_end(group).ok()))
    .unwrap_or_default()
}
fn jet_jit_regex_replace_impl(pat: i64, repl: i64, text: i64, first: bool) -> i64 {
    let text = clone_string(text);
    let replacement = clone_string(repl);
    clone_compiled_regex(pat)
        .map(|regex| {
            let replaced = if first {
                regex.replace_first(&text, &replacement)
            } else {
                regex.replace(&text, &replacement)
            };
            Concurrency::with_runtime_mut(|rt| {
                rt.heap.alloc_string(replaced)
            })
        })
        .unwrap_or_default()
}

fn jet_jit_regex_replace(pat: i64, repl: i64, text: i64) -> i64 {
    jet_jit_regex_replace_impl(pat, repl, text, false)
}

fn jet_jit_regex_replace_first(pat: i64, repl: i64, text: i64) -> i64 {
    jet_jit_regex_replace_impl(pat, repl, text, true)
}

fn jet_jit_regex_split(pat: i64, text: i64) -> i64 {
    let t = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| list_strings(regex.split(&t)))
        .unwrap_or_default()
}

fn jet_jit_regex_split_limit(pat: i64, text: i64, limit: i64) -> i64 {
    let text = clone_string(text);
    clone_compiled_regex(pat)
        .map(|regex| list_strings(regex.split_limit(&text, limit)))
        .unwrap_or_default()
}

fn jet_jit_string_matches(text: i64, pattern: i64) -> i64 {
    match text_rt::jet_std::jet_string_matches(&clone_string(text), &clone_string(pattern)) {
        Ok(value) => regex_result_ok(u64::from(value)),
        Err(error) => regex_result_err(error),
    }
}

fn jet_jit_string_match(text: i64, pattern: i64) -> i64 {
    match text_rt::jet_std::jet_string_match(&clone_string(text), &clone_string(pattern)) {
        Ok(value) => regex_result_ok(option_string_result(value) as u64),
        Err(error) => regex_result_err(error),
    }
}

fn jet_jit_regex_compile(pat: i64) -> i64 {
    match text_rt::jet_std::jet_regex_compile(&clone_string(pat)) {
        Ok(rx) => regex_result_ok(push_regex(RegexValue::Regex(rx)) as u64),
        Err(e) => regex_result_err(e),
    }
}

fn jet_jit_regex_compile_with(pat: i64, flags: i64) -> i64 {
    let flags = with_regex(flags, |v| match v {
        RegexValue::Flags(f) => Some(f.clone()),
        _ => None,
    });
    let Some(flags) = flags else {
        return regex_result_err("bad RegexFlags".into());
    };
    match text_rt::jet_std::jet_regex_compile_with(&clone_string(pat), &flags) {
        Ok(rx) => regex_result_ok(push_regex(RegexValue::Regex(rx)) as u64),
        Err(e) => regex_result_err(e),
    }
}


fn jet_jit_regex_escape(text: i64) -> i64 {
    let s = text_rt::jet_std::jet_regex_escape(&clone_string(text));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
}

host_fns! {
    struct TextHostFns;
    register: register_text_symbols;
    declare: declare_text_host_fns(module) {
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
        let mut quaternary = Signature::new(cc);
        for _ in 0..4 {
            quaternary.params.push(AbiParam::new(types::I64));
        }
        quaternary.returns.push(AbiParam::new(types::I64));
    }
    wrap: "jet_text_wrap" => jet_jit_text_wrap: binary;
    fill: "jet_text_fill" => jet_jit_text_fill: binary;
    shorten: "jet_text_shorten" => jet_jit_text_shorten: ternary;
    indent: "jet_text_indent" => jet_jit_text_indent: binary;
    indent_with: "jet_text_indent_with" => jet_jit_text_indent_with: ternary;
    wrap_paragraphs: "jet_text_wrap_paragraphs" => jet_jit_text_wrap_paragraphs: binary;
    hanging_indent: "jet_text_hanging_indent" => jet_jit_text_hanging_indent: quaternary;
    dedent: "jet_text_dedent" => jet_jit_text_dedent: unary;
    expand_tabs: "jet_text_expand_tabs" => jet_jit_text_expand_tabs: binary;
    html_escape: "jet_text_html_escape" => jet_jit_text_html_escape: binary;
    html_escape_quoted: "jet_text_html_escape_quoted" => jet_jit_text_html_escape_quoted: unary;
    html_escape_text: "jet_text_html_escape_text" => jet_jit_text_html_escape_text: unary;
    html_unescape: "jet_text_html_unescape" => jet_jit_text_html_unescape: unary;
    html_strip_tags: "jet_text_html_strip_tags" => jet_jit_text_html_strip_tags: unary;
    html_unescape_and_strip: "jet_text_html_unescape_and_strip" => jet_jit_text_html_unescape_and_strip: unary;
    unicode_lower: "jet_unicode_lower" => jet_jit_text_lower: unary;
    unicode_upper: "jet_unicode_upper" => jet_jit_text_upper: unary;
    ascii_lower: "jet_text_ascii_lower" => jet_jit_text_ascii_lower: unary;
    ascii_upper: "jet_text_ascii_upper" => jet_jit_text_ascii_upper: unary;
    casefold: "jet_text_casefold" => jet_jit_text_casefold: unary;
    unicode_scalar_count: "jet_text_unicode_scalar_count" => jet_jit_text_unicode_scalar_count: unary;
    unicode_byte_count: "jet_text_unicode_byte_count" => jet_jit_text_unicode_byte_count: unary;
    rsplitn: "jet_text_rsplitn" => jet_jit_text_rsplitn: ternary;
    splitn: "jet_text_splitn" => jet_jit_text_splitn: ternary;
    parse_split: "jet_text_parse_split" => jet_jit_text_parse_split: binary;
    parse_rsplit: "jet_text_parse_rsplit" => jet_jit_text_parse_rsplit: ternary;
    parse_split_once: "jet_text_parse_split_once" => jet_jit_text_parse_split_once: binary;
    parse_partition: "jet_text_parse_partition" => jet_jit_text_parse_partition: binary;
    parse_rpartition: "jet_text_parse_rpartition" => jet_jit_text_parse_rpartition: binary;
    parse_split_ws: "jet_text_parse_split_ws" => jet_jit_text_parse_split_ws: unary;
    parse_bool: "jet_text_parse_bool" => jet_jit_text_parse_bool: unary;
    parse_int: "jet_text_parse_int" => jet_jit_text_parse_int: unary;
    parse_int_base: "jet_text_parse_int_base" => jet_jit_text_parse_int_base: binary;
    parse_kv: "jet_text_parse_kv" => jet_jit_text_parse_kv: binary;
    parse_float: "jet_text_parse_float" => jet_jit_text_parse_float: unary;
    parse_find: "jet_text_parse_find" => jet_jit_text_parse_find: binary;
    parse_rfind: "jet_text_parse_rfind" => jet_jit_text_parse_rfind: binary;
    parse_count: "jet_text_parse_count" => jet_jit_text_parse_count: binary;
    parse_replace_n: "jet_text_parse_replace_n" => jet_jit_text_parse_replace_n: quaternary;
    parse_join: "jet_text_parse_join" => jet_jit_text_parse_join: binary;
    parse_splitlines: "jet_text_parse_splitlines" => jet_jit_text_parse_splitlines: binary;
    parse_capwords: "jet_text_parse_capwords" => jet_jit_text_parse_capwords: unary;
    parse_find_from: "jet_text_parse_find_from" => jet_jit_text_parse_find_from: ternary;
    parse_rfind_from: "jet_text_parse_rfind_from" => jet_jit_text_parse_rfind_from: ternary;
    parse_index: "jet_text_parse_index" => jet_jit_text_parse_index: binary;
    parse_contains: "jet_text_parse_contains" => jet_jit_text_parse_contains: binary_i8;
    parse_lstrip: "jet_text_parse_lstrip" => jet_jit_text_parse_lstrip: unary;
    parse_rstrip: "jet_text_parse_rstrip" => jet_jit_text_parse_rstrip: unary;
    parse_strip: "jet_text_parse_strip" => jet_jit_text_parse_strip: unary;
    parse_escape_c: "jet_text_parse_escape_c" => jet_jit_text_parse_escape_c: unary;
    parse_unescape_c: "jet_text_parse_unescape_c" => jet_jit_text_parse_unescape_c: unary;
    string_lines: "jet_string_lines" => jet_jit_string_lines: unary;
    lower: "jet_jit_text_lower" => jet_jit_text_lower: unary;
    upper: "jet_jit_text_upper" => jet_jit_text_upper: unary;
    graphemes: "jet_jit_text_graphemes" => jet_jit_text_graphemes: unary;
    words: "jet_jit_text_words" => jet_jit_text_words: unary;
    sentences: "jet_jit_text_sentences" => jet_jit_text_sentences: unary;
    grapheme_views: "jet_jit_text_grapheme_views" => jet_jit_text_grapheme_views: unary;
    word_views: "jet_jit_text_word_views" => jet_jit_text_word_views: unary;
    line_views: "jet_jit_text_line_views" => jet_jit_text_line_views: unary;
    byte_views: "jet_jit_text_byte_views" => jet_jit_text_byte_views: unary;
    nfc: "jet_jit_text_nfc" => jet_jit_text_nfc: unary;
    nfkc: "jet_jit_text_nfkc" => jet_jit_text_nfkc: unary;
    nfd: "jet_jit_text_nfd" => jet_jit_text_nfd: unary;
    nfkd: "jet_jit_text_nfkd" => jet_jit_text_nfkd: unary;
    caseless_eq: "jet_jit_text_caseless_eq" => jet_jit_text_caseless_eq: binary_i8;
    display_width: "jet_jit_text_display_width" => jet_jit_text_display_width: unary;
    display_width_policy: "jet_jit_text_display_width_policy" => jet_jit_text_display_width_policy: binary;
    is_alphabetic: "jet_jit_text_is_alphabetic" => jet_jit_text_is_alphabetic: unary_i8;
    is_numeric: "jet_jit_text_is_numeric" => jet_jit_text_is_numeric: unary_i8;
    is_whitespace: "jet_jit_text_is_whitespace" => jet_jit_text_is_whitespace: unary_i8;
    is_ascii: "jet_jit_text_is_ascii" => jet_jit_text_is_ascii: unary_i8;
    checked_text_is_alphabetic: "jet_text_is_alphabetic" => jet_jit_text_is_alphabetic: unary_i8;
    checked_text_is_numeric: "jet_text_is_numeric" => jet_jit_text_is_numeric: unary_i8;
    checked_text_is_whitespace: "jet_text_is_whitespace" => jet_jit_text_is_whitespace: unary_i8;
    is_lower: "jet_text_is_lower" => jet_jit_text_is_lower: unary_i8;
    is_upper: "jet_text_is_upper" => jet_jit_text_is_upper: unary_i8;
    capitalize: "jet_text_capitalize" => jet_jit_text_capitalize: unary;
    swapcase: "jet_text_swapcase" => jet_jit_text_swapcase: unary;
    remove_prefix: "jet_text_remove_prefix" => jet_jit_text_remove_prefix: binary;
    remove_suffix: "jet_text_remove_suffix" => jet_jit_text_remove_suffix: binary;
    compare: "jet_text_compare" => jet_jit_text_compare: binary;
    reverse: "jet_text_reverse" => jet_jit_text_reverse: unary;
    checked_text_is_ascii: "jet_text_unicode_is_ascii" => jet_jit_text_is_ascii: unary_i8;
    direct_trim_start: "jet_text_trim_start" => jet_jit_text_trim_start: unary;
    trim_start: "jet_jit_text_trim_start" => jet_jit_text_trim_start: unary;
    trim: "jet_unicode_trim" => jet_jit_text_trim: unary;
    checked_text_trim: "jet_text_trim" => jet_jit_text_trim: unary;
    direct_trim_end: "jet_text_trim_end" => jet_jit_text_trim_end: unary;
    trim_end: "jet_jit_text_trim_end" => jet_jit_text_trim_end: unary;
    direct_pad_start: "jet_text_pad_start" => jet_jit_text_pad_start: ternary;
    pad_start: "jet_jit_text_pad_start" => jet_jit_text_pad_start: ternary;
    direct_pad_end: "jet_text_pad_end" => jet_jit_text_pad_end: ternary;
    pad_end: "jet_jit_text_pad_end" => jet_jit_text_pad_end: ternary;
    index_of: "jet_jit_text_index_of" => jet_jit_text_index_of: binary;
    checked_unicode_index_of: "jet_unicode_index_of" => jet_jit_text_index_of: binary;
    checked_unicode_last_index_of: "jet_unicode_last_index_of" => jet_jit_text_last_index_of: binary;
    checked_unicode_count: "jet_unicode_count" => jet_jit_text_count: binary;
    count: "jet_jit_text_count" => jet_jit_text_count: binary;
    direct_title: "jet_text_title" => jet_jit_text_title: unary;
    title: "jet_jit_text_title" => jet_jit_text_title: unary;
    normalize_nfc: "jet_text_normalize_nfc" => jet_jit_text_nfc: unary;
    split_once: "jet_jit_text_split_once" => jet_jit_text_split_once: binary;
    checked_unicode_split_once: "jet_unicode_split_once" => jet_jit_text_split_once: binary;

    cut_last: "jet_jit_text_cut_last" => jet_jit_text_cut_last: binary;
    checked_unicode_cut_last: "jet_unicode_cut_last" => jet_jit_text_cut_last: binary;
    string_method: "jet_jit_string_method" => jet_jit_string_method: ternary;
    center: "jet_jit_text_center" => jet_jit_text_center: ternary;
    direct_center: "jet_text_center" => jet_jit_text_center: ternary;
    method_center_ref: "jet_text_center_ref" => jet_jit_text_center: ternary;
    method_pad_end_ref: "jet_text_pad_end_ref" => jet_jit_text_pad_end: ternary;
    method_pad_start_ref: "jet_text_pad_start_ref" => jet_jit_text_pad_start: ternary;
    string_center_i64: "jet_text_center_i64" => jet_jit_text_center: ternary;
    string_pad_end_i64: "jet_text_pad_end_i64" => jet_jit_text_pad_end: ternary;
    string_pad_start_i64: "jet_text_pad_start_i64" => jet_jit_text_pad_start: ternary;
    starts_any: "jet_jit_text_starts_any" => jet_jit_text_starts_any: binary_i8;
    ends_any: "jet_text_ends_any" => jet_jit_text_ends_any: binary_i8;
    inspect: "jet_jit_text_inspect" => jet_jit_text_inspect: unary;
    char_indices: "jet_jit_text_char_indices" => jet_jit_text_char_indices: unary;
    parse_rindex: "jet_text_rindex" => jet_jit_text_rindex: binary;
    parse_encode: "jet_text_encode" => jet_jit_text_encode: unary;
    parse_isdecimal: "jet_text_isdecimal" => jet_jit_text_isdecimal: unary_i8;
    parse_isnumeric: "jet_text_isnumeric" => jet_jit_text_isnumeric: unary_i8;
    parse_isalnum: "jet_text_isalnum" => jet_jit_text_isalnum: unary_i8;
    parse_isdigit: "jet_text_isdigit" => jet_jit_text_isdigit: unary_i8;
    parse_isidentifier: "jet_text_isidentifier" => jet_jit_text_isidentifier: unary_i8;
    parse_istitle: "jet_text_istitle" => jet_jit_text_istitle: unary_i8;
    parse_isprintable: "jet_text_isprintable" => jet_jit_text_isprintable: unary_i8;
    parse_expandtabs: "jet_text_expandtabs" => jet_jit_text_expandtabs: binary;
    parse_zfill: "jet_text_zfill" => jet_jit_text_zfill: binary;
    parse_zfill_int: "jet_text_zfill_int" => jet_jit_text_zfill: binary;
    regex_flags: "jet_jit_regex_flags" => jet_jit_regex_flags: ternary;
    regex_escape: "jet_jit_regex_escape" => jet_jit_regex_escape: unary;
    parse_zfill_ref: "jet_text_zfill_ref" => jet_jit_text_zfill: binary;
    regex_literal: "jet_jit_regex_literal" => jet_jit_regex_literal: unary;
    direct_regex_flags: "jet_std::jet_regex_flags" => jet_jit_regex_flags: ternary;
    direct_regex_escape: "jet_std::jet_regex_escape" => jet_jit_regex_escape: unary;
    direct_regex_compile: "jet_std::jet_regex_compile" => jet_jit_regex_compile: unary;
    direct_regex_compile_with: "jet_std::jet_regex_compile_with" => jet_jit_regex_compile_with: binary;
    direct_regex_literal: "jet_std::jet_regex_literal" => jet_jit_regex_literal: unary;
    regex_is_match: "jet_jit_regex_is_match" => jet_jit_regex_is_match: binary_i8;
    regex_full_match: "jet_jit_regex_full_match" => jet_jit_regex_full_match: binary_i8;
    regex_find: "jet_jit_regex_find" => jet_jit_regex_find: binary;
    regex_find_all: "jet_jit_regex_find_all" => jet_jit_regex_find_all: binary;
    regex_matches: "jet_jit_regex_matches" => jet_jit_regex_matches: binary;
    regex_match: "jet_jit_regex_match" => jet_jit_regex_match: binary;
    regex_replace: "jet_jit_regex_replace" => jet_jit_regex_replace: ternary;
    regex_replace_first: "jet_jit_regex_replace_first" => jet_jit_regex_replace_first: ternary;
    string_matches: "jet_std::jet_string_matches" => jet_jit_string_matches: binary;
    string_match: "jet_std::jet_string_match" => jet_jit_string_match: binary;
    regex_split: "jet_jit_regex_split" => jet_jit_regex_split: binary;
    regex_split_limit: "jet_jit_regex_split_limit" => jet_jit_regex_split_limit: ternary;
    direct_regex_is_match: "jet_std::jet_regex_is_match" => jet_jit_regex_is_match: binary_i8;
    direct_regex_full_match: "jet_std::jet_regex_full_match" => jet_jit_regex_full_match: binary_i8;
    direct_regex_match: "jet_std::jet_regex_match" => jet_jit_regex_match: binary;
    direct_regex_find: "jet_std::jet_regex_find" => jet_jit_regex_find: binary;
    direct_regex_find_all: "jet_std::jet_regex_find_all" => jet_jit_regex_find_all: binary;
    direct_regex_matches: "jet_std::jet_regex_matches" => jet_jit_regex_matches: binary;
    direct_regex_split: "jet_std::jet_regex_split" => jet_jit_regex_split: binary;
    direct_regex_split_limit: "jet_std::jet_regex_split_limit" => jet_jit_regex_split_limit: ternary;
    direct_regex_replace: "jet_std::jet_regex_replace" => jet_jit_regex_replace: ternary;
    direct_regex_replace_first: "jet_std::jet_regex_replace_first" => jet_jit_regex_replace_first: ternary;
    regex_compile: "jet_jit_regex_compile" => jet_jit_regex_compile: unary;
    regex_compile_with: "jet_jit_regex_compile_with" => jet_jit_regex_compile_with: binary;
    checked_regex_pattern: "jet_std::JetRegex::pattern" => jet_jit_regex_pattern: unary;
    checked_regex_source: "jet_std::JetRegex::source" => jet_jit_regex_source: unary;
    checked_regex_flags: "jet_std::JetRegex::flags" => jet_jit_regex_flags_property: unary;
    checked_regex_options: "jet_std::JetRegex::options" => jet_jit_regex_options: unary;
    checked_regex_names: "jet_std::JetRegex::names" => jet_jit_regex_names: unary;
    checked_regex_count: "jet_std::JetRegex::count" => jet_jit_regex_count: binary;
    checked_regex_is_match: "jet_std::JetRegex::is_match" => jet_jit_regex_is_match: binary_i8;
    checked_regex_full_match: "jet_std::JetRegex::full_match" => jet_jit_regex_full_match: binary_i8;
    checked_regex_match_value: "jet_std::JetRegex::match_value" => jet_jit_regex_match: binary;
    checked_regex_find: "jet_std::JetRegex::find" => jet_jit_regex_find: binary;
    checked_regex_find_all: "jet_std::JetRegex::find_all" => jet_jit_regex_find_all: binary;
    checked_regex_matches: "jet_std::JetRegex::matches" => jet_jit_regex_matches: binary;
    checked_regex_split: "jet_std::JetRegex::split" => jet_jit_regex_split: binary;
    checked_regex_replace: "jet_std::JetRegex::replace" => jet_jit_regex_replace_method: ternary;
    checked_regex_replace_first: "jet_std::JetRegex::replace_first" => jet_jit_regex_replace_first_method: ternary;
    checked_regex_replace_all_with: "jet_std::JetRegex::replace_all_with" => jet_jit_regex_replace_all_with: ternary;
    checked_regex_split_limit: "jet_std::JetRegex::split_limit" => jet_jit_regex_split_limit: ternary;
    checked_regex_match_start: "jet_std::JetRegexMatch::start" => jet_jit_regex_match_start: unary;
    checked_regex_match_end: "jet_std::JetRegexMatch::end" => jet_jit_regex_match_end: unary;
    checked_regex_match_named_captures: "jet_std::JetRegexMatch::named_captures" => jet_jit_regex_match_named_captures: unary;
    checked_regex_match_group: "jet_std::JetRegexMatch::group" => jet_jit_regex_match_group: binary;
    checked_regex_match_name: "jet_std::JetRegexMatch::name" => jet_jit_regex_match_name: binary;
    checked_regex_match_group_start: "jet_std::JetRegexMatch::group_start" => jet_jit_regex_match_group_start: binary;
    checked_regex_match_group_end: "jet_std::JetRegexMatch::group_end" => jet_jit_regex_match_group_end: binary;
}
