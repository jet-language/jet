//! REPL authorization and pinned process execution.

use crate::Diagnostics::{Diagnostic, Span};
use super::super::Diagnostics::unsupported;
use crate::AST::{CtValue, Type};
use jet_foundation::Effects::{core_effect, is_nondeterministic_core, Effect};
use super::core_calls::{
    apply_core_call_with_type, apply_impure_core_call_with_type, as_string, io_error_value,
    normalize_path_args, IoErrorOperation,
};

pub(super) fn repl_effect_request(module: &str, method: &str, args: &[CtValue]) -> super::super::ReplEffectRequest {
    let shown = |i: usize, fallback: &str| {
        args.get(i).map(CtValue::jet_show).unwrap_or_else(|| fallback.to_string())
    };
    let (root, operation, resource) = if is_nondeterministic_core(module, method) {
        let (root, operation) = match core_effect(module, method) {
            Some(Effect::Time) => ("Time", "Read"),
            Some(Effect::Rand) => ("Rand", "Draw"),
            _ => unreachable!("nondeterministic Core call has no time/rand effect"),
        };
        (root, operation, method.to_string())
    } else {
        match (module, method) {
            ("core.files", "read" | "read_bytes" | "exists" | "is_dir") =>
                ("FS", "Read", shown(0, "<path>")),
            ("core.files", "write" | "append_all" | "create_dir" | "remove") =>
                ("FS", "Write", shown(0, "<path>")),
            ("core.env", "get") => ("Env", "Read", shown(0, "<key>")),
            ("core.env", "set") => ("Env", "Write", shown(0, "<key>")),
            ("core.env", "current_dir") => ("Env", "Read", "PWD".to_string()),
            ("core.env", "home_dir") => ("Env", "Read", "HOME".to_string()),
            ("core.io", "eprint") => ("IO", "Write", "stderr".to_string()),
            ("core.io", "input" | "read_all_input" | "stdin") =>
                ("IO", "Read", "stdin".to_string()),
            ("core.io", "args") => ("IO", "Read", "argv".to_string()),
            ("core.process", "run") => ("Exec", "Run", shown(0, "<command>")),
            ("core.process", "exit") => ("Exec", "Exit", shown(0, "0")),
            ("core.random", _) => ("Rand", "Draw", method.to_string()),
            ("core.net" | "core.tls", _) =>
                ("Net", method, shown(0, "<network resource>")),
            ("core.exec", _) => ("Exec", method, shown(0, "<command>")),
            _ => ("IO", method, module.to_string()),
        }
    };
    super::super::ReplEffectRequest {
        root: root.to_string(),
        operation: operation.to_string(),
        resource,
    }
}

pub(super) fn apply_repl_fs_call(
    method: &str,
    args: &[CtValue],
    span: Span,
    authorizer: &mut dyn super::super::ReplAuthorizer,
) -> Result<CtValue, Diagnostic> {
    let one = |i: usize| {
        args.get(i)
            .ok_or_else(|| unsupported("core.files call has the wrong number of arguments", span))
    };
    let path = as_string(one(0)?, span)?;
    let io_operation = match method {
        "read" | "read_bytes" => IoErrorOperation::Read,
        "write" | "append_all" | "create_dir" | "remove" => IoErrorOperation::Write,
        _ => IoErrorOperation::Read,
    };
    let io_error = |error| CtValue::failed(Box::new(io_error_value(
        io_operation,
        &path,
        error,
    )));
    match method {
        "read" => Ok(match authorizer.fs_read(&path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => CtValue::Present(Box::new(CtValue::Str(text))),
                Err(error) => io_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            },
            Err(error) => io_error(error),
        }),
        "read_bytes" => Ok(match authorizer.fs_read(&path) {
            Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
            Err(error) => io_error(error),
        }),
        "write" | "append_all" => {
            let content = as_string(one(1)?, span)?;
            Ok(match authorizer.fs_write(&path, content.as_bytes(), method == "append_all") {
                Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                Err(error) => io_error(error),
            })
        }
        "exists" => authorizer
            .fs_exists(&path)
            .map(CtValue::Bool)
            .map_err(|error| unsupported(&format!("secure filesystem check failed: {error}"), span)),
        "is_dir" => authorizer
            .fs_is_dir(&path)
            .map(CtValue::Bool)
            .map_err(|error| unsupported(&format!("secure filesystem check failed: {error}"), span)),
        "create_dir" => Ok(match authorizer.fs_create_dir(&path) {
            Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
            Err(error) => io_error(error),
        }),
        "remove" => Ok(match authorizer.fs_remove(&path) {
            Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
            Err(error) => io_error(error),
        }),
        _ => Err(unsupported(&format!("core.files.{method}"), span)),
    }
}

/// Authorize and execute one REPL Core effect through the shared host seam.
/// TIR and AST evaluation both call this function, so invocation policy and
/// secure filesystem dispatch stay in one place.
pub fn apply_repl_authorized_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    base_dir: &std::path::Path,
    sink: Option<&mut super::super::Interpreter::DevSink>,
    grants: &[String],
    authorizer: Option<&mut dyn super::super::ReplAuthorizer>,
) -> Result<CtValue, Diagnostic> {
    apply_repl_authorized_core_call_with_type(
        module,
        method,
        args,
        span,
        base_dir,
        sink,
        grants,
        authorizer,
        None,
    )
}

pub fn apply_repl_authorized_core_call_with_type(
    module: &str,
    method: &str,
    mut args: Vec<CtValue>,
    span: Span,
    base_dir: &std::path::Path,
    sink: Option<&mut super::super::Interpreter::DevSink>,
    grants: &[String],
    authorizer: Option<&mut dyn super::super::ReplAuthorizer>,
    resolved_ret: Option<&Type>,
) -> Result<CtValue, Diagnostic> {
    args = normalize_path_args(module, method, args, span)?;
    // REPL eprint is the inline transcript sink. It does not need an effect
    // prompt or a lexical grant; the existing REPL surface keeps it available.
    if module == "core.io" && method == "eprint" {
        return apply_impure_core_call_with_type(
            module, method, args, span, base_dir, sink, true, None, None, resolved_ret,
        );
    }

    let pinned_executable = if matches!((module, method), ("core.process", "run")) {
        Some(pin_repl_command(&mut args, base_dir, span)?)
    } else {
        None
    };
    let request = repl_effect_request(module, method, &args);
    let Some(authorizer) = authorizer else {
        return Err(Diagnostic::error(
            "E1803",
            format!(
                "{}.{} for `{}` was denied",
                request.root, request.operation, request.resource
            ),
            "this REPL mode has no runtime authority provider, so the host operation did not run"
                .to_string(),
            format!(
                "restart with `jet repl --allow-{}` or use an interactive session and approve the exact operation",
                request.root.to_ascii_lowercase()
            ),
            Some(span),
        ));
    };
    authorizer.preflight(&request, span)?;
    let granted = grants.iter().any(|cap| {
        cap == &request.root || cap.starts_with(&format!("{}.", request.root))
    });
    if !granted {
        return Err(Diagnostic::error(
            "E1803",
            format!(
                "{}.{} for `{}` has no REPL runtime authority",
                request.root, request.operation, request.resource
            ),
            "REPL host effects require both lexical `#Grant` authority and invocation policy; no host operation ran"
                .to_string(),
            format!(
                "wrap this operation in `#Grant({}) {{ caps -> ... }}`; interactive sessions then prompt, while non-TTY sessions also need `--allow-{}`",
                request.root,
                request.root.to_ascii_lowercase()
            ),
            Some(span),
        ));
    }
    authorizer.authorize(&request, span)?;
    if module == "core.io" && matches!(method, "input" | "read_all_input") {
        let prompt = match args.first() {
            Some(CtValue::Str(value)) => value.as_str(),
            _ => "",
        };
        return Ok(match authorizer.read_input(prompt) {
            Ok(line) => CtValue::Present(Box::new(CtValue::Str(line))),
            Err(error) => CtValue::failed(Box::new(io_error_value(
                IoErrorOperation::Read,
                "stdin",
                error,
            ))),
        });
    }
    if module == "core.files" {
        return apply_repl_fs_call(method, &args, span, authorizer);
    }
    if module == "core.random" {
        return apply_core_call_with_type(module, method, args, span, true, resolved_ret);
    }
    let verified_root = if matches!((module, method), ("core.process", "run")) {
        Some(authorizer.verified_root().map_err(|error| {
            unsupported(
                &format!("REPL project root handle is unavailable: {error}"),
                span,
            )
        })?)
    } else {
        None
    };
    apply_impure_core_call_with_type(
        module,
        method,
        args,
        span,
        base_dir,
        sink,
        true,
        pinned_executable.as_ref(),
        verified_root.as_ref(),
        resolved_ret,
    )
}

pub(super) fn pin_repl_command(
    args: &mut [CtValue],
    base_dir: &std::path::Path,
    span: Span,
) -> Result<std::fs::File, Diagnostic> {
    let Some(CtValue::List(words)) = args.first_mut() else {
        return Err(unsupported("process.run expects a list of command words", span));
    };
    let Some(CtValue::Str(program)) = words.first_mut() else {
        return Err(unsupported("process.run needs an executable name", span));
    };
    let candidate = std::path::Path::new(program);
    let resolved = if candidate.components().count() > 1 || candidate.is_absolute() {
        let path = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            base_dir.join(candidate)
        };
        std::fs::canonicalize(path).ok()
    } else {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(candidate))
                .find_map(|path| std::fs::canonicalize(path).ok().filter(|p| p.is_file()))
        })
    };
    let Some(resolved) = resolved else {
        return Err(unsupported(
            &format!("process.run could not resolve executable `{program}`"),
            span,
        ));
    };
    let executable = std::fs::File::open(&resolved).map_err(|error| {
        unsupported(
            &format!("process.run could not pin executable `{}`: {error}", resolved.display()),
            span,
        )
    })?;
    if !executable.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err(unsupported("process.run executable is not a regular file", span));
    }
    *program = resolved.to_string_lossy().into_owned();
    Ok(executable)
}

#[cfg(target_os = "linux")]
fn repl_descriptor_path(fd: std::os::fd::RawFd) -> String {
    format!("/proc/self/fd/{fd}")
}

// Darwin and BSD expose inherited descriptors through fdescfs at `/dev/fd`.
// Resolve executable and cwd through the already-open descriptor, preserving
// the REPL's path-swap protection without relying on Linux `/proc`.
#[cfg(all(unix, not(target_os = "linux")))]
fn repl_descriptor_path(fd: std::os::fd::RawFd) -> String {
    format!("/dev/fd/{fd}")
}

pub(super) fn run_repl_process(
    cmd: &[String],
    base_dir: &std::path::Path,
    pinned_executable: Option<&std::fs::File>,
    verified_root: Option<&std::fs::File>,
    timeout: std::time::Duration,
) -> std::io::Result<std::process::Output> {
    use std::fs::OpenOptions;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    static CAPTURE_ID: AtomicU64 = AtomicU64::new(0);
    let capture_file = |stream: &str| {
        loop {
            let id = CAPTURE_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "jet-repl-process-{}-{id}-{stream}",
                std::process::id()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => break Ok((path, file)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => break Err(e),
            }
        }
    };
    let (stdout_path, stdout_file) = capture_file("stdout")?;
    let (stderr_path, stderr_file) = match capture_file("stderr") {
        Ok(capture) => capture,
        Err(e) => {
            let _ = std::fs::remove_file(&stdout_path);
            return Err(e);
        }
    };
    #[cfg(unix)]
    use std::os::fd::AsRawFd;
    #[cfg(unix)]
    use std::os::unix::process::CommandExt;

    #[cfg(unix)]
    let executable = pinned_executable
        .map(|file| repl_descriptor_path(file.as_raw_fd()))
        .unwrap_or_else(|| cmd[0].clone());
    #[cfg(not(unix))]
    let executable = if pinned_executable.is_some() {
        let _ = std::fs::remove_file(&stdout_path);
        let _ = std::fs::remove_file(&stderr_path);
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "this platform cannot launch a descriptor-pinned REPL executable",
        ));
    } else {
        cmd[0].clone()
    };
    #[cfg(unix)]
    let cwd = verified_root
        .map(|file| repl_descriptor_path(file.as_raw_fd()))
        .unwrap_or_else(|| base_dir.to_string_lossy().into_owned());
    #[cfg(not(unix))]
    let cwd = base_dir;
    let mut command = std::process::Command::new(executable);
    command
        .args(&cmd[1..])
        .current_dir(cwd)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    #[cfg(unix)]
    if pinned_executable.is_some() {
        command.process_group(0);
    }
    let child = command.spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_file(&stdout_path);
            let _ = std::fs::remove_file(&stderr_path);
            return Err(e);
        }
    };
    #[cfg(unix)]
    let _signal_forward = pinned_executable.map(|_| ReplSignalForward::install(child.id() as i32));
    #[cfg(unix)]
    let _terminal_signals = pinned_executable.and_then(|_| ReplTerminalSignals::enable());

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                #[cfg(unix)]
                if pinned_executable.is_some() {
                    kill_repl_process_group(child.id() as i32);
                } else {
                    let _ = child.kill();
                }
                #[cfg(not(unix))]
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&stdout_path);
                let _ = std::fs::remove_file(&stderr_path);
                return Err(e);
            }
        }
        if Instant::now() >= deadline {
            #[cfg(unix)]
            if pinned_executable.is_some() {
                kill_repl_process_group(child.id() as i32);
            } else {
                let _ = child.kill();
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&stdout_path);
            let _ = std::fs::remove_file(&stderr_path);
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "process.run exceeded the 30 second REPL limit",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    #[cfg(unix)]
    if pinned_executable.is_some() {
        // A command that backgrounds descendants does not get to leak them past
        // the REPL operation boundary after its group leader exits.
        kill_repl_process_group(child.id() as i32);
    }
    let stdout = std::fs::read(&stdout_path);
    let stderr = std::fs::read(&stderr_path);
    let _ = std::fs::remove_file(&stdout_path);
    let _ = std::fs::remove_file(&stderr_path);
    Ok(std::process::Output {
        status,
        stdout: stdout?,
        stderr: stderr?,
    })
}

#[cfg(unix)]
static REPL_CHILD_GROUP: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

// POSIX `signal(3)` keeps platform-specific `sigaction` layouts out of this
// std-only crate. Store returned disposition opaquely so nested guards restore
// either prior Jet handler or SIG_DFL/SIG_IGN exactly.
#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
    fn signal(signal: i32, handler: usize) -> usize;
}

#[cfg(unix)]
extern "C" fn forward_repl_interrupt(signal_number: i32) {
    super::super::note_repl_interrupt();
    super::super::warn_repl_runtime_call_stopping();
    let group = REPL_CHILD_GROUP.load(std::sync::atomic::Ordering::Relaxed);
    if group > 0 {
        unsafe { kill(-group, signal_number) };
    }
}

#[cfg(unix)]
struct ReplSignalForward {
    previous: usize,
}

#[cfg(unix)]
impl ReplSignalForward {
    fn install(group: i32) -> Self {
        REPL_CHILD_GROUP.store(group, std::sync::atomic::Ordering::SeqCst);
        let previous = unsafe { signal(2, forward_repl_interrupt as *const () as usize) };
        Self { previous }
    }
}

#[cfg(unix)]
impl Drop for ReplSignalForward {
    fn drop(&mut self) {
        REPL_CHILD_GROUP.store(0, std::sync::atomic::Ordering::SeqCst);
        unsafe { signal(2, self.previous) };
    }
}

#[cfg(unix)]
fn kill_repl_process_group(group: i32) {
    unsafe { kill(-group, 9) };
}

#[cfg(unix)]
struct ReplTerminalSignals {
    saved: String,
}

#[cfg(unix)]
impl ReplTerminalSignals {
    fn enable() -> Option<Self> {
        use std::io::IsTerminal;
        use std::process::Stdio;

        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return None;
        }
        let saved = std::process::Command::new("stty")
            .arg("-g")
            .stdin(Stdio::inherit())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !saved.status.success() {
            return None;
        }
        let saved = String::from_utf8_lossy(&saved.stdout).trim().to_string();
        if saved.is_empty() {
            return None;
        }
        let enabled = std::process::Command::new("stty")
            .arg("isig")
            .stdin(Stdio::inherit())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()?
            .success();
        enabled.then_some(Self { saved })
    }
}

#[cfg(unix)]
impl Drop for ReplTerminalSignals {
    fn drop(&mut self) {
        use std::process::Stdio;
        let _ = std::process::Command::new("stty")
            .arg(&self.saved)
            .stdin(Stdio::inherit())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(all(test, unix))]
mod repl_process_tests {
    use super::run_repl_process;
    use std::time::{Duration, Instant};

    #[test]
    fn timeout_kills_and_reaps_the_process_group() {
        let executable = std::fs::File::open("/bin/sh").expect("pin /bin/sh");
        let root = std::fs::File::open(".").expect("open cwd");
        let cmd = vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "while :; do :; done".to_string(),
        ];
        let started = Instant::now();
        let error = run_repl_process(
            &cmd,
            std::path::Path::new("."),
            Some(&executable),
            Some(&root),
            Duration::from_millis(50),
        )
        .expect_err("long process must time out");
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
