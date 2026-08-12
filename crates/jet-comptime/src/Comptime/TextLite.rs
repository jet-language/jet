//! CtValue adapters over the canonical Prelude text/Unicode kernel.

mod text_kernel {
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

        pub fn io_error_at(operation: IOOperation, path: &str, error: std::io::Error) -> IOError {
            let context = IOContext::new(
                operation,
                Some(path.to_string()),
                error.raw_os_error().map(i64::from),
                Some(error.to_string()),
            );
            match error.kind() {
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
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../jet-codegen/src/Prelude/Core/UnicodeString.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/Text.rs");

    pub(super) fn nfd(s: &str) -> String {
        jet_text_nfd(&s.to_string())
    }
    pub(super) fn nfkd(s: &str) -> String {
        jet_text_nfkd(&s.to_string())
    }
    pub(super) fn nfc(s: &str) -> String {
        jet_text_nfc(&s.to_string())
    }
    pub(super) fn nfkc(s: &str) -> String {
        jet_text_nfkc(&s.to_string())
    }
    pub(super) fn casefold(s: &str) -> String {
        jet_text_casefold(&s.to_string())
    }
    pub(super) fn alphabetic(cp: u32) -> bool {
        jet_text_alphabetic(cp)
    }
    pub(super) fn letter(cp: u32) -> bool {
        jet_text_letter(cp)
    }
    pub(super) fn numeric(cp: u32) -> bool {
        jet_text_numeric(cp)
    }
    pub(super) fn whitespace(cp: u32) -> bool {
        jet_text_whitespace(cp)
    }
    pub(super) fn lower(s: &str) -> String {
        jet_text_lower(&s.to_string())
    }
    pub(super) fn upper(s: &str) -> String {
        jet_text_upper(&s.to_string())
    }
    pub(super) fn title(s: &str) -> String {
        jet_text_title(&s.to_string())
    }
    pub(super) fn caseless_eq(a: &str, b: &str) -> bool {
        jet_text_caseless_eq(&a.to_string(), &b.to_string())
    }
    pub(super) fn graphemes(s: &str) -> Vec<String> {
        jet_text_graphemes(&s.to_string())
    }
    pub(super) fn word_segments(s: &str) -> Vec<String> {
        jet_text_word_segments(&s.to_string())
    }
    pub(super) fn words(s: &str) -> Vec<String> {
        jet_text_words(&s.to_string())
    }
    pub(super) fn sentence_segments(s: &str) -> Vec<String> {
        jet_text_sentence_segments(&s.to_string())
    }
    pub(super) fn sentences(s: &str) -> Vec<String> {
        jet_text_sentences(&s.to_string())
    }
    pub(super) fn display_width_default(s: &str) -> i64 {
        jet_text_display_width_default(&s.to_string())
    }
    pub(super) fn display_width_policy(
        s: &str,
        ambiguous_wide: bool,
        controls_reject: bool,
    ) -> Result<i64, String> {
        jet_text_display_width_policy(&s.to_string(), ambiguous_wide, controls_reject)
    }
    pub(super) fn is_alphabetic(s: &str) -> bool {
        jet_text_is_alphabetic(&s.to_string())
    }
    pub(super) fn is_numeric(s: &str) -> bool {
        jet_text_is_numeric(&s.to_string())
    }
    pub(super) fn is_whitespace(s: &str) -> bool {
        jet_text_is_whitespace(&s.to_string())
    }
    pub(super) fn is_lower(s: &str) -> bool {
        jet_text_is_lower(&s.to_string())
    }
    pub(super) fn is_upper(s: &str) -> bool {
        jet_text_is_upper(&s.to_string())
    }
    pub(super) fn capitalize(s: &str) -> String {
        jet_text_capitalize(&s.to_string())
    }
    pub(super) fn swapcase(s: &str) -> String {
        jet_text_swapcase(&s.to_string())
    }
    pub(super) fn remove_prefix(s: &str, prefix: &str) -> String {
        jet_text_remove_prefix(&s.to_string(), &prefix.to_string())
    }
    pub(super) fn remove_suffix(s: &str, suffix: &str) -> String {
        jet_text_remove_suffix(&s.to_string(), &suffix.to_string())
    }
    pub(super) fn compare(a: &str, b: &str) -> i64 {
        jet_text_compare(&a.to_string(), &b.to_string())
    }
    pub(super) fn reverse(s: &str) -> String {
        jet_text_reverse(&s.to_string())
    }
    pub(super) fn trim_start(s: &str) -> String {
        jet_text_trim_start(&s.to_string())
    }
    pub(super) fn trim_end(s: &str) -> String {
        jet_text_trim_end(&s.to_string())
    }
    pub(super) fn trim(s: &str) -> String {
        jet_text_trim(&s.to_string())
    }
    pub(super) fn splitn(s: &str, pattern: &str, count: i64) -> Vec<String> {
        jet_text_splitn(&s.to_string(), &pattern.to_string(), count)
    }
    pub(super) fn rsplitn(s: &str, pattern: &str, count: i64) -> Vec<String> {
        jet_text_rsplitn(&s.to_string(), &pattern.to_string(), count)
    }
    pub(super) fn pad_start(s: &str, width: i64, fill: &str) -> String {
        jet_text_pad_start(&s.to_string(), width, &fill.to_string())
    }
    pub(super) fn pad_end(s: &str, width: i64, fill: &str) -> String {
        jet_text_pad_end(&s.to_string(), width, &fill.to_string())
    }
    pub(super) fn center(s: &str, width: i64, fill: &str) -> String {
        jet_text_center(&s.to_string(), width, &fill.to_string())
    }
    pub(super) fn starts_any(s: &str, prefixes: &[String]) -> bool {
        jet_text_starts_any(&s.to_string(), &prefixes.to_vec())
    }
    pub(super) fn ends_any(s: &str, suffixes: &[String]) -> bool {
        jet_text_ends_any(&s.to_string(), &suffixes.to_vec())
    }
    pub(super) fn char_indices(s: &str) -> Vec<String> {
        jet_text_char_indices(&s.to_string())
    }
    pub(super) fn unicode_scalar_count(s: &str) -> i64 {
        jet_text_unicode_scalar_count(&s.to_string())
    }
    pub(super) fn unicode_byte_count(s: &str) -> i64 {
        jet_text_unicode_byte_count(&s.to_string())
    }
    pub(super) fn unicode_is_ascii(s: &str) -> bool {
        jet_text_unicode_is_ascii(&s.to_string())
    }
    pub(super) fn unicode_lower(s: &str) -> String {
        jet_text_unicode_lower(&s.to_string())
    }
    pub(super) fn unicode_upper(s: &str) -> String {
        jet_text_unicode_upper(&s.to_string())
    }
    pub(super) fn unicode_scalars(s: &str) -> Vec<String> {
        jet_text_unicode_scalars(&s.to_string())
    }
}

#[derive(Clone, Copy)]
pub(super) enum IoErrorOperation {
    Read,
    Write,
    Resolve,
}

pub(super) fn io_error_value(
    operation: IoErrorOperation,
    path: &str,
    error: std::io::Error,
) -> crate::AST::CtValue {
    let operation = match operation {
        IoErrorOperation::Read => text_kernel::jet_std::IOOperation::Read,
        IoErrorOperation::Write => text_kernel::jet_std::IOOperation::Write,
        IoErrorOperation::Resolve => text_kernel::jet_std::IOOperation::Resolve,
    };
    let error = text_kernel::jet_std::io_error_at(operation, path, error);
    let operation_value = |operation| {
        let variant = match operation {
            text_kernel::jet_std::IOOperation::Read => "Read",
            text_kernel::jet_std::IOOperation::Write => "Write",
            text_kernel::jet_std::IOOperation::Flush => "Flush",
            text_kernel::jet_std::IOOperation::Connect => "Connect",
            text_kernel::jet_std::IOOperation::Accept => "Accept",
            text_kernel::jet_std::IOOperation::Close => "Close",
            text_kernel::jet_std::IOOperation::Resolve => "Resolve",
            text_kernel::jet_std::IOOperation::Codec => "Codec",
        };
        crate::AST::CtValue::Enum {
            type_name: "IOOperation".to_string(),
            variant: variant.to_string(),
            args: Vec::new(),
        }
    };
    let context_value = |context: text_kernel::jet_std::IOContext| {
        let optional_string = |value: Option<String>| {
            value
                .map(|value| crate::AST::CtValue::Present(Box::new(crate::AST::CtValue::Str(value))))
                .unwrap_or_else(|| crate::AST::CtValue::absent(crate::AST::Type::String))
        };
        let optional_int = |value: Option<i64>| {
            value
                .map(|value| crate::AST::CtValue::Present(Box::new(crate::AST::CtValue::Int(value))))
                .unwrap_or_else(|| crate::AST::CtValue::absent(crate::AST::Type::Int))
        };
        crate::AST::CtValue::Struct {
            type_name: "IOContext".to_string(),
            fields: vec![
                ("operation".to_string(), operation_value(context.operation)),
                ("resource".to_string(), optional_string(context.resource)),
                ("os_code".to_string(), optional_int(context.os_code)),
                ("cause".to_string(), optional_string(context.cause)),
            ],
        }
    };
    let (variant, context) = match error {
        text_kernel::jet_std::IOError::InvalidInput(context) => ("InvalidInput", context),
        text_kernel::jet_std::IOError::NotFound(context) => ("NotFound", context),
        text_kernel::jet_std::IOError::PermissionDenied(context) => {
            ("PermissionDenied", context)
        }
        text_kernel::jet_std::IOError::TimedOut(context) => ("TimedOut", context),
        text_kernel::jet_std::IOError::Cancelled(context) => ("Cancelled", context),
        text_kernel::jet_std::IOError::Closed(context) => ("Closed", context),
        text_kernel::jet_std::IOError::Protocol(context) => ("Protocol", context),
        text_kernel::jet_std::IOError::Other(context) => ("Other", context),
    };
    crate::AST::CtValue::Enum {
        type_name: "IOError".to_string(),
        variant: variant.to_string(),
        args: vec![(None, context_value(context))],
    }
}

pub(super) fn nfd(s: &str) -> String { text_kernel::nfd(s) }
pub(super) fn nfkd(s: &str) -> String { text_kernel::nfkd(s) }
pub(super) fn nfc(s: &str) -> String { text_kernel::nfc(s) }
pub(super) fn nfkc(s: &str) -> String { text_kernel::nfkc(s) }
pub(super) fn casefold(s: &str) -> String { text_kernel::casefold(s) }
pub(super) fn alphabetic(cp: u32) -> bool { text_kernel::alphabetic(cp) }
pub(super) fn letter(cp: u32) -> bool { text_kernel::letter(cp) }
pub(super) fn numeric(cp: u32) -> bool { text_kernel::numeric(cp) }
pub(super) fn whitespace(cp: u32) -> bool { text_kernel::whitespace(cp) }
pub(super) fn lower(s: &str) -> String { text_kernel::lower(s) }
pub(super) fn upper(s: &str) -> String { text_kernel::upper(s) }
pub(super) fn title(s: &str) -> String { text_kernel::title(s) }
pub(super) fn caseless_eq(a: &str, b: &str) -> bool { text_kernel::caseless_eq(a, b) }
pub(super) fn graphemes(s: &str) -> Vec<String> { text_kernel::graphemes(s) }
pub(super) fn word_segments(s: &str) -> Vec<String> { text_kernel::word_segments(s) }
pub(super) fn words(s: &str) -> Vec<String> { text_kernel::words(s) }
pub(super) fn sentence_segments(s: &str) -> Vec<String> { text_kernel::sentence_segments(s) }
pub(super) fn sentences(s: &str) -> Vec<String> { text_kernel::sentences(s) }
pub(super) fn display_width_default(s: &str) -> i64 { text_kernel::display_width_default(s) }
pub(super) fn display_width_policy(s: &str, wide: bool, reject: bool) -> Result<i64, String> {
    text_kernel::display_width_policy(s, wide, reject)
}
pub(super) fn is_alphabetic(s: &str) -> bool { text_kernel::is_alphabetic(s) }
pub(super) fn is_numeric(s: &str) -> bool { text_kernel::is_numeric(s) }
pub(super) fn is_whitespace(s: &str) -> bool { text_kernel::is_whitespace(s) }
pub(super) fn is_lower(s: &str) -> bool { text_kernel::is_lower(s) }
pub(super) fn is_upper(s: &str) -> bool { text_kernel::is_upper(s) }
pub(super) fn capitalize(s: &str) -> String { text_kernel::capitalize(s) }
pub(super) fn swapcase(s: &str) -> String { text_kernel::swapcase(s) }
pub(super) fn remove_prefix(s: &str, prefix: &str) -> String { text_kernel::remove_prefix(s, prefix) }
pub(super) fn remove_suffix(s: &str, suffix: &str) -> String { text_kernel::remove_suffix(s, suffix) }
pub(super) fn compare(a: &str, b: &str) -> i64 { text_kernel::compare(a, b) }
pub(super) fn reverse(s: &str) -> String { text_kernel::reverse(s) }
pub(super) fn trim_start(s: &str) -> String { text_kernel::trim_start(s) }
pub(super) fn trim_end(s: &str) -> String { text_kernel::trim_end(s) }
pub(super) fn trim(s: &str) -> String { text_kernel::trim(s) }
pub(super) fn splitn(s: &str, pattern: &str, count: i64) -> Vec<String> {
    text_kernel::splitn(s, pattern, count)
}
pub(super) fn rsplitn(s: &str, pattern: &str, count: i64) -> Vec<String> {
    text_kernel::rsplitn(s, pattern, count)
}
pub(super) fn pad_start(s: &str, width: i64, fill: &str) -> String {
    text_kernel::pad_start(s, width, fill)
}
pub(super) fn pad_end(s: &str, width: i64, fill: &str) -> String {
    text_kernel::pad_end(s, width, fill)
}
pub(super) fn center(s: &str, width: i64, fill: &str) -> String {
    text_kernel::center(s, width, fill)
}
pub(super) fn starts_any(s: &str, prefixes: &[String]) -> bool {
    text_kernel::starts_any(s, prefixes)
}
pub(super) fn ends_any(s: &str, suffixes: &[String]) -> bool {
    text_kernel::ends_any(s, suffixes)
}
pub(super) fn char_indices(s: &str) -> Vec<String> { text_kernel::char_indices(s) }
pub(super) fn unicode_scalar_count(s: &str) -> i64 { text_kernel::unicode_scalar_count(s) }
pub(super) fn unicode_byte_count(s: &str) -> i64 { text_kernel::unicode_byte_count(s) }
pub(super) fn unicode_is_ascii(s: &str) -> bool { text_kernel::unicode_is_ascii(s) }
pub(super) fn unicode_lower(s: &str) -> String { text_kernel::unicode_lower(s) }
pub(super) fn unicode_upper(s: &str) -> String { text_kernel::unicode_upper(s) }
pub(super) fn unicode_scalars(s: &str) -> Vec<String> { text_kernel::unicode_scalars(s) }
