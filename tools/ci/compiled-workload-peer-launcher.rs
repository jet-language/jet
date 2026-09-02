#![allow(clippy::missing_safety_doc)]

//! Native boundary for compiled-workload peer commands.
//!
//! The launcher deliberately has no ambient fallback. Linux uses Bubblewrap
//! mount and namespace isolation, macOS uses Seatbelt, and Windows uses the
//! repository's AppContainer backend plus temporary Firewall rules for the
//! loopback-only policy.

use std::env;
use std::fs;
use std::io;
#[cfg(target_os = "windows")]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::atomic::{AtomicI32, Ordering};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

const CONTRACT: &str = "compiled-workload-peer-isolation-v1";
const EXIT_UNSUPPORTED: i32 = 78;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NetworkPolicy {
    Disabled,
    LoopbackOnly,
}

#[derive(Debug)]
struct Invocation {
    task_id: String,
    root: PathBuf,
    cwd: PathBuf,
    network: NetworkPolicy,
    command: String,
    args: Vec<String>,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    #[cfg(target_os = "linux")]
    if args.first().is_some_and(|arg| arg == "--internal-disabled") {
        run_internal_disabled(&args[1..]);
    }
    #[cfg(target_os = "linux")]
    if args.first().is_some_and(|arg| arg == "--internal-loopback") {
        run_internal_loopback(&args[1..]);
    }
    if args.as_slice() == ["--version"] {
        println!("{CONTRACT}");
        return;
    }
    if args.as_slice() == ["--contract"] {
        match capability_check() {
            Ok(()) => println!("{CONTRACT}"),
            Err(reason) => fail(&reason),
        }
        return;
    }
    match parse_invocation(&args).and_then(|invocation| {
        capability_check()?;
        run(invocation)
    }) {
        Ok(status) => finish_status(status),
        Err(reason) => fail(&reason),
    }
}
#[cfg(target_os = "linux")]
fn run_internal_status_file(args: &[String]) -> (Option<PathBuf>, &[String]) {
    if !args.first().is_some_and(|arg| arg == "--status-file") {
        return (None, args);
    }
    let Some(value) = args.get(1) else {
        fail("internal runner status file is missing");
    };
    if value.is_empty() || value.contains('\0') {
        fail("internal runner status file is invalid");
    }
    (Some(PathBuf::from(value)), &args[2..])
}

#[cfg(target_os = "linux")]
fn run_internal_disabled(args: &[String]) -> ! {
    // Bubblewrap removes every external interface. The extra filter denies
    // creation of network sockets while leaving pipe/socketpair primitives
    // available for compiler and linker subprocesses.
    if let Err(reason) = install_disabled_network_filter() {
        fail(&reason);
    }
    let (status_file, args) = run_internal_status_file(args);
    run_internal_supervisor(args, status_file);
}

#[cfg(target_os = "linux")]
fn run_internal_loopback(args: &[String]) -> ! {
    let (status_file, args) = run_internal_status_file(args);
    let Some(ip) = args.first() else {
        fail("loopback-only internal runner received no ip command");
    };
    if args.get(1).is_none() {
        fail("loopback-only internal runner received no command");
    }
    let status = Command::new(ip)
        .args(["link", "set", "lo", "up"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("loopback-only internal runner could not start `{ip}`: {error}"));
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => fail(&format!("loopback-only internal runner `{ip}` exited {status}")),
        Err(reason) => fail(&reason),
    }
    run_internal_supervisor(&args[1..], status_file);
}

#[cfg(target_os = "linux")]
fn run_internal_supervisor(args: &[String], status_file: Option<PathBuf>) -> ! {
    let Some(command) = args.first() else {
        fail("internal runner received no command");
    };
    let mut child_command = Command::new(command);
    child_command.args(&args[1..]);
    match run_linux_inner_child(child_command) {
        Ok(status) => {
            use std::os::unix::process::ExitStatusExt;
            if let (Some(path), Some(signal)) = (&status_file, status.signal()) {
                fs::write(path, [signal as u8]).map_err(|error| {
                    format!("internal runner signal status could not propagate: {error}")
                }).unwrap_or_else(|reason| fail(&reason));
            }
            finish_status(status)
        }
        Err(reason) => fail(&reason),
    }
}

fn fail(reason: &str) -> ! {
    eprintln!("compiled-workload-peer-launcher: {reason}");
    std::process::exit(EXIT_UNSUPPORTED);
}

fn parse_invocation(args: &[String]) -> Result<Invocation, String> {
    if args.is_empty() {
        return Err("missing execution arguments; use --contract for the capability probe".into());
    }
    let mut task_id = None;
    let mut root = None;
    let mut cwd = None;
    let mut network = None;
    let mut external_write = None;
    let mut host = None;
    let mut command = None;
    let mut command_args = Vec::new();
    let mut index = 0;
    let mut separator = false;
    while index < args.len() {
        let arg = &args[index];
        if separator {
            if command.is_none() {
                command = Some(arg.clone());
            } else {
                command_args.push(arg.clone());
            }
            index += 1;
            continue;
        }
        if arg == "--" {
            separator = true;
            index += 1;
            continue;
        }
        let option = arg
            .strip_prefix("--")
            .ok_or_else(|| format!("expected launcher option, found `{arg}`"))?;
        let (name, inline_value) = option
            .split_once('=')
            .map_or((option, None), |(name, value)| (name, Some(value)));
        let value = |name: &str, inline: Option<&str>, index: &mut usize| {
            if let Some(value) = inline {
                return Ok(value.to_string());
            }
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| format!("missing value for --{name}"))
        };
        match name {
            "contract" => {
                let value = value("contract", inline_value, &mut index)?;
                if value != CONTRACT {
                    return Err(format!("unsupported contract `{value}`"));
                }
            }
            "task-id" => {
                let value = value("task-id", inline_value, &mut index)?;
                if value.is_empty()
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                {
                    return Err(format!("invalid task id `{value}`"));
                }
                task_id = Some(value);
            }
            "root" => root = Some(value("root", inline_value, &mut index)?),
            "cwd" => cwd = Some(value("cwd", inline_value, &mut index)?),
            "network" => {
                let value = value("network", inline_value, &mut index)?;
                network = Some(match value.as_str() {
                    "disabled" => NetworkPolicy::Disabled,
                    "loopback-only" => NetworkPolicy::LoopbackOnly,
                    other => return Err(format!("unsupported network authority `{other}`")),
                });
            }
            "external-write" => {
                let value = value("external-write", inline_value, &mut index)?;
                if value != "disabled" {
                    return Err(format!("unsupported external-write authority `{value}`"));
                }
                external_write = Some(value);
            }
            "host" => {
                let value = value("host", inline_value, &mut index)?;
                if value != "ambient" {
                    return Err(format!("unsupported host authority `{value}`"));
                }
                host = Some(value);
            }
            other if other.starts_with("--") => {
                return Err(format!("unsupported launcher option `{other}`"));
            }
            other => return Err(format!("expected `--` before command, found `{other}`")),
        }
        index += 1;
    }
    if !separator {
        return Err("missing `--` command separator".into());
    }
    let task_id = task_id.ok_or_else(|| "missing --task-id".to_string())?;
    let root = canonical_directory(
        "root",
        root.ok_or_else(|| "missing --root".to_string())?,
    )?;
    let cwd = canonical_directory("cwd", cwd.ok_or_else(|| "missing --cwd".to_string())?)?;
    if root.parent().is_none() || root == Path::new("/") {
        return Err("root must not be a filesystem root".into());
    }
    if !contained(&root, &cwd) {
        return Err(format!(
            "cwd `{}` escapes root `{}` after canonicalization",
            cwd.display(),
            root.display()
        ));
    }
    if external_write.is_none() {
        return Err("missing --external-write=disabled".into());
    }
    if host.is_none() {
        return Err("missing --host=ambient".into());
    }
    let network = network.ok_or_else(|| "missing --network authority".to_string())?;
    let command = command.ok_or_else(|| "missing command after `--`".to_string())?;
    if command.is_empty() || command.contains('\0') {
        return Err("command is empty or contains NUL".into());
    }
    if command_args.iter().any(|arg| arg.contains('\0')) {
        return Err("command argument contains NUL".into());
    }
    Ok(Invocation {
        task_id,
        root,
        cwd,
        network,
        command,
        args: command_args,
    })
}

fn canonical_directory(label: &str, value: String) -> Result<PathBuf, String> {
    if value.is_empty() || value.contains('\0') {
        return Err(format!("{label} is empty or contains NUL"));
    }
    let path = PathBuf::from(value);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("{label} `{}` is unavailable: {error}", path.display()))?;
    if !metadata.is_dir() {
        return Err(format!("{label} `{}` is not a directory", path.display()));
    }
    fs::canonicalize(&path)
        .map_err(|error| format!("{label} `{}` cannot be canonicalized: {error}", path.display()))
}

fn contained(root: &Path, child: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let root = root.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
        let child = child.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
        return child == root
            || child
                .strip_prefix(&root)
                .is_some_and(|rest| rest.starts_with('\\'));
    }
    #[cfg(not(target_os = "windows"))]
    {
        child == root || child.starts_with(root)
    }
}

fn resolve_program(program: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(program);
    if candidate.is_absolute() || program.contains(std::path::MAIN_SEPARATOR) {
        return fs::canonicalize(candidate)
            .map_err(|error| format!("command `{program}` is unavailable: {error}"));
    }
    let path = env::var_os("PATH").ok_or_else(|| "PATH is unavailable".to_string())?;
    for directory in env::split_paths(&path) {
        let direct = directory.join(program);
        if direct.is_file() {
            return fs::canonicalize(&direct)
                .map_err(|error| format!("command `{program}` cannot be canonicalized: {error}"));
        }
        #[cfg(target_os = "windows")]
        for suffix in [".exe", ".cmd", ".bat", ".com"] {
            let with_suffix = directory.join(format!("{program}{suffix}"));
            if with_suffix.is_file() {
                return fs::canonicalize(&with_suffix).map_err(|error| {
                    format!("command `{program}` cannot be canonicalized: {error}")
                });
            }
        }
    }
    Err(format!("command `{program}` is unavailable on PATH"))
}

fn capability_check() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        return linux_capability_check();
    }
    #[cfg(target_os = "macos")]
    {
        return macos_capability_check();
    }
    #[cfg(target_os = "windows")]
    {
        return windows_capability_check();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(format!(
            "unsupported host `{}`: native peer isolation is implemented only on Linux, macOS, and Windows",
            env::consts::OS
        ))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn probe_root() -> Result<PathBuf, String> {
    let base = env::var_os("JET_COMPILED_WORKLOAD_PEER_PROBE_ROOT")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache/jet-test-scratch"))
        })
        .ok_or_else(|| "cannot locate a disk-backed capability probe directory".to_string())?;
    fs::create_dir_all(&base).map_err(|error| {
        format!(
            "capability probe root `{}` cannot be created: {error}",
            base.display()
        )
    })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("capability probe clock failed: {error}"))?
        .as_nanos();
    let root = base.join(format!(
        "compiled-workload-peer-probe-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&root).map_err(|error| {
        format!(
            "capability probe root `{}` cannot be created: {error}",
            root.display()
        )
    })?;
    Ok(root)
}
#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy)]
struct SockFilter {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct SockFprog {
    len: u16,
    filter: *const SockFilter,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const SOCKET_SYSCALL: u32 = 41;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const SOCKET_SYSCALL: u32 = 198;

#[cfg(target_os = "linux")]
fn install_disabled_network_filter() -> Result<(), String> {
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    return Err(format!(
        "Linux peer isolation unavailable: seccomp socket policy has no syscall table for `{}`",
        env::consts::ARCH
    ));
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        const BPF_LD_W_ABS: u16 = 0x20;
        const BPF_JMP_JEQ_K: u16 = 0x15;
        const BPF_RET_K: u16 = 0x06;
        const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
        const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
        const EPERM: u32 = 1;
        let filter = [
            SockFilter { code: BPF_LD_W_ABS, jt: 0, jf: 0, k: 0 },
            SockFilter { code: BPF_JMP_JEQ_K, jt: 0, jf: 1, k: SOCKET_SYSCALL },
            SockFilter { code: BPF_RET_K, jt: 0, jf: 0, k: SECCOMP_RET_ERRNO | EPERM },
            SockFilter { code: BPF_RET_K, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW },
        ];
        let program = SockFprog { len: filter.len() as u16, filter: filter.as_ptr() };
        const PR_SET_NO_NEW_PRIVS: i32 = 38;
        const PR_SET_SECCOMP: i32 = 22;
        const SECCOMP_MODE_FILTER: i32 = 2;
        if unsafe { prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err(format!(
                "Linux peer isolation unavailable: seccomp no_new_privs failed: {}",
                io::Error::last_os_error()
            ));
        }
        if unsafe {
            prctl(
                PR_SET_SECCOMP,
                SECCOMP_MODE_FILTER,
                &program as *const SockFprog,
            )
        } != 0
        {
            return Err(format!(
                "Linux peer isolation unavailable: seccomp socket filter failed: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}


#[cfg(target_os = "linux")]
fn linux_capability_check() -> Result<(), String> {
    let bwrap = find_on_path("bwrap").ok_or_else(|| {
        "Linux peer isolation unavailable: bubblewrap (`bwrap`) is not available on PATH".to_string()
    })?;
    let shell = find_on_path("sh").ok_or_else(|| {
        "Linux peer isolation unavailable: POSIX `sh` is not available on PATH".to_string()
    })?;
    let ip = find_on_path("ip").ok_or_else(|| {
        "Linux peer isolation unavailable: `ip` is required to enable loopback-only networking".to_string()
    })?;
    let launcher = env::current_exe()
        .and_then(|path| fs::canonicalize(path))
        .map_err(|error| {
            format!("Linux peer isolation unavailable: launcher path cannot be resolved: {error}")
        })?;
    let root = probe_root()?;
    for policy in [NetworkPolicy::Disabled, NetworkPolicy::LoopbackOnly] {
        let mut args = linux_base_args(&root, &root, policy);
        args.push("--".into());
        if policy == NetworkPolicy::LoopbackOnly {
            args.extend([
                launcher.to_string_lossy().into_owned(),
                "--internal-loopback".into(),
                ip.to_string_lossy().into_owned(),
                shell.to_string_lossy().into_owned(),
                "-c".into(),
                "exit 0".into(),
            ]);
        } else {
            args.push(launcher.to_string_lossy().into_owned());
            args.push("--internal-disabled".into());
            args.push(shell.to_string_lossy().into_owned());
            args.extend(["-c".into(), "exit 0".into()]);
        }
        let result = Command::new(&bwrap)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| {
                format!(
                    "Linux peer isolation unavailable: bubblewrap {policy:?} probe failed: {error}"
                )
            })?;
        if !result.status.success() {
            let _ = fs::remove_dir_all(&root);
            return Err(format!(
                "Linux peer isolation unavailable: bubblewrap {policy:?} probe exited {}{}",
                result.status,
                clean_probe_stderr(&result.stderr)
            ));
        }
    }
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[cfg(target_os = "linux")]
fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn clean_probe_stderr(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).replace('\r', " ").replace('\n', " ").replace('\t', " ");
    let text = text.trim();
    if text.is_empty() {
        String::new()
    } else {
        format!(": {text}")
    }
}

#[cfg(target_os = "linux")]
fn linux_base_args(root: &Path, cwd: &Path, network: NetworkPolicy) -> Vec<String> {
    let mut args = vec![
        "--die-with-parent".into(),
        "--unshare-all".into(),
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
        "--remount-ro".into(),
        "/".into(),
        "--bind".into(),
        root.to_string_lossy().into_owned(),
        root.to_string_lossy().into_owned(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        "--chdir".into(),
        cwd.to_string_lossy().into_owned(),
    ];
    if network == NetworkPolicy::LoopbackOnly {
        // CAP_NET_ADMIN exists only inside bwrap's fresh user/network
        // namespaces and is used solely to bring up the private loopback
        // interface. Disabled networking receives no extra capability.
        args.extend([
            "--uid".into(),
            "0".into(),
            "--gid".into(),
            "0".into(),
            "--cap-add".into(),
            "CAP_NET_ADMIN".into(),
        ]);
    }
    args
}

#[cfg(target_os = "linux")]
fn run_linux(invocation: Invocation) -> Result<ExitStatus, String> {
    let bwrap = find_on_path("bwrap")
        .ok_or_else(|| "Linux peer isolation unavailable: bubblewrap disappeared from PATH".to_string())?;
    let ip = find_on_path("ip")
        .ok_or_else(|| "Linux peer isolation unavailable: `ip` disappeared from PATH".to_string())?;
    let launcher = env::current_exe()
        .and_then(|path| fs::canonicalize(path))
        .map_err(|error| {
            format!("Linux peer isolation unavailable: launcher path cannot be resolved: {error}")
        })?;
    let command = resolve_program(&invocation.command)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("Linux peer isolation clock failed: {error}"))?
        .as_nanos();
    let status_file = invocation.root.join(format!(
        ".compiled-workload-peer-status-{}-{stamp}",
        std::process::id()
    ));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&status_file)
        .map_err(|error| {
            format!(
                "Linux peer isolation signal status cannot be created at `{}`: {error}",
                status_file.display()
            )
        })?;
    let mut child_command = Command::new(&bwrap);
    child_command.args(linux_base_args(&invocation.root, &invocation.cwd, invocation.network));
    child_command.arg("--");
    child_command.arg(launcher);
    if invocation.network == NetworkPolicy::LoopbackOnly {
        child_command
            .arg("--internal-loopback")
            .arg("--status-file")
            .arg(&status_file)
            .arg(&ip)
            .arg(&command)
            .args(&invocation.args);
    } else {
        child_command
            .arg("--internal-disabled")
            .arg("--status-file")
            .arg(&status_file)
            .arg(&command)
            .args(&invocation.args);
    }
    let result = run_unix_child(child_command);
    let reported = read_linux_signal(&status_file);
    let cleanup = fs::remove_file(&status_file);
    if let Err(reason) = cleanup {
        return Err(format!(
            "Linux peer isolation signal status cannot be removed from `{}`: {reason}",
            status_file.display()
        ));
    }
    match reported {
        Ok(Some(signal)) => {
            unsafe {
                signal_hook(signal, None);
                raise_signal(signal);
            }
            Err(format!("Linux peer isolation signal {signal} could not propagate"))
        }
        Ok(None) => result,
        Err(reason) => Err(reason),
    }
}

#[cfg(target_os = "linux")]
fn read_linux_signal(path: &Path) -> Result<Option<i32>, String> {
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "Linux peer isolation signal status cannot be read from `{}`: {error}",
            path.display()
        )
    })?;
    match bytes.as_slice() {
        [] => Ok(None),
        [signal] if (1..=64).contains(signal) => Ok(Some(*signal as i32)),
        _ => Err("Linux peer isolation signal status returned invalid data".into()),
    }
}

#[cfg(target_os = "macos")]
fn macos_capability_check() -> Result<(), String> {
    let sandbox = Path::new("/usr/bin/sandbox-exec");
    if !sandbox.is_file() {
        return Err(format!(
            "macOS peer isolation unavailable: native Seatbelt `{}` is unavailable",
            sandbox.display()
        ));
    }
    let root = probe_root()?;
    for policy in [NetworkPolicy::Disabled, NetworkPolicy::LoopbackOnly] {
        let profile = macos_profile(&root, policy);
        let result = Command::new(sandbox)
            .args(["-p", profile.as_str(), "/usr/bin/true"])
            .current_dir(&root)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| format!("macOS peer isolation unavailable: Seatbelt probe failed: {error}"))?;
        if !result.status.success() {
            let _ = fs::remove_dir_all(&root);
            return Err(format!(
                "macOS peer isolation unavailable: Seatbelt probe for {} exited {}{}",
                network_name(policy),
                result.status,
                clean_probe_stderr(&result.stderr)
            ));
        }
    }
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_profile(root: &Path, network: NetworkPolicy) -> String {
    let root = sbpl_quote(root);
    let network = match network {
        NetworkPolicy::Disabled => "(deny network*)".to_string(),
        NetworkPolicy::LoopbackOnly => concat!(
            "(allow network-outbound (remote tcp \"localhost:*\"))",
            " (allow network-outbound (remote udp \"localhost:*\"))",
            " (allow network-inbound (local tcp \"localhost:*\"))",
            " (allow network-inbound (local udp \"localhost:*\"))"
        )
        .to_string(),
    };
    format!(
        "(version 1)\n(deny default)\n(import \"system.sb\")\n(allow process-exec)\n(allow process-fork)\n(allow process-signal)\n(allow file-read*)\n(allow file-write* (subpath \"{root}\"))\n{network}\n"
    )
}

#[cfg(target_os = "macos")]
fn sbpl_quote(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}


#[cfg(target_os = "windows")]
mod windows_native {
    include!("../../crates/jet-codegen/src/Prelude/CoreLib/Top/ProcessSandbox.rs");
    include!("../../crates/jet-codegen/src/Prelude/CoreLib/Top/ProcessWindowsSandbox.rs");
}

#[cfg(target_os = "windows")]
fn windows_capability_check() -> Result<(), String> {
    let status = windows_native::windows_status();
    if !status.available {
        return Err(format!(
            "Windows peer isolation unavailable: {}",
            status.reason
        ));
    }
    let netsh = resolve_program("netsh")?;
    let result = Command::new(netsh)
        .args(["advfirewall", "show", "allprofiles"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("Windows peer isolation unavailable: Firewall probe failed: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "Windows peer isolation unavailable: Firewall probe exited {}{}",
            result.status,
            clean_probe_stderr(&result.stderr)
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
struct FirewallGuard {
    netsh: PathBuf,
    names: Vec<String>,
}

#[cfg(target_os = "windows")]
impl FirewallGuard {
    fn install(executable: &Path, network: NetworkPolicy) -> Result<Option<Self>, String> {
        if network == NetworkPolicy::Disabled {
            return Ok(None);
        }
        let netsh = resolve_program("netsh")?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("Firewall rule clock failed: {error}"))?
            .as_nanos();
        let prefix = format!("JetCompiledPeer-{stamp}-{}", std::process::id());
        let mut guard = Self {
            netsh,
            names: Vec::new(),
        };
        let program = format!("program={}", executable.display());
        let v4 = "0.0.0.0-126.255.255.255,128.0.0.0-255.255.255.255";
        let v6 = "::0-::0,::2-ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff";
        for (direction, remote) in [("out", v4), ("in", v4), ("out", v6), ("in", v6)] {
            let name = format!("{prefix}-{direction}-{remote}");
            guard.add_rule(&name, direction, &program, Some(remote))?;
        }
        Ok(Some(guard))
    }

    fn add_rule(
        &mut self,
        name: &str,
        direction: &str,
        program: &str,
        remote: Option<&str>,
    ) -> Result<(), String> {
        let mut args = vec![
            "advfirewall".to_string(),
            "firewall".to_string(),
            "add".to_string(),
            "rule".to_string(),
            format!("name={name}"),
            format!("dir={direction}"),
            "action=block".to_string(),
            program.to_string(),
            "enable=yes".to_string(),
            "profile=any".to_string(),
            "protocol=any".to_string(),
        ];
        if let Some(remote) = remote {
            args.push(format!("remoteip={remote}"));
        }
        let result = Command::new(&self.netsh)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .map_err(|error| format!("Firewall rule `{name}` could not be installed: {error}"))?;
        if !result.success() {
            return Err(format!("Firewall rule `{name}` could not be installed: {result}"));
        }
        self.names.push(name.to_string());
        Ok(())
    }

    fn cleanup(&mut self) -> Result<(), String> {
        let mut failed = Vec::new();
        for name in self.names.iter().rev() {
            let result = Command::new(&self.netsh)
                .args(["advfirewall", "firewall", "delete", "rule", &format!("name={name}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if result.as_ref().map_or(true, |status| !status.success()) {
                failed.push(name.clone());
            }
        }
        self.names = failed;
        if let Some(name) = self.names.first() {
            Err(format!("Firewall rule `{name}` could not be removed"))
        } else {
            Ok(())
        }
    }
}
#[cfg(target_os = "windows")]
impl Drop for FirewallGuard {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn run(invocation: Invocation) -> Result<ExitStatus, String> {
    let task_id = invocation.task_id.clone();
    let result = {
        #[cfg(target_os = "linux")]
        {
            run_linux(invocation)
        }
        #[cfg(target_os = "macos")]
        {
            run_macos(invocation)
        }
        #[cfg(target_os = "windows")]
        {
            run_windows(invocation)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = invocation;
            Err("native peer isolation is unavailable on this host".into())
        }
    };
    result.map_err(|reason| format!("task `{task_id}`: {reason}"))
}

#[cfg(target_os = "macos")]
fn run_macos(invocation: Invocation) -> Result<ExitStatus, String> {
    let command = resolve_program(&invocation.command)?;
    let profile = macos_profile(&invocation.root, invocation.network);
    let mut child_command = Command::new("/usr/bin/sandbox-exec");
    child_command
        .args(["-p", profile.as_str()])
        .arg(&command)
        .args(&invocation.args)
        .current_dir(&invocation.cwd);
    run_unix_child(child_command)
}

#[cfg(target_os = "windows")]
fn run_windows(invocation: Invocation) -> Result<ExitStatus, String> {
    use std::collections::BTreeMap;
    let command = resolve_program(&invocation.command)?;
    let mut firewall = FirewallGuard::install(&command, invocation.network)?;
    let environment: BTreeMap<String, String> = env::vars().collect();
    let result = windows_native::windows_output_with_read_only_mounts(
        &command,
        &invocation.args,
        &invocation.root,
        None,
        &environment,
        invocation.network == NetworkPolicy::LoopbackOnly,
        true,
        true,
        &[],
        None,
        None,
    )
    .map_err(|error| format!("Windows peer isolation launch failed: {error:?}"));
    let cleanup = firewall.as_mut().map(|guard| guard.cleanup()).transpose();
    if let Err(error) = cleanup {
        return Err(error);
    }
    let result = result?;
    io::stdout()
        .write_all(&result.output.stdout)
        .map_err(|error| format!("stdout propagation failed: {error}"))?;
    io::stderr()
        .write_all(&result.output.stderr)
        .map_err(|error| format!("stderr propagation failed: {error}"))?;
    Ok(result.output.status)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
static FORWARDED_SIGNAL: AtomicI32 = AtomicI32::new(0);

#[cfg(any(target_os = "linux", target_os = "macos"))]
extern "C" fn forward_signal(signal: i32) {
    FORWARDED_SIGNAL.store(signal, Ordering::Release);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn install_signal_handlers() {
    unsafe {
        signal_hook(2, Some(forward_signal));
        signal_hook(15, Some(forward_signal));
        signal_hook(1, Some(forward_signal));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_unix_child(command: Command) -> Result<ExitStatus, String> {
    run_unix_child_inner(command, false)
}

#[cfg(target_os = "linux")]
fn run_linux_inner_child(command: Command) -> Result<ExitStatus, String> {
    run_unix_child_inner(command, true)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_unix_child_inner(mut command: Command, cleanup_namespace: bool) -> Result<ExitStatus, String> {
    use std::os::unix::process::CommandExt;
    install_signal_handlers();
    command.process_group(0);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child = command
        .spawn()
        .map_err(|error| format!("native peer isolation child could not start: {error}"))?;
    let process_group = child.id() as i32;
    loop {
        if let Some(signal) = take_signal() {
            kill_group(process_group, signal);
            let _ = cleanup_namespace_processes(signal, cleanup_namespace);
            let _ = child.wait();
            kill_group(process_group, 9);
            let _ = cleanup_namespace_processes(9, cleanup_namespace);
            unsafe {
                signal_hook(signal, None);
                raise_signal(signal);
            }
            return Err(format!("launcher interrupted by signal {signal}"));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // A leader may leave a background descendant. The private
                // process group and, on Linux, PID namespace are closed before returning.
                kill_group(process_group, 15);
                let cleanup_error = cleanup_namespace_processes(15, cleanup_namespace).err();
                std::thread::sleep(Duration::from_millis(25));
                kill_group(process_group, 9);
                let cleanup_error = cleanup_namespace_processes(9, cleanup_namespace)
                    .err()
                    .or(cleanup_error);
                if let Some(reason) = cleanup_error {
                    return Err(reason);
                }
                return Ok(status);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                kill_group(process_group, 15);
                let _ = cleanup_namespace_processes(15, cleanup_namespace);
                let _ = child.wait();
                kill_group(process_group, 9);
                let _ = cleanup_namespace_processes(9, cleanup_namespace);
                return Err(format!("native peer isolation wait failed: {error}"));
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn take_signal() -> Option<i32> {
    let signal = FORWARDED_SIGNAL.swap(0, Ordering::AcqRel);
    (signal != 0).then_some(signal)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn cleanup_namespace_processes(signal: i32, enabled: bool) -> Result<(), String> {
    if !enabled {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let own = std::process::id() as i32;
        let entries = fs::read_dir("/proc")
            .map_err(|error| format!("Linux peer process tree cannot be enumerated: {error}"))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| format!("Linux peer process tree cannot be enumerated: {error}"))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Ok(pid) = name.parse::<i32>() else {
                continue;
            };
            if pid <= 1 || pid == own {
                continue;
            }
            let result = unsafe { kill(pid, signal) };
            if result == -1 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(3) {
                    return Err(format!("Linux peer process tree signal failed for pid {pid}: {error}"));
                }
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = signal;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn kill_group(process_group: i32, signal: i32) {
    unsafe {
        let _ = kill_process_group(-process_group, signal);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn finish_status(status: ExitStatus) -> ! {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        std::process::exit(code);
    }
    if let Some(signal) = status.signal() {
        unsafe {
            // SIG_DFL is represented by a null handler pointer in POSIX.
            signal_hook(signal, None);
            raise_signal(signal);
        }
        std::process::exit(128 + signal);
    }
    std::process::exit(EXIT_UNSUPPORTED);
}

#[cfg(target_os = "windows")]
fn finish_status(status: ExitStatus) -> ! {
    std::process::exit(status.code().unwrap_or(EXIT_UNSUPPORTED));
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn finish_status(_: ExitStatus) -> ! {
    std::process::exit(EXIT_UNSUPPORTED);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
    fn raise(signal: i32) -> i32;
    fn signal(
        signal: i32,
        handler: Option<extern "C" fn(i32)>,
    ) -> Option<extern "C" fn(i32)>;
    #[cfg(target_os = "linux")]
    fn prctl(option: i32, ...) -> i32;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe fn kill_process_group(pid: i32, signal: i32) -> i32 {
    kill(pid, signal)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe fn signal_hook(signal_number: i32, handler: Option<extern "C" fn(i32)>) {
    signal(signal_number, handler);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe fn raise_signal(signal_number: i32) {
    raise(signal_number);
}


#[cfg(target_os = "macos")]
fn network_name(policy: NetworkPolicy) -> &'static str {
    match policy {
        NetworkPolicy::Disabled => "disabled",
        NetworkPolicy::LoopbackOnly => "loopback-only",
    }
}
