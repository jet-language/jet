//! REPL authorization and pinned process execution.

use super::super::Diagnostics::unsupported;
use super::core_calls::{
    apply_core_call_with_type, apply_impure_core_call_with_type, as_string, io_error_value,
    normalize_path_args, IoErrorOperation,
};
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtValue, Type};
use jet_foundation::Authority::{answer, Holds, Verdict};
use jet_foundation::Effects::{core_effect, is_nondeterministic_core, Effect};

fn repl_effect_roles(
    required: &str,
    granted: &Holds,
    denied: &Holds,
    authority: &str,
) -> String {
    let render = |effects: &Holds| {
        if effects.is_empty() {
            "none".to_string()
        } else {
            effects.iter().cloned().collect::<Vec<_>>().join(", ")
        }
    };
    format!(
        "required_effects={required}; granted_effects={}; denied_effects={}; authority={authority}",
        render(granted),
        render(denied),
    )
}

pub(super) fn repl_effect_request(
    module: &str,
    method: &str,
    args: &[CtValue],
) -> super::super::ReplEffectRequest {
    let shown = |i: usize, fallback: &str| {
        args.get(i)
            .map(CtValue::jet_show)
            .unwrap_or_else(|| fallback.to_string())
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
            ("core.files", "read" | "read_bytes" | "exists" | "is_dir") => {
                ("FS", "Read", shown(0, "<path>"))
            }
            ("core.files", "write" | "append_all" | "create_dir" | "remove") => {
                ("FS", "Write", shown(0, "<path>"))
            }
            ("core.sys", "get") => ("Env", "Read", shown(0, "<key>")),
            ("core.sys", "set") => ("Env", "Write", shown(0, "<key>")),
            ("core.sys", "current_dir") => ("Env", "Read", "PWD".to_string()),
            ("core.sys", "home_dir") => ("Env", "Read", "HOME".to_string()),
            ("core.term", "eprint") => ("IO", "Write", "stderr".to_string()),
            ("core.term", "input" | "read_all_input" | "stdin") => {
                ("IO", "Read", "stdin".to_string())
            }
            ("core.process", "argv" | "args") => ("IO", "Read", "argv".to_string()),
            ("core.process", "run") => ("Exec", "Run", shown(0, "<command>")),
            ("core.process", "exit") => ("Exec", "Exit", shown(0, "0")),
            ("core.math.random", _) => ("Rand", "Draw", method.to_string()),
            ("core.net" | "core.net.tls", _) => ("Net", method, shown(0, "<network resource>")),
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
    let io_error = |error| CtValue::failed(Box::new(io_error_value(io_operation, &path, error)));
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
            Ok(
                match authorizer.fs_write(&path, content.as_bytes(), method == "append_all") {
                    Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                    Err(error) => io_error(error),
                },
            )
        }
        "exists" => authorizer
            .fs_exists(&path)
            .map(CtValue::Bool)
            .map_err(|error| {
                unsupported(&format!("secure filesystem check failed: {error}"), span)
            }),
        "is_dir" => authorizer
            .fs_is_dir(&path)
            .map(CtValue::Bool)
            .map_err(|error| {
                unsupported(&format!("secure filesystem check failed: {error}"), span)
            }),
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
    grants: &[CtValue],
    authorizer: Option<&mut dyn super::super::ReplAuthorizer>,
) -> Result<CtValue, Diagnostic> {
    apply_repl_authorized_core_call_with_type(
        module, method, args, span, base_dir, sink, grants, authorizer, None,
    )
}

pub fn apply_repl_authorized_core_call_with_type(
    module: &str,
    method: &str,
    mut args: Vec<CtValue>,
    span: Span,
    base_dir: &std::path::Path,
    sink: Option<&mut super::super::Interpreter::DevSink>,
    grants: &[CtValue],
    authorizer: Option<&mut dyn super::super::ReplAuthorizer>,
    resolved_ret: Option<&Type>,
) -> Result<CtValue, Diagnostic> {
    args = normalize_path_args(module, method, args, span)?;
    // Both qualified print spellings are ordinary transcript output. They do
    // not need an effect prompt or a lexical grant; the qualified twin must
    // keep the ambient `print` job usable in #NoPrelude REPL turns.
    if module == "core.term" && matches!(method, "print" | "eprint") {
        return apply_impure_core_call_with_type(
            module,
            method,
            args,
            span,
            base_dir,
            sink,
            true,
            None,
            None,
            resolved_ret,
        );
    }

    let pinned_executable = if matches!((module, method), ("core.process", "run")) {
        Some(pin_repl_command(&mut args, base_dir, span)?)
    } else {
        None
    };
    let request = repl_effect_request(module, method, &args);
    let Some(authorizer) = authorizer else {
        let granted = grants
            .iter()
            .filter_map(super::super::Builtins::authority_holds)
            .flatten()
            .collect::<Holds>();
        let denied = Holds::new();
        return Err(Diagnostic::error(
            "E1803",
            format!(
                "{}.{} for `{}` was denied",
                request.root, request.operation, request.resource
            ),
            format!(
                "this REPL mode has no runtime authority provider; {}; the host operation did not run",
                repl_effect_roles(&request.root, &granted, &denied, "REPL lexical Authority"),
            ),
            format!(
                "restart with `jet repl --allow-{}` or use an interactive session and approve the exact operation",
                request.root.to_ascii_lowercase()
            ),
            Some(span),
        ));
    };
    authorizer.preflight(&request, span)?;
    // The two-argument boundary form carries the actual named Authority value.
    // One-argument REPL calls use the active lexical authority values, which
    // are the same CtValue carrier rather than a copied list of right names.
    let boundary_authority = args.len() == 2
        && matches!(
            (module, method),
            ("core.process", "run") | ("core.plugin", "load")
        );
    let held = if boundary_authority {
        args.get(1)
            .and_then(super::super::Builtins::authority_holds)
            .unwrap_or_default()
    } else {
        grants
            .iter()
            .filter_map(super::super::Builtins::authority_holds)
            .flatten()
            .collect::<Holds>()
    };
    let denied = Holds::new();
    let granted = answer(&held, &denied, &request.root) == Verdict::Allowed;
    if !granted {
        return Err(Diagnostic::error(
            "E1803",
            format!(
                "{}.{} for `{}` has no REPL runtime authority",
                request.root, request.operation, request.resource
            ),
            format!(
                "REPL host effects require both lexical `#FX` access and invocation policy; {}; no host operation ran",
                repl_effect_roles(&request.root, &held, &denied, "REPL lexical Authority"),
            ),
            format!(
                "wrap this operation in `#FX(grant: {}) {{ ... }}`; interactive sessions then prompt, while non-TTY sessions also need `--allow-{}`",
                request.root,
                request.root.to_ascii_lowercase()
            ),
            Some(span),
        ));
    }
    authorizer.authorize(&request, span)?;
    if module == "core.term" && matches!(method, "input" | "read_all_input") {
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
    if module == "core.math.random" {
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
        return Err(unsupported(
            "process.run expects a list of command words",
            span,
        ));
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
            &format!(
                "process.run could not pin executable `{}`: {error}",
                resolved.display()
            ),
            span,
        )
    })?;
    if !executable
        .metadata()
        .is_ok_and(|metadata| metadata.is_file())
    {
        return Err(unsupported(
            "process.run executable is not a regular file",
            span,
        ));
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

pub(super) const REPL_PROCESS_OUTPUT_LIMIT_BYTES: usize = 64 * 1024 * 1024;

fn drain_repl_output(
    mut reader: impl std::io::Read,
    used: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
    overflowed: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(output);
        }
        if used
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |previous| {
                    previous
                        .checked_add(count)
                        .filter(|total| *total <= REPL_PROCESS_OUTPUT_LIMIT_BYTES)
                },
            )
            .is_err()
        {
            overflowed.store(true, std::sync::atomic::Ordering::Release);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "process.run output exceeded {REPL_PROCESS_OUTPUT_LIMIT_BYTES} bytes"
                ),
            ));
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

pub(super) fn run_repl_process(
    cmd: &[String],
    base_dir: &std::path::Path,
    pinned_executable: Option<&std::fs::File>,
    verified_root: Option<&std::fs::File>,
    timeout: std::time::Duration,
) -> std::io::Result<std::process::Output> {
    use std::process::Stdio;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    if pinned_executable.is_some() {
        command.process_group(0);
    }
    let child = command.spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return Err(e),
    };
    let stdout = child.stdout.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "process.run stdout pipe unavailable")
    });
    let stderr = child.stderr.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "process.run stderr pipe unavailable")
    });
    let (stdout, stderr) = match (stdout, stderr) {
        (Ok(stdout), Ok(stderr)) => (stdout, stderr),
        (Err(error), _) | (_, Err(error)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let output_used = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let output_overflowed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stdout_used = output_used.clone();
    let stdout_overflowed = output_overflowed.clone();
    let stdout_thread = std::thread::spawn(move || {
        drain_repl_output(stdout, &stdout_used, &stdout_overflowed)
    });
    let stderr_used = output_used;
    let stderr_overflowed = output_overflowed.clone();
    let stderr_thread = std::thread::spawn(move || {
        drain_repl_output(stderr, &stderr_used, &stderr_overflowed)
    });
    #[cfg(unix)]
    let _signal_forward = pinned_executable.map(|_| ReplSignalForward::install(child.id() as i32));
    #[cfg(unix)]
    let _terminal_signals = pinned_executable.and_then(|_| ReplTerminalSignals::enable());

    let stop_child = |child: &mut std::process::Child| {
        #[cfg(unix)]
        if pinned_executable.is_some() {
            kill_repl_process_group(child.id() as i32);
        } else {
            let _ = child.kill();
        }
        #[cfg(not(unix))]
        let _ = child.kill();
        let _ = child.wait();
    };
    let deadline = Instant::now() + timeout;
    let status = loop {
        if output_overflowed.load(Ordering::Acquire) {
            stop_child(&mut child);
            break Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("process.run output exceeded {REPL_PROCESS_OUTPUT_LIMIT_BYTES} bytes"),
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(e) => {
                stop_child(&mut child);
                break Err(e);
            }
        }
        if Instant::now() >= deadline {
            stop_child(&mut child);
            break Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "process.run exceeded the 30 second REPL limit",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if status.is_ok() && pinned_executable.is_some() {
        #[cfg(unix)]
        // A command that backgrounds descendants does not get to leak them past
        // the REPL operation boundary after its group leader exits.
        kill_repl_process_group(child.id() as i32);
    }
    let stdout = stdout_thread.join().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::Other, "process.run stdout reader panicked")
    });
    let stderr = stderr_thread.join().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::Other, "process.run stderr reader panicked")
    });
    let status = status?;
    Ok(std::process::Output {
        status,
        stdout: stdout??,
        stderr: stderr??,
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

    #[test]
    fn output_limit_kills_a_flooding_process_before_capture_grows_unbounded() {
        let executable = std::fs::File::open("/bin/sh").expect("pin /bin/sh");
        let root = std::fs::File::open(".").expect("open cwd");
        let cmd = vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "yes x | head -c 67108865".to_string(),
        ];
        let started = Instant::now();
        let error = run_repl_process(
            &cmd,
            std::path::Path::new("."),
            Some(&executable),
            Some(&root),
            Duration::from_secs(10),
        )
        .expect_err("output beyond the REPL budget must fail");
        assert!(
            error.to_string().contains("output exceeded"),
            "unexpected output-limit error: {error}"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
