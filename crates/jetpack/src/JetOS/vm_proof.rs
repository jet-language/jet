use super::desktop_store_vm::find_path_tool;
use super::generation::build_generation;
use super::generation_files::systems_dir;
use super::installer_media::write_installer_media;
use super::load_validate::validate_system_options;
use super::types::{Generation, OsFlags, VM_GUEST_PROOF_MARKER, VM_PROOF_TIMEOUT_MS, VM_TOOLS};
use jet_env_model::ModuleEval::{EnvPlan, SystemPlan, VmTestPlan};
use crate::Output::Theme;
use crate::JSON;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) fn write_vm_install_plan(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
    real_guest: bool,
) -> std::io::Result<PathBuf> {
    let proof_dir = systems_dir().join("vm-proofs");
    fs::create_dir_all(&proof_dir)?;
    let proof = proof_dir.join(format!("{}-{}-vm-proof.json", system.name, gen.name));
    let tools = vm_tools_json();
    let iso = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso", system.name));
    let staging_boot = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso.d/boot", system.name));
    let commands = qemu_proof_commands_json(&staging_boot, disk, &iso, &system.name, &gen.name);
    let guest = guest_proof_path(&proof);
    let proof_tier = if real_guest { "real-guest" } else { "plumbing" };
    let media_proof_sha256 = file_sha256(media_proof).map_err(std::io::Error::other)?;
    let iso_fingerprint = file_fingerprint(&iso).map_err(std::io::Error::other)?;
    let text = format!(
        "{{\"host\":{},\"generation\":{},\"state\":\"harness-ready\",\"proof_tier\":{},\"disk\":{},\"installer_media\":{},\"media_proof\":{},\"media_proof_sha256\":{},\"installer_iso_fingerprint\":{},\"expected_guest_proof\":{},\"tools\":[{}],\"commands\":[{}],\"steps\":[\"build-generation\",\"create-hybrid-iso\",\"boot-installer-qemu\",\"install-to-disk\",\"reboot-installed-disk\",\"verify-guest-proof\",\"boot-graphical-desktop\"],\"guest_assertions\":[{}]}}",
        JSON::quote(&system.name),
        JSON::quote(&gen.name),
        JSON::quote(proof_tier),
        JSON::quote(disk),
        JSON::quote(&format!("jetos-installer-{}.iso", system.name)),
        JSON::quote(&media_proof.display().to_string()),
        JSON::quote(&media_proof_sha256),
        JSON::quote(&iso_fingerprint),
        JSON::quote(&guest.display().to_string()),
        tools,
        commands,
        guest_assertions_json()
    );
    fs::write(&proof, text)?;
    Ok(proof)
}

pub(super) fn prove_vm_guest(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
    harness: &Path,
    real_guest: bool,
) -> Result<Option<PathBuf>, String> {
    if let Some(final_path) = finalize_vm_guest_proof(gen, system, disk, media_proof, harness)? {
        return Ok(Some(final_path));
    }
    if !run_vm_install_harness(gen, system, disk, media_proof, harness, real_guest)? {
        return Ok(None);
    }
    finalize_vm_guest_proof(gen, system, disk, media_proof, harness)
}

fn run_vm_install_harness(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
    harness: &Path,
    real_guest: bool,
) -> Result<bool, String> {
    let iso = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso", system.name));
    let staging_boot = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso.d/boot", system.name));
    let log_dir = vm_run_log_dir(harness);
    if log_dir.exists() {
        fs::remove_dir_all(&log_dir)
            .map_err(|e| format!("clearing `{}` failed: {e}", log_dir.display()))?;
    }
    fs::create_dir_all(&log_dir)
        .map_err(|e| format!("creating `{}` failed: {e}", log_dir.display()))?;
    let mut graphical_output = String::new();
    for command in qemu_proof_commands(&staging_boot, disk, &iso, &system.name, &gen.name) {
        let output = run_vm_command(&command, &log_dir)?;
        if command.phase == "boot-graphical-desktop" {
            graphical_output = output;
        }
    }
    let Some(report) = extract_guest_proof_report(&graphical_output) else {
        return Ok(false);
    };
    write_runner_guest_proof(gen, system, disk, media_proof, harness, &report, real_guest)?;
    Ok(true)
}

fn vm_run_log_dir(harness: &Path) -> PathBuf {
    let stem = harness
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("vm-proof");
    harness.with_file_name(format!("{stem}.run"))
}

fn run_vm_command(command: &VmCommand, log_dir: &Path) -> Result<String, String> {
    let Some(program) = command.argv.first() else {
        return Err(format!("VM phase `{}` has no executable", command.phase));
    };
    let stdout_path = log_dir.join(format!("{}.stdout", command.phase));
    let stderr_path = log_dir.join(format!("{}.stderr", command.phase));
    let stdout = fs::File::create(&stdout_path)
        .map_err(|e| format!("creating `{}` failed: {e}", stdout_path.display()))?;
    let stderr = fs::File::create(&stderr_path)
        .map_err(|e| format!("creating `{}` failed: {e}", stderr_path.display()))?;
    let mut child = Command::new(program)
        .args(command.argv.iter().skip(1))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| format!("starting VM phase `{}` failed: {e}", command.phase))?;
    let start = Instant::now();
    let timeout = vm_proof_timeout();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("waiting for VM phase `{}` failed: {e}", command.phase))?
        {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
            let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
            return Err(format!(
                "VM phase `{}` timed out after {}ms; stdout `{}`, stderr `{}`{}{}",
                command.phase,
                timeout.as_millis(),
                stdout_path.display(),
                stderr_path.display(),
                log_excerpt("stdout", &stdout),
                log_excerpt("stderr", &stderr)
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
    if !status.success() {
        return Err(format!(
            "VM phase `{}` exited with {}; stdout `{}`, stderr `{}`{}{}",
            command.phase,
            status,
            stdout_path.display(),
            stderr_path.display(),
            log_excerpt("stdout", &stdout),
            log_excerpt("stderr", &stderr)
        ));
    }
    Ok(format!("{stdout}\n{stderr}"))
}

pub(super) fn run_interactive_vm_command(command: &VmCommand) -> Result<i32, String> {
    let Some(program) = command.argv.first() else {
        return Err(format!("VM phase `{}` has no executable", command.phase));
    };
    let status = Command::new(program)
        .args(command.argv.iter().skip(1))
        .status()
        .map_err(|e| format!("starting VM phase `{}` failed: {e}", command.phase))?;
    Ok(status
        .code()
        .unwrap_or(if status.success() { 0 } else { 1 }))
}

fn log_excerpt(label: &str, text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if line.is_empty() {
        String::new()
    } else {
        let excerpt = line.chars().take(240).collect::<String>();
        format!("; {label}: {excerpt}")
    }
}

fn vm_proof_timeout() -> Duration {
    std::env::var("JETOS_VM_PROOF_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(VM_PROOF_TIMEOUT_MS))
}

pub(super) fn extract_guest_proof_report(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.split_once(VM_GUEST_PROOF_MARKER)
            .map(|(_, rest)| rest.trim().to_string())
    })
}

fn write_runner_guest_proof(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
    harness: &Path,
    report: &str,
    real_guest: bool,
) -> Result<(), String> {
    require_guest_report(report, system, gen)?;
    let guest = guest_proof_path(harness);
    let proof_tier = if real_guest { "real-guest" } else { "plumbing" };
    let installer_iso = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso", system.name));
    let text = format!(
        "{{\"state\":\"guest-passed\",\"proof_tier\":{},\"host\":{},\"generation\":{},\"disk\":{},\"media_proof\":{},\"media_proof_sha256\":{},\"installer_iso_fingerprint\":{},\"assertions\":[{}],\"tools\":[{}],\"serial_report\":{}}}\n",
        JSON::quote(proof_tier),
        JSON::quote(&system.name),
        JSON::quote(&gen.name),
        JSON::quote(disk),
        JSON::quote(&media_proof.display().to_string()),
        JSON::quote(&file_sha256(media_proof)?),
        JSON::quote(&file_fingerprint(&installer_iso)?),
        guest_assertions_json(),
        vm_tools_json(),
        JSON::quote(report)
    );
    fs::write(&guest, text).map_err(|e| format!("writing `{}` failed: {e}", guest.display()))
}

fn require_guest_report(report: &str, system: &SystemPlan, gen: &Generation) -> Result<(), String> {
    if !report.contains("\"state\":\"guest-passed\"") {
        return Err("guest serial proof did not report state=guest-passed".to_string());
    }
    require_json_field(report, "host", &system.name)?;
    require_json_field(report, "generation", &gen.name)?;
    require_guest_assertions(report)
}

fn finalize_vm_guest_proof(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
    harness: &Path,
) -> Result<Option<PathBuf>, String> {
    let guest = guest_proof_path(harness);
    if !guest.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&guest)
        .map_err(|e| format!("reading `{}` failed: {e}", guest.display()))?;
    if let Err(e) = validate_cached_guest_proof(&text, system, gen, disk, media_proof) {
        // Drop the unusable cached proof so a rerun records a fresh one, but
        // surface why it was rejected — silent invalidation hides real drift.
        let _ = fs::remove_file(&guest);
        return Err(e);
    }
    for (name, _path, sha) in vm_tool_facts() {
        if !text.contains(&name) || !text.contains(&sha) {
            return Err(format!("missing tool proof for `{name}`"));
        }
    }
    let guest_sha = fs::read(&guest)
        .map(|bytes| crate::SHA256::sha256_hex(&bytes))
        .map_err(|e| format!("hashing `{}` failed: {e}", guest.display()))?;
    let harness_text = fs::read_to_string(harness)
        .map_err(|e| format!("reading `{}` failed: {e}", harness.display()))?;
    let final_text = harness_text.replacen(
        "\"state\":\"harness-ready\"",
        &format!(
            "\"state\":\"guest-passed\",\"guest_proof\":{},\"guest_proof_sha256\":{}",
            JSON::quote(&guest.display().to_string()),
            JSON::quote(&guest_sha)
        ),
        1,
    );
    fs::write(harness, final_text)
        .map_err(|e| format!("writing `{}` failed: {e}", harness.display()))?;
    Ok(Some(harness.to_path_buf()))
}

fn validate_cached_guest_proof(
    text: &str,
    system: &SystemPlan,
    gen: &Generation,
    disk: &str,
    media_proof: &Path,
) -> Result<(), String> {
    require_json_field(text, "state", "guest-passed")?;
    require_json_field(text, "host", &system.name)?;
    require_json_field(text, "generation", &gen.name)?;
    require_json_field(text, "disk", disk)?;
    require_json_field(text, "media_proof", &media_proof.display().to_string())?;
    require_json_field(text, "media_proof_sha256", &file_sha256(media_proof)?)?;
    let installer_iso = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso", system.name));
    require_json_field(
        text,
        "installer_iso_fingerprint",
        &file_fingerprint(&installer_iso)?,
    )?;
    require_guest_assertions(text)
}

pub(super) fn require_vm_run_proof(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
    harness: &Path,
) -> Result<(), String> {
    if !harness.is_file() {
        return Err(format!("missing VM proof `{}`", harness.display()));
    }
    let harness_text = fs::read_to_string(harness)
        .map_err(|e| format!("reading `{}` failed: {e}", harness.display()))?;
    require_json_field(&harness_text, "state", "guest-passed")?;
    require_json_field(&harness_text, "host", &system.name)?;
    require_json_field(&harness_text, "generation", &gen.name)?;
    require_json_field(&harness_text, "disk", disk)?;
    require_json_field(&harness_text, "media_proof", &media_proof.display().to_string())?;
    Ok(())
}

pub(super) fn file_sha256(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|bytes| crate::SHA256::sha256_hex(&bytes))
        .map_err(|e| format!("hashing `{}` failed: {e}", path.display()))
}

fn file_fingerprint(path: &Path) -> Result<String, String> {
    // Content-bound: installer media is restaged on every prove run, so an
    // mtime-based fingerprint would invalidate every cached guest proof.
    Ok(format!("sha256={}", file_sha256(path)?))
}

fn guest_proof_path(harness: &Path) -> PathBuf {
    let stem = harness
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("vm-proof");
    harness.with_file_name(format!("{stem}-guest-proof.json"))
}

fn require_json_field(text: &str, key: &str, expected: &str) -> Result<(), String> {
    let needle = format!("\"{key}\"");
    let Some(mut rest) = text.split_once(&needle).map(|(_, r)| r.trim_start()) else {
        return Err(format!("missing `{key}`"));
    };
    if let Some(after) = rest.strip_prefix(':') {
        rest = after.trim_start();
    } else {
        return Err(format!("missing `:` after `{key}`"));
    }
    let Some(rest) = rest.strip_prefix('"') else {
        return Err(format!("`{key}` is not a string"));
    };
    let Some(end) = rest.find('"') else {
        return Err(format!("`{key}` is not closed"));
    };
    let found = &rest[..end];
    if found == expected {
        Ok(())
    } else {
        Err(format!("`{key}` expected `{expected}`, found `{found}`"))
    }
}

fn require_guest_assertions(text: &str) -> Result<(), String> {
    let expected = format!("\"assertions\":[{}]", guest_assertions_json());
    if text.contains(&expected) {
        Ok(())
    } else {
        Err("guest assertions did not match the required install/reboot proof set".to_string())
    }
}

pub(super) fn require_real_vm_tools() -> Result<(), String> {
    let mut rejected = Vec::new();
    // The real tier realizes the disk through the hidden system backend and
    // boots it directly, so only QEMU and `nix` must be real. The installer
    // media tools stay a plumbing-tier (E1279) concern.
    for name in ["qemu-system-x86_64", "qemu-img", "nix"] {
        let Some(path) = find_on_path(name) else {
            rejected.push(format!("{name}: missing"));
            continue;
        };
        // A real toolchain binary is a native executable. Scripts, empty
        // stubs, and non-ELF stand-ins are harness fixtures, not proof tools.
        let bytes = fs::read(&path).unwrap_or_default();
        let is_native_executable = bytes.starts_with(b"\x7fELF");
        if !is_native_executable {
            rejected.push(format!("{}: {}", name, path.display()));
        }
    }
    if rejected.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "D-JOS-REALGUEST1=C forbids script/fake VM toolchains for replacement acceptance; rejected {}.",
            rejected.join(", ")
        ))
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    })
}

const GUEST_ASSERTIONS: [&str; 9] = [
    "current-generation-matches",
    "packages-present",
    "services-active",
    "network-up",
    "rollback-generation-bootable",
    "terminal-login-ready",
    "desktop-session-ready",
    "graphical-console-ready",
    "desktop-launchers-run",
];

pub(super) struct VmCommand {
    pub(super) phase: &'static str,
    pub(super) argv: Vec<String>,
}

fn ovmf_code_path() -> Option<PathBuf> {
    std::env::var_os("JETOS_OVMF_CODE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn qemu_proof_commands(
    boot_dir: &Path,
    disk: &str,
    iso: &Path,
    host: &str,
    generation: &str,
) -> Vec<VmCommand> {
    let iso_path = iso.display().to_string();
    let kernel = boot_dir.join("kernel").display().to_string();
    let initrd = boot_dir.join("initrd").display().to_string();
    let mut boot_installer = vec![
        "qemu-system-x86_64".to_string(),
        "-m".to_string(),
        "2048".to_string(),
        "-nographic".to_string(),
        "-monitor".to_string(),
        "none".to_string(),
    ];
    if let Some(ovmf) = ovmf_code_path() {
        boot_installer.extend([
            "-drive".to_string(),
            format!("if=pflash,format=raw,readonly=on,file={}", ovmf.display()),
        ]);
    }
    boot_installer.extend([
        "-cdrom".to_string(),
        iso_path,
        "-drive".to_string(),
        format!("file={disk},format=qcow2,if=ide"),
        "-netdev".to_string(),
        format!("user,id=net0,hostname={host}"),
        "-device".to_string(),
        "virtio-net-pci,netdev=net0".to_string(),
        "-boot".to_string(),
        "d".to_string(),
    ]);
    let graphical_cmdline = format!(
        "console=ttyS0 rdinit=/jetos/init init=/jetos/init jetos.mode=desktop-verify jetos.host={host} jetos.generation={generation} root=LABEL=jetos-root rw"
    );
    let mut boot_installed_disk = vec![
        "qemu-system-x86_64".to_string(),
        "-m".to_string(),
        "2048".to_string(),
        "-nographic".to_string(),
        "-monitor".to_string(),
        "none".to_string(),
    ];
    if let Some(ovmf) = ovmf_code_path() {
        boot_installed_disk.extend([
            "-drive".to_string(),
            format!("if=pflash,format=raw,readonly=on,file={}", ovmf.display()),
        ]);
    }
    boot_installed_disk.extend([
        "-drive".to_string(),
        format!("file={disk},format=qcow2,if=ide"),
        "-netdev".to_string(),
        format!("user,id=net0,hostname={host}"),
        "-device".to_string(),
        "virtio-net-pci,netdev=net0".to_string(),
        "-boot".to_string(),
        "c".to_string(),
    ]);
    vec![
        VmCommand {
            phase: "create-disk",
            argv: vec![
                "qemu-img".to_string(),
                "create".to_string(),
                "-f".to_string(),
                "qcow2".to_string(),
                disk.to_string(),
                "16G".to_string(),
            ],
        },
        VmCommand {
            phase: "boot-installer",
            argv: boot_installer,
        },
        VmCommand {
            phase: "boot-installed-disk",
            argv: boot_installed_disk,
        },
        VmCommand {
            phase: "boot-graphical-desktop",
            argv: vec![
                "qemu-system-x86_64".to_string(),
                "-m".to_string(),
                "2048".to_string(),
                "-display".to_string(),
                qemu_vnc_display().to_string(),
                "-serial".to_string(),
                "stdio".to_string(),
                "-monitor".to_string(),
                "none".to_string(),
                "-vga".to_string(),
                "std".to_string(),
                "-device".to_string(),
                "qemu-xhci,id=xhci".to_string(),
                "-device".to_string(),
                "usb-kbd,bus=xhci.0".to_string(),
                "-device".to_string(),
                "usb-tablet,bus=xhci.0".to_string(),
                "-kernel".to_string(),
                kernel,
                "-initrd".to_string(),
                initrd,
                "-append".to_string(),
                graphical_cmdline,
                "-drive".to_string(),
                format!("file={disk},format=qcow2,if=ide"),
                "-netdev".to_string(),
                format!("user,id=net0,hostname={host}"),
                "-device".to_string(),
                "virtio-net-pci,netdev=net0".to_string(),
                "-boot".to_string(),
                "c".to_string(),
            ],
        },
    ]
}

pub(super) fn qemu_interactive_run_command(
    boot_dir: &Path,
    disk: &str,
    host: &str,
    generation: &str,
) -> VmCommand {
    let kernel = boot_dir.join("kernel").display().to_string();
    let initrd = boot_dir.join("initrd").display().to_string();
    let console = "tty0";
    let cmdline = format!(
        "console={console} rdinit=/jetos/init init=/jetos/init jetos.mode=run jetos.host={host} jetos.generation={generation} root=LABEL=jetos-root rw systemd.unit=graphical.target"
    );
    let mut argv = vec![
        "qemu-system-x86_64".to_string(),
        "-m".to_string(),
        "2048".to_string(),
        "-cpu".to_string(),
        "max".to_string(),
    ];
    if qemu_has_local_display() {
        argv.extend([
            "-display".to_string(),
            "gtk,gl=off".to_string(),
            "-serial".to_string(),
            "none".to_string(),
        ]);
    } else {
        argv.extend([
            "-display".to_string(),
            qemu_vnc_display().to_string(),
            "-serial".to_string(),
            "none".to_string(),
        ]);
    }
    argv.extend([
        "-monitor".to_string(),
        "none".to_string(),
        "-vga".to_string(),
        "std".to_string(),
        "-device".to_string(),
        "qemu-xhci,id=xhci".to_string(),
        "-device".to_string(),
        "usb-kbd,bus=xhci.0".to_string(),
        "-device".to_string(),
        "usb-tablet,bus=xhci.0".to_string(),
        "-kernel".to_string(),
        kernel,
        "-initrd".to_string(),
        initrd,
        "-append".to_string(),
        cmdline,
        "-drive".to_string(),
        format!("file={disk},format=qcow2,if=ide"),
        "-netdev".to_string(),
        format!("user,id=net0,hostname={host}"),
        "-device".to_string(),
        "virtio-net-pci,netdev=net0".to_string(),
        "-boot".to_string(),
        "c".to_string(),
    ]);
    VmCommand {
        phase: "run-installed-disk",
        argv,
    }
}

fn qemu_vnc_display() -> &'static str {
    "vnc=127.0.0.1:0"
}

pub(super) fn qemu_vnc_endpoint() -> &'static str {
    "127.0.0.1:5900"
}

pub(super) fn qemu_has_local_display() -> bool {
    if std::env::var_os("JETOS_QEMU_VNC").is_some() {
        return false;
    }
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn qemu_proof_commands_json(
    boot_dir: &Path,
    disk: &str,
    iso: &Path,
    host: &str,
    generation: &str,
) -> String {
    qemu_proof_commands(boot_dir, disk, iso, host, generation)
        .into_iter()
        .map(|command| {
            let argv_json = command
                .argv
                .iter()
                .map(|arg| JSON::quote(arg))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"phase\":{},\"argv\":[{}]}}",
                JSON::quote(command.phase),
                argv_json
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) fn run_vmtest(
    theme: &Theme,
    plan: &EnvPlan,
    vmtest: &VmTestPlan,
    disk: &str,
    flags: &OsFlags,
    source_config: &Path,
) -> Result<PathBuf, String> {
    let proof_dir = systems_dir().join("vm-tests");
    fs::create_dir_all(&proof_dir)
        .map_err(|e| format!("creating `{}` failed: {e}", proof_dir.display()))?;
    let mut host_facts = Vec::new();
    for host in &vmtest.hosts {
        let Some(system) = plan.systems.iter().find(|s| s.name == host.system).cloned() else {
            return Err(format!(
                "host `{}` names unknown system `{}`",
                host.name, host.system
            ));
        };
        if !validate_system_options(theme, &system) {
            return Err(format!("system `{}` failed option validation", system.name));
        }
        let host_disk = vmtest_host_disk(disk, &host.name, vmtest.hosts.len());
        let Some(gen) = build_generation(theme, plan, &system, flags, source_config) else {
            return Err(format!("building system `{}` failed", system.name));
        };
        let media = write_installer_media(&gen, &system, "guided-ext4")
            .map_err(|e| format!("writing installer media for `{}` failed: {e}", system.name))?;
        let harness = write_vm_install_plan(&gen, &system, &host_disk, &media, false)
            .map_err(|e| format!("writing VM proof plan for `{}` failed: {e}", system.name))?;
        let final_path = prove_vm_guest(&gen, &system, &host_disk, &media, &harness, false)
            .map_err(|e| format!("guest proof for `{}` failed: {e}", system.name))?
            .ok_or_else(|| format!("guest proof for `{}` was not recorded", system.name))?;
        host_facts.push(VmTestHostFact {
            name: host.name.clone(),
            system: system.name,
            generation: gen.name,
            disk: host_disk,
            proof: final_path.display().to_string(),
        });
    }
    let proof = proof_dir.join(format!("{}-vmtest-proof.json", vmtest.name));
    fs::write(&proof, vmtest_proof_json(vmtest, &host_facts))
        .map_err(|e| format!("writing `{}` failed: {e}", proof.display()))?;
    Ok(proof)
}

struct VmTestHostFact {
    name: String,
    system: String,
    generation: String,
    disk: String,
    proof: String,
}

fn vmtest_host_disk(disk: &str, host: &str, host_count: usize) -> String {
    if host_count <= 1 {
        return disk.to_string();
    }
    let path = Path::new(disk);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(disk);
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("qcow2");
    let file = format!("{stem}-{host}.{ext}");
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.join(&file).display().to_string())
        .unwrap_or(file)
}

fn vmtest_proof_json(vmtest: &VmTestPlan, hosts: &[VmTestHostFact]) -> String {
    let host_json = hosts
        .iter()
        .map(|host| {
            JSON::object_of(&[
                ("name", &host.name),
                ("system", &host.system),
                ("generation", &host.generation),
                ("disk", &host.disk),
                ("proof", &host.proof),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let assertions = vmtest
        .assertions
        .iter()
        .map(|a| JSON::quote(a))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"jetos.vmtest.proof\",\"schema_version\":1,\"state\":\"passed\",\"name\":{},\"hosts\":[{}],\"assertions\":[{}],\"run\":{},\"proofs\":[\"build-generation\",\"install-reboot-proof\",\"typed-assertion-record\"]}}\n",
        JSON::quote(&vmtest.name),
        host_json,
        assertions,
        JSON::quote(&vmtest.run)
    )
}

fn guest_assertions_json() -> String {
    GUEST_ASSERTIONS
        .iter()
        .map(|assertion| JSON::quote(assertion))
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) fn vm_tools_json() -> String {
    vm_tool_facts()
        .into_iter()
        .map(|(name, path, sha)| {
            JSON::object_of(&[("name", &name), ("path", &path), ("sha256", &sha)])
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn vm_tool_facts() -> Vec<(String, String, String)> {
    VM_TOOLS
        .iter()
        .map(|tool| {
            let Some(path) = find_path_tool(tool) else {
                return (
                    (*tool).to_string(),
                    "<missing>".to_string(),
                    "<missing>".to_string(),
                );
            };
            let path_text = path.display().to_string();
            let sha = fs::read(&path)
                .map(|bytes| crate::SHA256::sha256_hex(&bytes))
                .unwrap_or_else(|_| "<unreadable>".to_string());
            ((*tool).to_string(), path_text, sha)
        })
        .collect()
}
