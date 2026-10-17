//! Host shims for `core.sys`, `core.log`, `core.math`, and `core.files`,
//! plus `core.sys` and `core.process` CoreCalls (#729). Behavior
//! mirrors AOT helpers in the CoreLib prelude (`jet_std_os_*`, `jet_ring_log_*`,
//! `jet_std_math_*`, `jet_std_fs_*`, `jet_std_path_*`, `jet_std_env_*`,
//! `jet_std_process_*`) — thin std wrappers, not a third algorithm.
//! parity: guard tests/dev_tier_parity.rs::io_cli_terminal_and_time_match_interpreter_jit_and_aot

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

use jet_foundation::Devtools::*;

mod path_kernel {
    include!("../../jet-codegen/src/Prelude/Core/Path.rs");
}

mod fs_walk_kernel {
    include!("../../jet-codegen/src/Prelude/Core/FSIgnore.rs");
    include!("../../jet-codegen/src/Prelude/Core/FSWalk.rs");
    include!("../../jet-codegen/src/Prelude/Core/FSWalk8Raw.rs");
    use jet_foundation::AST::{CtValue, Type};
    fn tuple(fields: Vec<(&str, CtValue)>) -> CtValue {
        CtValue::Struct {
            type_name: format!("({})", fields.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(",")),
            fields: fields.into_iter().map(|(name, value)| (name.into(), value)).collect(),
        }
    }
    fn error_fields(kind: i64, code: Option<i64>, cause: Option<String>) -> Vec<(&'static str, CtValue)> {
        vec![
            ("kind", CtValue::Int(kind)),
            ("code", code.map_or_else(|| CtValue::absent(Type::Int), |value| CtValue::Present(Box::new(CtValue::Int(value))))),
            ("cause", cause.map_or_else(|| CtValue::absent(Type::String), |value| CtValue::Present(Box::new(CtValue::Str(value))))),
        ]
    }
    pub(crate) fn ambient_core_call(
        module: &str, method: &str, args: Vec<CtValue>,
        _span: jet_foundation::Diagnostics::Span, _resolved_ret: Option<Type>,
        _sink: Option<&mut jet_codegen::Comptime::DevSink>,
    ) -> Option<Result<CtValue, jet_foundation::Diagnostics::Diagnostic>> {
        if module != "core.files" { return None; }
        let value = match (method, args.as_slice()) {
            ("__entries_context", [CtValue::Str(root), CtValue::Str(destination), CtValue::Bool(copying)]) => CtValue::Int(jet_entries8_open(root, destination, *copying)),
            ("__entries_close", [CtValue::Int(context)]) => { jet_entries8_close(*context); CtValue::Unit },
            ("__entries_release", [CtValue::Int(context), CtValue::Int(slot)]) => { jet_entries8_release(*context, *slot); CtValue::Unit },
            ("__entries_shown", [CtValue::Int(context), CtValue::Int(slot)]) => CtValue::Str(jet_entries8_shown(*context, *slot)),
            ("__entries_before", [CtValue::Int(context), CtValue::Int(left), CtValue::Int(right)]) => CtValue::Bool(jet_entries8_before(*context, *left, *right)),
            ("__entries_read_dir" | "__entries_mkdir" | "__entries_copy", [CtValue::Int(context), CtValue::Int(slot)]) => {
                let (kind, code, cause) = match method {
                    "__entries_read_dir" => jet_entries8_read_dir(*context, *slot),
                    "__entries_mkdir" => jet_entries8_mkdir(*context, *slot),
                    _ => jet_entries8_copy(*context, *slot),
                };
                tuple(error_fields(kind, code, cause))
            }
            ("__entries_next", [CtValue::Int(context), CtValue::Int(slot)]) => {
                let (end, child, kind, code, cause) = jet_entries8_next(*context, *slot);
                let mut fields = vec![("end", CtValue::Bool(end)), ("child", CtValue::Int(child))];
                fields.extend(error_fields(kind, code, cause));
                tuple(fields)
            }
            ("__entries_file_type", [CtValue::Int(context), CtValue::Int(slot)]) => {
                let (directory, file, kind, code, cause) = jet_entries8_file_type(*context, *slot);
                let mut fields = vec![("directory", CtValue::Bool(directory)), ("file", CtValue::Bool(file))];
                fields.extend(error_fields(kind, code, cause));
                tuple(fields)
            }
            ("__walk_context", [CtValue::Str(root), CtValue::Str(ignore), CtValue::Bool(enabled)]) => CtValue::Int(jet_walk8_open(root, ignore, *enabled)),
            ("__walk_close", [CtValue::Int(context)]) => { jet_walk8_close(*context); CtValue::Unit },
            ("__walk_matcher_new", [CtValue::Int(context)]) => { jet_walk8_matcher_new(*context); CtValue::Unit },
            ("__walk_validation_path", [CtValue::Int(context), CtValue::Int(slot)]) => CtValue::Int(jet_walk8_validation_path(*context, *slot)),
            ("__walk_ancestors", [CtValue::Int(context), CtValue::Int(slot)]) => CtValue::List(jet_walk8_ancestors(*context, *slot).into_iter().map(CtValue::Int).collect()),
            ("__walk_shown", [CtValue::Int(context), CtValue::Int(slot)]) => CtValue::Str(jet_walk8_shown(*context, *slot)),
            ("__walk_before", [CtValue::Int(context), CtValue::Int(left), CtValue::Int(right)]) => CtValue::Bool(jet_walk8_before(*context, *left, *right)),
            ("__walk_inherit", [CtValue::Int(context), CtValue::Int(child), CtValue::Int(parent)]) => { jet_walk8_inherit(*context, *child, *parent); CtValue::Unit },
            ("__walk_ignored", [CtValue::Int(context), CtValue::Int(slot), CtValue::Str(relative), CtValue::Bool(directory)]) => CtValue::Bool(jet_walk8_ignored(*context, *slot, relative, *directory)),
            ("__walk_rules", [CtValue::Int(context), CtValue::Int(slot), CtValue::Str(filename)]) => {
                let (kind, code, cause) = jet_walk8_rules(*context, *slot, filename);
                tuple(error_fields(kind, code, cause))
            }
            ("__walk_read_dir", [CtValue::Int(context), CtValue::Int(slot)]) => {
                let (kind, code, cause) = jet_walk8_read_dir(*context, *slot);
                tuple(error_fields(kind, code, cause))
            }
            ("__walk_metadata", [CtValue::Int(context), CtValue::Int(slot)]) => {
                let (directory, symlink, kind, code, cause) = jet_walk8_metadata(*context, *slot);
                let mut fields = vec![("directory", CtValue::Bool(directory)), ("symlink", CtValue::Bool(symlink))];
                fields.extend(error_fields(kind, code, cause));
                tuple(fields)
            }
            ("__walk_next", [CtValue::Int(context), CtValue::Int(slot)]) => {
                let (end, child, directory, file, path, relative, ignore_file, kind, code, cause) = jet_walk8_next(*context, *slot);
                let mut fields = vec![
                    ("end", CtValue::Bool(end)), ("child", CtValue::Int(child)),
                    ("directory", CtValue::Bool(directory)), ("file", CtValue::Bool(file)),
                    ("path", CtValue::Str(path)), ("relative", CtValue::Str(relative)),
                    ("ignore_file", CtValue::Bool(ignore_file)),
                ];
                fields.extend(error_fields(kind, code, cause));
                tuple(fields)
            }
            _ => return None,
        };
        Some(Ok(value))
    }
}
pub(crate) use fs_walk_kernel::ambient_core_call as walk_ambient_core_call;

mod fs_ops_kernel {
    use crate::Collections::authority_semantics::JetFileScope;
    include!("../../jet-codegen/src/Prelude/Core/FSOps.rs");
}

mod keep_kernel {
}
pub(crate) mod jet_std {
    #[derive(Clone, Debug, PartialEq)]
    pub(crate) struct LogField {
        pub(crate) key: String,
        pub(crate) value: String,
        pub(crate) kind: String,
        pub(crate) redacted: bool
    }

    #[derive(Clone, Debug, PartialEq, Default)]
    pub(crate) struct LogSpan {
        pub(crate) id: i64,
        pub(crate) name: String
    }
}

fn jet_log_write_line(line: &str) {
    let frame = super::IO::term_prelude::jet_term_print_frame(line);
    let _ = super::IO::term_prelude::jet_term_write_stderr_bytes(frame.as_bytes(), false);
}

/// The evaluator owns process exit; a host-side log exit request is not an
/// exit channel.
fn jet_log_process_exit(_code: i64) {}

include!("../../jet-codegen/src/Prelude/Core/LogState.rs");
include!("../../jet-codegen/src/Prelude/CoreLib/Top/Log.rs");
include!("../../jet-codegen/src/Prelude/CoreLib/Top/CoreSlice7LogRaw.rs");

// The Prelude owns every core.sys fact. This module supplies only the small
// type/ambient surface needed to include that exact source; wrappers below
// marshal its Rust values to the resident heap ABI.
pub(crate) mod os_rt {
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs");

    pub(crate) fn jet_mapped_path_key(path: &str) -> String {
        crate::Collections::authority_semantics::jet_mapped_path_key(path)
    }

    pub(crate) fn jet_mapped_register(key: &str) {
        crate::Collections::authority_semantics::jet_mapped_register(key)
    }

    pub(crate) fn jet_mapped_unregister(key: &str) {
        crate::Collections::authority_semantics::jet_mapped_unregister(key)
    }

    pub(crate) fn jet_mapped_writer_refusal(
        path: &str,
    ) -> Option<crate::Collections::authority_semantics::JetMappedWriterRefusal> {
        crate::Collections::authority_semantics::jet_mapped_writer_refusal(path)
    }

    pub(crate) mod jet_std {
        pub(crate) fn jet_int_owned_from_i64(value: i64) -> jet_foundation::Numeric::JetInt {
            jet_foundation::Numeric::JetInt::from_i64(value)
        }
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub(crate) enum IOOperation {
            Read,
            Write,
            Flush,
            Connect,
            Accept,
            Close,
            Resolve,
            Codec
        }

        #[derive(Clone, Debug, PartialEq)]
        pub(crate) struct IOContext {
            pub(crate) operation: IOOperation,
            pub(crate) resource: Option<String>,
            pub(crate) os_code: Option<i64>,
            pub(crate) cause: Option<String>
        }

        impl IOContext {
            // Mirror of Prelude/CoreLib/JetStd/Open.rs:42-48; host fields already use Option.
            pub(crate) fn new(
                operation: IOOperation,
                resource: Option<String>,
                os_code: Option<i64>,
                cause: Option<String>,
            ) -> Self {
                Self { operation, resource, os_code, cause }
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        pub(crate) enum IOError {
            InvalidInput(IOContext),
            NotFound(IOContext),
            PermissionDenied(IOContext),
            TimedOut(IOContext),
            Cancelled(IOContext),
            Closed(IOContext),
            Protocol(IOContext),
            Other(IOContext),
            AlreadyExists(IOContext),
            Unsupported(IOContext)
        }

        impl IOError {
            pub(crate) fn other(
                operation: IOOperation,
                resource: Option<String>,
                cause: impl ToString,
            ) -> Self {
                Self::Other(IOContext {
                    operation,
                    resource,
                    os_code: None,
                    cause: Some(cause.to_string())
                })
            }
        }

        pub(crate) fn io_error_at(
            operation: IOOperation,
            path: &str,
            error: std::io::Error,
        ) -> IOError {
            let context = IOContext {
                operation,
                resource: Some(path.to_string()),
                os_code: error.raw_os_error().map(i64::from),
                cause: Some(error.to_string())
            };
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
                std::io::ErrorKind::AlreadyExists => IOError::AlreadyExists(context),
                std::io::ErrorKind::Unsupported => IOError::Unsupported(context),
                std::io::ErrorKind::NotConnected | std::io::ErrorKind::BrokenPipe => {
                    IOError::Closed(context)
                }
                _ => IOError::Other(context)
            }
        }
        include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/MappedFile.rs");
    }

    fn jet_std_env_get(name: &str) -> Option<String> {
        super::jit_env_value(name)
    }

    mod prelude_impl {
        use super::{jet_std, jet_std_env_get,};


        include!("../../jet-codegen/src/Prelude/CoreLib/Top/OsExtra.rs");

    }

    pub(crate) use prelude_impl::*;
}

// FSRuntimeOps.rs is the one policy-bearing filesystem fragment. The
// resident host supplies only these raw kernels and its result marshaller.
mod fs_prelude {
    use crate::Collections::authority_semantics::{JetAuthority, JetFileScope};
    use crate::Text::text_rt::{jet_fs_remove_entry, jet_view_iter_from_iter, JetViewIter};
    use super::fs_ops_kernel::{
        jet_fs_canonicalize, jet_fs_glob, jet_fs_rename, jet_fs_scope_read
    };
    use super::os_rt::jet_std;
    use crate::fault_injection::jet_fault_should_fail;

    include!("../../jet-codegen/src/Prelude/Core/FileHandleOwners.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/FSRuntimeOps.rs");
}
mod fs_write_prelude {
    use super::os_rt::jet_std;
    use crate::fault_injection::jet_fault_should_fail;

    include!("../../jet-codegen/src/Prelude/Core/StdinReader.rs");
}

// ── core.sys (Prelude facts; these functions only marshal values) ─────────────

/// The Prelude raw executable-path fact, for the ambient interpreter route.
pub(crate) fn os_executable_text() -> Option<String> {
    os_rt::jet_sys_executable_raw()
}
pub(crate) fn jet_jit_os_pid() -> i64 {
    os_rt::jet_std_os_pid()
}

// ── core.log (shared with the AOT Prelude) ───────────────────────────────────

// ── core.files and typed Path (mirrors jet_std_fs_* / jet_std_path_*) ────────

pub(crate) type JitEnvEntries = Vec<(std::ffi::OsString, std::ffi::OsString)>;

fn jit_env_table() -> &'static std::sync::RwLock<JitEnvEntries> {
    static TABLE: std::sync::OnceLock<std::sync::RwLock<JitEnvEntries>> =
        std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut entries: JitEnvEntries = Vec::new();
        for (name, value) in std::env::vars_os() {
            if let Some(old) = entries
                .iter()
                .position(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), name.as_os_str()))
            {
                entries.remove(old);
            }
            entries.push((name, value));
        }
        std::sync::RwLock::new(entries)
    })
}

pub(crate) fn jit_env_read() -> std::sync::RwLockReadGuard<'static, JitEnvEntries> {
    jit_env_table()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn jit_env_write() -> std::sync::RwLockWriteGuard<'static, JitEnvEntries> {
    jit_env_table()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(unix)]
fn jit_env_key_cmp(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> std::cmp::Ordering {
    use std::os::unix::ffi::OsStrExt;
    left.as_bytes().cmp(right.as_bytes())
}

// JET_VETTED_UNSAFE_BEGIN: jit_env_windows
#[cfg(windows)]
fn jit_env_key_cmp(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> std::cmp::Ordering {
    use std::os::windows::ffi::OsStrExt;
    extern "system" {
        fn CompareStringOrdinal(
            left: *const u16,
            left_len: i32,
            right: *const u16,
            right_len: i32,
            ignore_case: i32,
        ) -> i32;
    }
    let left: Vec<u16> = left.encode_wide().collect();
    let right: Vec<u16> = right.encode_wide().collect();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return left.cmp(&right);
    };
    let result =
        unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) };
    match result {
        1 => std::cmp::Ordering::Less,
        2 => std::cmp::Ordering::Equal,
        3 => std::cmp::Ordering::Greater,
        _ => left.cmp(&right)
    }
}
// JET_VETTED_UNSAFE_END: jit_env_windows

#[cfg(not(any(unix, windows)))]
fn jit_env_key_cmp(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> std::cmp::Ordering {
    left.cmp(right)
}

pub(crate) fn jit_env_key_eq(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> bool {
    jit_env_key_cmp(left, right) == std::cmp::Ordering::Equal
}

pub(crate) fn jit_env_validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.contains('\0') || name.contains('=') {
        Err("invalid environment variable name")
    } else {
        Ok(())
    }
}

pub(crate) fn jit_env_validate_value(value: &str) -> Result<(), &'static str> {
    if value.contains('\0') {
        Err("invalid environment variable value")
    } else {
        Ok(())
    }
}

pub(crate) fn jit_env_snapshot_raw() -> JitEnvEntries {
    jit_env_read().clone()
}

// Canonical column transport: one locked logical snapshot, not indexed rereads.
pub(crate) fn jet_sys_env_columns_raw() -> Vec<Vec<u8>> {
    jit_env_snapshot_raw()
        .into_iter()
        .flat_map(|(name, value)| [name.into_encoded_bytes(), value.into_encoded_bytes()])
        .collect()
}

// Match FSIoEnvOsTesting.rs's immutable host bootstrap, separate from the
// mutable logical table. Source owns entry pairing and environment policy.
pub(crate) fn jet_sys_startup_env_columns_raw() -> Vec<Vec<u8>> {
    static STARTUP: std::sync::OnceLock<Vec<Vec<u8>>> = std::sync::OnceLock::new();
    STARTUP
        .get_or_init(|| {
            std::env::vars_os()
                .flat_map(|(name, value)| [name.into_encoded_bytes(), value.into_encoded_bytes()])
                .collect()
        })
        .clone()
}

// JET_VETTED_UNSAFE_BEGIN: jit_env_windows
#[cfg(windows)]
pub(crate) fn jet_sys_env_key_compare_raw(left: &Vec<u8>, right: &Vec<u8>) -> i64 {
    // SAFETY: source passes complete valid Unicode names or unmodified
    // platform OsString encoded bytes, exactly like the AOT comparison leaf.
    let left = unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(left) };
    let right = unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(right) };
    match jit_env_key_cmp(left, right) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}
// JET_VETTED_UNSAFE_END: jit_env_windows

#[cfg(not(windows))]
pub(crate) fn jet_sys_env_key_compare_raw(left: &Vec<u8>, right: &Vec<u8>) -> i64 {
    match left.cmp(right) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

// #2003 / D-ENV-MUTATE1: one raw-by-name read over Jet's logical environment
// table, for every in-process engine. The resident JIT host (`IO.rs`), the
// `core.sys` shims above, the extern "C" heap-ABI shims below and the
// interpreter ambient (`ambient_interp`) all call THIS — an engine marshals
// arguments and results, it never re-derives the lookup (I9). The AOT Prelude
// keeps its own definition because generated programs do not link jet-jit; see
// the comment on `jet_env_value_raw` in
// `jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs`.
//
// Raw `OsString` on purpose: `NO_COLOR` counts by PRESENCE, so a value that is
// not valid Unicode must still read as present. Decoding here would turn
// "present but not UTF-8" into "absent" — the defect #1206's review caught in
// AOT. Callers that need a `String` go through `jit_env_value`.
pub(crate) fn jit_env_value_raw(name: &str) -> Option<std::ffi::OsString> {
    let name = std::ffi::OsStr::new(name);
    jit_env_read()
        .iter()
        .find(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), name))
        .map(|(_, value)| value.clone())
}

/// The decoding read behind `core.sys.get` (Prelude `jet_std_env_get`): a
/// non-Unicode value reads as absent to a `String` caller.
pub(crate) fn jit_env_value(name: &str) -> Option<String> {
    jit_env_value_raw(name).and_then(|value| value.into_string().ok())
}

/// The one write behind `core.sys.set` (Prelude `jet_std_env_set`): validate,
/// drop any earlier spelling of the key, then append so the last spelling
/// wins. Never touches the host process environment.
pub(crate) fn jit_env_set(name: &str, value: &str) -> Result<(), &'static str> {
    jit_env_validate_name(name)?;
    jit_env_validate_value(value)?;
    let key = std::ffi::OsString::from(name);
    let mut entries = jit_env_write();
    if let Some(old) = entries
        .iter()
        .position(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), key.as_os_str()))
    {
        entries.remove(old);
    }
    entries.push((key, std::ffi::OsString::from(value)));
    Ok(())
}
/// Remove one key from the logical environment table, preserving the
/// Prelude's validation and last-spelling-wins semantics.
pub(crate) fn jit_env_unset(name: &str) -> Result<bool, &'static str> {
    jit_env_validate_name(name)?;
    let key = std::ffi::OsStr::new(name);
    let mut entries = jit_env_write();
    let existed = entries
        .iter()
        .position(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), key))
        .map(|old| entries.remove(old))
        .is_some();
    Ok(existed)
}

/// Return the logical environment's Unicode names in the Prelude's stable
/// platform-aware order. A non-Unicode entry is an EnvError, not an omission.
pub(crate) fn jit_env_vars() -> Result<Vec<String>, &'static str> {
    let entries = jit_env_read();
    let mut names = Vec::with_capacity(entries.len());
    for (name, value) in entries.iter() {
        let Some(decoded) = name.to_str() else {
            return Err("environment contains a name or value that is not valid Unicode");
        };
        if value.to_str().is_none() {
            return Err("environment contains a name or value that is not valid Unicode");
        }
        names.push((name.clone(), decoded.to_string()));
    }
    names.sort_by(|(left, _), (right, _)| {
        let folded = jit_env_key_cmp(left.as_os_str(), right.as_os_str());
        if folded != std::cmp::Ordering::Equal {
            return folded;
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            return left.encode_wide().cmp(right.encode_wide());
        }
        #[cfg(not(windows))]
        std::cmp::Ordering::Equal
    });
    Ok(names.into_iter().map(|(_, name)| name).collect())
}

thread_local! {
}

// ── core.math (mirrors jet_std_math_* / f64 methods in Process.rs emit) ───────

// ── core.sys / core.process (mirrors jet_std_env_get / jet_std_process_exit) ─

