use super::*;
use super::super::repl_process::run_repl_process;

/// D-CTEFFECT1: execute a Tier-2 ambient comptime I/O effect (or REPL sandbox I/O).
/// Only called when `impure_depth > 0` and `allow_impure` (comptime) or from the
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
    if let Some(row) = jet_foundation::Syntax::core_call(module, method) {
        if !row.accepts_arity(args.len()) {
            return Err(unsupported(
                &format!(
                    "{}.{}(): expected {}..{} argument(s), got {}",
                    module,
                    method,
                    row.arity(),
                    row.signature.max_arity,
                    args.len()
                ),
                span,
            ));
        }
    }
    // Pure CorePureParity surfaces (crypto.expert, net.socket_*, datetime, …)
    // must still resolve under ambient impure depth — same as apply_core_call.
    if let Some(row) = jet_foundation::Syntax::core_call(module, method)
        .filter(|row| core_call_allows_pure_parity(row))
    {
        if let Some(result) = core_pure_parity::evaluate(row, &args, span) {
            return result;
        }
    }
    if let Some(result) = crate::Comptime::try_ambient_core_call(module, method, args.clone(), span)
    {
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
        ("core.files", "read") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::read_to_string(&path) {
                Ok(s) => Ok(CtValue::Present(Box::new(CtValue::Str(s)))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.files", "read_bytes") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::read(&path) {
                Ok(bs) => Ok(CtValue::Present(Box::new(CtValue::Bytes(bs)))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        // D-FILES-APPEND1=A: whole-file one-shot is `append_all` (not `append`,
        // which names the streaming handle's method).
        ("core.files", "write" | "append_all") => {
            let path_str = as_string(one(0)?, span)?;
            let content = as_string(one(1)?, span)?;
            let path = base_dir.join(path_str);
            let result = if method == "append_all" {
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .and_then(|mut f| f.write_all(content.as_bytes()).map(|_| ()))
            } else {
                std::fs::write(&path, content)
            };
            match result {
                Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.files", "exists" | "is_dir") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            let meta = std::fs::metadata(&path);
            Ok(CtValue::Bool(match (method, meta) {
                ("exists", Ok(_)) => true,
                ("exists", Err(_)) => false,
                ("is_dir", Ok(m)) => m.is_dir(),
                ("is_dir", Err(_)) => false,
                _ => false,
            }))
        }
        ("core.files", "create_dir") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::create_dir_all(&path) {
                Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        // D-LSDIR1: mirror AOT jet_std_fs_list_dir (sorted by name).
        ("core.files", "list_dir") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::read_dir(&path) {
                Ok(rd) => {
                    let mut entries = Vec::new();
                    let mut err: Option<std::io::Error> = None;
                    for entry in rd {
                        match entry {
                            Ok(entry) => {
                                let name = entry.file_name().to_string_lossy().to_string();
                                let full_path = path
                                    .join(&name)
                                    .to_string_lossy()
                                    .to_string();
                                let is_dir =
                                    entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                                entries.push((name, full_path, is_dir));
                            }
                            Err(e) => {
                                err = Some(e);
                                break;
                            }
                        }
                    }
                    if let Some(e) = err {
                        Ok(CtValue::failed(Box::new(io_error_value(
                            &path.to_string_lossy(),
                            e,
                        ))))
                    } else {
                        entries.sort_by(|a, b| a.0.cmp(&b.0));
                        Ok(CtValue::Present(Box::new(CtValue::List(
                            entries
                                .into_iter()
                                .map(|(name, full_path, is_dir)| CtValue::Struct {
                                    type_name: "DirEntry".to_string(),
                                    fields: vec![
                                        ("name".to_string(), CtValue::Str(name)),
                                        ("path".to_string(), CtValue::Str(full_path)),
                                        ("is_dir".to_string(), CtValue::Bool(is_dir)),
                                    ],
                                })
                                .collect(),
                        ))))
                    }
                }
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.files", "remove") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match result {
                Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.env", "get") => {
            let key = as_string(one(0)?, span)?;
            match std::env::var(key) {
                Ok(v) => Ok(CtValue::Present(Box::new(CtValue::Str(v)))),
                Err(_) => Ok(CtValue::absent(crate::AST::Type::String)),
            }
        }
        ("core.env", "set") => {
            let key = as_string(one(0)?, span)?;
            let val = as_string(one(1)?, span)?;
            std::env::set_var(key, val);
            Ok(CtValue::Unit)
        }
        ("core.env", "current_dir") => match std::env::current_dir() {
            Ok(p) => Ok(CtValue::Present(Box::new(CtValue::Str(
                p.to_string_lossy().into_owned(),
            )))),
            Err(e) => Ok(CtValue::failed(Box::new(io_error_value(".", e)))),
        },
        ("core.env", "home_dir") => Ok(
            match std::env::var("HOME")
                .ok()
                .or_else(|| std::env::var("USERPROFILE").ok())
            {
                Some(v) => CtValue::Present(Box::new(CtValue::Str(v))),
                None => CtValue::absent(crate::AST::Type::String),
            },
        ),
        ("core.io", "args") => {
            // Prefer argv installed for this jet run/deopt. Never fall back to
            // the host process argv — `cargo test` flags would leak into output.
            let argv = super::super::super::Interpreter::runtime_argv()
                .unwrap_or_else(|| vec!["jet".to_string()]);
            Ok(CtValue::List(argv.into_iter().map(CtValue::Str).collect()))
        }
        ("core.io", "progress") => {
            let Some(source) = args.first() else {
                return Err(unsupported("`core.io.progress` needs a source", span));
            };
            if let CtValue::Str(text) = source {
                if args.len() != 1 {
                    return Err(unsupported(
                        "`core.io.progress` text form takes one argument",
                        span,
                    ));
                }
                if let Some(sink) = sink {
                    sink.stdout.push_str(text);
                    sink.stdout.push('\n');
                }
                return Ok(CtValue::Unit);
            }
            let CtValue::List(items) = source else {
                return Err(unsupported(
                    "`core.io.progress` expects a List or Iter source",
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
        ("core.io", "print") => {
            let text = args
                .iter()
                .map(|v| v.jet_show())
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(s) = sink {
                s.stdout.push_str(&text);
                s.stdout.push('\n');
            }
            Ok(CtValue::Unit)
        }
        ("core.io", "eprint") => {
            let text = args
                .iter()
                .map(|v| v.jet_show())
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(s) = sink {
                s.stderr.push_str(&text);
                s.stderr.push('\n');
            }
            Ok(CtValue::Unit)
        }
        ("core.io", "input") | ("core.io", "read_all_input") => {
            if repl_mode {
                Err(repl_native_module_diag("core.io", method, span))
            } else {
                Ok(CtValue::Present(Box::new(CtValue::Str(String::new()))))
            }
        }
        ("core.io", "stdin") if repl_mode => Err(repl_native_module_diag("core.io", method, span)),
        ("core.io", "stdin") => Ok(CtValue::Struct {
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
                Err(e) => Ok(CtValue::failed(Box::new(io_error_value(&cmd[0], e)))),
            }
        }
        ("core.tls", _) => Err(Diagnostic::error(
            "E3412",
            format!("`core.tls.{}()` is not available at comptime", method),
            "live TLS sessions cannot be opened during compile-time evaluation".to_string(),
            "move the TLS operation to runtime; use `core.net.fetch(url, sha256: \"<hash>\")` for content-hash-pinned build-time downloads"
                .to_string(),
            Some(span),
        )),
        // Pure compress/archive/encoding codecs live on apply_core_call; reuse
        // them when the runtime evaluator has ambient impure depth open (#778
        // deopt / #715 default-dev encoding parity). Whole-value encoding must
        // not die as E0956 impure-tier after silent deopt.
        ("core.compress.gzip", _)
        | ("core.compress.zstd", _)
        | ("core.archive", _)
        | ("core.perf", _) => apply_core_call(module, method, args, span, repl_mode),
        (module, _) if module.starts_with("core.encoding.") => {
            apply_core_call(module, method, args, span, repl_mode)
        }
        // Ambient impure depth must not block pure-tier CorePureParity surfaces
        // that TirBridge already evaluates (date/math/measurement/testing/…).
        // Pure style/net helpers share the implementation dispatch so
        // impure_depth>0 (TirBridge / jet run deopt) still hits CorePureParity.
        ("core.io", method)
            if jet_foundation::Effects::core_effect("core.io", method).is_none() =>
        {
            apply_core_call(module, method, args, span, repl_mode)
        }
        ("core.random", _) | ("core.testing", "fake_rng") => {
            apply_core_call(module, method, args, span, repl_mode)
        }
        ("core.time.date", _)
        | ("core.time.duration", _)
        | ("core.time.instant", _)
        | ("core.math", _)
        | ("core.measurement", _)
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
        | ("core.linalg", _)
        | ("core.email", _)
        | ("core.xml", _)
        | ("core.json", _)
        | ("core.regex", _)
        | ("core.color", _)
        | ("core.units", _)
        | ("core.time", _)
        | ("core.time.datetime", _)
        | ("core.science.measurement", _) => apply_core_call(module, method, args, span, repl_mode),
        // Pure net helpers (e.g. ip_addr, socket_addr_parse) — not live sockets.
        // Keep E3412 for the rest. D-META-EFFECT1: "pure" is what the effect
        // table says, so both tiers agree without a second list here.
        ("core.net", method)
            if jet_foundation::Effects::core_effect("core.net", method).is_none() =>
        {
            apply_core_call(module, method, args, span, repl_mode)
        }
        ("core.net", _) => Err(Diagnostic::error(
            "E3412",
            format!("`core.net.{}()` is not available at comptime", method),
            "only `core.net.fetch(url, sha256:)` is supported at compile time".to_string(),
            "use `core.net.fetch(url, sha256: \"<hash>\")` for content-hash-pinned downloads"
                .to_string(),
            Some(span),
        )),
        _ => Err(unsupported(
            &format!("`{}.{}()` at comptime", module, method),
            span,
        )),
    }
}
