// Shared native child boundary used by #398 and authority-bound Core process.
//
// This is a Prelude fragment: generated AOT code, the resident JIT, and the
// interpreter compile the same backend source. Build execution includes the
// same fragment instead of maintaining a second platform implementation.

use std::collections::BTreeMap;
use std::fs;
#[cfg(target_os = "macos")]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "macos")]
const MACOS_SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

#[cfg(target_os = "macos")]
static PROFILE_ATTEMPT: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub available: bool,
    pub mechanism: String,
    pub policy: String,
    pub reason: String,
}

#[derive(Debug)]
pub enum Error {
    Unsupported(String),
    Io(String),
}

pub fn policy(output_is_separate: bool, share_network: bool) -> String {
    #[cfg(target_os = "linux")]
    {
        return format!(
            "filesystem={};process=private-pid,parent-death;network={};environment=clear;devices=private-dev;privilege=no-new-privs+cap-drop-all;resources=tmpfs-64MiB",
            if output_is_separate {
                "source-readonly,output-private-copy"
            } else {
                "private-workspace-readwrite"
            },
            if share_network {
                "declared-shared"
            } else {
                "isolated"
            },
        );
    }

    #[cfg(not(target_os = "linux"))]
    format!(
        "filesystem={};process=declared-tool-and-fork;network={};environment=clear;devices=denied;resources=none-declared",
        if output_is_separate {
            "source-readonly,output-readwrite"
        } else {
            "private-workspace-readwrite"
        },
        if share_network {
            "declared-shared"
        } else {
            "denied"
        },
    )
}

pub fn status() -> Status {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("JETPACK_FAKE_SANDBOX").is_some() {
            return Status {
                available: false,
                mechanism: "linux-bwrap-unavailable".to_string(),
                policy: "not-enforced".to_string(),
                reason: "test sandbox override prevents the native Linux backend probe".to_string(),
            };
        }
        let Some(bwrap) = find_program_path("bwrap") else {
            return Status {
                available: false,
                mechanism: "linux-bwrap-unavailable".to_string(),
                policy: "not-enforced".to_string(),
                reason: "bubblewrap (`bwrap`) is not available on PATH".to_string(),
            };
        };
        let Some(probe) = find_program_path("sh") else {
            return Status {
                available: false,
                mechanism: "linux-bwrap-unavailable".to_string(),
                policy: "not-enforced".to_string(),
                reason: "bubblewrap is present but no immutable shell probe is available"
                    .to_string(),
            };
        };
        let policy = policy(false, false);
        let mut command =
            linux_sandbox_command(&bwrap, None, None, &BTreeMap::new(), false, false, false);
        command.args([probe.as_os_str(), std::ffi::OsStr::new("-c")]);
        command.arg("exit 0");
        return match command.status() {
            Ok(status) if status.success() => Status {
                available: true,
                mechanism: "linux-bwrap".to_string(),
                policy,
                reason: "bubblewrap completed the isolated backend probe".to_string(),
            },
            Ok(status) => Status {
                available: false,
                mechanism: "linux-bwrap-unavailable".to_string(),
                policy: "not-enforced".to_string(),
                reason: format!("bubblewrap backend probe exited with {status}"),
            },
            Err(error) => Status {
                available: false,
                mechanism: "linux-bwrap-unavailable".to_string(),
                policy: "not-enforced".to_string(),
                reason: format!("bubblewrap backend probe failed: {error}"),
            },
        };
    }

    #[cfg(target_os = "macos")]
    {
        if std::env::var_os("JETPACK_FAKE_SANDBOX").is_some() {
            return Status {
                available: false,
                mechanism: "macos-seatbelt-unavailable".to_string(),
                policy: "not-enforced".to_string(),
                reason: "test sandbox override prevents the native macOS backend probe".to_string(),
            };
        }
        if !Path::new(MACOS_SANDBOX_EXEC).is_file() {
            return Status {
                available: false,
                mechanism: "macos-seatbelt-unavailable".to_string(),
                policy: "not-enforced".to_string(),
                reason: format!("native macOS backend `{MACOS_SANDBOX_EXEC}` is unavailable"),
            };
        }
        let profile = match write_profile(probe_profile()) {
            Ok(path) => path,
            Err(error) => {
                return Status {
                    available: false,
                    mechanism: "macos-seatbelt-unavailable".to_string(),
                    policy: "not-enforced".to_string(),
                    reason: format!(
                        "native macOS backend profile could not be prepared: {error:?}"
                    ),
                };
            }
        };
        let result = Command::new(MACOS_SANDBOX_EXEC)
            .arg("-f")
            .arg(&profile)
            .arg("/usr/bin/true")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = fs::remove_file(profile);
        return match result {
            Ok(status) if status.success() => Status {
                available: true,
                mechanism: "macos-seatbelt".to_string(),
                policy: policy(true, false),
                reason: "native macOS Seatbelt completed the isolated backend probe".to_string(),
            },
            Ok(status) => Status {
                available: false,
                mechanism: "macos-seatbelt-unavailable".to_string(),
                policy: "not-enforced".to_string(),
                reason: format!("native macOS Seatbelt backend probe exited with {status}"),
            },
            Err(error) => Status {
                available: false,
                mechanism: "macos-seatbelt-unavailable".to_string(),
                policy: "not-enforced".to_string(),
                reason: format!("native macOS Seatbelt backend probe failed: {error}"),
            },
        };
    }

    #[cfg(target_os = "windows")]
    {
        let native = windows_status();
        return Status {
            available: native.available,
            mechanism: native.mechanism,
            policy: native.policy,
            reason: native.reason,
        };
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Status {
            available: false,
            mechanism: "unsupported".to_string(),
            policy: "not-enforced".to_string(),
            reason: "native child sandbox backend is unavailable on this target".to_string(),
        }
    }
}

pub fn agent_output_dir(cwd: &Path) -> Result<PathBuf, Error> {
    let cwd = real_directory(cwd)?;
    let jet_dir = secure_child_directory(&cwd, ".jet")?;
    let output = secure_child_directory(&jet_dir, "build")?;
    if !output.starts_with(&cwd) {
        return Err(Error::Unsupported(
            "authority-bound process output escaped its workspace".to_string(),
        ));
    }
    Ok(output)
}

pub fn spawn<F>(
    executable: &Path,
    args: &[String],
    source_dir: &Path,
    output_dir: Option<&Path>,
    env: &BTreeMap<String, String>,
    share_network: bool,
    source_readable: bool,
    source_writable: bool,
    argv0: Option<&std::ffi::OsStr>,
    configure: F,
) -> Result<Child, Error>
where
    F: FnOnce(&mut Command) -> Result<(), Error>,
{
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if std::env::var_os("JETPACK_FAKE_SANDBOX").is_some() {
            return Err(Error::Unsupported(
                "test sandbox override prevents child execution".to_string(),
            ));
        }
        let status = status();
        if !status.available {
            return Err(Error::Unsupported(status.reason));
        }
        let executable = executable.canonicalize().map_err(|error| {
            Error::Unsupported(format!("sandbox executable is unavailable: {error}"))
        })?;
        if !executable.is_file() {
            return Err(Error::Unsupported(format!(
                "sandbox executable `{}` is not a file",
                executable.display()
            )));
        }
        let source_dir = real_directory(source_dir)?;
        let output_dir = output_dir.map(real_directory).transpose()?;
        #[cfg(target_os = "macos")]
        {
            let profile = build_profile(
                &executable,
                &source_dir,
                output_dir.as_deref(),
                share_network,
                source_readable,
                source_writable,
            )?;
            let mut command = Command::new(MACOS_SANDBOX_EXEC);
            command
                .arg("-f")
                .arg(&profile)
                .arg(&executable)
                .args(args)
                .env_clear()
                .current_dir(&source_dir);
            if let Some(output_dir) = output_dir.as_deref() {
                command.env("JET_BUILD_OUTPUT", output_dir);
            }
            let _ = argv0;
            for (key, value) in env {
                command.env(key, value);
            }
            if let Err(error) = configure(&mut command) {
                let _ = fs::remove_file(profile);
                return Err(error);
            }
            let result = command.spawn();
            let _ = fs::remove_file(profile);
            return result.map_err(|error| Error::Io(error.to_string()));
        }

        #[cfg(target_os = "linux")]
        {
            let bwrap = find_program_path("bwrap").ok_or_else(|| {
                Error::Unsupported("bubblewrap (`bwrap`) is not available on PATH".to_string())
            })?;
            let mut command = linux_sandbox_command(
                &bwrap,
                Some(&source_dir),
                output_dir.as_deref(),
                env,
                share_network,
                source_readable,
                source_writable,
            );
            if let Some(argv0) = argv0 {
                command.arg("--argv0").arg(argv0);
            }
            command.arg(&executable).args(args);
            configure(&mut command)?;
            return command
                .spawn()
                .map_err(|error| Error::Io(error.to_string()));
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (
            executable,
            args,
            source_dir,
            output_dir,
            env,
            share_network,
            source_readable,
            source_writable,
            argv0,
            configure,
        );
        Err(Error::Unsupported(
            "native Windows AppContainer backend does not expose streaming spawn; use captured run()"
                .to_string(),
        ))
    }
}

pub fn output(
    executable: &Path,
    args: &[String],
    source_dir: &Path,
    output_dir: Option<&Path>,
    env: &BTreeMap<String, String>,
    share_network: bool,
) -> Result<Output, Error> {
    let argv0 = executable.file_name().map(std::ffi::OsStr::to_owned);
    let child = spawn(
        executable,
        args,
        source_dir,
        output_dir,
        env,
        share_network,
        true,
        output_dir.is_none(),
        argv0.as_deref(),
        |command| {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            Ok(())
        },
    )?;
    child
        .wait_with_output()
        .map_err(|error| Error::Io(error.to_string()))
}

fn real_directory(path: &Path) -> Result<PathBuf, Error> {
    let canonical = path
        .canonicalize()
        .map_err(|error| Error::Io(format!("sandbox directory `{}`: {error}", path.display())))?;
    if !canonical.is_dir() {
        return Err(Error::Io(format!(
            "sandbox path `{}` is not a directory",
            path.display()
        )));
    }
    Ok(canonical)
}

fn find_program_path(program: &str) -> Option<PathBuf> {
    let direct = Path::new(program);
    if direct.components().count() > 1 && direct.is_file() {
        return fs::canonicalize(direct).ok();
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|directory| {
            let candidate = directory.join(program);
            candidate
                .is_file()
                .then(|| fs::canonicalize(candidate).ok())
                .flatten()
        })
    })
}

#[cfg(target_os = "linux")]
fn linux_sandbox_command(
    bwrap: &Path,
    source_dir: Option<&Path>,
    output_dir: Option<&Path>,
    env: &BTreeMap<String, String>,
    share_network: bool,
    source_readable: bool,
    source_writable: bool,
) -> Command {
    let mut command = Command::new(bwrap);
    command
        .arg("--die-with-parent")
        .arg("--new-session")
        .arg("--unshare-all")
        .arg("--unshare-user")
        .arg("--disable-userns")
        .arg("--assert-userns-disabled")
        .arg("--cap-drop")
        .arg("ALL")
        .arg("--ro-bind-try")
        .arg("/nix/store")
        .arg("/nix/store")
        .arg("--proc")
        .arg("/proc")
        .arg("--dev")
        .arg("/dev")
        .arg("--size")
        .arg("67108864")
        .arg("--tmpfs")
        .arg("/tmp")
        .arg("--dir")
        .arg("/homeless-shelter")
        .arg("--clearenv")
        .arg("--setenv")
        .arg("PATH")
        .arg("/nix/store")
        .arg("--setenv")
        .arg("HOME")
        .arg("/homeless-shelter");
    match (source_dir, output_dir) {
        (Some(source), Some(output)) => {
            command.arg("--dir").arg("/work");
            if source_writable {
                command.arg("--bind").arg(source).arg("/work/source");
            } else if source_readable {
                command.arg("--ro-bind").arg(source).arg("/work/source");
            } else {
                command.arg("--dir").arg("/work/source");
            }
            command
                .arg("--bind")
                .arg(output)
                .arg("/work/output")
                .arg("--chdir")
                .arg("/work/source")
                .arg("--setenv")
                .arg("JET_BUILD_OUTPUT")
                .arg("/work/output");
        }
        (Some(source), None) => {
            if source_writable {
                command.arg("--bind").arg(source).arg("/work");
            } else if source_readable {
                command.arg("--ro-bind").arg(source).arg("/work");
            } else {
                command.arg("--dir").arg("/work");
            }
            command.arg("--chdir").arg("/work");
        }
        (None, None) => {
            command
                .arg("--dir")
                .arg("/work")
                .arg("--chdir")
                .arg("/work");
        }
        (None, Some(output)) => {
            command
                .arg("--bind")
                .arg(output)
                .arg("/work")
                .arg("--chdir")
                .arg("/work");
        }
    }
    if share_network {
        command.arg("--share-net");
    }
    for (key, value) in env {
        command.arg("--setenv").arg(key).arg(value);
    }
    command
}

fn secure_child_directory(parent: &Path, name: &str) -> Result<PathBuf, Error> {
    let path = parent.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(Error::Unsupported(format!(
                "sandbox directory `{}` is a symlink",
                path.display()
            )));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(Error::Unsupported(format!(
                "sandbox directory `{}` is not a directory",
                path.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&path).map_err(|error| Error::Io(error.to_string()))?;
        }
        Err(error) => return Err(Error::Io(error.to_string())),
    }
    real_directory(&path)
}

#[cfg(target_os = "macos")]
fn probe_profile() -> &'static str {
    concat!(
        "(version 1)\n",
        "(deny default)\n",
        "(import \"system.sb\")\n",
        "(allow process-exec (literal \"/usr/bin/true\"))\n",
        "(allow file-read* (subpath \"/System\") (subpath \"/usr\") (subpath \"/bin\") (subpath \"/nix/store\"))\n",
    )
}

#[cfg(target_os = "macos")]
fn build_profile(
    executable: &Path,
    source_dir: &Path,
    output_dir: Option<&Path>,
    share_network: bool,
    source_readable: bool,
    source_writable: bool,
) -> Result<PathBuf, Error> {
    let executable = sbpl_path(executable)?;
    let source_dir = sbpl_path(source_dir)?;
    let output_dir = output_dir.map(sbpl_path).transpose()?;
    let mut profile = String::from(
        "(version 1)\n(deny default)\n(import \"system.sb\")\n(allow process-fork)\n(allow process-exec\n",
    );
    profile.push_str(&format!("  (literal \"{executable}\")\n)\n"));
    profile.push_str("(allow file-read*\n");
    if source_readable {
        profile.push_str(&format!("  (subpath \"{source_dir}\")\n"));
    }
    if let Some(output_dir) = output_dir.as_deref() {
        profile.push_str(&format!("  (subpath \"{output_dir}\")\n"));
    }
    profile.push_str(&format!("  (literal \"{executable}\")\n"));
    profile.push_str("  (subpath \"/System\")\n");
    profile.push_str("  (subpath \"/usr/lib\")\n");
    profile.push_str("  (subpath \"/usr/bin\")\n");
    profile.push_str("  (subpath \"/bin\")\n");
    profile.push_str("  (subpath \"/sbin\")\n");
    profile.push_str("  (subpath \"/nix/store\"))\n");
    if let Some(output_dir) = output_dir.as_deref() {
        profile.push_str(&format!("(allow file-write* (subpath \"{output_dir}\"))\n"));
    } else if source_writable {
        profile.push_str(&format!("(allow file-write* (subpath \"{source_dir}\"))\n"));
    }
    if share_network {
        profile.push_str("(allow network*)\n");
    } else {
        profile.push_str("(deny network*)\n");
    }
    profile.push_str("(deny device*)\n");
    write_profile(&profile)
}

#[cfg(target_os = "macos")]
fn sbpl_path(path: &Path) -> Result<String, Error> {
    let value = path.to_str().ok_or_else(|| {
        Error::Unsupported(format!(
            "sandbox path is not valid UTF-8: {}",
            path.display()
        ))
    })?;
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            character if character.is_control() => {
                return Err(Error::Unsupported(
                    "sandbox path contains a control character".to_string(),
                ));
            }
            character => escaped.push(character),
        }
    }
    Ok(escaped)
}

#[cfg(target_os = "macos")]
fn write_profile(profile: &str) -> Result<PathBuf, Error> {
    let path = std::env::temp_dir().join(format!(
        "jet-native-sandbox-{}-{}.sb",
        std::process::id(),
        PROFILE_ATTEMPT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| Error::Io(format!("could not create sandbox profile: {error}")))?;
    if let Err(error) = file.write_all(profile.as_bytes()) {
        let _ = fs::remove_file(&path);
        return Err(Error::Io(format!(
            "could not write sandbox profile: {error}"
        )));
    }
    Ok(path)
}
