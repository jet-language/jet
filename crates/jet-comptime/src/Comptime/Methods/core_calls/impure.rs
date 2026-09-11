use super::super::repl_process::{run_repl_process, REPL_PROCESS_OUTPUT_LIMIT_BYTES};
use super::*;

mod process_args_kernel {
    include!("../../../../../jet-codegen/src/Prelude/Core/ProcessArgs.rs");
}
// #1480: use the same line/byte stdin Prelude as AOT and JIT. This adapter
// supplies only the host-side IOError shape required by that shared source;
// line ownership and newline policy stay in `Prelude/CoreLib/Top/IoLineStream.rs`.
#[allow(dead_code)]
mod io_line_stream {
    use super::super::term_semantics::{
        jet_term_read_stdin_line, jet_term_read_text, jet_term_write_stdout,
    };

    fn jet_fault_should_fail(_operation: &str) -> bool {
        false
    }

    #[allow(dead_code)]
    mod jet_std {
        #[derive(Debug)]
        pub enum IOOperation {
            Read,
            Write,
            Flush,
        }

        #[derive(Debug)]
        pub struct IOContext {
            pub operation: IOOperation,
            pub resource: Option<String>,
            pub os_code: Option<i64>,
            pub cause: Option<String>,
        }

        #[derive(Debug)]
        pub enum IOError {
            Other(IOContext),
        }

        impl IOError {
            pub fn other(
                operation: IOOperation,
                resource: Option<String>,
                cause: impl ToString,
            ) -> Self {
                Self::Other(IOContext {
                    operation,
                    resource,
                    os_code: None,
                    cause: Some(cause.to_string()),
                })
            }
        }
    }

    include!("../../../../../jet-codegen/src/Prelude/CoreLib/Top/IoLineStream.rs");

    pub(super) fn readline() -> Result<String, std::io::Error> {
        jet_std_io_readline()
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, format!("{error:?}")))
    }

    pub(super) fn input(prompt: Option<&String>) -> Result<String, std::io::Error> {
        jet_std_io_input(prompt)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, format!("{error:?}")))
    }

    pub(super) fn read_all_input() -> Result<String, std::io::Error> {
        jet_std_io_read_all_input()
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, format!("{error:?}")))
    }
}
fn log_string_arg<'a>(value: &'a CtValue, span: Span) -> Result<&'a String, Diagnostic> {
    match value {
        CtValue::Str(value) => Ok(value),
        _ => Err(unsupported("core.log expects a String argument", span)),
    }
}

fn log_bool_arg(value: &CtValue, span: Span) -> Result<bool, Diagnostic> {
    match value {
        CtValue::Bool(value) => Ok(*value),
        _ => Err(unsupported("core.log expects a Bool argument", span)),
    }
}

fn log_field_arg(
    value: &CtValue,
    span: Span,
) -> Result<super::log_kernel::jet_std::LogField, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("core.log expects a LogField", span));
    };
    if type_name != "LogField" {
        return Err(unsupported("core.log expects a LogField", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find_map(|(field_name, value)| (field_name == name).then_some(value))
            .ok_or_else(|| unsupported("malformed LogField", span))
    };
    let key = log_string_arg(field("key")?, span)?.clone();
    let value = log_string_arg(field("value")?, span)?.clone();
    let kind = log_string_arg(field("kind")?, span)?.clone();
    let redacted = log_bool_arg(field("redacted")?, span)?;
    Ok(super::log_kernel::jet_std::LogField {
        key,
        value,
        kind,
        redacted,
    })
}

fn log_fields_arg(
    value: &CtValue,
    span: Span,
) -> Result<Vec<super::log_kernel::jet_std::LogField>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported("core.log expects a List[LogField]", span));
    };
    values
        .iter()
        .map(|value| log_field_arg(value, span))
        .collect()
}

fn log_span_arg(
    value: &CtValue,
    span: Span,
) -> Result<super::log_kernel::jet_std::LogSpan, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("core.log expects a LogSpan", span));
    };
    if type_name != "LogSpan" {
        return Err(unsupported("core.log expects a LogSpan", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find_map(|(field_name, value)| (field_name == name).then_some(value))
            .ok_or_else(|| unsupported("malformed LogSpan", span))
    };
    let id = as_int(field("id")?, span)?;
    let name = log_string_arg(field("name")?, span)?.clone();
    Ok(super::log_kernel::jet_std::LogSpan { id, name })
}

fn ct_log_field(value: super::log_kernel::jet_std::LogField) -> CtValue {
    CtValue::Struct {
        type_name: "LogField".to_string(),
        fields: vec![
            ("key".to_string(), CtValue::Str(value.key)),
            ("value".to_string(), CtValue::Str(value.value)),
            ("kind".to_string(), CtValue::Str(value.kind)),
            ("redacted".to_string(), CtValue::Bool(value.redacted)),
        ],
    }
}

fn ct_log_span(value: super::log_kernel::jet_std::LogSpan) -> CtValue {
    CtValue::Struct {
        type_name: "LogSpan".to_string(),
        fields: vec![
            ("id".to_string(), CtValue::Int(value.id)),
            ("name".to_string(), CtValue::Str(value.name)),
        ],
    }
}

/// D-CTEFFECT1: execute a Tier-2 ambient comptime I/O effect (or REPL sandbox I/O).
/// Only called when `impure_depth > 0` and `gates` (comptime) or from the
/// runtime TIR evaluator used by `jet run` deopt (#778).
pub fn apply_impure_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    base_dir: &std::path::Path,
    sink: Option<&mut super::super::super::Interpreter::DevSink>,
    repl_mode: bool,
    pinned_executable: Option<&std::fs::File>,
    verified_root: Option<&std::fs::File>,
) -> Result<CtValue, Diagnostic> {
    apply_impure_core_call_with_type_args(
        module,
        method,
        args,
        span,
        base_dir,
        sink,
        repl_mode,
        pinned_executable,
        verified_root,
        &[],
        None,
    )
}

pub fn apply_impure_core_call_with_type(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    base_dir: &std::path::Path,
    sink: Option<&mut super::super::super::Interpreter::DevSink>,
    repl_mode: bool,
    pinned_executable: Option<&std::fs::File>,
    verified_root: Option<&std::fs::File>,
    resolved_ret: Option<&Type>,
) -> Result<CtValue, Diagnostic> {
    apply_impure_core_call_with_type_args(
        module,
        method,
        args,
        span,
        base_dir,
        sink,
        repl_mode,
        pinned_executable,
        verified_root,
        &[],
        resolved_ret,
    )
}

pub fn apply_impure_core_call_with_type_args(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    base_dir: &std::path::Path,
    sink: Option<&mut super::super::super::Interpreter::DevSink>,
    repl_mode: bool,
    pinned_executable: Option<&std::fs::File>,
    verified_root: Option<&std::fs::File>,
    type_args: &[Type],
    resolved_ret: Option<&Type>,
) -> Result<CtValue, Diagnostic> {
    let args = super::normalize_path_args(module, method, args, span)?;
    super::validate_core_call_projection(
        module,
        method,
        args.len(),
        jet_foundation::Syntax::CoreCallCoverage::COMPTIME,
        span,
    )?;
    super::validate_interpreter_route(module, method, span)?;
    // The registry route selects the adapter. Ambient rows cross the host
    // boundary; pure and typed-intrinsic rows stay on their shared evaluator
    // paths. Unknown rows retain the legacy ambient hook.
    let mut sink = sink;
    let route = jet_foundation::Syntax::core_call(module, method).map(|row| row.interpreter_route);
    if route.is_none()
        || matches!(
            route,
            Some(jet_foundation::Syntax::CoreCallInterpreterRoute::Ambient)
        )
    {
        if let Some(result) = crate::Comptime::try_core_call_typed_with_sink(
            module,
            method,
            args.clone(),
            span,
            resolved_ret.cloned(),
            sink.as_deref_mut(),
        ) {
            return result;
        }
    }
    // Effects are already approved at this entry point. Reuse registered pure
    // constructors even when their module carries an ambient effect, such as UI.
    if let Some(result) = super::apply_core_pure_call(module, method, &args, span) {
        return result;
    }
    if module == "core.handle"
        && matches!(
            method,
            "path.from"
                | "path.home"
                | "path.join"
                | "path.parent"
                | "path.extension"
                | "path.stem"
                | "path.normalize"
                | "path.is_within"
                | "path.walk"
        )
    {
        return super::apply_path_call(method, &args, span);
    }
    let one = |i: usize| {
        args.get(i).ok_or_else(|| {
            unsupported(
                &format!("`{}.{}` (wrong number of arguments)", module, method),
                span,
            )
        })
    };
    if let Some(result) = super::apply_raylib_core_call(module, method, &args, span) {
        return result;
    }
    match (module, method) {
        // Every registered `core.log` row uses the shared Log.rs kernel. The
        // adapter only marshals erased CtValues; filtering, sampling, trace
        // context, formatting, sinks, and telemetry remain in that kernel.
        ("core.log", "debug" | "info" | "warn" | "error" | "critical" | "fatal") => {
            let message = log_string_arg(one(0)?, span)?;
            match method {
                "debug" => super::log_kernel::debug(message),
                "info" => super::log_kernel::info(message),
                "warn" => super::log_kernel::warn(message),
                "error" => super::log_kernel::error(message),
                "critical" => super::log_kernel::critical(message),
                "fatal" => super::log_kernel::fatal(message),
                _ => unreachable!("core.log level row was not selected"),
            }
            Ok(CtValue::Unit)
        }
        ("core.log", "debug_fields" | "info_fields" | "warn_fields" | "error_fields") => {
            let message = log_string_arg(one(0)?, span)?;
            let fields = log_fields_arg(one(1)?, span)?;
            match method {
                "debug_fields" => super::log_kernel::debug_fields(message, &fields),
                "info_fields" => super::log_kernel::info_fields(message, &fields),
                "warn_fields" => super::log_kernel::warn_fields(message, &fields),
                "error_fields" => super::log_kernel::error_fields(message, &fields),
                _ => unreachable!("core.log fields row was not selected"),
            }
            Ok(CtValue::Unit)
        }
        ("core.log", "field") => {
            let key = log_string_arg(one(0)?, span)?;
            let value = log_string_arg(one(1)?, span)?;
            Ok(ct_log_field(super::log_kernel::field(key, value)))
        }
        ("core.log", "int") => {
            let key = log_string_arg(one(0)?, span)?;
            let value = as_int(one(1)?, span)?;
            Ok(ct_log_field(super::log_kernel::int(key, value)))
        }
        ("core.log", "float") => {
            let key = log_string_arg(one(0)?, span)?;
            let value = super::as_float(one(1)?, span)?;
            Ok(ct_log_field(super::log_kernel::float(key, value)))
        }
        ("core.log", "bool") => {
            let key = log_string_arg(one(0)?, span)?;
            let value = log_bool_arg(one(1)?, span)?;
            Ok(ct_log_field(super::log_kernel::bool_field(key, value)))
        }
        ("core.log", "redact") => {
            let key = log_string_arg(one(0)?, span)?;
            Ok(ct_log_field(super::log_kernel::redact(key)))
        }
        ("core.log", "counter") => {
            let name = log_string_arg(one(0)?, span)?;
            let value = as_int(one(1)?, span)?;
            Ok(ct_log_field(super::log_kernel::counter(name, value)))
        }
        ("core.log", "span") => {
            let name = log_string_arg(one(0)?, span)?;
            Ok(ct_log_span(super::log_kernel::span(name)))
        }
        ("core.log", "enter" | "close") => {
            let span_value = log_span_arg(one(0)?, span)?;
            match method {
                "enter" => super::log_kernel::enter(&span_value),
                "close" => super::log_kernel::close(&span_value),
                _ => unreachable!("core.log span row was not selected"),
            }
            Ok(CtValue::Unit)
        }
        ("core.log", "enabled") => {
            let level = log_string_arg(one(0)?, span)?;
            Ok(CtValue::Bool(super::log_kernel::enabled(level)))
        }
        ("core.log", "disable") => {
            super::log_kernel::disable();
            Ok(CtValue::Unit)
        }
        ("core.log", "flush") => {
            super::log_kernel::flush();
            Ok(CtValue::Unit)
        }
        ("core.log", "set_sink") => {
            let kind = log_string_arg(one(0)?, span)?;
            let path = log_string_arg(one(1)?, span)?;
            super::log_kernel::set_sink(kind, path);
            Ok(CtValue::Unit)
        }
        ("core.log", "sample_every") => {
            super::log_kernel::sample_every(as_int(one(0)?, span)?);
            Ok(CtValue::Unit)
        }
        ("core.log", "otlp_file") => {
            super::log_kernel::otlp_file(log_string_arg(one(0)?, span)?);
            Ok(CtValue::Unit)
        }
        ("core.log", "set_level") => {
            super::log_kernel::set_level(log_string_arg(one(0)?, span)?);
            Ok(CtValue::Unit)
        }
        ("core.log", "setup") => {
            super::log_kernel::setup(log_string_arg(one(0)?, span)?);
            Ok(CtValue::Unit)
        }
        ("core.log", "set_trace_id") => {
            super::log_kernel::set_trace_id(as_string(one(0)?, span)?);
            Ok(CtValue::Unit)
        }
        // D-FILES-SCOPE1: keep the Authority identity in the opaque handle;
        // only the shared Prelude read adapter interprets its resource roots.
        ("core.files", "scope") => {
            let authority = one(0)?;
            if super::super::super::Builtins::authority_holds(authority).is_none() {
                return Err(unsupported(
                    "core.files.scope expects an Authority",
                    span,
                ));
            }
            Ok(CtValue::Struct {
                type_name: "FileScope".to_string(),
                fields: vec![("authority".to_string(), authority.clone())],
            })
        }
        // ── D-I9 `core.files`: marshal to the ONE Prelude kernel ───────────
        // The filesystem Prelude fragments own every operation below — fault
        // injection, the recursive/non-recursive split, and the `IOError`
        // shape — through the same `jet_std_fs_*` symbols AOT emits and the
        // resident Cranelift host calls. Pass the marshalled path through
        // unchanged: the shared kernel and native tiers resolve relative paths
        // from the process working directory. The hand-written per-member arms
        // it replaces are why `create_dir_all` and `remove_all` had no arm at
        // all: a shipped example (`io/watcher`) passed sema and then died at
        // run time on E0956 while AOT ran the same source.
        ("core.files", "absolute") => {
            let path = as_string(one(0)?, span)?;
            use crate::Comptime::TextLite as files_kernel;
            Ok(match files_kernel::fs_absolute(path) {
                Ok(path) => CtValue::Present(Box::new(CtValue::Str(path))),
                Err(error) => CtValue::failed(Box::new(error)),
            })
        }
        ("core.files", "open" | "create" | "append") => {
            let path = as_string(one(0)?, span)?;
            use crate::Comptime::TextLite as files_kernel;
            let result = match method {
                "open" => files_kernel::fs_open(path),
                "create" => files_kernel::fs_create(path),
                "append" => files_kernel::fs_append_stream(path),
                _ => unreachable!("streaming file route is closed"),
            };
            Ok(match result {
                Ok(value) => CtValue::Present(Box::new(value)),
                Err(error) => CtValue::failed(Box::new(error)),
            })
        }

        (
            "core.files",
            "read" | "read_bytes" | "write" | "append_all" | "exists" | "is_dir"
            | "create_dir" | "create_dir_all" | "remove" | "remove_dir" | "remove_all"
            | "list_dir" | "copy" | "copy_dir" | "rename" | "glob" | "walk" | "walk_parallel"
            | "walk_files" | "stat" | "fsync",
        ) => {
            let path_arg = |value: &CtValue| -> Result<String, Diagnostic> {
                Ok(as_string(value, span)?.to_string())
            };
            let path = path_arg(one(0)?)?;
            let ignore_name = match args.get(1) {
                None | Some(CtValue::Failed(CtReport::Clean(_))) => None,
                Some(_) => Some(as_string(one(1)?, span)?.to_string()),
            };
            let unit = |result: Result<(), CtValue>| match result {
                Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                Err(error) => CtValue::failed(Box::new(error)),
            };
            let present = |result: Result<CtValue, CtValue>| match result {
                Ok(value) => CtValue::Present(Box::new(value)),
                Err(error) => CtValue::failed(Box::new(error)),
            };
            use crate::Comptime::TextLite as files_kernel;
            Ok(match method {
                "read" => present(files_kernel::fs_read(&path).map(CtValue::Str)),
                "read_bytes" => present(files_kernel::fs_read_bytes(&path).map(CtValue::Bytes)),
                // D-FILES-APPEND1=A: whole-file one-shot is `append_all` (not
                // `append`, which names the streaming handle's method).
                "write" => unit(files_kernel::fs_write(&path, as_string(one(1)?, span)?)),
                "append_all" => unit(files_kernel::fs_append(&path, as_string(one(1)?, span)?)),
                "fsync" => unit(files_kernel::fs_fsync(&path)),
                "exists" => CtValue::Bool(files_kernel::fs_exists(&path)),
                "is_dir" => CtValue::Bool(files_kernel::fs_is_dir(&path)),
                "stat" => present(files_kernel::fs_stat(&path)),
                "create_dir" => unit(files_kernel::fs_create_dir(&path)),
                "create_dir_all" => unit(files_kernel::fs_create_dir_all(&path)),
                "remove" => unit(files_kernel::fs_remove(&path)),
                "remove_dir" => unit(files_kernel::fs_remove_dir(&path)),
                "remove_all" => unit(files_kernel::fs_remove_all(&path)),
                "copy" => unit(files_kernel::fs_copy(&path, &path_arg(one(1)?)?)),
                "copy_dir" => unit(files_kernel::fs_copy_dir(&path, &path_arg(one(1)?)?)),
                "rename" => unit(files_kernel::fs_rename(&path, &path_arg(one(1)?)?)),
                "glob" => present(files_kernel::fs_glob(&path).map(|paths| {
                    CtValue::List(paths.into_iter().map(CtValue::Str).collect())
                })),
                "walk" | "walk_parallel" => {
                    let walk_entries = |entries: Vec<(String, String, bool, i64)>| {
                        CtValue::List(
                            entries
                                .into_iter()
                                .map(|(path, relative, is_dir, depth)| CtValue::Struct {
                                    type_name: "WalkEntry".to_string(),
                                    fields: vec![
                                        ("path".to_string(), CtValue::Str(path)),
                                        ("relative".to_string(), CtValue::Str(relative)),
                                        ("is_dir".to_string(), CtValue::Bool(is_dir)),
                                        ("depth".to_string(), CtValue::Int(depth)),
                                    ],
                                })
                                .collect(),
                        )
                    };
                    present(
                        files_kernel::fs_walk_parallel_with_ignore(
                            &path,
                            ignore_name.as_deref(),
                        )
                        .map(walk_entries),
                    )
                }
                "walk_files" => {
                    let walk_entries = |entries: Vec<(String, String, bool, i64)>| {
                        CtValue::List(
                            entries
                                .into_iter()
                                .map(|(path, relative, is_dir, depth)| CtValue::Struct {
                                    type_name: "WalkEntry".to_string(),
                                    fields: vec![
                                        ("path".to_string(), CtValue::Str(path)),
                                        ("relative".to_string(), CtValue::Str(relative)),
                                        ("is_dir".to_string(), CtValue::Bool(is_dir)),
                                        ("depth".to_string(), CtValue::Int(depth)),
                                    ],
                                })
                                .collect(),
                        )
                    };
                    present(
                        files_kernel::fs_walk_files_parallel_with_ignore(
                            &path,
                            ignore_name.as_deref(),
                        )
                        .map(walk_entries),
                    )
                }
                // D-LSDIR1: the kernel returns rows already sorted by name.
                _ => present(files_kernel::fs_list_dir(&path).map(|entries| {
                    CtValue::List(
                        entries
                            .into_iter()
                            .map(|(name, path, is_dir)| CtValue::Struct {
                                type_name: "DirEntry".to_string(),
                                fields: vec![
                                    ("name".to_string(), CtValue::Str(name)),
                                    ("path".to_string(), CtValue::Str(path)),
                                    ("is_dir".to_string(), CtValue::Bool(is_dir)),
                                ],
                            })
                            .collect(),
                    )
                })),
            })
        }
        ("core.sys", "get") => {
            let key = as_string(one(0)?, span)?;
            match std::env::var(key) {
                Ok(v) => Ok(CtValue::Present(Box::new(CtValue::Str(v)))),
                Err(_) => Ok(CtValue::absent(crate::AST::Type::String)),
            }
        }
        ("core.sys", "set") => {
            let key = as_string(one(0)?, span)?;
            let val = as_string(one(1)?, span)?;
            std::env::set_var(key, val);
            Ok(CtValue::Unit)
        }
        ("core.sys", "current_dir") => match std::env::current_dir() {
            Ok(p) => Ok(CtValue::Present(Box::new(CtValue::Str(
                p.to_string_lossy().into_owned(),
            )))),
            Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                IoErrorOperation::Resolve,
                ".",
                e,
            )))),
        },
        ("core.sys", "home_dir") => Ok(
            match std::env::var("HOME")
                .ok()
                .or_else(|| std::env::var("USERPROFILE").ok())
            {
                Some(v) => CtValue::Present(Box::new(CtValue::Str(v))),
                None => CtValue::absent(crate::AST::Type::String),
            },
        ),
        ("core.process", "argv") => {
            // Prefer argv installed for this jet run/deopt. Never fall back to
            // the host process argv — `cargo test` flags would leak into output.
            let argv = super::super::super::Interpreter::runtime_argv()
                .unwrap_or_else(|| vec!["jet".to_string()]);
            Ok(CtValue::List(argv.into_iter().map(CtValue::Str).collect()))
        }
        ("core.process", "args") => {
            // Keep the projection in the shared Prelude. The returned list is
            // newly allocated on every call, just like the native adapters.
            let argv = super::super::super::Interpreter::runtime_argv()
                .unwrap_or_else(|| vec!["jet".to_string()]);
            Ok(CtValue::List(
                process_args_kernel::jet_process_args_view(argv)
                    .into_iter()
                    .map(CtValue::Str)
                    .collect(),
            ))
        }
        ("core.term", "progress") => {
            let Some(source) = args.first() else {
                return Err(unsupported("`core.term.progress` needs a source", span));
            };
            let CtValue::Str(text) = source else {
                return Err(unsupported(
                    "`core.term.progress` expects a String source",
                    span,
                ));
            };
            if args.len() != 1 {
                return Err(unsupported(
                    "`core.term.progress` text form takes one argument",
                    span,
                ));
            }
            if let Some(sink) = sink {
                if !super::term_semantics::jet_term_progress_enabled() {
                    return Ok(CtValue::Unit);
                }
                let tty = super::term_semantics::jet_term_stderr_is_terminal();
                let frame = super::term_semantics::jet_term_progress_frame(tty, text);
                if super::term_semantics::jet_term_stdout_is_program_stream() {
                    super::term_semantics::jet_term_write_stderr(&frame, true).map_err(|error| {
                        unsupported(&format!("write stderr: {error}"), span)
                    })?;
                } else {
                    sink.stderr.push_str(&frame);
                }
            }
            Ok(CtValue::Unit)
        }
        // The static Prelude `print` row is `(text, flush)`. Qualified
        // `core.term.print` calls arrive with one already-joined text value;
        // preserve their variadic display behavior without treating the
        // canonical flush control as payload.
        ("core.term", "print") => {
            let (text, flush) = match args.as_slice() {
                [value, CtValue::Bool(flush)] => (
                    display_core_pure_value(value).unwrap_or_else(|| value.jet_show()),
                    *flush,
                ),
                _ => (
                    args.iter()
                        .map(|v| display_core_pure_value(v).unwrap_or_else(|| v.jet_show()))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    true,
                ),
            };
            if let Some(s) = sink {
                let frame = super::term_semantics::jet_term_print_frame(&text);
                // A REPL/notebook caller consumes this sink as its transcript
                // (a cell projects it into its own output bundle), so the host
                // process having a terminal must not divert the frame away from
                // it — the same rule the TIR evaluator's `write_print` states.
                if !repl_mode && super::term_semantics::jet_term_stdout_is_program_stream() {
                    let _ = super::term_semantics::jet_term_write_stdout(&frame, flush);
                } else {
                    s.stdout.push_str(&frame);
                }
            }
            Ok(CtValue::Unit)
        }
        ("core.term", "eprint") => {
            let text = args
                .iter()
                .map(|v| display_core_pure_value(v).unwrap_or_else(|| v.jet_show()))
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(s) = sink {
                let frame = super::term_semantics::jet_term_print_frame(&text);
                if !repl_mode && super::term_semantics::jet_term_stderr_is_program_stream() {
                    let _ = super::term_semantics::jet_term_write_stderr(&frame, true);
                } else {
                    s.stderr.push_str(&frame);
                }
            }
            Ok(CtValue::Unit)
        }
        ("core.term", "readline") => {
            if repl_mode {
                Err(repl_native_module_diag("core.term", method, span))
            } else {
                match io_line_stream::readline() {
                    Ok(text) => Ok(CtValue::Present(Box::new(CtValue::Str(text)))),
                    Err(error) => Ok(CtValue::failed(Box::new(io_error_value(
                        IoErrorOperation::Read,
                        "stdin",
                        error,
                    )))),
                }
            }
        }

        ("core.term", "input") => {
            if repl_mode {
                Err(repl_native_module_diag("core.term", method, span))
            } else {
                let prompt = match args.as_slice() {
                    [CtValue::Failed(CtReport::Clean(_))] => None,
                    [CtValue::Str(prompt)] => Some(prompt),
                    _ => {
                        return Err(unsupported(
                            "core.term.input expects one optional String prompt",
                            span,
                        ))
                    }
                };
                match io_line_stream::input(prompt) {
                    Ok(text) => Ok(CtValue::Present(Box::new(CtValue::Str(text)))),
                    Err(error) => Ok(CtValue::failed(Box::new(io_error_value(
                        IoErrorOperation::Read,
                        "stdin",
                        error,
                    )))),
                }
            }
        }
        ("core.term", "read_all_input") => {
            if repl_mode {
                Err(repl_native_module_diag("core.term", method, span))
            } else {
                if !args.is_empty() {
                    return Err(unsupported(
                        "core.term.read_all_input expects no arguments",
                        span,
                    ));
                }
                match io_line_stream::read_all_input() {
                    Ok(text) => Ok(CtValue::Present(Box::new(CtValue::Str(text)))),
                    Err(error) => Ok(CtValue::failed(Box::new(io_error_value(
                        IoErrorOperation::Read,
                        "stdin",
                        error,
                    )))),
                }
            }
        }
        ("core.term", "stdin") if repl_mode => Err(repl_native_module_diag("core.term", method, span)),
        ("core.term", "stdin") => Ok(CtValue::Struct {
            type_name: "StdinHandle".to_string(),
            fields: vec![],
        }),
        ("core.process", "exit") => {
            let code = match one(0)? {
                CtValue::Int(n) => *n,
                _ => 0,
            };
            // In-process interpreter/deopt must not kill the host (cargo test,
            // jet dev). Soft-exit via the sink; bare comptime keeps hard exit.
            if let Some(s) = sink {
                s.exit_code = Some(code as i32);
                return Err(Diagnostic::soft_exit(
                    code.to_string(),
                    "process.exit requested".to_string(),
                    Some(span),
                ));
            }
            std::process::exit(code as i32);
        }
        ("core.process", "run") => {
            let cmd = match one(0)? {
                CtValue::List(items) => items.iter().map(|v| v.jet_show()).collect::<Vec<_>>(),
                _ => {
                    return Err(unsupported(
                        "process.run expects a list of command words",
                        span,
                    ))
                }
            };
            if cmd.is_empty() {
                return Ok(CtValue::failed(Box::new(CtValue::Struct {
                    type_name: "IOError".to_string(),
                    fields: vec![(
                        "message".to_string(),
                        CtValue::Str("process.run needs at least one command word".to_string()),
                    )],
                })));
            }
            match run_repl_process(
                &cmd,
                base_dir,
                pinned_executable,
                verified_root,
                std::time::Duration::from_secs(30),
            ) {
                Ok(out) => Ok(CtValue::Present(Box::new(CtValue::Struct {
                    type_name: "ProcessReceipt".to_string(),
                    fields: vec![
                        (
                            "code".to_string(),
                            CtValue::Int(out.status.code().unwrap_or(-1) as i64),
                        ),
                        (
                            "output".to_string(),
                            CtValue::Str(String::from_utf8_lossy(&out.stdout).into_owned()),
                        ),
                        (
                            "errors".to_string(),
                            CtValue::Str(String::from_utf8_lossy(&out.stderr).into_owned()),
                        ),
                        (
                            "signal".to_string(),
                            CtValue::absent(Type::Int),
                        ),
                        (
                            "success".to_string(),
                            CtValue::Bool(out.status.success()),
                        ),
                        (
                            "timed_out".to_string(),
                            CtValue::Bool(false),
                        ),
                        (
                            "executable_identity".to_string(),
                            CtValue::Str(cmd.first().cloned().unwrap_or_default()),
                        ),
                        (
                            "argv".to_string(),
                            CtValue::List(cmd.iter().cloned().map(CtValue::Str).collect()),
                        ),
                        ("input_digest".to_string(), CtValue::Str(String::new())),
                        ("policy_digest".to_string(), CtValue::Str(String::new())),
                        (
                            "backend".to_string(),
                            CtValue::Str("comptime-ambient".to_string()),
                        ),
                        (
                            "authority".to_string(),
                            CtValue::List(Vec::new()),
                        ),
                        (
                            "descendants".to_string(),
                            CtValue::Str("direct".to_string()),
                        ),
                        (
                            "limits".to_string(),
                            CtValue::List(vec![
                                CtValue::Str("timeout-ms=30000".to_string()),
                                CtValue::Str(format!(
                                    "output-limit={REPL_PROCESS_OUTPUT_LIMIT_BYTES}"
                                )),
                            ]),
                        ),
                        (
                            "outputs".to_string(),
                            CtValue::List(vec![
                                CtValue::Str("stdout=capture".to_string()),
                                CtValue::Str("stderr=capture".to_string()),
                            ]),
                        ),
                        ("redacted".to_string(), CtValue::Bool(false)),
                        ("pid".to_string(), CtValue::Int(0)),
                    ],
                }))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    IoErrorOperation::Resolve,
                    &cmd[0],
                    e,
                )))),
            }
        }
        ("core.net.tls", _) => Err(Diagnostic::error(
            "E3412",
            format!("`core.net.tls.{}()` is not available at comptime", method),
            "live TLS sessions cannot be opened during compile-time evaluation".to_string(),
            "move the TLS operation to runtime; use `core.net.fetch(url, sha256: \"<hash>\")` for content-hash-pinned build-time downloads"
                .to_string(),
            Some(span),
        )),
        // The MIR HandleMethod route enters the impure seam at runtime; this
        // pure duration read still uses the shared Comptime adapter.
        ("core.handle", "duration.in") => {
            super::apply_core_call_without_ambient_with_type(
                module,
                method,
                args,
                span,
                repl_mode,
                resolved_ret,
            )
        }
        // Web Core rows are TypedIntrinsic: enter the shared Prelude/AppLite
        // dispatcher directly, without a second ambient adapter.
        (module, _) if module == "core.web" || module.starts_with("core.web.") => {
            super::apply_core_call_without_ambient_with_type(
                module,
                method,
                args,
                span,
                repl_mode,
                resolved_ret,
            )
        }
        // Pure compress/archive/encoding codecs live on apply_core_call; reuse
        // them when the runtime evaluator has ambient impure depth open (#778
        // deopt / #715 default-dev encoding parity). Whole-value encoding must
        // not die as E0956 impure-tier after silent deopt.
        ("core.archive.gzip", _)
        | ("core.archive.zstd", _)
        | ("core.archive", _)
        | ("core.perf", _) => {
            apply_core_call_with_type(module, method, args, span, repl_mode, resolved_ret)
        }
        (module, _) if module.starts_with("core.encoding.") => {
            apply_core_call_with_type(module, method, args, span, repl_mode, resolved_ret)
        }
        // Ambient impure depth must not block pure-tier CorePureParity surfaces
        // that MirBridge already evaluates (date/math/measurement/testing/…).
        // Pure style/net helpers share the implementation dispatch so
        // impure_depth>0 (MirBridge / jet run deopt) still hits CorePureParity.
        ("core.term", method)
            if jet_foundation::Effects::core_effect("core.term", method).is_none() =>
        {
            apply_core_call_with_type(module, method, args, span, repl_mode, resolved_ret)
        }
        ("core.testing", "histories") => {
            super::apply_core_call_without_ambient_with_type_args(
                module,
                method,
                args,
                span,
                repl_mode,
                type_args,
                resolved_ret,
            )
        }
        ("core.math.random", _) | ("core.testing", "fake_rng") => {
            apply_core_call_with_type(module, method, args, span, repl_mode, resolved_ret)
        }
        ("core.crypto.random", "bytes") => {
            apply_core_call_with_type(module, method, args, span, repl_mode, resolved_ret)
        }
        // `core.text.fmt` is pure text rendering; at runtime it carries the
        // shared Display route every `"{value}"` and `print(value)` lowers to.
        ("core.time", _)
        | ("core.math", _)
        | ("core.testing", _)
        | ("core.data", _)
        | ("core.compute", _)
        | ("core.service", _)
        | ("core.auth", _)
        | ("core.sync", _)
        | ("app", _)
        | ("core.ui", _)
        | ("core.crypto", _)
        | ("core.crypto.expert", _)
        | ("core.email", _)
        | ("core.regex", _)
        | ("core.units", _)
        | ("core.text.fmt", _)
        => {
            apply_core_call_with_type(module, method, args, span, repl_mode, resolved_ret)
        }
        // Pure net helpers (e.g. ip_addr, socket_addr_parse) — not live sockets.
        // Keep E3412 for the rest. D-META-EFFECT1: "pure" is what the effect
        // table says, so both tiers agree without a second list here.
        ("core.net", method)
            if jet_foundation::Effects::core_effect("core.net", method).is_none() =>
        {
            apply_core_call_with_type(module, method, args, span, repl_mode, resolved_ret)
        }
        ("core.net", _) => Err(Diagnostic::error(
            "E3412",
            format!("`core.net.{}()` is not available at comptime", method),
            "only `core.net.fetch(url, sha256:)` is supported at compile time".to_string(),
            "use `core.net.fetch(url, sha256: \"<hash>\")` for content-hash-pinned downloads"
                .to_string(),
            Some(span),
        )),
        // I4: this adapter is also the runtime TIR evaluator's Core seam (see
        // the doc comment above: `jet run` deopt, #778), so the `what` names
        // the call and never a phase. "at comptime" here labelled a plain
        // runtime IO call site as compile-time work.
        _ => Err(unsupported(&format!("`{}.{}()`", module, method), span)),
    }
}
