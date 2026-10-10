//! CtValue adapters over the canonical Prelude text/Unicode kernel.

mod text_kernel {
    pub mod jet_std {
        pub(crate) use super::JetProcessDirectory;

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

        pub fn io_error_at(operation: IOOperation, path: &str, error: std::io::Error) -> IOError {
            let context = IOContext::new(
                operation,
                Some(path.to_string()),
                error.raw_os_error().map(i64::from),
                Some(error.to_string()),
            );
            let not_supported = matches!(
                context.os_code,
                Some(code) if (cfg!(target_os = "linux") && code == 95)
                    || (cfg!(target_os = "macos") && (code == 45 || code == 102))
            );
            if not_supported {
                return IOError::Unsupported(context);
            }
            match error.kind() {
                std::io::ErrorKind::AlreadyExists => IOError::AlreadyExists(context),
                std::io::ErrorKind::Unsupported => IOError::Unsupported(context),
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

        #[derive(Clone, Debug, PartialEq)]
        pub struct Stat {
            pub size: i64,
            pub modified_ms: i64,
            pub created_ms: i64,
            pub readonly: bool,
            pub is_file: bool,
            pub is_dir: bool,
            pub is_symlink: bool,
            pub kind: String,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct WalkEntry {
            pub path: String,
            pub relative: String,
            pub is_dir: bool,
            pub depth: i64,
        }

        include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/MappedFile.rs");
        include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/FileHandles.rs");
        include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/FileLockRaw.rs");
    }

    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs");
    #[allow(unused_imports)]
    pub use jet_unicode::*;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../jet-codegen/src/Prelude/Core/UnicodeString.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    pub(crate) struct JetFileReader {
        pub(crate) inner: std::sync::Mutex<std::io::BufReader<std::fs::File>>,
        pub(crate) path: String,
    }
    pub(crate) struct JetFileWriter {
        pub(crate) inner: std::sync::Mutex<std::io::BufWriter<std::fs::File>>,
        pub(crate) path: String,
    }
    include!("../../../jet-codegen/src/Prelude/Core/StdinReader.rs");

    // TextLite keeps the same runtime Authority/FileScope carrier as AOT; only
    // the CtValue adapter differs at this boundary.
    mod authority_semantics {
        include!("../../../jet-codegen/src/Prelude/Core/Authority.rs");
    }
    fn jet_mapped_path_key(path: &str) -> String {
        authority_semantics::jet_mapped_path_key(path)
    }

    fn jet_mapped_register(key: &str) {
        authority_semantics::jet_mapped_register(key)
    }

    fn jet_mapped_unregister(key: &str) {
        authority_semantics::jet_mapped_unregister(key)
    }
    fn jet_mapped_writer_refusal(
        path: &str,
    ) -> Option<authority_semantics::JetMappedWriterRefusal> {
        authority_semantics::jet_mapped_writer_refusal(path)
    }
    use authority_semantics::{JetAuthority, JetFileScope};
    fn jet_fault_should_fail(_operation: &str) -> bool {
        false
    }
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/Text.rs");
    include!("../../../jet-codegen/src/Prelude/Core/FSIgnore.rs");
    include!("../../../jet-codegen/src/Prelude/Core/FSWalk.rs");
    include!("../../../jet-codegen/src/Prelude/Core/FSWalk8Raw.rs");
    include!("../../../jet-codegen/src/Prelude/Core/FSOps.rs");
    include!("../../../jet-codegen/src/Prelude/Core/FileHandleOwners.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/FSRuntimeOps.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/FSWriteOps.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/FileStream.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/PlatformFamily.rs");

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
        jet_text_compare(a, b)
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
        jet_text_pad_start(
            &s.to_string(),
            jet_foundation::Numeric::JetInt::from_i64(width),
            &fill.to_string(),
        )
    }
    pub(super) fn pad_end(s: &str, width: i64, fill: &str) -> String {
        jet_text_pad_end(
            &s.to_string(),
            jet_foundation::Numeric::JetInt::from_i64(width),
            &fill.to_string(),
        )
    }
    pub(super) fn center(s: &str, width: i64, fill: &str) -> String {
        jet_text_center(
            &s.to_string(),
            jet_foundation::Numeric::JetInt::from_i64(width),
            &fill.to_string(),
        )
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
    pub(super) fn inspect(s: &str) -> Vec<String> {
        jet_text_inspect(&s.to_string())
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
    /// The receiver-first String routes of `string_method_route` whose
    /// AOT/JIT kernel lives in Text.rs. The evaluator calls the same kernel
    /// functions, so `find` stays a byte offset and `partition` keeps the
    /// `(head, sep, tail)` shape on every tier (I9). `None`: not a kernel row.
    pub(super) fn string_route(
        s: &str,
        member: &str,
        args: &[crate::AST::CtValue],
    ) -> Option<crate::AST::CtValue> {
        use crate::AST::CtValue;
        let text = s.to_string();
        let str_arg = |index: usize| match args.get(index) {
            Some(CtValue::Str(value)) => Some(value.clone()),
            _ => None,
        };
        let int_arg = |index: usize| match args.get(index) {
            Some(CtValue::Int(value)) => Some(*value),
            _ => None,
        };
        let triple = |(head, sep, tail): (String, String, String)| CtValue::Struct {
            type_name: "(head,sep,tail)".to_string(),
            fields: vec![
                ("head".to_string(), CtValue::Str(head)),
                ("sep".to_string(), CtValue::Str(sep)),
                ("tail".to_string(), CtValue::Str(tail)),
            ],
        };
        let width = |index: usize| int_arg(index).map(jet_foundation::Numeric::JetInt::from_i64);
        Some(match (member, args.len()) {
            ("casefold", 0) => CtValue::Str(jet_text_casefold(&text)),
            ("strip", 0) => CtValue::Str(jet_text_parse_strip(&text)),
            ("lstrip", 0) => CtValue::Str(jet_text_parse_lstrip(&text)),
            ("rstrip", 0) => CtValue::Str(jet_text_parse_rstrip(&text)),
            ("isalnum", 0) => CtValue::Bool(jet_text_isalnum(&text)),
            ("isalpha", 0) => CtValue::Bool(jet_text_is_alphabetic(&text)),
            ("isascii", 0) => CtValue::Bool(jet_text_unicode_is_ascii(&text)),
            ("isdecimal", 0) => CtValue::Bool(jet_text_isdecimal(&text)),
            ("isdigit", 0) => CtValue::Bool(jet_text_isdigit(&text)),
            ("isidentifier", 0) => CtValue::Bool(jet_text_isidentifier(&text)),
            ("isnumeric", 0) => CtValue::Bool(jet_text_isnumeric(&text)),
            ("isprintable", 0) => CtValue::Bool(jet_text_isprintable(&text)),
            ("isspace", 0) => CtValue::Bool(jet_text_is_whitespace(&text)),
            ("istitle", 0) => CtValue::Bool(jet_text_istitle(&text)),
            ("islower", 0) => CtValue::Bool(jet_text_is_lower(&text)),
            ("isupper", 0) => CtValue::Bool(jet_text_is_upper(&text)),
            ("find", 1) => CtValue::Int(jet_text_parse_find(&text, &str_arg(0)?)),
            ("rfind", 1) => CtValue::Int(jet_text_parse_rfind(&text, &str_arg(0)?)),
            ("partition", 1) => triple(jet_text_parse_partition(&text, &str_arg(0)?)),
            ("rpartition", 1) => triple(jet_text_parse_rpartition(&text, &str_arg(0)?)),
            ("zfill", 1) => CtValue::Str(jet_text_zfill(&text, int_arg(0)?)),
            ("expandtabs", 1) => CtValue::Str(jet_text_expandtabs(&text, width(0)?)),
            ("center", 2) => CtValue::Str(jet_text_center(&text, width(0)?, &str_arg(1)?)),
            ("ljust", 2) => CtValue::Str(jet_text_pad_end(&text, width(0)?, &str_arg(1)?)),
            ("rjust", 2) => CtValue::Str(jet_text_pad_start(&text, width(0)?, &str_arg(1)?)),
            _ => return None,
        })
    }

    // ── D-I9: the ONE `core.files` kernel ────────────────────────────────
    // `Prelude/CoreLib/Top/Text.rs` (included above) owns fault injection,
    // the recursive/non-recursive split and every `IOError` this family
    // reports — the same `jet_std_fs_*` symbols AOT emits and the resident
    // Cranelift host calls. The shared evaluator marshals a path in and a
    // `CtValue` out; it never spells a second `std::fs` call. Hand-written
    // per-member arms are exactly what left `create_dir_all` and `remove_all`
    // with no arm at all while `create_dir` silently ran the recursive one.
    pub(super) fn fs_read(path: &str) -> Result<String, jet_std::IOError> {
        jet_std_fs_read(&path.to_string())
    }
    pub(super) fn fs_scope_read(
        rights: &std::collections::BTreeSet<String>,
        path: &str,
    ) -> Result<String, jet_std::IOError> {
        let authority = JetAuthority::from_rights(rights.iter().cloned().collect());
        let scope = jet_std_fs_scope(&authority);
        jet_std_fs_scope_read(&scope, &path.to_string())
    }
    pub(super) fn fs_read_bytes(path: &str) -> Result<Vec<u8>, jet_std::IOError> {
        jet_std_fs_read_bytes(path)
    }
    pub(super) fn fs_write(path: &str, text: &str) -> Result<(), jet_std::IOError> {
        jet_std_fs_write(&path.to_string(), &text.to_string())
    }
    pub(super) fn fs_append(path: &str, text: &str) -> Result<(), jet_std::IOError> {
        jet_std_fs_append(&path.to_string(), &text.to_string())
    }
    pub(super) fn fs_create_dir(path: &str) -> Result<(), jet_std::IOError> {
        jet_std_fs_create_dir(&path.to_string())
    }
    pub(super) fn fs_create_dir_all(path: &str) -> Result<(), jet_std::IOError> {
        jet_std_fs_create_dir_all(&path.to_string())
    }
    pub(super) fn fs_remove(path: &str) -> Result<(), jet_std::IOError> {
        jet_std_fs_remove(&path.to_string())
    }
    pub(super) fn fs_remove_dir(path: &str) -> Result<(), jet_std::IOError> {
        jet_std_fs_remove_dir(&path.to_string())
    }
    pub(super) fn fs_remove_all(path: &str) -> Result<(), jet_std::IOError> {
        jet_std_fs_remove_all(&path.to_string())
    }
    pub(super) fn fs_copy(from: &str, to: &str) -> Result<(), jet_std::IOError> {
        jet_std_fs_copy(&from.to_string(), &to.to_string())
    }
    pub(super) fn fs_list_dir(path: &str) -> Result<Vec<jet_std::DirEntry>, jet_std::IOError> {
        jet_std_fs_list_dir(&path.to_string())
    }
    pub(super) fn fs_walk_parallel(
        path: &str,
    ) -> Result<Vec<jet_std::WalkEntry>, jet_std::IOError> {
        fs_walk_parallel_with_ignore(path, None)
    }
    pub(super) fn fs_walk_parallel_with_ignore(
        path: &str,
        ignore_name: Option<&str>,
    ) -> Result<Vec<jet_std::WalkEntry>, jet_std::IOError> {
        let mut entries = jet_fs_walk_parallel_with_ignore(
            path,
            path,
            ignore_name,
            |path, relative, is_dir, depth| jet_std::WalkEntry {
                path,
                relative,
                is_dir,
                depth,
            },
            |shown, error| jet_std::io_error_at(jet_std::IOOperation::Read, shown, error),
        )?;
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }
    pub(super) fn fs_walk_files_parallel(
        path: &str,
    ) -> Result<Vec<jet_std::WalkEntry>, jet_std::IOError> {
        fs_walk_files_parallel_with_ignore(path, None)
    }
    pub(super) fn fs_walk_files_parallel_with_ignore(
        path: &str,
        ignore_name: Option<&str>,
    ) -> Result<Vec<jet_std::WalkEntry>, jet_std::IOError> {
        let mut entries = jet_fs_walk_files_parallel_with_ignore(
            path,
            path,
            ignore_name,
            |path, relative, is_dir, depth| jet_std::WalkEntry {
                path,
                relative,
                is_dir,
                depth,
            },
            |shown, error| jet_std::io_error_at(jet_std::IOOperation::Read, shown, error),
        )?;
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }
}

#[derive(Clone, Debug)]
enum MirFileOwnerKind {
    TempDir(text_kernel::JetTempDirOwner),
    TempFile(text_kernel::JetTempFileOwner),
    FileLock(text_kernel::JetFileLockOwner),
}

/// Private runtime carrier used by the MIR interpreter to retain the same
/// provider-owned cleanup value as AOT.
#[derive(Clone, Debug)]
pub struct MirFileOwner {
    inner: MirFileOwnerKind,
}

impl MirFileOwner {
    pub fn path(&self) -> &str {
        match &self.inner {
            MirFileOwnerKind::TempDir(owner) => &owner.path,
            MirFileOwnerKind::TempFile(owner) => &owner.path,
            MirFileOwnerKind::FileLock(owner) => &owner.path,
        }
    }
}

pub fn mir_fs_temp_dir(prefix: &str) -> Result<MirFileOwner, crate::AST::CtValue> {
    text_kernel::jet_std_fs_temp_dir(&prefix.to_string())
        .map(|owner| MirFileOwner {
            inner: MirFileOwnerKind::TempDir(owner),
        })
        .map_err(io_error_ct)
}

pub fn mir_fs_temp_file(prefix: &str) -> Result<MirFileOwner, crate::AST::CtValue> {
    text_kernel::jet_std_fs_temp_file(&prefix.to_string())
        .map(|owner| MirFileOwner {
            inner: MirFileOwnerKind::TempFile(owner),
        })
        .map_err(io_error_ct)
}


pub fn mir_fs_temp_dir_raw(path: String) -> Result<MirFileOwner, crate::AST::CtValue> {
    text_kernel::jet_std_fs_temp_dir_raw(path)
        .map(|owner| MirFileOwner { inner: MirFileOwnerKind::TempDir(owner) })
        .map_err(io_error_ct)
}

pub fn mir_fs_temp_file_raw(path: String) -> Result<MirFileOwner, crate::AST::CtValue> {
    text_kernel::jet_std_fs_temp_file_raw(path)
        .map(|owner| MirFileOwner { inner: MirFileOwnerKind::TempFile(owner) })
        .map_err(io_error_ct)
}

pub fn mir_fs_lock_owner_raw(path: String, fd: i64) -> MirFileOwner {
    MirFileOwner { inner: MirFileOwnerKind::FileLock(text_kernel::jet_std::jet_fs_lock_owner_raw(path, fd)) }
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
    io_error_ct(text_kernel::jet_std::io_error_at(operation, path, error))
}

/// Project the ONE Prelude `IOError` carrier into the evaluator's value
/// carrier. The kernel decides operation, resource, OS code and cause; this
/// only renames the Rust shape into a `CtValue` (I9).
fn io_error_ct(error: text_kernel::jet_std::IOError) -> crate::AST::CtValue {
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
                .map(|value| {
                    crate::AST::CtValue::Present(Box::new(crate::AST::CtValue::Str(value)))
                })
                .unwrap_or_else(|| crate::AST::CtValue::absent(crate::AST::Type::String))
        };
        let optional_int = |value: Option<i64>| {
            value
                .map(|value| {
                    crate::AST::CtValue::Present(Box::new(crate::AST::CtValue::Int(value)))
                })
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
        text_kernel::jet_std::IOError::PermissionDenied(context) => ("PermissionDenied", context),
        text_kernel::jet_std::IOError::TimedOut(context) => ("TimedOut", context),
        text_kernel::jet_std::IOError::Cancelled(context) => ("Cancelled", context),
        text_kernel::jet_std::IOError::Closed(context) => ("Closed", context),

        text_kernel::jet_std::IOError::Protocol(context) => ("Protocol", context),
        text_kernel::jet_std::IOError::Other(context) => ("Other", context),
        text_kernel::jet_std::IOError::AlreadyExists(context) => ("AlreadyExists", context),
        text_kernel::jet_std::IOError::Unsupported(context) => ("Unsupported", context),
    };
    crate::AST::CtValue::Enum {
        type_name: "IOError".to_string(),
        variant: variant.to_string(),
        args: vec![(None, context_value(context))],
    }
}
struct FileReaderHandle(
    std::sync::Arc<std::sync::Mutex<Option<text_kernel::JetFileReader>>>,
);
struct FileWriterHandle(
    std::sync::Arc<std::sync::Mutex<Option<text_kernel::JetFileWriter>>>,
);

fn opaque_file<T: std::any::Any + Send + Sync>(value: T) -> crate::AST::CtValue {
    crate::AST::CtValue::Closure(std::sync::Arc::new(crate::AST::ClosureData {
        lambda: crate::AST::Lambda {
            take_names: Vec::new(),
            params: Vec::new(),
            effects: None,
            body: crate::AST::LambdaBody::Block(Vec::new()),
            span: crate::Diagnostics::Span::new(0, 0),
            meta: crate::AST::LambdaMeta::default(),
        },
        captured: std::collections::HashMap::new(),
        return_type: None,
        opaque: Some(crate::AST::CtOpaque::new(value)),
    }))
}

fn file_reader_handle(
    value: &crate::AST::CtValue,
) -> Option<&FileReaderHandle> {
    let crate::AST::CtValue::Closure(data) = value else {
        return None;
    };
    data.opaque
        .as_ref()
        .and_then(|opaque| opaque.downcast_ref::<FileReaderHandle>())
}

fn file_writer_handle(
    value: &crate::AST::CtValue,
) -> Option<&FileWriterHandle> {
    let crate::AST::CtValue::Closure(data) = value else {
        return None;
    };
    data.opaque
        .as_ref()
        .and_then(|opaque| opaque.downcast_ref::<FileWriterHandle>())
}

fn closed_file_error(operation: text_kernel::jet_std::IOOperation, message: &str) -> crate::AST::CtValue {
    io_error_ct(text_kernel::jet_std::IOError::Closed(
        text_kernel::jet_std::IOContext::new(
            operation,
            None,
            None,
            Some(message.to_string()),
        ),
    ))
}

fn reader_line(
    handle: &FileReaderHandle,
) -> Result<Option<String>, crate::AST::CtValue> {
    let mut reader = handle.0.lock().map_err(|_| {
        io_error_ct(text_kernel::jet_std::IOError::other(
            text_kernel::jet_std::IOOperation::Read,
            None,
            "file reader handle is poisoned",
        ))
    })?;
    let reader = reader
        .as_mut()
        .ok_or_else(|| {
            closed_file_error(
                text_kernel::jet_std::IOOperation::Read,
                "file reader is closed",
            )
        })?;
    text_kernel::jet_std_file_reader_read_line(reader).map_err(io_error_ct)
}

fn writer_line(
    handle: &FileWriterHandle,
    line: &str,
) -> Result<(), crate::AST::CtValue> {
    let mut writer = handle.0.lock().map_err(|_| {
        io_error_ct(text_kernel::jet_std::IOError::other(
            text_kernel::jet_std::IOOperation::Write,
            None,
            "file writer handle is poisoned",
        ))
    })?;
    let writer = writer
        .as_mut()
        .ok_or_else(|| {
            closed_file_error(
                text_kernel::jet_std::IOOperation::Write,
                "file writer is closed",
            )
        })?;
    text_kernel::jet_std_file_writer_write_line(writer, &line.to_string()).map_err(io_error_ct)
}

fn writer_flush(handle: &FileWriterHandle) -> Result<(), crate::AST::CtValue> {
    let mut writer = handle.0.lock().map_err(|_| {
        io_error_ct(text_kernel::jet_std::IOError::other(
            text_kernel::jet_std::IOOperation::Write,
            None,
            "file writer handle is poisoned",
        ))
    })?;
    let writer = writer
        .as_mut()
        .ok_or_else(|| {
            closed_file_error(
                text_kernel::jet_std::IOOperation::Write,
                "file writer is closed",
            )
        })?;
    text_kernel::jet_std_file_writer_flush(writer).map_err(io_error_ct)
}

/// #3593: run one leaf on a live reader; a poisoned or closed handle is an IOError.
fn with_reader<T>(
    handle: &FileReaderHandle,
    run: impl FnOnce(&mut text_kernel::JetFileReader) -> Result<T, text_kernel::jet_std::IOError>,
) -> Result<T, crate::AST::CtValue> {
    let mut reader = handle.0.lock().map_err(|_| {
        io_error_ct(text_kernel::jet_std::IOError::other(
            text_kernel::jet_std::IOOperation::Read,
            None,
            "file reader handle is poisoned",
        ))
    })?;
    let reader = reader.as_mut().ok_or_else(|| {
        closed_file_error(text_kernel::jet_std::IOOperation::Read, "file reader is closed")
    })?;
    run(reader).map_err(io_error_ct)
}

/// #3593: run one leaf on a live writer; a poisoned or closed handle is an IOError.
fn with_writer<T>(
    handle: &FileWriterHandle,
    run: impl FnOnce(&mut text_kernel::JetFileWriter) -> Result<T, text_kernel::jet_std::IOError>,
) -> Result<T, crate::AST::CtValue> {
    let mut writer = handle.0.lock().map_err(|_| {
        io_error_ct(text_kernel::jet_std::IOError::other(
            text_kernel::jet_std::IOOperation::Write,
            None,
            "file writer handle is poisoned",
        ))
    })?;
    let writer = writer.as_mut().ok_or_else(|| {
        closed_file_error(text_kernel::jet_std::IOOperation::Write, "file writer is closed")
    })?;
    run(writer).map_err(io_error_ct)
}

fn handle_result<T>(
    result: Result<T, crate::AST::CtValue>,
    value: impl FnOnce(T) -> crate::AST::CtValue,
) -> crate::AST::CtValue {
    match result {
        Ok(ok) => crate::AST::CtValue::Present(Box::new(value(ok))),
        Err(error) => crate::AST::CtValue::failed(Box::new(error)),
    }
}

/// `[U8]` from a comptime list or byte value.
fn ct_bytes(value: &crate::AST::CtValue) -> Option<Vec<u8>> {
    match value {
        crate::AST::CtValue::Bytes(bytes) => Some(bytes.clone()),
        crate::AST::CtValue::List(values) => values
            .iter()
            .map(|value| match value {
                crate::AST::CtValue::Int(byte) => u8::try_from(*byte).ok(),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn reader_line_value(handle: &FileReaderHandle) -> crate::AST::CtValue {
    match reader_line(handle) {
        Ok(line) => crate::AST::CtValue::Present(Box::new(match line {
            Some(line) => crate::AST::CtValue::Present(Box::new(crate::AST::CtValue::Str(line))),
            None => crate::AST::CtValue::absent(crate::AST::Type::String),
        })),
        Err(error) => crate::AST::CtValue::failed(Box::new(error)),
    }
}

fn writer_result(result: Result<(), crate::AST::CtValue>) -> crate::AST::CtValue {
    match result {
        Ok(()) => crate::AST::CtValue::Present(Box::new(crate::AST::CtValue::Unit)),
        Err(error) => crate::AST::CtValue::failed(Box::new(error)),
    }
}

pub(crate) fn fs_open(path: &str) -> FsResult<crate::AST::CtValue> {
    text_kernel::jet_std_files_open(&path.to_string())
        .map(|reader| {
            opaque_file(FileReaderHandle(std::sync::Arc::new(
                std::sync::Mutex::new(Some(reader)),
            )))
        })
        .map_err(io_error_ct)
}

pub(crate) fn fs_create(path: &str) -> FsResult<crate::AST::CtValue> {
    text_kernel::jet_std_files_create(&path.to_string())
        .map(|writer| {
            opaque_file(FileWriterHandle(std::sync::Arc::new(
                std::sync::Mutex::new(Some(writer)),
            )))
        })
        .map_err(io_error_ct)
}

pub(crate) fn fs_append_stream(path: &str) -> FsResult<crate::AST::CtValue> {
    text_kernel::jet_std_files_append(&path.to_string())
        .map(|writer| {
            opaque_file(FileWriterHandle(std::sync::Arc::new(
                std::sync::Mutex::new(Some(writer)),
            )))
        })
        .map_err(io_error_ct)
}

pub fn file_reader_next_line(
    receiver: &crate::AST::CtValue,
) -> Result<Option<String>, String> {
    let handle = file_reader_handle(receiver)
        .ok_or_else(|| "value is not a FileReader handle".to_string())?;
    reader_line(handle).map_err(|error| error.jet_show())
}

pub(crate) fn apply_file_handle(
    receiver: &crate::AST::CtValue,
    method: &str,
    args: &[crate::AST::CtValue],
    span: crate::Diagnostics::Span,
) -> Option<Result<crate::AST::CtValue, crate::Diagnostics::Diagnostic>> {
    if let Some(handle) = file_reader_handle(receiver) {
        return Some(match method {
            "file_reader.path" if args.is_empty() => {
                let path = handle
                    .0
                    .lock()
                    .ok()
                    .and_then(|reader| reader.as_ref().map(|reader| reader.path.clone()));
                match path {
                    Some(path) => Ok(crate::AST::CtValue::Str(path)),
                    None => Err(crate::Comptime::Diagnostics::unsupported(
                        "FileReader.path requires a live handle",
                        span,
                    )),
                }
            }
            "file_reader.path" => Err(crate::Comptime::Diagnostics::unsupported(
                "FileReader.path takes no arguments",
                span,
            )),
            "file_reader.read_line" | "read_line" if args.is_empty() => {
                Ok(reader_line_value(handle))
            }
            "file_reader.read_line" | "read_line" => Err(
                crate::Comptime::Diagnostics::unsupported(
                    "FileReader.read_line expects no arguments",
                    span,
                ),
            ),
            "file_reader.read" | "read" => {
                let [crate::AST::CtValue::Int(max)] = args else {
                    return Some(Err(crate::Comptime::Diagnostics::unsupported(
                        "FileReader.read expects one Int argument",
                        span,
                    )));
                };
                let max = jet_foundation::Numeric::JetInt::from_i64(*max);
                Ok(handle_result(
                    with_reader(handle, |reader| text_kernel::jet_std_file_reader_read(reader, max)),
                    crate::AST::CtValue::Bytes,
                ))
            }
            _ => Err(crate::Comptime::Diagnostics::unsupported(
                &format!("FileReader.{method}"),
                span,
            )),
        });
    }
    if let Some(handle) = file_writer_handle(receiver) {
        return Some(match method {
            "file_writer.path" if args.is_empty() => {
                let path = handle
                    .0
                    .lock()
                    .ok()
                    .and_then(|writer| writer.as_ref().map(|writer| writer.path.clone()));
                match path {
                    Some(path) => Ok(crate::AST::CtValue::Str(path)),
                    None => Err(crate::Comptime::Diagnostics::unsupported(
                        "FileWriter.path requires a live handle",
                        span,
                    )),
                }
            }
            "file_writer.path" => Err(crate::Comptime::Diagnostics::unsupported(
                "FileWriter.path takes no arguments",
                span,
            )),
            "file_writer.write_line" | "write_line" => {
                let [line] = args else {
                    return Some(Err(crate::Comptime::Diagnostics::unsupported(
                        "FileWriter.write_line expects one String argument",
                        span,
                    )));
                };
                let crate::AST::CtValue::Str(line) = line else {
                    return Some(Err(crate::Comptime::Diagnostics::unsupported(
                        "FileWriter.write_line expects one String argument",
                        span,
                    )));
                };
                Ok(writer_result(writer_line(handle, line)))
            }
            "file_writer.flush" | "flush" if args.is_empty() => {
                Ok(writer_result(writer_flush(handle)))
            }
            "file_writer.write_all" | "write_all" => {
                let Some(bytes) = (match args {
                    [bytes] => ct_bytes(bytes),
                    _ => None,
                }) else {
                    return Some(Err(crate::Comptime::Diagnostics::unsupported(
                        "FileWriter.write_all expects one [U8] argument",
                        span,
                    )));
                };
                Ok(handle_result(
                    with_writer(handle, |writer| text_kernel::jet_std_file_writer_write_all(writer, &bytes)),
                    |()| crate::AST::CtValue::Unit,
                ))
            }
            "file_writer.fsync" | "fsync" if args.is_empty() => Ok(handle_result(
                with_writer(handle, |writer| text_kernel::jet_std_file_writer_fsync(writer)),
                |()| crate::AST::CtValue::Unit,
            )),
            "file_writer.fsync" | "fsync" => Err(crate::Comptime::Diagnostics::unsupported(
                "FileWriter.fsync expects no arguments",
                span,
            )),
            "file_writer.flush" | "flush" => Err(crate::Comptime::Diagnostics::unsupported(
                "FileWriter.flush expects no arguments",
                span,
            )),
            _ => Err(crate::Comptime::Diagnostics::unsupported(
                &format!("FileWriter.{method}"),
                span,
            )),
        });
    }
    None
}

pub(super) fn fs_scope_read(
    rights: &std::collections::BTreeSet<String>,
    path: &str,
) -> FsResult<String> {
    text_kernel::fs_scope_read(rights, path).map_err(io_error_ct)
}
// ── D-I9 `core.files`: one arm per Prelude symbol, no second spelling ──────
// Every helper below is pure marshalling: it hands the resolved path to the
// `jet_std_fs_*` symbol AOT emits and projects that symbol's `IOError` with
// the shared `io_error_ct`. `Ok`/`Err` stay Rust-shaped here so the caller
// decides the outcome carrier for its own row.
pub(super) type FsResult<T> = Result<T, crate::AST::CtValue>;

pub(super) fn fs_read(path: &str) -> FsResult<String> {
    text_kernel::fs_read(path).map_err(io_error_ct)
}
pub(super) fn fs_read_bytes(path: &str) -> FsResult<Vec<u8>> {
    text_kernel::fs_read_bytes(path).map_err(io_error_ct)
}
pub(super) fn fs_write_bytes(path: &str, bytes: &[u8]) -> FsResult<()> {
    text_kernel::jet_std_fs_write_bytes(&path.to_string(), &bytes.to_vec()).map_err(io_error_ct)
}
pub(super) fn fs_write(path: &str, text: &str) -> FsResult<()> {
    text_kernel::fs_write(path, text).map_err(io_error_ct)
}
pub(super) fn fs_append(path: &str, text: &str) -> FsResult<()> {
    text_kernel::fs_append(path, text).map_err(io_error_ct)
}
pub(super) fn fs_fsync(path: &str) -> FsResult<()> {
    text_kernel::jet_std_fs_fsync(&path.to_string()).map_err(io_error_ct)
}
pub(super) fn fs_set_mode(path: &str, mode: i64) -> FsResult<()> {
    text_kernel::jet_std_fs_set_mode(&path.to_string(), mode).map_err(io_error_ct)
}
pub(super) fn fs_create_dir(path: &str) -> FsResult<()> {
    text_kernel::fs_create_dir(path).map_err(io_error_ct)
}
pub(super) fn fs_create_dir_all(path: &str) -> FsResult<()> {
    text_kernel::fs_create_dir_all(path).map_err(io_error_ct)
}
pub(super) fn fs_remove(path: &str) -> FsResult<()> {
    text_kernel::fs_remove(path).map_err(io_error_ct)
}
pub(super) fn fs_remove_dir(path: &str) -> FsResult<()> {
    text_kernel::fs_remove_dir(path).map_err(io_error_ct)
}
pub(super) fn fs_remove_all(path: &str) -> FsResult<()> {
    text_kernel::fs_remove_all(path).map_err(io_error_ct)
}
pub(super) fn fs_copy(from: &str, to: &str) -> FsResult<()> {
    text_kernel::fs_copy(from, to).map_err(io_error_ct)
}
pub(super) fn fs_rename(from: &str, to: &str) -> FsResult<()> {
    text_kernel::jet_fs_rename(from, to).map_err(|error| {
        io_error_ct(text_kernel::jet_std::io_error_at(
            text_kernel::jet_std::IOOperation::Write,
            from,
            error,
        ))
    })
}
pub(super) fn fs_symlink(from: &str, to: &str) -> FsResult<()> {
    text_kernel::jet_std_fs_symlink(&from.to_string(), &to.to_string()).map_err(io_error_ct)
}
pub(super) fn fs_hard_link(from: &str, to: &str) -> FsResult<()> {
    text_kernel::jet_std_fs_hard_link_path(&from.to_string(), &to.to_string()).map_err(io_error_ct)
}
pub(super) fn fs_read_link(path: &str) -> FsResult<String> {
    text_kernel::jet_std_fs_read_link_path(&path.to_string()).map_err(io_error_ct)
}
pub(super) fn fs_canonicalize(path: &str) -> FsResult<String> {
    text_kernel::jet_std_fs_canonicalize(&path.to_string()).map_err(io_error_ct)
}
/// #3593: the interpreter adapter for the `core.files.__*` provider leaves.
/// Each arm only converts CtValue arguments and results; the leaf itself is
/// the shared Prelude FileHandles.rs / FileStream.rs function AOT calls.
/// `Err(String)` is a malformed call, which the caller reports.
pub(crate) fn apply_files_leaf(
    method: &str,
    args: &[crate::AST::CtValue],
) -> Option<Result<crate::AST::CtValue, String>> {
    use crate::AST::CtValue;
    let int = |i: usize| match args.get(i) {
        Some(CtValue::Int(value)) => Ok(*value),
        _ => Err(format!("core.files.{method} argument {i} must be an Int")),
    };
    let boolean = |i: usize| match args.get(i) {
        Some(CtValue::Bool(value)) => Ok(*value),
        _ => Err(format!("core.files.{method} argument {i} must be a Bool")),
    };
    let text = |i: usize| match args.get(i) {
        Some(CtValue::Str(value)) => Ok(value.clone()),
        _ => Err(format!("core.files.{method} argument {i} must be a String")),
    };
    let bytes = |i: usize| match args.get(i) {
        Some(CtValue::Bytes(value)) => Ok(value.clone()),
        Some(CtValue::List(values)) => values
            .iter()
            .map(|value| match value {
                CtValue::Int(byte) => u8::try_from(*byte).map_err(|_| format!("core.files.{method} received an invalid U8")),
                _ => Err(format!("core.files.{method} expects [U8]")),
            })
            .collect::<Result<Vec<u8>, String>>(),
        _ => Err(format!("core.files.{method} argument {i} must be [U8]")),
    };
    fn done<T>(result: Result<T, text_kernel::jet_std::IOError>, value: impl FnOnce(T) -> CtValue) -> CtValue {
        match result {
            Ok(ok) => CtValue::Present(Box::new(value(ok))),
            Err(error) => CtValue::failed(Box::new(io_error_ct(error))),
        }
    }
    fn facts(fields: Vec<(&str, CtValue)>) -> CtValue {
        CtValue::Struct {
            type_name: format!("({})", fields.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(",")),
            fields: fields.into_iter().map(|(name, value)| (name.into(), value)).collect(),
        }
    }
    fn error_facts(kind: i64, code: Option<i64>, cause: Option<String>) -> Vec<(&'static str, CtValue)> {
        vec![
            ("kind", CtValue::Int(kind)),
            ("code", code.map_or_else(|| CtValue::absent(crate::AST::Type::Int), |value| CtValue::Present(Box::new(CtValue::Int(value))))),
            ("cause", cause.map_or_else(|| CtValue::absent(crate::AST::Type::String), |value| CtValue::Present(Box::new(CtValue::Str(value))))),
        ]
    }
    let unit = |_: ()| CtValue::Unit;
    let run = || -> Result<CtValue, String> {
        Ok(match method {
            "__lock_open" | "__lock_try" => {
                let raw = if method == "__lock_open" {
                    text_kernel::jet_std::jet_fs_lock_open_raw(&text(0)?)
                } else { text_kernel::jet_std::jet_fs_lock_try_raw(int(0)?) };
                CtValue::Struct { type_name: "__LockCall".into(), fields: vec![
                    ("value".into(), CtValue::Int(raw.value)),
                    ("errno".into(), CtValue::Int(raw.errno)),
                    ("platform".into(), CtValue::Int(raw.platform)),
                ] }
            }
            "__lock_fd_identity" | "__lock_path_identity" => {
                let raw = if method == "__lock_fd_identity" {
                    text_kernel::jet_std::jet_fs_lock_fd_identity_raw(int(0)?)
                } else { text_kernel::jet_std::jet_fs_lock_path_identity_raw(&text(0)?) };
                CtValue::Struct { type_name: "__LockIdentity".into(), fields: vec![
                    ("device".into(), CtValue::Int(raw.device)),
                    ("low".into(), CtValue::Int(raw.low)),
                    ("high".into(), CtValue::Int(raw.high)),
                    ("regular".into(), CtValue::Bool(raw.regular)),
                    ("reparse".into(), CtValue::Bool(raw.reparse)),
                    ("errno".into(), CtValue::Int(raw.errno)),
                    ("platform".into(), CtValue::Int(raw.platform)),
                ] }
            }
            "__lock_wait" => {
                let raw = text_kernel::jet_std::jet_fs_lock_wait_raw(int(0)?, int(1)?, boolean(2)?, int(3)?);
                CtValue::Struct { type_name: "__LockWait".into(), fields: vec![
                    ("acquired".into(), CtValue::Bool(raw.acquired)),
                    ("timed_out".into(), CtValue::Bool(raw.timed_out)),
                    ("errno".into(), CtValue::Int(raw.errno)),
                    ("platform".into(), CtValue::Int(raw.platform)),
                    ("deadline_ns".into(), CtValue::Int(raw.deadline_ns)),
                ] }
            }
            "__lock_close" => CtValue::Int(text_kernel::jet_std::jet_fs_lock_close_raw(int(0)?)),
            "__entries_context" => CtValue::Int(text_kernel::jet_entries8_open(&text(0)?, &text(1)?, boolean(2)?)),
            "__entries_close" => { text_kernel::jet_entries8_close(int(0)?); CtValue::Unit },
            "__entries_release" => { text_kernel::jet_entries8_release(int(0)?, int(1)?); CtValue::Unit },
            "__entries_shown" => CtValue::Str(text_kernel::jet_entries8_shown(int(0)?, int(1)?)),
            "__entries_before" => CtValue::Bool(text_kernel::jet_entries8_before(int(0)?, int(1)?, int(2)?)),
            "__entries_read_dir" | "__entries_mkdir" | "__entries_copy" => {
                let (kind, code, cause) = match method {
                    "__entries_read_dir" => text_kernel::jet_entries8_read_dir(int(0)?, int(1)?),
                    "__entries_mkdir" => text_kernel::jet_entries8_mkdir(int(0)?, int(1)?),
                    _ => text_kernel::jet_entries8_copy(int(0)?, int(1)?),
                };
                facts(error_facts(kind, code, cause))
            }
            "__entries_next" => {
                let (end, child, kind, code, cause) = text_kernel::jet_entries8_next(int(0)?, int(1)?);
                let mut fields = vec![("end", CtValue::Bool(end)), ("child", CtValue::Int(child))];
                fields.extend(error_facts(kind, code, cause));
                facts(fields)
            }
            "__entries_file_type" => {
                let (directory, file, kind, code, cause) = text_kernel::jet_entries8_file_type(int(0)?, int(1)?);
                let mut fields = vec![("directory", CtValue::Bool(directory)), ("file", CtValue::Bool(file))];
                fields.extend(error_facts(kind, code, cause));
                facts(fields)
            }
            "__os_family" => CtValue::Str(text_kernel::jet_std_os_family()),
            "__process_id_raw" => CtValue::Int(std::process::id() as i64),
            "__map_raw" => done(text_kernel::jet_std::jet_std_files_map(&text(0)?), opaque_file),
            "__remove_raw" => done(text_kernel::jet_std_fs_remove_raw(&text(0)?), unit),
            "__remove_dir_raw" => done(text_kernel::jet_std_fs_remove_dir_raw(&text(0)?), unit),
            "__remove_all_raw" => done(text_kernel::jet_std_fs_remove_all_raw(&text(0)?, boolean(1)?), unit),
            "__copy_raw" => done(text_kernel::jet_std_fs_copy_raw(&text(0)?, &text(1)?), unit),
            "__hard_link_raw" => done(text_kernel::jet_std_fs_hard_link_raw(&text(0)?, &text(1)?), unit),
            "__fsync_raw" => done(text_kernel::jet_std_fs_fsync_raw(&text(0)?), unit),
            "__stat_mode_raw" => done(text_kernel::jet_std_fs_stat_mode_raw(&text(0)?), CtValue::Int),
            "__chown_raw" => done(text_kernel::jet_std_fs_chown_raw(&text(0)?, int(1)?, int(2)?), unit),
            "__set_mode_raw" => done(text_kernel::jet_std_fs_set_mode_raw(&text(0)?, int(1)?), unit),
            "__symlink_directory_raw" => done(text_kernel::jet_std_fs_symlink_directory_raw(&text(0)?), CtValue::Bool),
            "__symlink_raw" => done(text_kernel::jet_std_fs_symlink_raw(&text(0)?, &text(1)?, boolean(2)?), unit),
            "__mkdtemp_raw" => done(text_kernel::jet_std_fs_temp_dir_path_raw(text(0)?), CtValue::Str),
            "__canonicalize_raw" => done(text_kernel::jet_std_fs_canonicalize_raw(&text(0)?), CtValue::Str),
            "__list_dir_raw" => done(text_kernel::jet_std_fs_list_dir_raw(&text(0)?), |entries| {
                CtValue::List(entries.into_iter().map(|entry| CtValue::Struct {
                    type_name: "DirEntry".to_string(), fields: vec![
                        ("name".to_string(), CtValue::Str(entry.name)),
                        ("path".to_string(), CtValue::Str(entry.path)),
                        ("is_dir".to_string(), CtValue::Bool(entry.is_dir)),
                    ],
                }).collect())
            }),
            "__walk_parallel_raw" => {
                let ignore = match args.get(1) {
                    Some(CtValue::Present(value)) => match value.as_ref() {
                        CtValue::Str(value) => Some(value.as_str()),
                        _ => return Err(format!("core.files.{method} ignore must be String?")),
                    },
                    Some(value) if value.is_clean_stop() => None,
                    _ => return Err(format!("core.files.{method} ignore must be String?")),
                };
                done(text_kernel::fs_walk_parallel_with_ignore(&text(0)?, ignore), |entries| {
                    CtValue::List(entries.into_iter().map(|entry| CtValue::Struct {
                        type_name: "WalkEntry".to_string(), fields: vec![
                            ("path".to_string(), CtValue::Str(entry.path)),
                            ("relative".to_string(), CtValue::Str(entry.relative)),
                            ("is_dir".to_string(), CtValue::Bool(entry.is_dir)),
                            ("depth".to_string(), CtValue::Int(entry.depth)),
                        ],
                    }).collect())
                })
            },
            "__create_dir_all_raw" => {
                let path = match args.first() {
                    Some(CtValue::Str(value)) => value,
                    _ => return Err("core.files.__create_dir_all_raw argument 0 must be a String".to_string()),
                };
                done(text_kernel::jet_std_fs_create_dir_all_raw(path), unit)
            },
            "__temp_path_sample" | "__temp_clock_raw" => {
                let (available, nanos) = text_kernel::jet_std::jet_fs_temp_path_sample_raw();
                CtValue::Struct { type_name: "(clock_available,nanos)".to_string(),
                    fields: vec![("clock_available".to_string(), CtValue::Bool(available)), ("nanos".to_string(), CtValue::Str(nanos))] }
            },
            "__directory_fd" => {
                let (available, held, fd) = text_kernel::jet_std::jet_fs_directory_fd_raw(int(0)?);
                CtValue::Struct { type_name: "(available,held,fd)".to_string(), fields: vec![
                    ("available".to_string(), CtValue::Bool(available)), ("held".to_string(), CtValue::Bool(held)),
                    ("fd".to_string(), CtValue::Int(fd.to_i64().expect("directory fd is i32")))] }
            },
            "__temp_directory" | "__temp_directory_raw" => CtValue::Str(text_kernel::jet_std::jet_fs_temp_directory_raw()),
            "__host_path_is_absolute" => CtValue::Bool(text_kernel::jet_std::jet_fs_host_path_is_absolute_raw(&text(0)?)),
            "__host_path_join" => CtValue::Str(text_kernel::jet_std::jet_fs_host_path_join_raw(&text(0)?, &text(1)?)),
            "__metadata_directory" => match text_kernel::jet_std::jet_fs_metadata_directory_raw(&text(0)?) {
                Ok(value) => CtValue::Present(Box::new(CtValue::Bool(value))),
                Err(cause) => CtValue::failed(Box::new(CtValue::Str(cause))),
            },
            "jet_files_link_reader_at_raw" => {
                let handle = args.first().and_then(file_reader_handle).ok_or_else(|| {
                    format!("core.files.{method} argument 0 must be a FileReader")
                })?;
                let reader = handle.0.lock().unwrap_or_else(|poison| poison.into_inner());
                let reader = reader.as_ref().ok_or_else(|| {
                    "compiler-private link requires a live checked FileReader".to_string()
                })?;
                let (available, held, value, errno) =
                    text_kernel::jet_files_link_reader_at_raw(reader, int(1)?, &text(2)?);
                CtValue::Struct {
                    type_name: "(available,held,value,errno)".to_string(),
                    fields: vec![
                        ("available".to_string(), CtValue::Bool(available)),
                        ("held".to_string(), CtValue::Bool(held)),
                        ("value".to_string(), CtValue::Int(value.to_i64().expect("linkat returned i32"))),
                        ("errno".to_string(), CtValue::Int(errno.to_i64().expect("errno was i32"))),
                    ],
                }
            }
            "__open_reader" => done(text_kernel::jet_std_files_open_reader(int(0)?, &text(1)?, int(2)?), |reader| {
                opaque_file(FileReaderHandle(std::sync::Arc::new(std::sync::Mutex::new(Some(reader)))))
            }),
            "__open_writer" => done(
                text_kernel::jet_std_files_open_writer(int(0)?, &text(1)?, boolean(2)?, int(3)?, int(4)?, int(5)?),
                |writer| opaque_file(FileWriterHandle(std::sync::Arc::new(std::sync::Mutex::new(Some(writer))))),
            ),
            "__open_directory" => done(text_kernel::jet_std::jet_fs_directory_open(int(0)?, &text(1)?), CtValue::Int),
            "__close_directory" => {
                text_kernel::jet_std::jet_fs_directory_close(int(0)?);
                CtValue::Unit
            }
            "__fsync_directory" => done(text_kernel::jet_std::jet_fs_directory_fsync(int(0)?), unit),
            "__flock" => done(text_kernel::jet_std::jet_fs_flock(int(0)?, int(1)?), unit),
            "__create_directory" => done(text_kernel::jet_std::jet_fs_directory_create(int(0)?, &text(1)?, int(2)?), unit),
            "__list_directory" => done(text_kernel::jet_std::jet_fs_directory_list(int(0)?), |rows| {
                CtValue::List(rows.into_iter().map(CtValue::Bytes).collect())
            }),
            "__stat_at" => done(text_kernel::jet_std::jet_fs_stat_at(int(0)?, &text(1)?, boolean(2)?), |row| {
                CtValue::List(row.into_iter().map(CtValue::Int).collect())
            }),
            "__path_kind" => done(text_kernel::jet_std::jet_fs_path_kind(&text(0)?), CtValue::Int),
            "__rename_at" => done(
                text_kernel::jet_std::jet_fs_rename_at(int(0)?, &text(1)?, int(2)?, &text(3)?, int(4)?),
                unit,
            ),
            "__remove_at" => done(text_kernel::jet_std::jet_fs_remove_at(int(0)?, &text(1)?, boolean(2)?), unit),
            "__read_link_at" => done(text_kernel::jet_std::jet_fs_read_link_at(int(0)?, &text(1)?), CtValue::Str),
            "__xattr_names" => done(text_kernel::jet_std::jet_fs_xattr_names(&text(0)?), |names| {
                CtValue::List(names.into_iter().map(CtValue::Str).collect())
            }),
            "__xattr_get" => done(text_kernel::jet_std::jet_fs_xattr_get(&text(0)?, &text(1)?), |value| match value {
                Some(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                None => CtValue::absent(crate::AST::Type::List(Box::new(crate::AST::Type::Named("U8".to_string())))),
            }),
            "__xattr_set" => done(text_kernel::jet_std::jet_fs_xattr_set(&text(0)?, &text(1)?, &bytes(2)?), unit),
            "__reader_stat" => {
                let handle = args.first().and_then(file_reader_handle).ok_or_else(|| {
                    format!("core.files.{method} argument 0 must be a FileReader")
                })?;
                handle_result(with_reader(handle, |reader| text_kernel::jet_std_file_reader_stat(reader)), |row| {
                    CtValue::List(row.into_iter().map(CtValue::Int).collect())
                })
            }
            "__writer_stat" => {
                let handle = args.first().and_then(file_writer_handle).ok_or_else(|| {
                    format!("core.files.{method} argument 0 must be a FileWriter")
                })?;
                handle_result(with_writer(handle, |writer| text_kernel::jet_std_file_writer_stat(writer)), |row| {
                    CtValue::List(row.into_iter().map(CtValue::Int).collect())
                })
            }
            "__writer_set_mode" => {
                let handle = args.first().and_then(file_writer_handle).ok_or_else(|| {
                    format!("core.files.{method} argument 0 must be a FileWriter")
                })?;
                let mode = int(1)?;
                handle_result(
                    with_writer(handle, |writer| text_kernel::jet_std_file_writer_set_mode(writer, mode)),
                    unit,
                )
            }
            _ => return Err(format!("core.files.{method} is not a provider leaf")),
        })
    };
    if !method.starts_with("__") {
        return None;
    }
    Some(run())
}
/// `(name, full path, is_dir)` rows in the kernel's own sorted order (D-LSDIR1).
pub(super) fn fs_list_dir(path: &str) -> FsResult<Vec<(String, String, bool)>> {
    text_kernel::fs_list_dir(path)
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| (entry.name, entry.path, entry.is_dir))
                .collect()
        })
        .map_err(io_error_ct)
}
pub(super) fn fs_walk_parallel(path: &str) -> FsResult<Vec<(String, String, bool, i64)>> {
    fs_walk_parallel_with_ignore(path, None)
}
pub(super) fn fs_walk_parallel_with_ignore(
    path: &str,
    ignore_name: Option<&str>,
) -> FsResult<Vec<(String, String, bool, i64)>> {
    text_kernel::fs_walk_parallel_with_ignore(path, ignore_name)
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| (entry.path, entry.relative, entry.is_dir, entry.depth))
                .collect()
        })
        .map_err(io_error_ct)
}
pub(super) fn fs_walk_files_parallel(path: &str) -> FsResult<Vec<(String, String, bool, i64)>> {
    fs_walk_files_parallel_with_ignore(path, None)
}
pub(super) fn fs_walk_files_parallel_with_ignore(
    path: &str,
    ignore_name: Option<&str>,
) -> FsResult<Vec<(String, String, bool, i64)>> {
    text_kernel::fs_walk_files_parallel_with_ignore(path, ignore_name)
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| (entry.path, entry.relative, entry.is_dir, entry.depth))
                .collect()
        })
        .map_err(io_error_ct)
}

pub(super) fn nfd(s: &str) -> String {
    text_kernel::nfd(s)
}
pub(super) fn nfkd(s: &str) -> String {
    text_kernel::nfkd(s)
}
pub(super) fn nfc(s: &str) -> String {
    text_kernel::nfc(s)
}
pub(super) fn nfkc(s: &str) -> String {
    text_kernel::nfkc(s)
}
pub(super) fn casefold(s: &str) -> String {
    text_kernel::casefold(s)
}
pub(super) fn alphabetic(cp: u32) -> bool {
    text_kernel::alphabetic(cp)
}
pub(super) fn letter(cp: u32) -> bool {
    text_kernel::letter(cp)
}
pub(super) fn numeric(cp: u32) -> bool {
    text_kernel::numeric(cp)
}
pub(super) fn whitespace(cp: u32) -> bool {
    text_kernel::whitespace(cp)
}
pub(super) fn lower(s: &str) -> String {
    text_kernel::lower(s)
}
pub(super) fn upper(s: &str) -> String {
    text_kernel::upper(s)
}
pub(super) fn title(s: &str) -> String {
    text_kernel::title(s)
}
pub(super) fn caseless_eq(a: &str, b: &str) -> bool {
    text_kernel::caseless_eq(a, b)
}
pub(super) fn graphemes(s: &str) -> Vec<String> {
    text_kernel::graphemes(s)
}
pub(super) fn word_segments(s: &str) -> Vec<String> {
    text_kernel::word_segments(s)
}
pub(super) fn words(s: &str) -> Vec<String> {
    text_kernel::words(s)
}
pub(super) fn sentence_segments(s: &str) -> Vec<String> {
    text_kernel::sentence_segments(s)
}
pub(super) fn sentences(s: &str) -> Vec<String> {
    text_kernel::sentences(s)
}
pub(super) fn display_width_default(s: &str) -> i64 {
    text_kernel::display_width_default(s)
}
pub(super) fn display_width_policy(s: &str, wide: bool, reject: bool) -> Result<i64, String> {
    text_kernel::display_width_policy(s, wide, reject)
}
pub(super) fn is_alphabetic(s: &str) -> bool {
    text_kernel::is_alphabetic(s)
}
pub(super) fn is_numeric(s: &str) -> bool {
    text_kernel::is_numeric(s)
}
pub(super) fn is_whitespace(s: &str) -> bool {
    text_kernel::is_whitespace(s)
}
pub(super) fn is_lower(s: &str) -> bool {
    text_kernel::is_lower(s)
}
pub(super) fn is_upper(s: &str) -> bool {
    text_kernel::is_upper(s)
}
pub(super) fn capitalize(s: &str) -> String {
    text_kernel::capitalize(s)
}
pub(super) fn swapcase(s: &str) -> String {
    text_kernel::swapcase(s)
}
pub(super) fn remove_prefix(s: &str, prefix: &str) -> String {
    text_kernel::remove_prefix(s, prefix)
}
pub(super) fn remove_suffix(s: &str, suffix: &str) -> String {
    text_kernel::remove_suffix(s, suffix)
}
pub(super) fn compare(a: &str, b: &str) -> i64 {
    text_kernel::compare(a, b)
}
pub(super) fn reverse(s: &str) -> String {
    text_kernel::reverse(s)
}
pub(super) fn trim_start(s: &str) -> String {
    text_kernel::trim_start(s)
}
pub(super) fn trim_end(s: &str) -> String {
    text_kernel::trim_end(s)
}
pub(super) fn trim(s: &str) -> String {
    text_kernel::trim(s)
}
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
pub(super) fn char_indices(s: &str) -> Vec<String> {
    text_kernel::char_indices(s)
}
pub(super) fn inspect(s: &str) -> Vec<String> {
    text_kernel::inspect(s)
}
pub(super) fn string_count(s: &str, needle: &str) -> i64 {
    text_kernel::jet_unicode_count(s, needle)
}
pub(super) fn unicode_scalar_count(s: &str) -> i64 {
    text_kernel::unicode_scalar_count(s)
}
pub(super) fn unicode_byte_count(s: &str) -> i64 {
    text_kernel::unicode_byte_count(s)
}
pub(super) fn unicode_is_ascii(s: &str) -> bool {
    text_kernel::unicode_is_ascii(s)
}
pub(super) fn unicode_lower(s: &str) -> String {
    text_kernel::unicode_lower(s)
}
pub(super) fn unicode_upper(s: &str) -> String {
    text_kernel::unicode_upper(s)
}
pub(super) fn unicode_scalars(s: &str) -> Vec<String> {
    text_kernel::unicode_scalars(s)
}
pub(super) fn string_route(
    s: &str,
    member: &str,
    args: &[crate::AST::CtValue],
) -> Option<crate::AST::CtValue> {
    text_kernel::string_route(s, member, args)
}

#[cfg(test)]
mod tests {
    #[test]
    fn process_directory_carrier_has_one_owner_across_the_text_kernel() {
        assert_eq!(
            std::any::TypeId::of::<super::text_kernel::JetProcessDirectory>(),
            std::any::TypeId::of::<super::text_kernel::jet_std::JetProcessDirectory>(),
        );
    }
}
