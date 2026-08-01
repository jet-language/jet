//! Driver-side bridge for packaged build-plugin components.
//!
//! The driver owns policy and graph mutation but never links Wasmtime. It
//! sends the verified package identity to the sibling `jetpack` host and
//! validates the returned typed contribution before `BuildContext` commits it.

use jet_comptime::Comptime::Build::{
    decode_build_plugin_response, encode_build_plugin_request, PackagedPluginContribution,
    WasmComponentPluginSpec, BUILD_PLUGIN_HOST_SUBCOMMAND, BUILD_PLUGIN_MAX_REQUEST_BYTES,
    BUILD_PLUGIN_MAX_RESPONSE_BYTES,
};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MAX_HOST_STDERR_BYTES: usize = 64 * 1024;
const HOST_TIMEOUT: Duration = Duration::from_secs(2);

pub fn run_packaged_build_plugin(
    manifest_path: &Path,
    component_path: &Path,
    spec: &WasmComponentPluginSpec,
    manifest_digest: &str,
) -> Result<PackagedPluginContribution, String> {
    let host = host_path()?;
    let mut command = Command::new(host);
    command
        .arg(BUILD_PLUGIN_HOST_SUBCOMMAND)
        .arg(manifest_path)
        .arg(component_path);
    command
        .arg(manifest_digest)
        .arg(&spec.component_digest);
    let request = encode_build_plugin_request(spec);
    let response = run_host_process(command, request)?;
    decode_build_plugin_response(&response)
}

fn host_path() -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("couldn't resolve compiler executable: {error}"))?;
    let Some(directory) = executable.parent() else {
        return Err("compiler executable has no parent directory".to_string());
    };
    let binary = format!(
        "{}{}",
        crate::Syntax::JETPACK_BINARY_NAME,
        std::env::consts::EXE_SUFFIX
    );
    let mut candidates = vec![directory.join(&binary)];
    if directory.file_name().is_some_and(|name| name == "deps") {
        if let Some(parent) = directory.parent() {
            candidates.push(parent.join(binary));
        }
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "packaged build-plugin host jetpack is missing beside Jet".to_string())
}

fn run_host_process(mut command: Command, request: Vec<u8>) -> Result<Vec<u8>, String> {
    if request.len() > BUILD_PLUGIN_MAX_REQUEST_BYTES {
        return Err(format!(
            "build-plugin request exceeds {BUILD_PLUGIN_MAX_REQUEST_BYTES} bytes"
        ));
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("couldn't start build-plugin host: {error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "build-plugin host stdin is unavailable".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "build-plugin host stdout is unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "build-plugin host stderr is unavailable".to_string())?;
    let writer = thread::spawn(move || {
        let result = stdin.write_all(&request);
        drop(stdin);
        result
    });
    let stdout_reader = thread::spawn(move || read_bounded(stdout, BUILD_PLUGIN_MAX_RESPONSE_BYTES));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAX_HOST_STDERR_BYTES));
    let deadline = Instant::now() + HOST_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer.join();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!(
                    "build-plugin host timed out after {}ms",
                    HOST_TIMEOUT.as_millis()
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer.join();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("couldn't wait for build-plugin host: {error}"));
            }
        }
    };
    let write_result = writer
        .join()
        .map_err(|_| "build-plugin host input thread panicked".to_string())?;
    let (stdout, stdout_overflow) = stdout_reader
        .join()
        .map_err(|_| "build-plugin host output thread panicked".to_string())?
        .map_err(|error| format!("couldn't read build-plugin host output: {error}"))?;
    let (stderr, stderr_overflow) = stderr_reader
        .join()
        .map_err(|_| "build-plugin host error thread panicked".to_string())?
        .map_err(|error| format!("couldn't read build-plugin host error: {error}"))?;
    if stdout_overflow {
        return Err(format!(
            "build-plugin host response exceeds {BUILD_PLUGIN_MAX_RESPONSE_BYTES} bytes"
        ));
    }
    if stderr_overflow {
        return Err(format!(
            "build-plugin host error exceeds {MAX_HOST_STDERR_BYTES} bytes"
        ));
    }
    if !status.success() {
        let message = String::from_utf8_lossy(&stderr);
        let message = message.trim();
        return Err(if message.is_empty() {
            format!("build-plugin host exited with {status}")
        } else {
            message.to_string()
        });
    }
    write_result.map_err(|error| format!("couldn't send request to build-plugin host: {error}"))?;
    Ok(stdout)
}

fn read_bounded(reader: impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    let overflow = bytes.len() > limit;
    if overflow {
        bytes.truncate(limit);
    }
    Ok((bytes, overflow))
}
