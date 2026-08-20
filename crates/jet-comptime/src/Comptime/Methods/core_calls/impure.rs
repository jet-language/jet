use super::*;
use super::super::repl_process::run_repl_process;

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
    apply_impure_core_call_with_type(
        module,
        method,
        args,
        span,
        base_dir,
        sink,
        repl_mode,
        pinned_executable,
        verified_root,
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
    let args = super::normalize_path_args(module, method, args, span)?;
    super::validate_core_call_projection(
        module,
        method,
        args.len(),
        jet_foundation::Syntax::CoreCallCoverage::COMPTIME,
        span,
    )?;
    // Pure CorePureParity surfaces (crypto.expert, net.socket_*, datetime, …)
    // must still resolve under ambient impure depth — same as apply_core_call.
    if let Some(row) = jet_foundation::Syntax::core_call(module, method)
        .filter(|row| core_call_allows_pure_parity(row))
    {
        if let Some(result) = core_pure_parity::evaluate(row, &args, span) {
            return result;
        }
    }
    let mut sink = sink;
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
    let one = |i: usize| {
        args.get(i).ok_or_else(|| {
            unsupported(
                &format!("`{}.{}` (wrong number of arguments)", module, method),
                span,
            )
        })
    };
    match (module, method) {
        // ── D-I9 `core.files`: marshal to the ONE Prelude kernel ───────────
        // `Prelude/CoreLib/Top/Text.rs` owns every operation below — fault
        // injection, the recursive/non-recursive split, and the `IOError`
        // shape — through the same `jet_std_fs_*` symbols AOT emits and the
        // resident Cranelift host calls. This arm resolves the path against
        // `base_dir` and projects the kernel's result; it spells no `std::fs`
        // call of its own. The hand-written per-member arms it replaces are
        // why `create_dir_all` and `remove_all` had no arm at all: a shipped
        // example (`io/watcher`) passed sema and then died at run time on
        // E0956 while AOT ran the same source.
        (
            "core.files",
            "read" | "read_bytes" | "write" | "append_all" | "exists" | "is_dir"
            | "create_dir" | "create_dir_all" | "remove" | "remove_dir" | "remove_all"
            | "list_dir" | "copy",
        ) => {
            let resolve = |value: &CtValue| -> Result<String, Diagnostic> {
                Ok(base_dir
                    .join(as_string(value, span)?)
                    .to_string_lossy()
                    .into_owned())
            };
            let path = resolve(one(0)?)?;
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
                "exists" => CtValue::Bool(files_kernel::fs_exists(&path)),
                "is_dir" => CtValue::Bool(files_kernel::fs_is_dir(&path)),
                "create_dir" => unit(files_kernel::fs_create_dir(&path)),
                "create_dir_all" => unit(files_kernel::fs_create_dir_all(&path)),
                "remove" => unit(files_kernel::fs_remove(&path)),
                "remove_dir" => unit(files_kernel::fs_remove_dir(&path)),
                "remove_all" => unit(files_kernel::fs_remove_all(&path)),
                "copy" => unit(files_kernel::fs_copy(&path, &resolve(one(1)?)?)),
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
        ("core.term", "progress") => {
            let Some(source) = args.first() else {
                return Err(unsupported("`core.term.progress` needs a source", span));
            };
            if let CtValue::Str(text) = source {
                if args.len() != 1 {
                    return Err(unsupported(
                        "`core.term.progress` text form takes one argument",
                        span,
                    ));
                }
                if let Some(sink) = sink {
                    let tty = super::term_semantics::jet_term_stdout_is_terminal();
                    let frame = super::term_semantics::jet_term_progress_frame(tty, text);
                    if tty {
                        super::term_semantics::jet_term_write_stdout(&frame, true).map_err(|error| {
                            unsupported(&format!("write stdout: {error}"), span)
                        })?;
                    } else {
                        sink.stdout.push_str(&frame);
                    }
                }
                return Ok(CtValue::Unit);
            }
            let CtValue::List(items) = source else {
                return Err(unsupported(
                    "`core.term.progress` expects a List or Iter source",
                    span,
                ));
            };
            let description = args
                .get(1)
                .map(|value| as_string(value, span))
                .transpose()?
                .unwrap_or("Progress")
                .to_string();
            let format = args
                .get(2)
                .map(|value| as_string(value, span))
                .transpose()?
                .unwrap_or("")
                .to_string();
            // Keep the adapter lazy in the TIR interpreter.  The loop evaluator
            // unwraps this erased carrier and renders one update per pulled
            // item.  Rendering here would report progress before the caller
            // consumes anything and would diverge from AOT/JIT.
            let started_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs_f64())
                .unwrap_or(0.0);
            Ok(CtValue::Struct {
                type_name: "__JetProgressIter".to_string(),
                fields: vec![
                    ("items".to_string(), CtValue::List(items.clone())),
                    ("description".to_string(), CtValue::Str(description)),
                    ("format".to_string(), CtValue::Str(format)),
                    ("started_at".to_string(), CtValue::Float(crate::AST::CtFloat::f64(started_at))),
                    (
                        "pulls".to_string(),
                        CtValue::List(vec![CtValue::Int(1); items.len()]),
                    ),
                    ("tail".to_string(), CtValue::Int(0)),
                    ("total".to_string(), CtValue::Int(items.len() as i64)),
                    ("known_total".to_string(), CtValue::Bool(true)),
                ],
            })
        }
        // D-VERDICT-1321-1: variadic — each argument renders on its own line.
        ("core.term", "print") => {
            let text = args
                .iter()
                .map(|v| display_core_pure_value(v).unwrap_or_else(|| v.jet_show()))
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(s) = sink {
                let frame = super::term_semantics::jet_term_print_frame(&text);
                // A REPL/notebook caller consumes this sink as its transcript
                // (a cell projects it into its own output bundle), so the host
                // process having a terminal must not divert the frame away from
                // it — the same rule the TIR evaluator's `write_print` states.
                if !repl_mode && super::term_semantics::jet_term_stdout_is_terminal() {
                    let _ = super::term_semantics::jet_term_write_stdout(&frame, true);
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
                if !repl_mode && super::term_semantics::jet_term_stderr_is_terminal() {
                    let _ = super::term_semantics::jet_term_write_stderr(&frame, true);
                } else {
                    s.stderr.push_str(&frame);
                }
            }
            Ok(CtValue::Unit)
        }
        ("core.term", "input") | ("core.term", "read_all_input") => {
            if repl_mode {
                Err(repl_native_module_diag("core.term", method, span))
            } else {
                Ok(CtValue::Present(Box::new(CtValue::Str(String::new()))))
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
                    type_name: "ProcessResult".to_string(),
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
        // that TirBridge already evaluates (date/math/measurement/testing/…).
        // Pure style/net helpers share the implementation dispatch so
        // impure_depth>0 (TirBridge / jet run deopt) still hits CorePureParity.
        ("core.term", method)
            if jet_foundation::Effects::core_effect("core.term", method).is_none() =>
        {
            apply_core_call_with_type(module, method, args, span, repl_mode, resolved_ret)
        }
        ("core.math.random", _) | ("core.testing", "fake_rng") => {
            apply_core_call_with_type(module, method, args, span, repl_mode, resolved_ret)
        }
        ("core.crypto.random", "bytes") => {
            apply_core_call_with_type(module, method, args, span, repl_mode, resolved_ret)
        }
        ("core.time", _)
        | ("core.math", _)
        | ("core.testing", _)
        | ("core.data", _)
        | ("core.compute", _)
        | ("core.services", _)
        | ("core.auth", _)
        | ("core.sync", _)
        | ("app", _)
        | ("core.ui", _)
        | ("core.crypto", _)
        | ("core.crypto.expert", _)
        | ("core.email", _)
        | ("core.regex", _)
        | ("core.units", _)
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
