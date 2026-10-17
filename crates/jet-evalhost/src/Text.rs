//! `core.text` hosts (#729). `include!` canonical UnicodeTables + UnicodeString +
//! Top/Text.rs — no third algorithm.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

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
            AlreadyExists(IOContext),
            Unsupported(IOContext),
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
            let not_supported = matches!(
                context.os_code,
                Some(code) if (cfg!(target_os = "linux") && code == 95)
                    || (cfg!(target_os = "macos") && (code == 45 || code == 102))
            );
            if not_supported {
                return IOError::Unsupported(context);
            }
            match e.kind() {
                std::io::ErrorKind::AlreadyExists => IOError::AlreadyExists(context),
                std::io::ErrorKind::Unsupported => IOError::Unsupported(context),
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
                    IOError::InvalidInput(context)
                }
                std::io::ErrorKind::NotFound => IOError::NotFound(context),
                std::io::ErrorKind::PermissionDenied => IOError::PermissionDenied(context),
                std::io::ErrorKind::TimedOut => IOError::TimedOut(context),
                std::io::ErrorKind::AlreadyExists => IOError::AlreadyExists(context),
                std::io::ErrorKind::Unsupported => IOError::Unsupported(context),
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
    pub(crate) fn rsplitn(s: &str, pat: &str, n: i64) -> Vec<String> {
        jet_text_rsplitn(&s.to_string(), &pat.to_string(), n)
    }
    pub(crate) fn splitn(s: &str, pat: &str, n: i64) -> Vec<String> {
        jet_text_splitn(&s.to_string(), &pat.to_string(), n)
    }
    pub(crate) fn words(s: &str) -> Vec<String> {
        jet_text_words(&s.to_string())
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
    pub(crate) fn index_of(s: &str, needle: &str) -> Option<i64> {
        jet_unicode_index_of(&s.to_string(), &needle.to_string())
    }
    pub(crate) fn count(s: &str, needle: &str) -> i64 {
        jet_unicode_count(&s.to_string(), &needle.to_string())
    }
    pub(crate) fn title(s: &str) -> String {
        jet_text_title(&s.to_string())
    }
    pub(crate) fn compare(a: &str, b: &str) -> i64 {
        jet_text_compare(a, b)
    }
    pub(crate) fn reverse(s: &str) -> String {
        jet_text_reverse(&s.to_string())
    }
    pub(crate) fn last_index_of(s: &str, needle: &str) -> Option<i64> {
        jet_unicode_last_index_of(&s.to_string(), &needle.to_string())
    }
    pub(crate) fn split_once(s: &str, separator: &str) -> Option<(String, String)> {
        jet_unicode_split_once(&s.to_string(), &separator.to_string()).ok()
    }
    pub(crate) fn char_indices(s: &str) -> Vec<String> {
        jet_text_char_indices(&s.to_string())
    }
    pub(crate) fn inspect(s: &str) -> Vec<String> {
        jet_text_inspect(&s.to_string())
    }
}

