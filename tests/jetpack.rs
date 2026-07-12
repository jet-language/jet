//! End-to-end tests for the `jetpack` binary (Phase 1, D-JPK*).
//!
//! These drive the compiled `jetpack` binary the way a user would, using
//! offline provider fixtures so they need neither network nor Nix (the Forge
//! fixture pattern from forge-salvage.md). They cover the golden exit criteria
//! for JPK-1 / JPK-2 / JPK-3:
//!   * resolve a fixture ref → store path, exit 0
//!   * `-- cmd` runs in the composed env and returns the child's status
//!   * the parent environment is unchanged afterwards
//!   * a bad ref produces a friendly diagnostic and exit 2
//!   * add/remove edit `env.jet`

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::{jetos_bin, jetpack_bin};

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

fn jet() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jet"))
}

fn jetos() -> Command {
    Command::new(jetos_bin())
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    let mut entries = fs::read_dir(src)
        .unwrap()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_recursive(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

fn studio_http(addr: &str, method: &str, path: &str, body: &str) -> String {
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    {
        use std::io::Write;
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: local\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut response = String::new();
    {
        use std::io::Read;
        if let Err(e) = stream.read_to_string(&mut response) {
            assert_eq!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset,
                "studio HTTP read failed: {e}"
            );
        }
    }
    response
}

fn studio_json_string(response: &str, key: &str) -> String {
    let needle = format!("\"{key}\":\"");
    response
        .split_once(&needle)
        .and_then(|(_, rest)| rest.split_once('\"').map(|(value, _)| value.to_string()))
        .unwrap_or_else(|| panic!("missing Studio JSON string `{key}`: {response}"))
}

fn studio_json(response: &str) -> jetpack::JSON::Json {
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or(response);
    jetpack::JSON::parse(body.trim())
        .unwrap_or_else(|error| panic!("invalid Studio JSON response: {error}: {response}"))
}

fn json_string(json: &jetpack::JSON::Json, key: &str) -> String {
    json.get(key)
        .and_then(jetpack::JSON::Json::as_str)
        .unwrap_or_else(|error| panic!("invalid JSON string `{key}`: {error}: {json:?}"))
        .to_string()
}

#[test]
fn doctor_checks_real_state_and_is_read_only() {
    let project = Scratch::new("doctor-project");
    let root = Scratch::new("doctor-root");
    let keys = Scratch::new("doctor-keys");
    let keygen = jet().args(["registry", "keygen"])
        .current_dir(&project.path).env("JET_KEYS_DIR", &keys.path).output().unwrap();
    assert!(keygen.status.success(), "keygen: {}", String::from_utf8_lossy(&keygen.stderr));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for stream in listener.incoming().take(3) {
            let mut stream = stream.unwrap();
            use std::io::{Read, Write};
            let mut request = [0u8; 1024];
            let n = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..n]);
            assert!(request.contains("Authorization: Basic dXNlcjpzdXBlci1zZWNyZXQ=\r\n"), "{request}");
            stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        }
    });
    let registry_url = format!("http://user:super-secret@{addr}/index");
    let helper = jetpack::FFI::cached_crypto_helper_path();
    let helper_before = fs::metadata(&helper).unwrap();
    let mut helper_parent_before = fs::read_dir(helper.parent().unwrap()).unwrap()
        .map(|e| e.unwrap().file_name()).collect::<Vec<_>>();
    helper_parent_before.sort();

    let healthy = jetpack()
        .args(["doctor", "--json", "--online"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JET_KEYS_DIR", &keys.path)
        .env("JET_REGISTRY_URL", &registry_url)
        .output().unwrap();
    assert!(healthy.status.success(), "stderr: {}", String::from_utf8_lossy(&healthy.stderr));
    let healthy_json = jetpack::JSON::parse(&String::from_utf8_lossy(&healthy.stdout)).unwrap();
    assert_eq!(json_string(&healthy_json, "status"), "healthy");
    assert_eq!(fs::metadata(&helper).unwrap().len(), helper_before.len(), "doctor changed signing helper");
    let mut helper_parent_after = fs::read_dir(helper.parent().unwrap()).unwrap()
        .map(|e| e.unwrap().file_name()).collect::<Vec<_>>();
    helper_parent_after.sort();
    assert_eq!(helper_parent_after, helper_parent_before, "doctor changed signing helper cache");

    fs::remove_file(keys.join("jet.ed25519")).unwrap();
    let degraded = jetpack()
        .args(["doctor", "--online"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JET_KEYS_DIR", &keys.path)
        .env("JET_REGISTRY_URL", &registry_url)
        .output().unwrap();
    assert_eq!(degraded.status.code(), Some(1));
    let degraded_text = String::from_utf8(degraded.stderr).unwrap();
    assert!(degraded_text.contains("[warn] signing"), "{degraded_text}");
    assert!(degraded_text.ends_with("result: degraded\n"), "{degraded_text}");
    assert!(!degraded_text.contains("super-secret"), "credential leaked: {degraded_text}");
    let keygen = jet().args(["registry", "keygen", "--force"])
        .current_dir(&project.path).env("JET_KEYS_DIR", &keys.path).output().unwrap();
    assert!(keygen.status.success(), "keygen: {}", String::from_utf8_lossy(&keygen.stderr));
    let public_path = keys.join("jet.ed25519.pub");
    let matching_public = fs::read_to_string(&public_path).unwrap();
    let mut mismatched_public = matching_public.clone().into_bytes();
    mismatched_public[0] = if mismatched_public[0] == b'0' { b'1' } else { b'0' };
    fs::write(&public_path, &mismatched_public).unwrap();
    let mismatch = jetpack().args(["doctor", "--online"])
        .current_dir(&project.path).env("JETPACK_ROOT", &root.path)
        .env("JET_KEYS_DIR", &keys.path).env("JET_REGISTRY_URL", &registry_url)
        .output().unwrap();
    let mismatch_text = String::from_utf8(mismatch.stderr).unwrap();
    assert_eq!(mismatch.status.code(), Some(2), "{mismatch_text}");
    assert!(mismatch_text.contains("does not match its public key"), "{mismatch_text}");
    assert!(!mismatch_text.contains("super-secret"), "credential leaked: {mismatch_text}");
    fs::write(&public_path, matching_public).unwrap();
    server.join().unwrap();

    let output = root.join("owned-output");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("payload"), "trusted bytes").unwrap();
    let envelope = jetpack::Envelope::Envelope::for_output(
        &output.to_string_lossy(), "path:demo", "test-recipe");
    let roots = jetpack::Store::Roots { root: root.path.clone(), dev_mode: false };
    let entry = jetpack::Store::record(&roots, "demo", "1", "path:demo",
        &output.to_string_lossy(), "", "", &envelope).unwrap();
    let meta = root.join(&format!("hangar/{}/meta.json", entry.id));
    let old_meta = fs::read_to_string(&meta).unwrap();
    let stale_meta = old_meta.replace(
        &format!("\"last_used_at\": \"{}\"", entry.last_used_at),
        "\"last_used_at\": \"0\"");
    fs::write(&meta, &stale_meta).unwrap();
    fs::write(output.join("payload"), "corrupt bytes").unwrap();
    fs::create_dir_all(root.join(".locks")).unwrap();
    let stale_lock = root.join(".locks/abandoned.lock");
    fs::write(&stale_lock, "pid=4294967294\n").unwrap();
    fs::remove_file(keys.join("jet.ed25519")).unwrap();
    let before_meta = fs::read(&meta).unwrap();
    let before_lock = fs::read(&stale_lock).unwrap();
    let before_public = fs::read(keys.join("jet.ed25519.pub")).unwrap();
    let before_public_permissions = fs::metadata(keys.join("jet.ed25519.pub")).unwrap().permissions();
    let before_output_permissions = fs::metadata(output.join("payload")).unwrap().permissions();

    let broken = jetpack()
        .args(["doctor", "--json", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JET_KEYS_DIR", &keys.path)
        .env("JET_REGISTRY_URL", format!("file://{}", project.join("missing").display()))
        .output().unwrap();
    assert_eq!(broken.status.code(), Some(2));
    let text = String::from_utf8(broken.stdout).unwrap();
    assert!(text.contains("failed its content digest"), "{text}");
    assert!(text.contains("local index missing"), "{text}");
    assert!(text.contains("stale lock"), "{text}");
    assert!(text.contains("unused for more than 30 days"), "{text}");
    assert!(text.contains("signing key for `jet` is missing"), "{text}");
    assert_eq!(fs::read(&meta).unwrap(), before_meta, "doctor changed metadata");
    assert_eq!(fs::read(&stale_lock).unwrap(), before_lock, "doctor changed lock state");
    assert_eq!(fs::read(keys.join("jet.ed25519.pub")).unwrap(), before_public, "doctor changed public key");
    assert_eq!(fs::metadata(keys.join("jet.ed25519.pub")).unwrap().permissions(), before_public_permissions, "doctor changed key permissions");
    assert_eq!(fs::metadata(output.join("payload")).unwrap().permissions(), before_output_permissions, "doctor changed output permissions");
}

#[derive(Clone)]
struct StudioTestOwner {
    session_id: String,
    token: String,
    base_revision: String,
}

fn studio_session(addr: &str) -> String {
    let response = studio_http(
        addr,
        "POST",
        "/studio/transaction",
        "{\"op\":\"session\"}",
    );
    studio_json_string(&response, "session_id")
}

fn studio_changeset_owner(response: &str, session_id: &str) -> StudioTestOwner {
    StudioTestOwner {
        session_id: session_id.to_string(),
        token: studio_json_string(response, "token"),
        base_revision: studio_json_string(response, "base_revision"),
    }
}

fn studio_owned_transaction(addr: &str, op: &str, owner: &StudioTestOwner) -> String {
    studio_http(
        addr,
        "POST",
        "/studio/transaction",
        &format!(
            "{{\"op\":\"{op}\",\"session_id\":\"{}\",\"token\":\"{}\",\"base_revision\":\"{}\"}}",
            owner.session_id, owner.token, owner.base_revision
        ),
    )
}

fn studio_session_transaction(addr: &str, op: &str, session_id: &str) -> String {
    studio_http(
        addr,
        "POST",
        "/studio/transaction",
        &format!("{{\"op\":\"{op}\",\"session_id\":\"{session_id}\"}}"),
    )
}

fn studio_stage_option(
    addr: &str,
    session_id: &str,
    key: &str,
    value: &str,
    write: bool,
) -> String {
    studio_http(
        addr,
        "POST",
        "/studio/transaction",
        &format!(
            "{{\"op\":\"set-option\",\"session_id\":\"{session_id}\",\"key\":\"{}\",\"value\":\"{}\",\"write\":{write}}}",
            test_json_escape(key),
            test_json_escape(value),
        ),
    )
}

fn studio_attack_snapshot(server_pid: u32, truncate: bool) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let snapshot = fs::read_dir(format!("/proc/{server_pid}/fd"))
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .find(|entry| {
                    fs::read_link(entry.path())
                        .map(|target| target.to_string_lossy().contains("memfd:jetos-studio-source-"))
                        .unwrap_or(false)
                })
                .map(|entry| entry.path());
            if let Some(path) = snapshot {
                let attempt = std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(truncate)
                    .open(path);
                assert!(attempt.is_err(), "sealed Studio source accepted hostile write access");
                return;
            }
            assert!(std::time::Instant::now() < deadline, "Studio snapshot never appeared");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    })
}

/// A throwaway directory under the system temp dir, removed on drop.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "jpk-it-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
    fn join(&self, p: &str) -> PathBuf {
        self.path.join(p)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        make_tree_writable(&self.path);
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn make_tree_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            make_tree_writable(&entry.unwrap().path());
        }
    }
    if !meta.file_type().is_symlink() {
        let mode = if meta.is_dir() { 0o755 } else { meta.permissions().mode() | 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }
}

fn assert_no_ephemeral_links(path: &Path) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.file_type().is_symlink() {
        let target = fs::read_link(path).unwrap();
        let text = target.to_string_lossy();
        assert!(!text.contains("/proc/self/fd/"), "ephemeral FD link: {} -> {text}", path.display());
        assert!(!text.contains("/leases/"), "ephemeral lease link: {} -> {text}", path.display());
        return;
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path).unwrap().flatten() {
            assert_no_ephemeral_links(&entry.path());
        }
    }
}

#[cfg(not(unix))]
fn make_tree_writable(path: &Path) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    let mut permissions = meta.permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).unwrap();
}

fn example_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-project/fixtures")
}

/// The committed jetpack project fixture (`env.jet` + `jet-pkgs/`).
fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-project")
}

/// Write a provider fixture whose `out` points at a real dir we control, so a
/// `-- cmd` invocation can actually execute a binary from the realized env.
fn write_runnable_fixture(fixtures: &Path, out_dir: &Path) {
    fs::create_dir_all(fixtures).unwrap();
    let bin = out_dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let greet = bin.join("greet");
    fs::write(&greet, "#!/bin/sh\necho hello from jetpack\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let json = format!(
        "[{{\"outputs\":{{\"out\":{:?}}}}}]",
        out_dir.to_string_lossy()
    );
    fs::write(fixtures.join("nixpkgs-greet.json"), json).unwrap();
}

/// Write a `nixpkgs:fastfetch` fixture whose `out` points at a real directory
/// we control (see `write_runnable_fixture`). The committed
/// `tests/fixtures/jetpack-project/fixtures` set uses placeholder
/// `/nix/store/...` paths that never exist on disk — fine for tests that only
/// check `jetpack build`'s ledger output, but Store's fail-closed leasing
/// (`snapshot_lease`) refuses to hand a consumer any path whose `out` doesn't
/// exist, so a test that enters the composed env (`run`/`dev` with no
/// explicit command consuming the package) needs a real backing tree.
fn write_fastfetch_fixture(fixtures: &Path, out_dir: &Path) {
    fs::create_dir_all(fixtures).unwrap();
    let bin = out_dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let fastfetch = bin.join("fastfetch");
    fs::write(&fastfetch, "#!/bin/sh\necho fastfetch stub\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fastfetch, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let json = format!(
        "[{{\"outputs\":{{\"out\":{:?}}}}}]",
        out_dir.to_string_lossy()
    );
    fs::write(fixtures.join("nixpkgs-fastfetch.json"), json).unwrap();
}

fn write_channel_fixture(fixtures: &Path, base: &str, channel: &str, exact: &str) {
    fs::create_dir_all(fixtures).unwrap();
    fs::write(
        fixtures.join("channels.txt"),
        format!("{base} {channel} {exact} 240000000\n"),
    )
    .unwrap();
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn test_json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn test_shell_quote(value: &Path) -> String {
    format!("'{}'", value.display().to_string().replace('\'', "'\\''"))
}

fn write_executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn write_fake_vm_tools(bin: &Path, guest_passes: bool) {
    fs::create_dir_all(bin).unwrap();
    let limine_data = bin.join("limine-data");
    fs::create_dir_all(&limine_data).unwrap();
    fs::write(limine_data.join("limine-bios.sys"), "fake limine bios\n").unwrap();
    fs::write(limine_data.join("BOOTX64.EFI"), "fake limine efi\n").unwrap();
    write_executable(
        &bin.join("limine"),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"--print-datadir\" ]; then printf '%s\\n' '{}'; exit 0; fi\nexit 0\n",
            limine_data.display()
        ),
    );
    write_executable(
        &bin.join("xorriso"),
        "#!/bin/sh\nout=''\nprev=''\nfor arg in \"$@\"; do\n  if [ \"$prev\" = '-o' ]; then out=\"$arg\"; fi\n  prev=\"$arg\"\ndone\nif [ -n \"$out\" ]; then printf 'fake iso\\n' > \"$out\"; fi\nexit 0\n",
    );
    write_executable(
        &bin.join("qemu-img"),
        "#!/bin/sh\nif [ \"$1\" = 'create' ]; then printf 'fake qcow2\\n' > \"$4\"; fi\nexit 0\n",
    );
    let guest_line = if guest_passes {
        "host=unknown\ngeneration=unknown\nfor arg in \"$@\"; do\n  case \"$arg\" in\n    *jetos.host=*) host=\"${arg#*jetos.host=}\"; host=\"${host%% *}\" ;;\n  esac\n  case \"$arg\" in\n    *jetos.generation=*) generation=\"${arg#*jetos.generation=}\"; generation=\"${generation%% *}\" ;;\n  esac\ndone\ncase \" $* \" in\n  *'jetos.mode=desktop-verify'*) echo \"jetos proof: display manager command gdm\"; echo \"jetos proof: desktop session command gnome-session\"; echo \"jetos proof: terminal fallback ready\"; echo \"JETOS_GUEST_PROOF: {\\\"state\\\":\\\"guest-passed\\\",\\\"host\\\":\\\"$host\\\",\\\"generation\\\":\\\"$generation\\\",\\\"assertions\\\":[\\\"current-generation-matches\\\",\\\"packages-present\\\",\\\"services-active\\\",\\\"network-up\\\",\\\"rollback-generation-bootable\\\",\\\"terminal-login-ready\\\",\\\"desktop-session-ready\\\",\\\"graphical-console-ready\\\",\\\"desktop-launchers-run\\\"]}\" ;;\n  *' -boot c '*) echo \"JETOS_GUEST_PROOF: {\\\"state\\\":\\\"guest-passed\\\",\\\"host\\\":\\\"$host\\\",\\\"generation\\\":\\\"$generation\\\",\\\"assertions\\\":[\\\"current-generation-matches\\\",\\\"packages-present\\\",\\\"services-active\\\",\\\"network-up\\\",\\\"rollback-generation-bootable\\\",\\\"terminal-login-ready\\\",\\\"desktop-session-ready\\\"]}\" ;;\nesac\n"
    } else {
        "echo 'qemu booted without guest proof'\n"
    };
    write_executable(
        &bin.join("qemu-system-x86_64"),
        &format!("#!/bin/sh\n{guest_line}exit 0\n"),
    );
    write_executable(&bin.join("mkfs.ext4"), "#!/bin/sh\nexit 0\n");
    write_executable(&bin.join("mkfs.vfat"), "#!/bin/sh\nexit 0\n");
    write_executable(&bin.join("sfdisk"), "#!/bin/sh\nexit 0\n");
    write_executable(&bin.join("blockdev"), "#!/bin/sh\nexit 0\n");
    for tool in [
        "cat", "cp", "ln", "mkdir", "mount", "rm", "setsid", "sleep", "sync", "poweroff", "halt",
    ] {
        write_executable(&bin.join(tool), "#!/bin/sh\nexit 0\n");
    }
    write_executable(&bin.join("mmd"), "#!/bin/sh\nexit 0\n");
    write_executable(&bin.join("mcopy"), "#!/bin/sh\nexit 0\n");
    write_executable(
        &bin.join("zstd"),
        "#!/bin/sh\nlast=''\nfor arg in \"$@\"; do last=\"$arg\"; done\nwhile IFS= read -r line; do printf '%s\\n' \"$line\"; done < \"$last\"\n",
    );
}

fn write_hangar_meta(
    root: &Path,
    id: &str,
    name: &str,
    version: &str,
    output_hash: &str,
    last_used_at: Option<u64>,
) -> PathBuf {
    let dir = root.join("hangar").join(id);
    fs::create_dir_all(&dir).unwrap();
    let timestamps = last_used_at
        .map(|ts| format!("  \"realized_at\": \"{ts}\",\n  \"last_used_at\": \"{ts}\",\n"))
        .unwrap_or_default();
    fs::write(
        dir.join("meta.json"),
        format!(
            "{{\n  \"name\": \"{name}\",\n  \"version\": \"{version}\",\n  \"ref\": \"nixpkgs:{name}\",\n  \"out\": \"/nix/store/{name}\",\n  \"bin\": \"/nix/store/{name}/bin\",\n  \"rlib\": \"\",\n  \"output_hash\": \"{output_hash}\",\n  \"platform\": \"test\",\n  \"signature\": \"\",\n  \"provenance\": \"fixture\",\n{timestamps}  \"end\": \"meta\"\n}}"
        ),
    )
    .unwrap();
    dir
}

fn write_lock_with_live_output(project: &Path, name: &str, version: &str, output_hash: &str) {
    let dot = project.join(".jet");
    fs::create_dir_all(&dot).unwrap();
    fs::write(
        dot.join("lock"),
        format!(
            "version = 1\n\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\nsource = {{ path = \".\" }}\nfingerprint = \"sha256-test\"\ndependencies = []\noutput-hash = \"{output_hash}\"\n\n[root]\ndependencies = [\"{name}\"]\n"
        ),
    )
    .unwrap();
}

#[test]
fn override_draft_writes_reviewed_workspace_policy_and_explains_it() {
    let project = Scratch::new("override-draft");
    fs::create_dir_all(project.join("patches")).unwrap();
    fs::write(project.join("patches/foo.patch"), "patch body\n").unwrap();

    let out = jetpack()
        .args([
            "override",
            "draft",
            "nixpkgs:foo",
            "--overlay",
            "plasma_beta",
            "--provider",
            "nixpkgs",
            "--channel",
            "plasma-beta",
            "--patch",
            "patches/foo.patch",
            "--allow-unfree",
            "--no-color",
        ])
        .current_dir(&project.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let workspace = fs::read_to_string(project.join("workspace.jet")).unwrap();
    assert!(workspace.contains("overlay plasma_beta"), "{workspace}");
    assert!(
        workspace.contains("Provider.nixpkgs(channel: \"plasma-beta\")"),
        "{workspace}"
    );
    assert!(
        workspace.contains("package(\"foo\").patches += [patch(\"patches/foo.patch\")]"),
        "{workspace}"
    );
    assert!(
        workspace.contains("package(\"foo\").allowUnfree: true"),
        "{workspace}"
    );

    let explain = jetpack()
        .args(["explain", "package-overlay:plasma_beta:foo", "--no-color"])
        .current_dir(&project.path)
        .output()
        .unwrap();
    assert!(
        explain.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&explain.stderr)
    );
    let stdout = String::from_utf8_lossy(&explain.stdout);
    assert!(
        stdout.contains("package-overlay:plasma_beta:foo")
            && stdout.contains("provider: nixpkgs")
            && stdout.contains("policy: workspace.overlay.plasma_beta"),
        "explain: {stdout}"
    );
}

#[test]
fn build_resolves_fixture_ref() {
    let root = Scratch::new("root");
    let run = || {
        jetpack()
            .args(["build", "nixpkgs:fastfetch", "--no-color", "--offline"])
            .env("JETPACK_ROOT", &root.path)
            .env("JETPACK_FIXTURES", example_fixtures())
            .output()
            .unwrap()
    };
    let out = run();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("fastfetch"), "stderr: {stderr}");
    assert!(stderr.contains("/nix/store/"), "stderr: {stderr}");
    let repeated = run();
    assert!(repeated.status.success());
    let repeated_stderr = String::from_utf8_lossy(&repeated.stderr);
    assert!(!repeated_stderr.contains("E2604"), "stderr: {repeated_stderr}");
    assert!(
        repeated_stderr.contains("substituted"),
        "Nix fixture must re-enter its provider, not claim a Jetpack cache hit: {repeated_stderr}"
    );
}

#[test]
fn list_shows_realized_package() {
    let root = Scratch::new("root");
    jetpack()
        .args(["build", "nixpkgs:ripgrep", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    let out = jetpack()
        .args(["list", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("ripgrep"), "stderr: {stderr}");
}

#[test]
fn clean_removes_only_stale_unreferenced_hangar_objects() {
    let root = Scratch::new("root");
    let stale = write_hangar_meta(&root.path, "old-1", "old", "1.0", "sha256-old", Some(1));
    let fresh = write_hangar_meta(
        &root.path,
        "fresh-1",
        "fresh",
        "1.0",
        "sha256-fresh",
        Some(now_secs()),
    );
    fs::write(stale.join("payload"), "old bytes").unwrap();
    fs::write(fresh.join("payload"), "fresh bytes").unwrap();

    let out = jetpack()
        .args(["clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!stale.exists(), "stale object should be collected");
    assert!(fresh.exists(), "fresh object should be kept");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("removed 1 stale object"),
        "stderr: {stderr}"
    );
}

#[test]
fn clean_without_yes_prints_plan_and_does_not_apply_in_non_tty() {
    let root = Scratch::new("root");
    let stale = write_hangar_meta(&root.path, "old-plan", "oldplan", "1.0", "", Some(1));
    fs::write(stale.join("payload"), "old bytes").unwrap();

    let out = jetpack()
        .args(["clean", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stale.exists(), "plan-only clean must not delete objects");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Plan hangar clean"), "stderr: {stderr}");
    assert!(stderr.contains("- stale-objects"), "stderr: {stderr}");
    assert!(stderr.contains("-y or --yes"), "stderr: {stderr}");
}

#[test]
fn clean_keeps_lock_reachable_and_legacy_unknown_hangar_objects() {
    let root = Scratch::new("root");
    let project = Scratch::new("proj");
    let live = write_hangar_meta(&root.path, "live-1", "live", "1.0", "sha256-live", Some(1));
    let legacy = write_hangar_meta(&root.path, "legacy-1", "legacy", "1.0", "", None);
    write_lock_with_live_output(&project.path, "live", "1.0", "sha256-live");

    let out = jetpack()
        .args(["clean", "--no-color", "--yes"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(live.exists(), "lock-reachable object should be kept");
    assert!(
        legacy.exists(),
        "legacy object without timestamps should be kept"
    );
}

#[test]
fn clean_sweeps_orphan_build_scratch_but_keeps_active_scratch() {
    let root = Scratch::new("root");
    let scratch = root.path.join("hangar/build-scratch");
    let orphan = scratch.join("orphan");
    let active = scratch.join("active");
    fs::create_dir_all(&orphan).unwrap();
    fs::create_dir_all(&active).unwrap();
    fs::write(orphan.join("tmp"), "dead").unwrap();
    fs::write(active.join(".active"), "").unwrap();
    fs::write(active.join("tmp"), "live").unwrap();

    let out = jetpack()
        .args(["clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!orphan.exists(), "orphan scratch should be swept");
    assert!(active.exists(), "active scratch marker protects scratch");
}

#[test]
fn clean_optimizes_duplicate_files_inside_hangar_only() {
    let root = Scratch::new("root");
    let first = write_hangar_meta(
        &root.path,
        "dup-a",
        "dupa",
        "1.0",
        "sha256-a",
        Some(now_secs()),
    );
    let second = write_hangar_meta(
        &root.path,
        "dup-b",
        "dupb",
        "1.0",
        "sha256-b",
        Some(now_secs()),
    );
    fs::write(first.join("blob"), "same payload").unwrap();
    fs::write(second.join("blob"), "same payload").unwrap();

    let out = jetpack()
        .args(["clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("optimized 1 file"), "stderr: {stderr}");
    assert_eq!(
        fs::read_to_string(first.join("blob")).unwrap(),
        "same payload"
    );
    assert_eq!(
        fs::read_to_string(second.join("blob")).unwrap(),
        "same payload"
    );
}

#[test]
fn jet_clean_delegates_to_jetpack_clean() {
    let root = Scratch::new("root");
    let stale = write_hangar_meta(&root.path, "old-top", "oldtop", "1.0", "", Some(1));

    let out = jet()
        .args(["clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!stale.exists(), "`jet clean` should collect via jetpack");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("cleaned hangar"), "stderr: {stderr}");
}

#[test]
fn build_runs_opportunistic_clean_after_success() {
    let root = Scratch::new("root");
    let stale = write_hangar_meta(&root.path, "old-auto", "oldauto", "1.0", "", Some(1));

    let out = jetpack()
        .args(["build", "nixpkgs:fastfetch", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .env("JETPACK_AUTO_CLEAN_ALWAYS", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !stale.exists(),
        "successful build should run opportunistic clean"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("auto-cleaned hangar"), "stderr: {stderr}");
}

#[test]
fn run_dash_dash_executes_in_env_and_returns_status() {
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &out_dir.path);

    let output = jetpack()
        .args([
            "run",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--",
            "greet",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "hello from jetpack");
}

#[test]
fn run_explicit_package_without_command_runs_package_visibly() {
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &out_dir.path);

    let output = jetpack()
        .args(["run", "nixpkgs:greet", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jetpack"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("running nixpkgs:greet -> greet"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("(no args)"), "stderr: {stderr}");
}

#[test]
fn run_dash_dash_propagates_failure_status() {
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &out_dir.path);

    let output = jetpack()
        .args([
            "run",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--",
            "false",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn parent_env_unchanged_after_run() {
    // The composed PATH only reaches the child. Ask the child to echo PATH and
    // confirm our bin dirs lead; the test process's own PATH is unaffected
    // because we never mutate it.
    //
    // Realization leases are mandatory (card #418): the consumer never sees
    // the raw fixture `out_dir` directly, only a sealed, hardlinked snapshot
    // copy under the hangar's `leases/` dir. The sealed, FD-pinned
    // exec-wrapper dir (`/proc/self/fd/N` on Linux, immutable and race-safe
    // against parent rename/symlink swaps) leads PATH ahead of that snapshot
    // bin dir.
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &out_dir.path);
    let before = std::env::var("PATH").unwrap_or_default();

    let output = jetpack()
        .args([
            "run",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--",
            "sh",
            "-c",
            "printf %s \"$PATH\"",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    let child_path = String::from_utf8_lossy(&output.stdout);
    let mut entries = child_path.split(':');
    let wrapper = entries.next().unwrap_or_default();
    assert!(
        wrapper.starts_with("/proc/self/fd/"),
        "expected the sealed FD-pinned exec-wrapper dir first, got: {child_path}"
    );
    let bin = entries.next().unwrap_or_default();
    assert!(
        bin.starts_with(&root.path.to_string_lossy().into_owned()) && bin.ends_with("/bin"),
        "expected the leased snapshot bin dir (under JETPACK_ROOT) second, got: {child_path}"
    );
    assert_eq!(std::env::var("PATH").unwrap_or_default(), before);
}

#[test]
fn bad_ref_is_friendly_and_exits_2() {
    let out = jetpack()
        .args(["run", "fastfetch", "--no-color"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing a source"), "stderr: {stderr}");
    assert!(stderr.contains("<source>:<package>"), "stderr: {stderr}");
}

#[test]
fn unknown_source_is_friendly() {
    let out = jetpack()
        .args(["build", "brew:wget", "--no-color"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not a known source"), "stderr: {stderr}");
}

#[test]
fn add_then_remove_edits_env_file() {
    let (_base, proj, root) = core_hello_project("add-remove");
    let env_path = proj.join("env.jet");
    fs::write(
        &env_path,
        fs::read_to_string(&env_path).unwrap().replace("\"mine:hello\"", ""),
    )
    .unwrap();
    let add = jetpack()
        .args(["add", "mine:hello", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(
        String::from_utf8_lossy(&add.stderr).contains("✓ hello     0.1.0"),
        "add must print its verified resolved version: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let env = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(env.contains("hello"), "env.jet: {env}");
    assert!(env.contains("pkg.packages"), "env.jet: {env}");

    let remove = jetpack()
        .args(["remove", "mine:hello", "--no-color", "--yes"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(remove.status.success());
    let env = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(
        !env.contains("\"mine:hello\""),
        "env.jet still has hello: {env}"
    );
}

#[test]
fn remove_without_yes_prints_plan_and_keeps_env_file_in_non_tty() {
    let (_base, proj, root) = core_hello_project("remove-plan");
    let env_path = proj.join("env.jet");
    fs::write(
        &env_path,
        fs::read_to_string(&env_path).unwrap().replace("\"mine:hello\"", ""),
    )
    .unwrap();
    let add = jetpack()
        .args(["add", "mine:hello", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(add.status.success());

    let remove = jetpack()
        .args(["remove", "mine:hello", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(remove.status.success());
    let env = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(env.contains("\"mine:hello\""), "env.jet was changed: {env}");
    let stderr = String::from_utf8_lossy(&remove.stderr);
    assert!(stderr.contains("Plan env edit"), "stderr: {stderr}");
    assert!(stderr.contains("- hello"), "stderr: {stderr}");
    assert!(stderr.contains("Download 0 B"), "stderr: {stderr}");
    assert!(stderr.contains("-y or --yes"), "stderr: {stderr}");
}

#[test]
fn remove_with_short_yes_applies_identically_to_long_yes() {
    // D-FE-CLI1: `-y` and `--yes` bypass the mutation gate identically.
    let (_base, proj, root) = core_hello_project("remove-short-yes");
    let env_path = proj.join("env.jet");
    fs::write(
        &env_path,
        fs::read_to_string(&env_path).unwrap().replace("\"mine:hello\"", ""),
    )
    .unwrap();
    let add = jetpack()
        .args(["add", "mine:hello", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let remove = jetpack()
        .args(["remove", "mine:hello", "--no-color", "-y"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        remove.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&remove.stderr)
    );
    let env = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(
        !env.contains("\"mine:hello\""),
        "short -y must apply the remove plan: {env}"
    );
    let stderr = String::from_utf8_lossy(&remove.stderr);
    assert!(stderr.contains("Plan env edit"), "stderr: {stderr}");
    assert!(stderr.contains("- hello"), "stderr: {stderr}");
    assert!(
        stderr.contains("applying plan (--yes)") || stderr.contains("removed"),
        "short -y must take the yes-bypass path: {stderr}"
    );
}

#[test]
fn run_with_project_env_file_resolves_declared_packages() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fixtures");
    let fastfetch_out = Scratch::new("fastfetch-out");
    write_fastfetch_fixture(&fixtures.path, &fastfetch_out.path);
    // Declare one package, then run with no ref → it resolves from env.jet.
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [\n        pkg.source(\"nixpkgs\");\n        pkg.packages([\"fastfetch\"]);\n    ];\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["run", "--no-color", "--offline", "--", "true"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fastfetch"), "stderr: {stderr}");
}

#[test]
fn typed_env_copy_adapter_realizes_local_source() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let vendor = proj.join("vendor/tool");
    fs::create_dir_all(vendor.join("share")).unwrap();
    fs::write(vendor.join("share/readme.txt"), "adapted\n").unwrap();
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env.{
        packages: [
            Pkg.adapt(
                name: "tool",
                source: path@vendor/tool,
                recipe: Recipe.copy()
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let entries = fs::read_dir(root.path.join("hangar"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect::<Vec<_>>();
    assert!(
        entries.iter().any(
            |p| fs::read_to_string(p.join("share/readme.txt")).unwrap_or_default() == "adapted\n"
        ),
        "adapter output missing copied file: {entries:?}"
    );
    let cached = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        cached.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cached.stderr)
    );
    assert!(
        String::from_utf8_lossy(&cached.stderr).contains("1 cached"),
        "stderr: {}",
        String::from_utf8_lossy(&cached.stderr)
    );
}

#[test]
fn typed_env_prebuilt_adapter_runs_from_path() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let vendor = proj.join("vendor/weirdctl");
    fs::create_dir_all(&vendor).unwrap();
    let bin = vendor.join("weirdctl");
    fs::write(&bin, "#!/bin/sh\necho weird ok\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env.{
        packages: [
            Pkg.adapt(
                name: "weirdctl",
                source: path@vendor/weirdctl,
                recipe: Recipe.prebuilt(bin: "weirdctl", as: "weirdctl")
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["run", "--no-color", "--", "weirdctl"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "weird ok");
}

#[test]
fn no_nix_nixpkgs_package_reports_e1272() {
    let root = Scratch::new("root");
    let output = jetpack()
        .args(["build", "nixpkgs:postgres", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1272"), "stderr: {stderr}");
    assert!(stderr.contains("nixpkgs:postgres"), "stderr: {stderr}");
    assert!(stderr.contains("install Nix"), "stderr: {stderr}");
    assert!(stderr.contains("--adapt"), "stderr: {stderr}");
    assert!(!stderr.contains("E1256"), "stderr: {stderr}");
    assert!(!stderr.contains("couldn't run `nix`"), "stderr: {stderr}");
}

#[test]
fn no_nix_ad_hoc_package_reports_e1272() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let output = jetpack()
        .args([
            "enter",
            "-p",
            "postgres",
            "--no-color",
            "--trust",
            "--",
            "true",
        ])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1272"), "stderr: {stderr}");
    assert!(stderr.contains("nixpkgs:postgres"), "stderr: {stderr}");
}

#[test]
fn no_nix_mixed_env_realizes_core_then_reports_nix_hole() {
    let (base, proj, root) = core_hello_project("no-nix-mixed");
    fs::write(
        proj.join("env.jet"),
        fs::read_to_string(proj.join("env.jet")).unwrap().replace(
            "pkg.packages([\"mine:hello\"])",
            "pkg.packages([\"mine:hello\", \"nixpkgs:postgres\"])",
        ),
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    // T4 ledger row: `✓ hello  <version>  built` (columns padded).
    assert!(
        stderr
            .lines()
            .any(|l| l.contains("hello") && l.trim_end().ends_with("built")),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("E1272"), "stderr: {stderr}");
    assert!(stderr.contains("nixpkgs:postgres"), "stderr: {stderr}");
    let metas = fs::read_dir(root.join("hangar"))
        .unwrap()
        .flatten()
        .filter_map(|e| fs::read_to_string(e.path().join("meta.json")).ok())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(metas.contains("\"name\": \"hello\""), "metas: {metas}");
}

#[test]
fn no_nix_json_lists_realized_refs_and_holes() {
    let (base, proj, root) = core_hello_project("no-nix-json");
    fs::write(
        proj.join("env.jet"),
        fs::read_to_string(proj.join("env.jet")).unwrap().replace(
            "pkg.packages([\"mine:hello\"])",
            "pkg.packages([\"mine:hello\", \"nixpkgs:postgres\"])",
        ),
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color", "--json"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"code\":\"E1272\""), "stdout: {stdout}");
    assert!(
        stdout.contains("\"realized\":[\"mine:hello\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"holes\":[\"nixpkgs:postgres\"]"),
        "stdout: {stdout}"
    );
}

#[test]
fn typed_env_bad_adapter_is_e1270() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env.{
        packages: [
            Pkg.adapt(
                name: "broken",
                source: path@vendor/broken,
                recipe: Recipe.build()
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1270"), "stderr: {stderr}");
}

#[test]
fn channel_update_writes_exact_lock_and_build_uses_it_offline() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { default: github@acme/tools#latest }
    env.dev: Env.{ packages: [default.greet] }
}
"#,
    )
    .unwrap();
    write_channel_fixture(
        &fixtures.path,
        "github:acme/tools",
        "latest",
        "github:acme/tools#v1.2.0",
    );
    fs::write(
        fixtures.join("default-greet.json"),
        r#"[{"outputs":{"out":"/nix/store/0000000000000000000000000000000a-greet-1.2.0"}}]"#,
    )
    .unwrap();

    let update = jetpack()
        .args(["update", "--no-color", "--yes", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        update.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&update.stderr)
    );
    assert!(
        String::from_utf8_lossy(&update.stderr).contains("Download 240 MB"),
        "stderr: {}",
        String::from_utf8_lossy(&update.stderr)
    );
    let lock = fs::read_to_string(proj.join(".jet/lock")).unwrap();
    assert!(lock.contains("[[source_channel]]"), "lock: {lock}");
    assert!(lock.contains("channel = \"latest\""), "lock: {lock}");
    assert!(
        lock.contains("exact = \"github:acme/tools#v1.2.0\""),
        "lock: {lock}"
    );

    let build = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&build.stderr)
    );
}

#[test]
fn channel_build_without_lock_is_e1271() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { default: github@acme/tools#latest }
    env.dev: Env.{ packages: [default.greet] }
}
"#,
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1271"), "stderr: {stderr}");
    assert!(
        stderr.contains("jetpack update default"),
        "stderr: {stderr}"
    );
}

#[test]
fn channel_update_accepts_main_and_semver_mask() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: {
        trunk: github@acme/tools#main,
        stable: github@acme/tools#v0.x,
    }
    env.dev: Env.{ packages: [trunk.greet, stable.greet] }
}
"#,
    )
    .unwrap();
    fs::create_dir_all(&fixtures.path).unwrap();
    fs::write(
        fixtures.join("channels.txt"),
        "github:acme/tools main github:acme/tools#abc123\n\
         github:acme/tools v0.x github:acme/tools#v0.9.4\n",
    )
    .unwrap();

    let out = jetpack()
        .args(["update", "--no-color", "--yes", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lock = fs::read_to_string(proj.join(".jet/lock")).unwrap();
    assert!(lock.contains("name = \"trunk\""), "lock: {lock}");
    assert!(lock.contains("channel = \"main\""), "lock: {lock}");
    assert!(
        lock.contains("exact = \"github:acme/tools#abc123\""),
        "lock: {lock}"
    );
    assert!(lock.contains("name = \"stable\""), "lock: {lock}");
    assert!(lock.contains("channel = \"v0.x\""), "lock: {lock}");
    assert!(
        lock.contains("exact = \"github:acme/tools#v0.9.4\""),
        "lock: {lock}"
    );
}

#[test]
fn outdated_reports_newer_channel_without_mutating_lock() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { default: github@acme/tools#latest }
    env.dev: Env.{ packages: [default.greet] }
}
"#,
    )
    .unwrap();
    fs::create_dir_all(proj.join(".jet")).unwrap();
    fs::write(
        proj.join(".jet/lock"),
        "version = 1\n\n[[source_channel]]\nname = \"default\"\nchannel = \"latest\"\nexact = \"github:acme/tools#v1.2.0\"\n\n[root]\ndependencies = []\n",
    )
    .unwrap();
    write_channel_fixture(
        &fixtures.path,
        "github:acme/tools",
        "latest",
        "github:acme/tools#v1.3.0",
    );

    let out = jetpack()
        .args(["outdated", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("v1.2.0"), "stderr: {stderr}");
    assert!(stderr.contains("v1.3.0"), "stderr: {stderr}");
    let lock = fs::read_to_string(proj.join(".jet/lock")).unwrap();
    assert!(
        lock.contains("exact = \"github:acme/tools#v1.2.0\""),
        "lock mutated: {lock}"
    );
}

#[test]
fn top_level_jet_outdated_dispatches_to_jetpack() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { default: github@acme/tools#latest }
    env.dev: Env.{ packages: [default.greet] }
}
"#,
    )
    .unwrap();
    fs::create_dir_all(proj.join(".jet")).unwrap();
    fs::write(
        proj.join(".jet/lock"),
        "version = 1\n\n[[source_channel]]\nname = \"default\"\nchannel = \"latest\"\nexact = \"github:acme/tools#v1.2.0\"\n\n[root]\ndependencies = []\n",
    )
    .unwrap();
    write_channel_fixture(
        &fixtures.path,
        "github:acme/tools",
        "latest",
        "github:acme/tools#v1.3.0",
    );

    let out = jet()
        .args(["outdated", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("v1.2.0"), "stderr: {stderr}");
    assert!(stderr.contains("v1.3.0"), "stderr: {stderr}");
}

#[test]
fn add_adapt_prints_snippet_without_editing_env() {
    let proj = Scratch::new("proj");
    let output = jetpack()
        .args(["add", "path:vendor/weirdctl", "--adapt", "--no-color"])
        .current_dir(&proj.path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Pkg.adapt("), "stdout: {stdout}");
    assert!(
        stdout.contains("source: path@vendor/weirdctl"),
        "stdout: {stdout}"
    );
    assert!(!proj.join("env.jet").exists());
}

#[test]
fn named_source_env_resolves_with_pin() {
    // An env that declares a named source `stable` and references it inline as
    // `stable:ripgrep` resolves via the nix provider against the pin. The
    // fixture is keyed by the source name (`stable-ripgrep.json`).
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [\n        pkg.source(\"stable\", \"github:NixOS/nixpkgs/nixos-24.05\");\n        pkg.packages([\"stable:ripgrep\"]);\n    ];\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ripgrep"), "stderr: {stderr}");
}

#[test]
fn unknown_named_source_in_env_is_friendly() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    // References `beta:neovim` but only declares `stable`.
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [\n        pkg.source(\"stable\", \"github:NixOS/nixpkgs/nixos-24.05\");\n        pkg.packages([\"beta:neovim\"]);\n    ];\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not a known source"), "stderr: {stderr}");
    assert!(
        stderr.contains("stable"),
        "should list declared names: {stderr}"
    );
}

/// Build a scratch project whose `env.jet` pulls a first-party `core` package
/// (`hello`) from a local repo. Returns `(base, proj, root)` so a test can run
/// a jetpack command in `proj` with `JETPACK_ROOT=root` and no nix on PATH.
fn core_hello_project(tag: &str) -> (Scratch, PathBuf, PathBuf) {
    let base = Scratch::new(tag);
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    fs::write(
        repo.join("pkg.jet"),
        "payload: { name: \"fixture\", version: \"0.1.0\" }\npackages: { hello: executable }\n",
    )
    .unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        proj.join("env.jet"),
        format!(
            "use jetpack as pkg;\npub fn shell() -> [JSON] {{\n    return [\n        pkg.source(\"mine\", \"path:{}\", \"core\");\n        pkg.packages([\"mine:hello\"]);\n    ];\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();
    (base, proj, root)
}

#[test]
fn jetpack_enter_runs_command_in_project_env() {
    // Gap #6 / U §8 (Scale-2): `jetpack enter` is the project-env command — it
    // never takes an explicit ref, it always composes the env declared by the
    // project `env.jet`. The `-- cmd` form runs a one-off command in the
    // realized env, which is how we prove `enter` put the package on PATH.
    let (base, proj, root) = core_hello_project("enter");
    let output = jetpack()
        // U19: `enter` trust-gates a project that declares packages; `--trust`
        // is the one-shot bypass so this test can assert on PATH composition
        // without exercising the interactive prompt.
        .args(["enter", "--no-color", "--trust", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jet-pkgs"
    );
}

#[test]
fn jet_env_delegates_to_jetpack_enter() {
    // D-DEV4 (ratified 2026-06-17): `jet env` is the friendly Scale-2 front door
    // into the project dev shell — it delegates straight to `jetpack enter`,
    // forwarding flags and the trailing `-- cmd`. (`jet dev` is now reserved for
    // the E2-M4 watch/interpret loop.) Running through the `jet` binary must
    // reach the same composed env.
    let (base, proj, root) = core_hello_project("jet-env");
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        // U19: same trust gate reached through `jet env`; `--trust` bypasses.
        .args(["env", "--no-color", "--trust", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jet-pkgs"
    );
}

// ── U16: -p ad-hoc packages / --flake foreign-flake fallback ──

#[test]
fn enter_dash_p_adds_adhoc_package_with_no_manifest_at_all() {
    // U16: `jet env -p <pkg>... -- cmd` needs no env.jet/pkg.jet at all — the
    // ad-hoc package becomes an ordinary nixpkgs RefSpec, folded into an
    // otherwise-empty plan, trust-gated and realized exactly like a
    // manifest-declared ref.
    let root = Scratch::new("dashp-root");
    let proj = Scratch::new("dashp-proj");
    let fixtures = Scratch::new("dashp-fx");
    let out = Scratch::new("dashp-out");
    write_runnable_fixture(&fixtures.path, &out.path);
    let output = jetpack()
        .args(["enter", "--no-color", "--trust", "--offline", "--fixtures"])
        .arg(&fixtures.path)
        .args(["-p", "greet", "--", "greet"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jetpack"
    );
}

#[test]
fn enter_dash_p_merges_with_project_declared_packages() {
    // The project's own declared package (`hello`, a `core` ref) and the
    // ad-hoc `-p greet` (nixpkgs) both land on PATH in the same shell.
    let (base, proj, root) = core_hello_project("dashp-merge");
    let fixtures = base.join("fixtures");
    let out = base.join("greet-out");
    write_runnable_fixture(&fixtures, &out);
    let output = jetpack()
        .args(["enter", "--no-color", "--trust", "--offline", "--fixtures"])
        .arg(&fixtures)
        .args(["-p", "greet", "--", "sh", "-c", "hello && greet"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello from jet-pkgs"), "stdout: {stdout}");
    assert!(stdout.contains("hello from jetpack"), "stdout: {stdout}");
}

#[test]
fn enter_without_env_jet_or_packages_is_still_nothing_to_do() {
    // The pre-U16 refusal is unchanged when there is truly nothing: no
    // env.jet and no `-p`.
    let root = Scratch::new("nothing-root");
    let proj = Scratch::new("nothing-proj");
    let output = jetpack()
        .args(["enter", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("nothing to do"), "stderr: {stderr}");
}

#[test]
fn enter_flake_detection_ordering_project_env_wins_without_flag() {
    // U16's ordering rule: a project that declares `env.*` (here the
    // Phase-1 directive surface) is never silently swapped for a foreign
    // flake.nix, even when one is present — only `--flake` forces it. Proven
    // here by an offline realize of the *declared* `hello` package
    // succeeding with no `nix` on PATH and no flake.nix ever being touched
    // (a bad flake.nix would fail loudly if `nix develop` ran against it).
    let (base, proj, root) = core_hello_project("flake-ordering");
    fs::write(proj.join("flake.nix"), "this is not valid nix").unwrap();
    let output = jetpack()
        .args(["enter", "--no-color", "--trust", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jet-pkgs"
    );
}

#[test]
fn enter_flake_flag_forces_foreign_flake_and_reports_missing_nix() {
    // `--flake` forces the foreign-flake fallback even though the project
    // declares `env.*`; with no `nix` on PATH this is a clean E1256, not a
    // panic or a raw spawn error.
    let (base, proj, root) = core_hello_project("flake-forced");
    fs::write(proj.join("flake.nix"), "{ }").unwrap();
    let output = jetpack()
        .args(["enter", "--no-color", "--flake"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1256"), "stderr: {stderr}");
    assert!(stderr.contains("nix"), "stderr: {stderr}");
}

#[test]
fn enter_flake_with_no_foreign_flake_present_is_friendly() {
    let root = Scratch::new("flake-none-root");
    let proj = Scratch::new("flake-none-proj");
    let output = jetpack()
        .args(["enter", "--no-color", "--flake"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no foreign flake"), "stderr: {stderr}");
}

#[test]
fn top_level_jet_run_nixpkgs_colon_tool_execs_tool() {
    // U16: `nix run nixpkgs#tool` parity at the public `jet` front door. The
    // top-level spelling uses CLI refs (`nixpkgs:tool`) and lowers to the
    // same jetpack realization path as `jetpack run nixpkgs:tool -- tool`.
    let root = Scratch::new("jet-run-nixpkgs-root");
    let proj = Scratch::new("jet-run-nixpkgs-proj");
    let fixtures = Scratch::new("jet-run-nixpkgs-fx");
    let out = Scratch::new("jet-run-nixpkgs-out");
    write_runnable_fixture(&fixtures.path, &out.path);
    let output = jet()
        .args([
            "run",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jetpack"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("running nixpkgs:greet -> greet").count(),
        1,
        "stderr: {stderr}"
    );
}

// ── U16: `jetpack bridge flake` ──

#[test]
fn bridge_flake_missing_nix_is_e1256_not_a_panic() {
    let dir = Scratch::new("bridge-nonix");
    fs::write(dir.join("flake.nix"), "{ }").unwrap();
    let output = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1256"), "stderr: {stderr}");
}

#[test]
fn bridge_flake_prints_shim_and_warns_on_unmapped_shell_hook() {
    // The best-effort translation: buildInputs become a plain env.dev
    // packages list on stdout; a non-empty shellHook (no env.* equivalent)
    // fires L0204 on stderr without blocking the print.
    let dir = Scratch::new("bridge-shim");
    fs::write(dir.join("flake.nix"), "{ }").unwrap();
    let fixtures = Scratch::new("bridge-shim-fx");
    fs::write(
        fixtures.join("flake-devshell.json"),
        r#"{"buildInputs": ["ripgrep", "fd"], "shellHook": "export FOO=1"}"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["bridge", "flake", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&dir.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("module env.dev {"), "stdout: {stdout}");
    assert!(
        stdout.contains("packages: [fd, ripgrep]"),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("L0204"), "stderr: {stderr}");
    assert!(stderr.contains("shellHook"), "stderr: {stderr}");
}

#[test]
fn bridge_flake_twice_produces_identical_shim_stdout() {
    // Drift-check (U16 plan doc): the bridge is a pure function of the
    // flake's facts, so two runs against the same fixture print
    // byte-identical shims.
    let dir = Scratch::new("bridge-drift");
    fs::write(dir.join("flake.nix"), "{ }").unwrap();
    let fixtures = Scratch::new("bridge-drift-fx");
    fs::write(
        fixtures.join("flake-devshell.json"),
        r#"{"buildInputs": ["nodejs", "ripgrep"], "shellHook": ""}"#,
    )
    .unwrap();
    let run = || {
        jetpack()
            .args(["bridge", "flake", "--no-color", "--fixtures"])
            .arg(&fixtures.path)
            .current_dir(&dir.path)
            .output()
            .unwrap()
    };
    let a = run();
    let b = run();
    assert!(a.status.success());
    assert!(b.status.success());
    assert_eq!(a.stdout, b.stdout);
}

#[test]
fn bridge_flake_no_flake_nix_here_is_friendly() {
    let dir = Scratch::new("bridge-noflake");
    let output = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no flake.nix"), "stderr: {stderr}");
}

#[test]
fn core_provider_runs_first_party_package_without_nix() {
    // R2/U10: a `core` named source realizes a first-party Jet package with no
    // nix anywhere. Package is discovered by module name — no env.jet index.
    let base = Scratch::new("core");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The project declares a `core` named source pointing at the local repo.
    fs::write(
        proj.join("env.jet"),
        format!(
            "use jetpack as pkg;\npub fn shell() -> [JSON] {{\n    return [\n        pkg.source(\"mine\", \"path:{}\", \"core\");\n        pkg.packages([\"mine:hello\"]);\n    ];\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["run", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jet-pkgs"
    );
}

#[test]
fn typed_core_source_inferred_from_pack_jet() {
    // U9/U10: a typed `module { … }` env declares `sources: { mine: path@<dir> }`
    // with no provider marker. The kind is *inferred* from `pkg.jet` in the
    // target → realizes through the first-party `core` provider. U10 Chunk 3:
    // the package is discovered by module name — `module hello` in the source tree
    // — with no `env.jet` index. No nix on PATH proves no nix is involved.
    let base = Scratch::new("typed-core");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    // `pkg.jet` is both the U9 probe marker and the U10 package index.
    fs::write(
        repo.join("pkg.jet"),
        "payload: {\n    name: \"jet-pkgs\",\n    version: \"0.1.0\",\n}\npackages: {\n    hello: executable,\n}\n",
    )
    .unwrap();
    // The `module hello` declaration is the U10 Chunk 3 discovery target — no
    // `env.jet` pkg.package index needed anymore (dual marker retired).
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The typed env declares the source with no `via`/`core` marker — just
    // `provider@target`. `mine.hello` is the Pkg sugar → `mine:hello`.
    fs::write(
        proj.join("env.jet"),
        format!(
            "module dev {{\n    sources: {{ mine: path@{} }}\n    env.dev: Env.{{\n        packages: [mine.hello],\n    }}\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["run", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jet-pkgs"
    );
}

#[test]
fn core_provider_builds_library_package_without_nix() {
    // U10 Chunk 4: a `library` package realizes through the `core` provider
    // (no nix), staging its module source. Unlike an `executable`, it puts no
    // `bin/` on PATH — but `jetpack build` realizes it just the same. The kind
    // comes from the repo's `pkg.jet` `packages:` index.
    let base = Scratch::new("core-library");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let lib_pkg = repo.join("lib/mathlib");
    fs::create_dir_all(&lib_pkg).unwrap();
    fs::create_dir_all(&proj).unwrap();
    // `pkg.jet` declares the package as a `library` (the kind index).
    fs::write(
        repo.join("pkg.jet"),
        "payload: {\n    name: \"jet-pkgs\",\n    version: \"0.1.0\",\n}\npackages: {\n    mathlib: library,\n}\n",
    )
    .unwrap();
    // The library's source: a `module mathlib` discovered by name (Chunk 3),
    // with no `bin/` — it is imported for its code, not installed on PATH.
    fs::write(
        lib_pkg.join("mathlib.jet"),
        "module mathlib {\n    pub fn add(a: Int, b: Int) -> Int { return a + b }\n}\n",
    )
    .unwrap();
    // A typed env references the library package; the source kind is inferred
    // from `pkg.jet` → core.
    fs::write(
        proj.join("env.jet"),
        format!(
            "module dev {{\n    sources: {{ mine: path@{} }}\n    env.dev: Env.{{\n        packages: [mine.mathlib],\n    }}\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("built 1 package(s)"),
        "expected build success status, got: {stderr}"
    );
}

#[test]
fn committed_example_builds_offline_end_to_end() {
    // I5: the committed jetpack project fixture is the executable spec for
    // a real env.jet. `jetpack build` with no ref reads env.jet and realizes
    // everything it declares — nix-backed named sources (`stable:ripgrep`,
    // `unstable:neovim`) resolved from the committed fixtures, plus a
    // first-party `mine:hello` realized through the `core` provider with no
    // nix. The whole thing runs fully offline. The store lives under a scratch
    // JETPACK_ROOT, so nothing is written back into the example dir.
    let root = Scratch::new("example-e2e");
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("building completed 0/3 · current: stable -> ripgrep · resolving")
            && stderr.contains("building completed 1/3 · current: unstable -> neovim · resolving")
            && stderr.contains("building completed 2/3 · current: mine -> hello · resolving"),
        "plain non-TTY output must preserve ordered source-to-package edges: {stderr}"
    );
    for pkg in ["ripgrep", "neovim", "hello"] {
        assert!(
            stderr.contains(pkg),
            "expected `{pkg}` in build output: {stderr}"
        );
    }
    assert!(stderr.contains("built 3 package(s)"), "stderr: {stderr}");
}

#[test]
fn failed_first_dependency_reports_zero_completed_nodes() {
    let (_base, proj, root) = core_hello_project("progress-first-failure");
    let env_path = proj.join("env.jet");
    let env = fs::read_to_string(&env_path)
        .unwrap()
        .replace("[\"mine:hello\"]", "[\"mine:missing\", \"mine:hello\"]");
    fs::write(&env_path, env).unwrap();
    let out = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("building completed 0/2 · current: mine -> missing · resolving"),
        "first failure must not claim completion: {stderr}"
    );
    assert!(!stderr.contains("building completed 1/2 · current: mine -> missing"));
    // Region erased before diagnostic: a verbatim error block follows the
    // dependency-status line (D-FE-CLI1 failure rule / hybrid.html still 8).
    assert!(
        stderr.contains("error:") || stderr.to_lowercase().contains("could not"),
        "failure must print a diagnostic after erasing the live region: {stderr}"
    );
}

#[test]
fn typed_module_example_builds_offline_end_to_end() {
    // I5: the committed jetpack-typed fixture is the executable spec
    // for the typed `module { … }` env surface (U3/U6/U8) including U4 import-tree
    // discovery. `jetpack build` with no ref evaluates env.jet through `modeval`:
    // the `default` source merges to its pinned nixpkgs upstream,
    // `default.[ripgrep, fd]` expands to two `Pkg` refs, and `imports:
    // find("./modules")` walks `modules/tools.jet` and folds its `default.jq`
    // into the same merge. All three realize from the committed fixtures, fully
    // offline. The store lives under a scratch JETPACK_ROOT, so nothing is
    // written back.
    let typed_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-typed");
    let root = Scratch::new("typed-e2e");
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&typed_dir)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", typed_dir.join("fixtures"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for pkg in ["ripgrep", "fd", "jq"] {
        assert!(
            stderr.contains(pkg),
            "expected `{pkg}` in build output: {stderr}"
        );
    }
    assert!(stderr.contains("built 3 package(s)"), "stderr: {stderr}");
}

#[test]
fn core_provider_fetches_remote_git_package_from_env() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("note: skipping remote core provider integration test (git not found)");
        return;
    }

    let base = Scratch::new("core-remote");
    let repo = base.join("remote");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from remote jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }

    for args in [
        vec!["init"],
        vec!["config", "user.email", "jetpack@example.invalid"],
        vec!["config", "user.name", "Jetpack Test"],
        vec!["add", "."],
        vec!["commit", "-m", "init"],
    ] {
        let out = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fs::write(
        proj.join("env.jet"),
        format!(
            "use jetpack as pkg;\npub fn shell() -> [JSON] {{\n    return [\n        pkg.source(\"mine\", \"file://{}#HEAD\", \"core\");\n        pkg.packages([\"mine:hello\"]);\n    ];\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["run", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from remote jet-pkgs"
    );
    assert!(
        root.join("sources").is_dir(),
        "remote source cache was not created"
    );

    let offline = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(
        offline.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&offline.stderr)
    );
}

// ── E7 jetos runtime: `jet os <verb> <host>` / `path@host` ─────────

/// The committed jetpack-config fixture dir.
fn config_example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-config")
}

fn assert_jetos_stderr_snapshot(name: &str, stderr: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jetpack-diagnostics")
        .join(format!("{name}.stderr"));
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing jetos diagnostic snapshot {}: {e}", path.display()));
    assert_eq!(
        stderr, expected,
        "jetos diagnostic snapshot `{name}` changed"
    );
}

fn assert_jetos_stderr_snapshot_trimmed(name: &str, stderr: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jetpack-diagnostics")
        .join(format!("{name}.stderr"));
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing jetos diagnostic snapshot {}: {e}", path.display()));
    assert_eq!(
        stderr.trim_end(),
        expected.trim_end(),
        "jetos diagnostic snapshot `{name}` changed"
    );
}

fn assert_jetos_stderr_snapshot_normalized(name: &str, stderr: &str, replacements: &[(&str, &str)]) {
    let mut normalized = stderr.to_string();
    for (from, to) in replacements {
        normalized = normalized.replace(from, to);
    }
    assert_jetos_stderr_snapshot(name, &normalized);
}

fn write_cachyos_source_recipe(pkg: &Path) {
    let source = pkg.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("recipe.jet"),
        "kernel cachyos { source: \"cachyos/linux-cachyos\", build: source }\n",
    )
    .unwrap();
    write_executable(&source.join("build.sh"), "#!/bin/sh\nexit 0\n");
    fs::write(
        source.join("config"),
        "CONFIG_CACHYOS=y\nCONFIG_EXT4_FS=y\n",
    )
    .unwrap();
    fs::write(source.join("patches.manifest"), "cachyos-scheduler.patch\n").unwrap();
    fs::write(
        source.join("initrd-inputs.manifest"),
        "busybox\nkmod\nsystemd\n",
    )
    .unwrap();
}

fn write_cachyos_source_builder(pkg: &Path, body: &str) {
    write_executable(&pkg.join("source/build.sh"), body);
}

fn write_bootlike_cachyos_artifacts(pkg: &Path) {
    fs::create_dir_all(pkg.join("boot")).unwrap();
    fs::write(pkg.join("boot/vmlinuz-cachyos"), "MZ test kernel\nHdrS\n").unwrap();
    fs::write(pkg.join("boot/initrd-cachyos"), "070701 test initrd\n").unwrap();
}

#[test]
fn os_import_writes_semantic_nixos_facts_with_audit() {
    let src = Scratch::new("os-import-src");
    fs::write(
        src.join("jetos-import-facts.json"),
        r#"{
  "host": "halcyon",
  "target": "linux.x64",
  "nixpkgs": "github@NixOS/nixpkgs/nixos-24.05",
  "packages": ["git", "ripgrep", "jetbrains.idea-ultimate"],
  "services": ["openssh", "pipewire"],
  "options": {
    "network.hostName": "halcyon",
    "services.openssh.enable": true,
    "boot.loader": ".Limine"
  },
  "flakePartsModules": ["./nix/hosts/halcyon.nix"],
  "homeManagerModules": ["./home/nate.nix"],
  "users": [
    {
      "name": "nate",
      "home": "/home/nate",
      "groups": ["wheel"],
      "packages": ["neovim", "ghostty"],
      "homeManager": true
    }
  ],
  "omissions": ["programs.firefox.profiles need profile-specific Canvas editing"]
}"#,
    )
    .unwrap();
    let out_dir = Scratch::new("os-import-out");
    let out = jet()
        .args([
            "os",
            "import",
            src.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--user",
            "nate",
            "--write",
            "--out",
            out_dir.path.to_str().unwrap(),
            "--no-color",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = fs::read_to_string(out_dir.join("config.jet")).unwrap();
    assert!(config.contains("system.halcyon"), "{config}");
    assert!(config.contains("nixpkgs: github@NixOS/nixpkgs/nixos-24.05"), "{config}");
    assert!(config.contains("packages: [nixpkgs.[git, ripgrep]]"), "{config}");
    assert!(config.contains("openssh: { enable: true"), "{config}");
    assert!(config.contains("user.nate.packages: [nixpkgs.[neovim, ghostty]]"), "{config}");
    assert!(config.contains("user.nate.homeManager: true"), "{config}");
    let audit = fs::read_to_string(out_dir.join("jetos-import-audit.json")).unwrap();
    assert!(audit.contains("\"mode\":\"semantic-facts\""), "{audit}");
    assert!(audit.contains("jetbrains.idea-ultimate"), "{audit}");
    assert!(audit.contains("programs.firefox.profiles"), "{audit}");
}

/// Stage a flake-root fixture plus a stub `nix` on PATH whose `eval` prints
/// the canned live-extractor result (or fails when `output` is None).
fn write_live_import_fixture(src: &Path, tools: &Path, output: Option<&str>) {
    fs::write(
        src.join("flake.nix"),
        "{\n  outputs = { ... }: { nixosConfigurations.halcyon = { }; };\n}\n",
    )
    .unwrap();
    fs::write(
        src.join("flake.lock"),
        r#"{"nodes":{"nixpkgs":{"locked":{"owner":"NixOS","repo":"nixpkgs","rev":"fef9403a3e4d31b0a23f0bacebbec52c248fbb51"}}}}"#,
    )
    .unwrap();
    fs::create_dir_all(tools).unwrap();
    let body = match output {
        Some(json) => format!(
            "#!/bin/sh\ncase \" $* \" in\n  *' eval '*) printf '%s\\n' '{json}'; exit 0 ;;\nesac\nexit 0\n"
        ),
        None => "#!/bin/sh\necho 'error: attribute missing' >&2\nexit 1\n".to_string(),
    };
    write_executable(&tools.join("nix"), &body);
}

const LIVE_IMPORT_EVAL_JSON: &str = r#"{"host":"halcyon","stateVersion":"26.05","tz":"America/New_York","locale":"en_US.UTF-8","keyboard":"us","desktopGnome":false,"desktopPlasma":true,"dmGdm":false,"dmSddm":true,"loaderLimine":true,"loaderSystemdBoot":false,"efiTouch":false,"kernelName":"linux-cachyos","kernelParams":["quiet"],"sysctl":{"vm.swappiness":10},"firewallTcp":[22,443],"firewallUdp":[53317],"nameservers":["1.1.1.1"],"networkmanager":true,"zramEnable":true,"zramPercent":25,"svcOpenssh":true,"svcPipewire":true,"svcRtkit":true,"svcTailscale":true,"svcLibvirtd":true,"svcDocker":true,"svcFlatpak":false,"svcSteam":true,"svcGamemode":true,"svcPcscd":true,"svcBluetooth":false,"stylix":true,"packages":["git","ripgrep","jetbrains.idea-ultimate"],"users":[{"name":"nate","home":"/home/nate","groups":["wheel","networkmanager"],"shell":"fish"}],"hm":[{"name":"nate","packages":["ghostty"],"programs":["git","starship"]}]}"#;

#[test]
fn os_import_live_recovers_package_provenance_from_flake_inputs() {
    let src = Scratch::new("os-import-provenance-src");
    let tools = Scratch::new("os-import-provenance-tools");
    fs::write(
        src.join("flake.nix"),
        "{\n  outputs = { ... }: { nixosConfigurations.halcyon = { }; };\n}\n",
    )
    .unwrap();
    fs::write(
        src.join("flake.lock"),
        r#"{
  "nodes": {
    "nixpkgs": {"locked": {"owner": "NixOS", "repo": "nixpkgs", "rev": "fef9403a3e4d31b0a23f0bacebbec52c248fbb51"}},
    "zen-beta": {"locked": {"owner": "0xc000022070", "repo": "zen-browser-flake", "rev": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}
  }
}"#,
    )
    .unwrap();
    // Live extractor returns an external package; nixpkgs probe reports it
    // unresolvable; zen-beta probe reports it resolved (empty unresolvable list).
    let live = r#"{"host":"halcyon","stateVersion":"26.05","tz":"UTC","locale":"en_US.UTF-8","keyboard":"us","desktopGnome":false,"desktopPlasma":false,"dmGdm":false,"dmSddm":false,"loaderLimine":true,"loaderSystemdBoot":false,"efiTouch":false,"kernelName":"linux","kernelParams":[],"sysctl":{},"firewallTcp":[],"firewallUdp":[],"nameservers":[],"networkmanager":false,"zramEnable":false,"zramPercent":0,"svcOpenssh":false,"svcPipewire":false,"svcRtkit":false,"svcTailscale":false,"svcLibvirtd":false,"svcDocker":false,"svcFlatpak":false,"svcSteam":false,"svcGamemode":false,"svcPcscd":false,"svcBluetooth":false,"packages":["git","zen-browser"],"users":[],"hm":[]}"#;
    let stub = format!(
        r#"#!/bin/sh
# Package resolvability probe uses getFlake + resolves; live extractor uses --apply.
case " $* " in
  *'getFlake'*'0xc000022070'*|*'getFlake'*'zen-browser-flake'*)
    printf '%s\n' '[]'
    exit 0
    ;;
  *'getFlake'*)
    printf '%s\n' '["zen-browser"]'
    exit 0
    ;;
  *'--apply'*)
    printf '%s\n' '{live}'
    exit 0
    ;;
esac
exit 0
"#
    );
    fs::create_dir_all(&tools.path).unwrap();
    write_executable(&tools.join("nix"), &stub);
    let out = jet()
        .args([
            "os",
            "import",
            src.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--no-color",
        ])
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = String::from_utf8_lossy(&out.stdout);
    assert!(
        config.contains("zen_beta: github@0xc000022070/zen-browser-flake/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        "extra source must be pinned from flake.lock:\n{config}"
    );
    assert!(
        config.contains("zen_beta.[zen-browser]")
            || config.contains("packages: [nixpkgs.[git], zen_beta.[zen-browser]]"),
        "recovered package must be sourced from zen_beta:\n{config}"
    );
    assert!(
        !config.contains("package-provenance import will recover it"),
        "recovered packages must not stay as deferred omissions:\n{config}"
    );
}

#[test]
fn os_import_live_semantic_eval_maps_real_options() {
    let src = Scratch::new("os-import-live-src");
    let tools = Scratch::new("os-import-live-tools");
    write_live_import_fixture(&src.path, &tools.path, Some(LIVE_IMPORT_EVAL_JSON));
    let out = jet()
        .args([
            "os",
            "import",
            src.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--no-color",
        ])
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = String::from_utf8_lossy(&out.stdout);
    assert!(config.contains("nixpkgs: github@NixOS/nixpkgs/fef9403a3e4d31b0a23f0bacebbec52c248fbb51"), "{config}");
    assert!(config.contains("network.hostName: \"halcyon\""), "{config}");
    assert!(config.contains("network.networkmanager.enable: true"), "{config}");
    assert!(config.contains("network.firewall.allowedTcpPorts: [22, 443]"), "{config}");
    assert!(config.contains("filesystem.timeZone: \"America/New_York\""), "{config}");
    assert!(config.contains("boot.loader: .Limine"), "{config}");
    assert!(config.contains("boot.kernel: .CachyOS"), "{config}");
    assert!(config.contains("services.desktop.plasma.enable: true"), "{config}");
    assert!(config.contains("services.displayManager: \"sddm\""), "{config}");
    assert!(config.contains("services.audio.pipewire.enable: true"), "{config}");
    assert!(config.contains("services.virtualization.libvirtd.enable: true"), "{config}");
    assert!(config.contains("services.gaming.steam.enable: true"), "{config}");
    assert!(config.contains("performance.sysctl.vm.swappiness: 10"), "{config}");
    assert!(config.contains("performance.zram.memoryPercent: 25"), "{config}");
    assert!(config.contains("users.nate.shell: nixpkgs.fish"), "{config}");
    assert!(config.contains("user.nate.homeManager: true"), "{config}");
    assert!(config.contains("apps.program.git.enable: true"), "{config}");
    assert!(config.contains("apps.program.starship.enable: true"), "{config}");
    assert!(
        config.contains("services.virtualization.docker.enable: true"),
        "{config}"
    );
    assert!(config.contains("packages: [nixpkgs.[git, ripgrep]]"), "{config}");
    assert!(config.contains("openssh: { enable: true"), "{config}");
    assert!(config.contains("tailscale: { enable: true"), "{config}");
}

#[test]
fn os_import_live_semantic_eval_reports_omissions() {
    let src = Scratch::new("os-import-live-audit-src");
    let tools = Scratch::new("os-import-live-audit-tools");
    write_live_import_fixture(&src.path, &tools.path, Some(LIVE_IMPORT_EVAL_JSON));
    let out_dir = Scratch::new("os-import-live-audit-out");
    let out = jet()
        .args([
            "os",
            "import",
            src.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--write",
            "--out",
            out_dir.path.to_str().unwrap(),
            "--no-color",
        ])
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let audit = fs::read_to_string(out_dir.join("jetos-import-audit.json")).unwrap();
    assert!(audit.contains("\"mode\":\"semantic-eval\""), "{audit}");
    assert!(audit.contains("jetbrains.idea-ultimate"), "{audit}");
    assert!(
        audit.contains("no `nix-cachyos-kernel` pin"),
        "{audit}"
    );
    assert!(
        audit.contains("stylix theming is enabled upstream"),
        "{audit}"
    );
    assert!(
        !audit.contains("virtualisation.docker.enable has no jetos option"),
        "docker must map to services.virtualization.docker.enable, not omit: {audit}"
    );
    assert!(
        !audit.contains("Home Manager program `starship`"),
        "known HM programs must map to apps.program.*, not omit: {audit}"
    );
}

#[test]
fn os_import_live_eval_failure_is_loud() {
    let src = Scratch::new("os-import-live-fail-src");
    let tools = Scratch::new("os-import-live-fail-tools");
    write_live_import_fixture(&src.path, &tools.path, None);
    let out = jet()
        .args([
            "os",
            "import",
            src.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--no-color",
        ])
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1289"), "{stderr}");
    assert!(stderr.contains("attribute missing"), "{stderr}");
    assert!(stderr.contains("--facts-only"), "{stderr}");
}

#[test]
fn os_import_missing_source_has_snapshot() {
    let out = jet()
        .args([
            "os",
            "import",
            "/definitely/not/here",
            "--host",
            "halcyon",
            "--no-color",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_jetos_stderr_snapshot(
        "import_missing_source",
        &String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn os_lift_is_audited_facts_only_import_draft() {
    let src = Scratch::new("os-lift-src");
    fs::write(
        src.join("flake.nix"),
        r#"{
  inputs.flake-parts.url = "github:hercules-ci/flake-parts";
  inputs.home-manager.url = "github:nix-community/home-manager";
  outputs = { self, nixpkgs, ... }: {
    nixosConfigurations = { laptop = nixpkgs.lib.nixosSystem {}; };
  };
}"#,
    )
    .unwrap();
    let out = jet()
        .args([
            "os",
            "lift",
            "laptop",
            src.path.to_str().unwrap(),
            "--no-color",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("Generated by `jet os import`"), "{stdout}");
    assert!(stdout.contains("system.laptop"), "{stdout}");
    assert!(stderr.contains("facts-only"), "{stderr}");
}

#[test]
fn os_build_realizes_selected_system_offline() {
    // I5/D-JPK-OSVERB1/D-JPK-OSHOST1: `jet os build <host>` loads config.jet
    // from the current repo, selects system.<host>, and realizes its packages
    // into a named generation.
    // System named <host>, and realizes its packages into a system generation —
    // fully offline (the packages come from a first-party `core` source repo, so
    // no nix). The store lives under a scratch JETPACK_ROOT.
    let root = Scratch::new("os-build-root");
    let run = || {
        jet()
            .args([
                "os",
                "build",
                "halcyon",
                "--name",
                "fixture-source-built",
                "--no-color",
                "--offline",
            ])
            .current_dir(config_example_dir())
            .env("JETPACK_ROOT", &root.path)
            .env("PATH", "/usr/bin:/bin") // no nix on PATH
            .output()
            .unwrap()
    };
    let out = run();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.lines().any(|line| {
            line.contains("building system")
                && line.contains(" -> hello · resolving")
                && !line.contains('\u{1b}')
        }),
        "plain jetos build must project its real package edge without ANSI: {stderr}"
    );
    for pkg in ["hello", "btop"] {
        assert!(stderr.contains(pkg), "expected `{pkg}` in output: {stderr}");
    }
    assert!(stderr.contains("halcyon"), "stderr: {stderr}");
    assert!(stderr.contains("generation"), "stderr: {stderr}");
    let cached = run();
    assert!(
        cached.status.success(),
        "cached generation stderr: {}",
        String::from_utf8_lossy(&cached.stderr)
    );
    assert!(
        String::from_utf8_lossy(&cached.stderr).contains("cached"),
        "second generation must exercise leased cache paths: {}",
        String::from_utf8_lossy(&cached.stderr)
    );
    // A generation directory was assembled under the managed system store.
    assert!(
        root.join("systems").is_dir(),
        "expected a systems dir under the root"
    );
    let generation = root.join("systems/generations/fixture-source-built");
    let kernel = fs::read_to_string(generation.join("boot/kernel")).unwrap();
    let initrd = fs::read_to_string(generation.join("boot/initrd")).unwrap();
    assert!(
        kernel.contains("fixture-built cachyos kernel"),
        "kernel should come from source/build.sh: {kernel}"
    );
    assert!(
        initrd.contains("fixture-built cachyos initrd"),
        "initrd should come from source/build.sh: {initrd}"
    );
    assert_no_ephemeral_links(&generation);
    let hello = Command::new(generation.join("sw/bin/hello")).output().unwrap();
    assert!(hello.status.success());
    assert!(
        String::from_utf8_lossy(&hello.stdout).contains("hello"),
        "generation-owned executable must survive lease close and FD reuse"
    );
}

#[test]
fn os_switch_activates_and_sets_current() {
    // U15: `switch` builds the generation, then activates it — flips a `current`
    // pointer (and a boot `default`). The internal mechanic is a symlink in the
    // managed system store; the user sees a clear "activated" line.
    let root = Scratch::new("os-switch-root");
    let out = jet()
        .args([
            "os",
            "switch",
            "halcyon",
            "--name",
            "known-good",
            "--no-color",
            "--yes",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("activated"), "stderr: {stderr}");
    assert!(stderr.contains("known-good"), "stderr: {stderr}");
    // The `current` pointer now exists.
    let current = root.join("systems").join("current");
    assert!(
        current.exists(),
        "expected a `current` generation pointer at {}",
        current.display()
    );
    let generation = root.join("systems/generations/known-good");
    assert!(
        generation
            .join("etc/systemd/system/openssh.service")
            .is_file(),
        "expected generated systemd unit"
    );
    assert!(
        generation.join("sw/bin/hello").exists(),
        "expected hello in the system package closure"
    );
    assert!(
        generation.join("sw/bin/btop").exists(),
        "expected btop in the system package closure"
    );
    assert!(
        generation.join("sw/bin/systemd").exists(),
        "expected systemd in the system package closure"
    );
    assert!(
        generation.join("sw/bin/gdm").exists()
            && generation.join("sw/bin/gnome-session").exists()
            && generation.join("sw/bin/gnome-shell").exists(),
        "expected GNOME desktop commands in the system package closure"
    );
    assert_eq!(
        fs::read_to_string(generation.join("etc/hostname")).unwrap(),
        "halcyon\n"
    );
    assert_eq!(
        fs::read_to_string(generation.join("etc/timezone")).unwrap(),
        "Europe/London\n"
    );
    let fstab = fs::read_to_string(generation.join("etc/fstab")).unwrap();
    assert!(fstab.contains("jetos-root"), "fstab: {fstab}");
    assert!(
        fstab.contains("/dev/disk/by-label/swap\tnone\tswap\tpri=5"),
        "fstab: {fstab}"
    );
    let diff = fs::read_to_string(generation.join("activation-diff.txt")).unwrap();
    assert!(diff.contains("packages: 7"), "diff: {diff}");
    assert!(diff.contains("services: 3"), "diff: {diff}");
    let health = fs::read_to_string(generation.join("health-checks.txt")).unwrap();
    assert!(health.contains("openssh"), "health: {health}");
    assert!(health.contains("backup"), "health: {health}");
    assert!(health.contains("metrics"), "health: {health}");
    let provenance = fs::read_to_string(generation.join("provenance.json")).unwrap();
    assert!(provenance.contains("\"hello\""), "provenance: {provenance}");
    assert!(
        provenance.contains("\"cachyos-kernel\""),
        "provenance: {provenance}"
    );
    assert!(
        provenance.contains("core-source"),
        "provenance: {provenance}"
    );
    assert!(
        provenance.contains("packages.overlay.nixpkgs"),
        "provenance should expose compatibility escape hatches: {provenance}"
    );
    assert!(
        provenance.contains("\"bootstrap\":\"source-built\""),
        "provenance should record source-built CachyOS bootstrap: {provenance}"
    );
    let passwd = fs::read_to_string(generation.join("etc/passwd")).unwrap();
    assert!(passwd.contains("nate:x:1000"), "passwd: {passwd}");
    assert!(
        passwd.contains("/run/current-system/sw/bin/hello"),
        "passwd: {passwd}"
    );
    let group = fs::read_to_string(generation.join("etc/group")).unwrap();
    assert!(group.contains("wheel:x:2000:nate"), "group: {group}");
    let sysusers = fs::read_to_string(generation.join("etc/sysusers.d/jetos.conf")).unwrap();
    assert!(sysusers.contains("u nate 1000"), "sysusers: {sysusers}");
    assert!(sysusers.contains("g wheel 2000"), "sysusers: {sysusers}");
    let shells = fs::read_to_string(generation.join("etc/shells")).unwrap();
    assert!(
        shells.contains("/run/current-system/sw/bin/hello"),
        "shells: {shells}"
    );
    let profile = fs::read_to_string(generation.join("etc/profile")).unwrap();
    assert!(
        profile.contains("/run/current-system/sw/bin")
            && profile.contains("export JETOS_BRAND=JetOS")
            && profile.contains("export JETOS_PROMPT='JetOS halcyon'")
            && profile.contains("\\033[1;36m\\]JetOS"),
        "profile: {profile}"
    );
    let issue = fs::read_to_string(generation.join("etc/issue")).unwrap();
    assert!(
        issue.contains("JetOS halcyon") && issue.contains("proof-backed system shell"),
        "issue: {issue}"
    );
    let motd = fs::read_to_string(generation.join("etc/motd")).unwrap();
    assert!(
        motd.contains("JetOS halcyon") && motd.contains("source-owned, proof-backed"),
        "motd: {motd}"
    );
    let terminal = fs::read_to_string(generation.join("terminal/facts.json")).unwrap();
    assert!(
        terminal.contains("\"login_user\":\"nate\"")
            && terminal.contains("\"serial_tty\":\"ttyS0\"")
            && terminal.contains("\"prompt\":\"JetOS halcyon $ \"")
            && terminal.contains("terminal-login-ready"),
        "terminal: {terminal}"
    );
    assert!(
        generation
            .join("etc/systemd/system/serial-getty@ttyS0.service")
            .is_file(),
        "expected serial getty unit"
    );
    assert!(
        generation
            .join("etc/systemd/system/getty.target.wants/serial-getty@ttyS0.service")
            .exists(),
        "expected serial getty enabled"
    );
    let boot = fs::read_to_string(generation.join("boot/facts.json")).unwrap();
    assert!(boot.contains("\"loader\":\"Limine\""), "boot: {boot}");
    assert!(boot.contains("\"kernel\":\"CachyOS\""), "boot: {boot}");
    assert!(
        boot.contains("\"kernel_package\""),
        "boot facts should name the realized kernel package: {boot}"
    );
    assert!(
        boot.contains("\"output_hash\""),
        "boot facts should carry kernel provenance hash: {boot}"
    );
    assert!(
        boot.contains("\"source_recipe\""),
        "boot facts should carry source recipe provenance: {boot}"
    );
    assert!(
        boot.contains("\"sha256\""),
        "boot facts should hash kernel recipe inputs: {boot}"
    );
    let kernel = fs::read_to_string(generation.join("boot/kernel")).unwrap();
    assert!(
        kernel.contains("MZ fixture-built cachyos kernel") && kernel.contains("HdrS"),
        "kernel artifact: {kernel}"
    );
    assert!(
        generation.join("boot/limine.conf").is_file(),
        "expected Limine config"
    );
    let network = fs::read_to_string(generation.join("network/facts.json")).unwrap();
    assert!(
        network.contains("\"interface\":\"enp0s1\""),
        "network: {network}"
    );
    assert!(
        network.contains("\"firewall_allowed_tcp_ports\":[\"22\",\"443\"]"),
        "network: {network}"
    );
    let networkd =
        fs::read_to_string(generation.join("etc/systemd/network/10-jetos.network")).unwrap();
    assert!(
        networkd.contains("Address=192.0.2.10/24"),
        "networkd: {networkd}"
    );
    let nft = fs::read_to_string(generation.join("etc/nftables/jetos-firewall.nft")).unwrap();
    assert!(nft.contains("tcp dport { 22, 443 } accept"), "nft: {nft}");
    let init = fs::read_to_string(generation.join("init/systemd.json")).unwrap();
    assert!(init.contains("graphical.target"), "init: {init}");
    assert!(init.contains("\"systemd\""), "init: {init}");
    assert!(
        generation.join("sbin/init").exists(),
        "expected bootable /sbin/init projection"
    );
    assert!(
        generation
            .join("usr/lib/systemd/system/graphical.target")
            .exists()
            && generation
                .join("systemd/lib/systemd/system/graphical.target")
                .exists()
            && generation
                .join("etc/systemd/system/graphical.target")
                .exists()
            && generation
                .join("usr/lib/systemd/system/rescue.target")
                .exists()
            && generation.join("etc/systemd/system/default.target").exists(),
        "expected base systemd target units in bootable generation"
    );
    assert!(
        generation.join("root/etc/hostname").is_file(),
        "expected root-shaped /etc projection"
    );
    assert!(
        generation.join("root/boot/kernel").is_file(),
        "expected root-shaped /boot projection"
    );
    assert!(
        generation.join("root/sbin/init").exists(),
        "expected root-shaped /sbin/init projection"
    );
    assert!(
        generation
            .join("root/run/current-system/etc/systemd/system/openssh.service")
            .is_file(),
        "expected current-system projection inside root"
    );
    assert!(
        generation.join("root/home/nate/.profile").is_file(),
        "expected user home profile in installed root"
    );
    let user_profile = fs::read_to_string(generation.join("users/nate/profile.json")).unwrap();
    assert!(
        user_profile.contains("\"kind\":\"jetos.user-generation\"")
            && user_profile.contains("\"user\":\"nate\"")
            && user_profile.contains("\"syncthing\""),
        "user profile: {user_profile}"
    );
    assert!(
        generation
            .join("etc/systemd/user/jetos-user-nate.service")
            .is_file(),
        "expected user environment unit"
    );
    assert!(
        generation.join("sw/bin/jetos-user-apply").is_file(),
        "expected standalone user apply helper"
    );
    let user_home = root.path.join("applied-home");
    let user_apply = Command::new(generation.join("sw/bin/jetos-user-apply"))
        .arg("nate")
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_USER_HOME", &user_home)
        .output()
        .unwrap();
    assert!(
        user_apply.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&user_apply.stdout),
        String::from_utf8_lossy(&user_apply.stderr)
    );
    let ghostty = fs::read_to_string(user_home.join(".config/ghostty/config")).unwrap();
    assert!(
        ghostty.contains("managed-by=jetos") && ghostty.contains("home/ghostty/config"),
        "ghostty: {ghostty}"
    );
    assert!(
        user_home.join(".jetos/profile/bin/hello").exists(),
        "expected per-user package profile link"
    );
    assert!(
        user_home
            .join(".config/systemd/user/syncthing.service")
            .is_file(),
        "expected per-user service unit"
    );
    let user_apply_proof =
        fs::read_to_string(user_home.join(".jetos/proof/user-nate.json")).unwrap();
    assert!(
        user_apply_proof.contains("\"state\":\"applied\""),
        "user_apply_proof: {user_apply_proof}"
    );
    assert!(
        generation
            .join("root/run/current-system/terminal/facts.json")
            .is_file(),
        "expected terminal proof facts in current-system projection"
    );
    assert!(
        generation.join("etc/systemd/system/backup.timer").is_file(),
        "expected backup timer"
    );
    assert!(
        generation
            .join("etc/systemd/system/timers.target.wants/backup.timer")
            .exists(),
        "expected enabled backup timer"
    );
    assert!(
        generation
            .join("etc/systemd/system/metrics.socket")
            .is_file(),
        "expected metrics socket"
    );
    assert!(
        generation
            .join("etc/systemd/system/sockets.target.wants/metrics.socket")
            .exists(),
        "expected enabled metrics socket"
    );
    assert!(
        generation
            .join("etc/systemd/system/display-manager.service")
            .is_file(),
        "expected display manager unit"
    );
    assert!(
        generation
            .join("etc/systemd/system/graphical.target.wants/display-manager.service")
            .exists(),
        "expected enabled display manager"
    );
    assert!(
        generation
            .join("etc/systemd/system/multi-user.target.wants/openssh.service")
            .exists(),
        "expected enabled openssh service"
    );
    let hardware = fs::read_to_string(generation.join("hardware/facts.json")).unwrap();
    assert!(hardware.contains("iwlwifi"), "hardware: {hardware}");
    assert!(hardware.contains("amdgpu"), "hardware: {hardware}");
    assert!(
        hardware.contains("framework-13-amd"),
        "hardware: {hardware}"
    );
    assert!(
        hardware.contains("jetos.hardware") && hardware.contains("jetos-hardware-doctor"),
        "hardware: {hardware}"
    );
    assert!(
        generation.join("hardware/halcyon.jet").is_file()
            && generation
                .join("hardware/profile-framework-13-amd.json")
                .is_file()
            && generation
                .join("boot/specialisations/plasmaBeta.conf")
                .is_file()
            && generation.join("sw/bin/jetos-hardware-scan").is_file()
            && generation.join("sw/bin/jetos-hardware-doctor").is_file(),
        "expected hardware scan/profile/specialisation artifacts"
    );
    let specialisation = fs::read_to_string(
        generation.join("boot/specialisations/plasmaBeta.conf"),
    )
    .unwrap();
    assert!(
        specialisation.contains("title jetos 26.10 (Apex) — halcyon (plasmaBeta)"),
        "specialisation title: {specialisation}"
    );
    let hardware_root = root.path.join("fake-hardware");
    fs::create_dir_all(hardware_root.join("proc")).unwrap();
    fs::create_dir_all(hardware_root.join("sys/class/block/nvme0n1")).unwrap();
    fs::create_dir_all(hardware_root.join("sys/class/drm/card0")).unwrap();
    fs::write(
        hardware_root.join("proc/modules"),
        "amdgpu 1 0 - Live 0\nnvme 1 0 - Live 0\n",
    )
    .unwrap();
    let scan_out = root.path.join("halcyon-scan.jet");
    let scan = Command::new(generation.join("sw/bin/jetos-hardware-scan"))
        .arg("halcyon")
        .env("JETOS_HW_ROOT", &hardware_root)
        .env("JETOS_HARDWARE_OUT", &scan_out)
        .output()
        .unwrap();
    assert!(
        scan.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&scan.stdout),
        String::from_utf8_lossy(&scan.stderr)
    );
    let scanned_source = fs::read_to_string(&scan_out).unwrap();
    assert!(
        scanned_source.contains("hardware.halcyon.scan.modules")
            && scanned_source.contains("amdgpu,nvme")
            && scanned_source.contains("nvme0n1"),
        "scanned_source: {scanned_source}"
    );
    let doctor = Command::new(generation.join("sw/bin/jetos-hardware-doctor"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_HW_ROOT", &hardware_root)
        .output()
        .unwrap();
    assert!(
        doctor.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&doctor.stdout),
        String::from_utf8_lossy(&doctor.stderr)
    );
    let doctor = String::from_utf8_lossy(&doctor.stdout);
    assert!(
        doctor.contains("\"state\":\"match\"") && doctor.contains("hardware-drift-checked"),
        "doctor: {doctor}"
    );
    let performance = fs::read_to_string(generation.join("performance/facts.json")).unwrap();
    assert!(
        performance.contains("\"profile\":\"Gaming\"")
            && performance.contains("\"kernel_profile\":\"CachyOSLatest\"")
            && performance.contains("vm.swappiness")
            && performance.contains("performance/initrd.json")
            && performance.contains("performance/bootloader.json"),
        "performance: {performance}"
    );
    let perf_profile = fs::read_to_string(generation.join("performance/profile.json")).unwrap();
    assert!(
        perf_profile.contains("kernel-tuning-profile-ready")
            && perf_profile.contains("CachyOSLatest"),
        "perf_profile: {perf_profile}"
    );
    let scheduler = fs::read_to_string(generation.join("performance/scheduler.json")).unwrap();
    assert!(
        scheduler.contains("ScxLavd") && scheduler.contains("sched-ext-service-ready"),
        "scheduler: {scheduler}"
    );
    assert!(
        generation
            .join("etc/systemd/system/jetos-performance-scheduler.service")
            .is_file()
            && generation
                .join("etc/systemd/system/multi-user.target.wants/jetos-performance-scheduler.service")
                .exists()
            && generation.join("sw/bin/jetos-performance-scheduler").is_file(),
        "expected scheduler service"
    );
    let scheduler_bin = root.path.join("fake-scheduler");
    let scheduler_log = root.path.join("scheduler.log");
    write_executable(
        &scheduler_bin,
        "#!/bin/sh\nprintf '%s\\n' scheduler >> \"$JETOS_SCHEDULER_LOG\"\n",
    );
    let scheduler_run = Command::new(generation.join("sw/bin/jetos-performance-scheduler"))
        .env("JETOS_SCHEDULER_BIN", &scheduler_bin)
        .env("JETOS_SCHEDULER_LOG", &scheduler_log)
        .output()
        .unwrap();
    assert!(
        scheduler_run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&scheduler_run.stdout),
        String::from_utf8_lossy(&scheduler_run.stderr)
    );
    assert!(
        fs::read_to_string(&scheduler_log)
            .unwrap()
            .contains("scheduler"),
        "expected scheduler log"
    );
    let initrd = fs::read_to_string(generation.join("performance/initrd.json")).unwrap();
    assert!(
        initrd.contains("\"systemd\":true") && initrd.contains("\"verbosity\":\"quiet\""),
        "initrd: {initrd}"
    );
    let bootloader = fs::read_to_string(generation.join("performance/bootloader.json")).unwrap();
    assert!(
        bootloader.contains("\"limine_max_generations\":\"7\"")
            && bootloader.contains("\"efi_can_touch_variables\":false"),
        "bootloader: {bootloader}"
    );
    assert!(
        generation
            .join("etc/sysctl.d/90-jetos-performance.conf")
            .is_file(),
        "expected sysctl projection"
    );
    assert!(
        generation
            .join("etc/systemd/zram-generator.conf.d/jetos.conf")
            .is_file(),
        "expected zram projection"
    );
    let storage = fs::read_to_string(generation.join("storage/facts.json")).unwrap();
    assert!(
        storage.contains("jetos.storage-tree")
            && storage.contains("disk.main.device")
            && storage.contains("jetos-storage-apply")
            && storage.contains("\"ephemeral_root\":true"),
        "storage: {storage}"
    );
    let storage_plan = fs::read_to_string(generation.join("storage/plan.json")).unwrap();
    assert!(
        storage_plan.contains("\"root_fs\":\"Btrfs\"")
            && storage_plan.contains("/var/lib")
            && storage_plan.contains("requires --manual plus --execute"),
        "storage_plan: {storage_plan}"
    );
    assert!(
        generation.join("storage/mounts.fstab").is_file()
            && generation.join("sw/bin/jetos-storage-plan").is_file()
            && generation.join("sw/bin/jetos-storage-apply").is_file()
            && generation.join("sw/bin/jetos-persist-activate").is_file(),
        "expected storage scripts and mounts"
    );
    let storage_apply_log = root.path.join("storage-apply.sh");
    let storage_proofs = root.path.join("storage-proofs");
    let storage_apply = Command::new(generation.join("sw/bin/jetos-storage-apply"))
        .arg("--manual")
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_STORAGE_LOG", &storage_apply_log)
        .env("JETOS_STORAGE_PROOF_DIR", &storage_proofs)
        .output()
        .unwrap();
    assert!(
        storage_apply.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&storage_apply.stdout),
        String::from_utf8_lossy(&storage_apply.stderr)
    );
    let storage_apply_log = fs::read_to_string(&storage_apply_log).unwrap();
    assert!(
        storage_apply_log.contains("sfdisk --wipe always /dev/sda")
            && storage_apply_log.contains("mkfs.btrfs -L jetos-root"),
        "storage_apply_log: {storage_apply_log}"
    );
    let storage_apply_proof = fs::read_to_string(storage_proofs.join("apply-proof.json")).unwrap();
    assert!(
        storage_apply_proof.contains("\"executed\":false")
            && storage_apply_proof.contains("manual-storage-plan-reviewed"),
        "storage_apply_proof: {storage_apply_proof}"
    );
    let persist_root = root.path.join("persist-root");
    let ephemeral_root = root.path.join("ephemeral-root");
    let persist = Command::new(generation.join("sw/bin/jetos-persist-activate"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_PERSIST_ROOT", &persist_root)
        .env("JETOS_EPHEMERAL_ROOT", &ephemeral_root)
        .env("JETOS_STORAGE_PROOF_DIR", &storage_proofs)
        .output()
        .unwrap();
    assert!(
        persist.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&persist.stdout),
        String::from_utf8_lossy(&persist.stderr)
    );
    assert!(
        persist_root.join("home/nate").is_dir()
            && persist_root.join("var/lib").is_dir()
            && ephemeral_root.join("home/nate").is_dir(),
        "expected persisted paths"
    );
    let persist_proof = fs::read_to_string(storage_proofs.join("persistence-proof.json")).unwrap();
    assert!(
        persist_proof.contains("\"state\":\"activated\"")
            && persist_proof.contains("impermanence-persist-ready"),
        "persist_proof: {persist_proof}"
    );
    let module_explain = fs::read_to_string(generation.join("module-system/explain.json")).unwrap();
    assert!(
        module_explain.contains("\"key\":\"services.displayManager\"")
            && module_explain.contains("\"value\":\"gdm\"")
            && module_explain.contains("\"value\":\"sddm\"")
            && module_explain.contains("\"winner\":true")
            && module_explain.contains("Force")
            && module_explain.contains("stylix.kmscon"),
        "module explain: {module_explain}"
    );
    let disabled_modules =
        fs::read_to_string(generation.join("module-system/disabled-modules.manifest")).unwrap();
    assert!(
        disabled_modules.contains("stylix.kmscon"),
        "disabled_modules: {disabled_modules}"
    );
    let theme = fs::read_to_string(generation.join("theme/facts.json")).unwrap();
    assert!(
        theme.contains("\"name\":\"halcyon\"") && theme.contains("theme-projected"),
        "theme: {theme}"
    );
    assert!(
        generation
            .join("share/themes/jetos/gtk-4.0/gtk.css")
            .is_file(),
        "expected theme projection"
    );
    for themed in [
        "share/qt6ct/colors/jetos.conf",
        "share/terminal/theme.toml",
        "share/editor/theme.json",
        "share/display-manager/theme.conf",
        "studio/theme-preview.json",
    ] {
        assert!(
            generation.join(themed).is_file(),
            "expected theme projection {themed}"
        );
    }
    let studio_theme = fs::read_to_string(generation.join("studio/theme-preview.json")).unwrap();
    assert!(
        studio_theme.contains("jetos.theme-preview") && studio_theme.contains("#7aa2f7"),
        "studio_theme: {studio_theme}"
    );
    let flatpak = fs::read_to_string(generation.join("flatpak/plan.json")).unwrap();
    assert!(
        flatpak.contains("com.discordapp.Discord")
            && flatpak.contains("flatpak-reconcile-planned")
            && flatpak.contains("obsidian"),
        "flatpak: {flatpak}"
    );
    let appimage = fs::read_to_string(generation.join("appimage/plan.json")).unwrap();
    assert!(
        appimage.contains("appimage-runtime-integrated")
            && appimage.contains("/opt/apps/Obsidian.AppImage"),
        "appimage: {appimage}"
    );
    assert!(
        generation.join("sw/bin/jetos-flatpak-reconcile").is_file()
            && generation.join("sw/bin/jetos-appimage-run").is_file()
            && generation.join("appimage/obsidian.desktop").is_file(),
        "expected foreign app helpers"
    );
    let flatpak_bin = root.path.join("fake-flatpak");
    let flatpak_log = root.path.join("flatpak.log");
    write_executable(
        &flatpak_bin,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$JETOS_FLATPAK_LOG\"\nif [ \"$1\" = list ]; then\n  printf '%s\\n' com.discordapp.Discord com.spotify.Client\nfi\n",
    );
    let flatpak_run = Command::new(generation.join("sw/bin/jetos-flatpak-reconcile"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_FLATPAK_BIN", &flatpak_bin)
        .env("JETOS_FLATPAK_LOG", &flatpak_log)
        .output()
        .unwrap();
    assert!(
        flatpak_run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&flatpak_run.stdout),
        String::from_utf8_lossy(&flatpak_run.stderr)
    );
    let flatpak_steps = fs::read_to_string(&flatpak_log).unwrap();
    assert!(
        flatpak_steps.contains("remote-add --if-not-exists flathub")
            && flatpak_steps.contains("install -y flathub com.discordapp.Discord")
            && flatpak_steps.contains("override com.discordapp.Discord --filesystem=Downloads")
            && flatpak_steps.contains("uninstall -y com.spotify.Client")
            && flatpak_steps.contains("update -y"),
        "flatpak_steps: {flatpak_steps}"
    );
    let appimage_print = Command::new(generation.join("sw/bin/jetos-appimage-run"))
        .args(["obsidian", "--print"])
        .env("JETOS_SYSTEM_ROOT", &generation)
        .output()
        .unwrap();
    assert!(
        appimage_print.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&appimage_print.stdout),
        String::from_utf8_lossy(&appimage_print.stderr)
    );
    assert!(
        String::from_utf8_lossy(&appimage_print.stdout).contains("/opt/apps/Obsidian.AppImage"),
        "appimage_print: {}",
        String::from_utf8_lossy(&appimage_print.stdout)
    );
    let workloads = fs::read_to_string(generation.join("workloads/facts.json")).unwrap();
    assert!(
        workloads.contains("\"name\":\"web\"")
            && workloads.contains("\"backend\":\"Container\"")
            && workloads.contains("\"name\":\"sandbox\"")
            && workloads.contains("\"backend\":\"MicroVM\"")
            && workloads.contains("web-token"),
        "workloads: {workloads}"
    );
    let web_plan = fs::read_to_string(generation.join("workloads/web.plan.json")).unwrap();
    assert!(
        web_plan.contains("/srv/web:/srv/web:ro")
            && web_plan.contains("\"memory\":\"512M\"")
            && web_plan.contains("\"rollback_keep\":\"3\"")
            && web_plan.contains("workload-proof-ready"),
        "web_plan: {web_plan}"
    );
    let sandbox_plan = fs::read_to_string(generation.join("workloads/sandbox.plan.json")).unwrap();
    assert!(
        sandbox_plan.contains("qemu-system-x86_64")
            && sandbox_plan.contains("-m 2048M")
            && sandbox_plan.contains("\"backend\":\"MicroVM\""),
        "sandbox_plan: {sandbox_plan}"
    );
    assert!(
        generation
            .join("etc/systemd/system/workload-web.service")
            .is_file(),
        "expected workload systemd unit"
    );
    assert!(
        generation.join("workloads/web.rollback.manifest").is_file()
            && generation.join("workloads/health-web.sh").is_file()
            && generation
                .join("workloads/sandbox.rollback.manifest")
                .is_file(),
        "expected workload health/rollback artifacts"
    );
    let workload_log = root.path.join("workload.log");
    let workload_run = Command::new(generation.join("sw/bin/jetos-workload-run"))
        .arg("web")
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_WORKLOAD_LOG", &workload_log)
        .output()
        .unwrap();
    assert!(
        workload_run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&workload_run.stdout),
        String::from_utf8_lossy(&workload_run.stderr)
    );
    let workload_steps = fs::read_to_string(&workload_log).unwrap();
    assert!(
        workload_steps.contains("workload"),
        "workload: {workload_steps}"
    );
    let fleet = fs::read_to_string(generation.join("fleet/deploy-plan.json")).unwrap();
    assert!(
        fleet.contains("staged-proof-gated-rollback-stop") && fleet.contains("\"fleet\": \"home\""),
        "fleet: {fleet}"
    );
    assert!(
        generation.join("sw/bin/jetos-fleet-deploy").is_file(),
        "expected fleet deploy launcher"
    );
    let deploy_log = root.path.join("deploy.log");
    let deploy_proofs = root.path.join("deploy-proofs");
    fs::create_dir_all(&deploy_proofs).unwrap();
    let deploy = Command::new(generation.join("sw/bin/jetos-fleet-deploy"))
        .arg("halcyon")
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_DEPLOY_PROOF_DIR", &deploy_proofs)
        .env("JETOS_DEPLOY_LOG", &deploy_log)
        .output()
        .unwrap();
    assert!(
        deploy.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&deploy.stdout),
        String::from_utf8_lossy(&deploy.stderr)
    );
    let deploy_steps = fs::read_to_string(&deploy_log).unwrap();
    assert!(
        deploy_steps.contains("push")
            && deploy_steps.contains("proof")
            && deploy_steps.contains("switch")
            && deploy_steps.contains("health"),
        "deploy_steps: {deploy_steps}"
    );
    let deploy_proof = fs::read_to_string(deploy_proofs.join("home-halcyon.json")).unwrap();
    assert!(
        deploy_proof.contains("\"state\":\"deployed\"")
            && deploy_proof.contains("remote-proof-before-switch"),
        "deploy_proof: {deploy_proof}"
    );
    let options_ref = fs::read_to_string(generation.join("options/reference.json")).unwrap();
    assert!(
        options_ref.contains("apps.flatpak.app.discord.ref")
            && options_ref.contains("performance.sysctl.vm.swappiness")
            && options_ref.contains("\"type\":")
            && options_ref.contains("\"doc\":")
            && options_ref.contains("\"tier\":"),
        "options reference: {options_ref}"
    );
    assert!(
        generation.join("sw/bin/jetos-options-search").is_file(),
        "expected options search helper"
    );
    let option_exact = Command::new(generation.join("sw/bin/jetos-options-search"))
        .args(["--exact", "services.displayManager"])
        .env("JETOS_SYSTEM_ROOT", &generation)
        .output()
        .unwrap();
    assert!(
        option_exact.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&option_exact.stdout),
        String::from_utf8_lossy(&option_exact.stderr)
    );
    let option_exact = String::from_utf8_lossy(&option_exact.stdout);
    assert!(
        option_exact.contains("services.displayManager") && option_exact.contains("gdm"),
        "option_exact: {option_exact}"
    );
    let option_explain = Command::new(generation.join("sw/bin/jetos-options-search"))
        .args(["--explain", "services.displayManager"])
        .env("JETOS_SYSTEM_ROOT", &generation)
        .output()
        .unwrap();
    assert!(
        option_explain.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&option_explain.stdout),
        String::from_utf8_lossy(&option_explain.stderr)
    );
    let option_explain = String::from_utf8_lossy(&option_explain.stdout);
    assert!(
        option_explain.contains("services.displayManager") && option_explain.contains("winner"),
        "option_explain: {option_explain}"
    );
    let images = fs::read_to_string(generation.join("image-variants/matrix.json")).unwrap();
    assert!(
        images.contains("\"name\": \"installer\"") && images.contains("image-variant-plan-ready"),
        "image variants: {images}"
    );
    let lifecycle = fs::read_to_string(generation.join("lifecycle/policy.json")).unwrap();
    assert!(
        lifecycle.contains("gc")
            && lifecycle.contains("rollback_window")
            && lifecycle.contains("\"auto_upgrade\":true"),
        "lifecycle: {lifecycle}"
    );
    let auto_upgrade = fs::read_to_string(generation.join("lifecycle/auto-upgrade.json")).unwrap();
    assert!(
        auto_upgrade.contains("auto-upgrade-proof-gated")
            && auto_upgrade.contains("rollback-on-fail"),
        "auto_upgrade: {auto_upgrade}"
    );
    let channel = fs::read_to_string(generation.join("lifecycle/channel.json")).unwrap();
    assert!(
        channel.contains("\"channel\":\"stable\"") && channel.contains("channel-policy-ready"),
        "channel: {channel}"
    );
    assert!(
        generation.join("sw/bin/jetos-lifecycle-gc").is_file()
            && generation.join("sw/bin/jetos-channel-update").is_file()
            && generation.join("sw/bin/jetos-auto-upgrade").is_file()
            && generation
                .join("etc/systemd/system/jetos-auto-upgrade.timer")
                .is_file(),
        "expected lifecycle launchers"
    );
    let gc_systems = root.path.join("gc-systems");
    let old = gc_systems.join("generations/old");
    let mid = gc_systems.join("generations/mid");
    let new = gc_systems.join("generations/new");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&mid).unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::write(
        gc_systems.join("generations.log"),
        format!(
            "1\thalcyon\told\t{}\n2\thalcyon\tmid\t{}\n3\thalcyon\tnew\t{}\n",
            old.display(),
            mid.display(),
            new.display()
        ),
    )
    .unwrap();
    let gc = Command::new(generation.join("sw/bin/jetos-lifecycle-gc"))
        .arg("--apply")
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_SYSTEMS_DIR", &gc_systems)
        .output()
        .unwrap();
    assert!(
        gc.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&gc.stdout),
        String::from_utf8_lossy(&gc.stderr)
    );
    assert!(!old.exists(), "old generation should be deleted by GC");
    assert!(
        mid.exists() && new.exists(),
        "newer generations should be kept"
    );
    let gc_plan = fs::read_to_string(generation.join("lifecycle/gc-plan.txt")).unwrap();
    assert!(
        gc_plan.contains("reason=older-than-retention")
            && gc_plan.contains("reason=within-retention"),
        "gc_plan: {gc_plan}"
    );
    let lifecycle_log = root.path.join("lifecycle.log");
    let lifecycle_log_q = test_shell_quote(&lifecycle_log);
    let lifecycle_proofs = root.path.join("lifecycle-proofs");
    let upgrade = Command::new(generation.join("sw/bin/jetos-auto-upgrade"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_LIFECYCLE_PROOF_DIR", &lifecycle_proofs)
        .env(
            "JETOS_UPGRADE_FETCH_CMD",
            format!("echo fetch >> {lifecycle_log_q}"),
        )
        .env(
            "JETOS_UPGRADE_BUILD_CMD",
            format!("echo build >> {lifecycle_log_q}"),
        )
        .env(
            "JETOS_UPGRADE_PROOF_CMD",
            format!("echo proof >> {lifecycle_log_q}"),
        )
        .env(
            "JETOS_UPGRADE_SWITCH_CMD",
            format!("echo switch >> {lifecycle_log_q}"),
        )
        .env(
            "JETOS_UPGRADE_HEALTH_CMD",
            format!("echo health >> {lifecycle_log_q}"),
        )
        .output()
        .unwrap();
    assert!(
        upgrade.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&upgrade.stdout),
        String::from_utf8_lossy(&upgrade.stderr)
    );
    let lifecycle_steps = fs::read_to_string(&lifecycle_log).unwrap();
    assert!(
        lifecycle_steps.contains("fetch")
            && lifecycle_steps.contains("build")
            && lifecycle_steps.contains("proof")
            && lifecycle_steps.contains("switch")
            && lifecycle_steps.contains("health"),
        "lifecycle_steps: {lifecycle_steps}"
    );
    let upgrade_proof =
        fs::read_to_string(lifecycle_proofs.join("auto-upgrade-proof.json")).unwrap();
    assert!(
        upgrade_proof.contains("\"state\":\"switched\"") && upgrade_proof.contains("health-passed"),
        "upgrade_proof: {upgrade_proof}"
    );
    let rollback_log = root.path.join("lifecycle-rollback.log");
    let rollback_log_q = test_shell_quote(&rollback_log);
    let rollback_proofs = root.path.join("lifecycle-rollback-proofs");
    let rollback = Command::new(generation.join("sw/bin/jetos-auto-upgrade"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_LIFECYCLE_PROOF_DIR", &rollback_proofs)
        .env("JETOS_UPGRADE_FETCH_CMD", "true")
        .env("JETOS_UPGRADE_BUILD_CMD", "true")
        .env("JETOS_UPGRADE_PROOF_CMD", "true")
        .env("JETOS_UPGRADE_SWITCH_CMD", "true")
        .env("JETOS_UPGRADE_HEALTH_CMD", "false")
        .env(
            "JETOS_UPGRADE_ROLLBACK_CMD",
            format!("echo rollback >> {rollback_log_q}"),
        )
        .output()
        .unwrap();
    assert!(
        !rollback.status.success(),
        "rollback path should fail after health failure"
    );
    assert!(
        fs::read_to_string(&rollback_log)
            .unwrap()
            .contains("rollback"),
        "expected rollback log"
    );
    let rollback_proof =
        fs::read_to_string(rollback_proofs.join("auto-upgrade-proof.json")).unwrap();
    assert!(
        rollback_proof.contains("\"state\":\"rolled-back\"")
            && rollback_proof.contains("health-failed"),
        "rollback_proof: {rollback_proof}"
    );
    let services = fs::read_to_string(generation.join("service-manager/facts.json")).unwrap();
    assert!(
        services.contains("tmpfiles")
            && services.contains("hardening")
            && services.contains("journal"),
        "service depth: {services}"
    );
    assert!(
        generation.join("etc/tmpfiles.d/backup.conf").is_file(),
        "expected tmpfiles projection"
    );
    assert!(
        generation.join("sw/bin/jetos-service-logs").is_file(),
        "expected service log helper"
    );
    let journal_bin = root.path.join("fake-journalctl");
    let journal_log = root.path.join("journalctl.log");
    write_executable(
        &journal_bin,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$JETOS_JOURNAL_LOG\"\n",
    );
    let service_logs = Command::new(generation.join("sw/bin/jetos-service-logs"))
        .args(["openssh", "--since", "1 hour ago"])
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_JOURNALCTL_BIN", &journal_bin)
        .env("JETOS_JOURNAL_LOG", &journal_log)
        .output()
        .unwrap();
    assert!(
        service_logs.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&service_logs.stdout),
        String::from_utf8_lossy(&service_logs.stderr)
    );
    let journal_args = fs::read_to_string(&journal_log).unwrap();
    assert!(
        journal_args.contains("-u openssh --since 1 hour ago"),
        "journal_args: {journal_args}"
    );
    let app_modules = fs::read_to_string(generation.join("apps/modules.json")).unwrap();
    assert!(
        app_modules.contains("app-module-library")
            && app_modules.contains("ghosttyConfig")
            && app_modules.contains("\"name\":\"git\"")
            && app_modules.contains("\"name\":\"vscode\"")
            && app_modules.contains("jetos-app-module-apply"),
        "app modules: {app_modules}"
    );
    assert!(
        generation.join("apps/programs/git/module.json").is_file()
            && generation.join("apps/programs/vscode/config").is_file()
            && generation
                .join("apps/programs/discord/module.json")
                .is_file()
            && generation.join("apps/coverage.manifest").is_file()
            && generation.join("apps/gap-cards.manifest").is_file()
            && generation.join("sw/bin/jetos-app-module-apply").is_file(),
        "expected app module library artifacts"
    );
    let git_config = fs::read_to_string(generation.join("apps/programs/git/config")).unwrap();
    assert!(
        git_config.contains("user.name = Nate") && git_config.contains("user.email"),
        "git_config: {git_config}"
    );
    let app_home = root.path.join("app-home");
    let app_apply = Command::new(generation.join("sw/bin/jetos-app-module-apply"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_USER_HOME", &app_home)
        .output()
        .unwrap();
    assert!(
        app_apply.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&app_apply.stdout),
        String::from_utf8_lossy(&app_apply.stderr)
    );
    assert!(
        app_home.join(".config/git/config").is_file()
            && app_home.join(".config/Code/User/settings.json").is_file()
            && app_home.join(".jetos/proof/app-modules.json").is_file(),
        "expected app module apply output"
    );
    let acceptance =
        fs::read_to_string(generation.join("acceptance/jetos-host-coverage.json")).unwrap();
    assert!(
        acceptance.contains("jetos.host-coverage")
            && acceptance.contains("\"state\": \"covered\"")
            && acceptance.contains("jetos-host-covered")
            && acceptance.contains("\"omissions\":[]"),
        "acceptance: {acceptance}"
    );
    let coverage = fs::read_to_string(generation.join("acceptance/coverage-matrix.tsv")).unwrap();
    assert!(
        coverage.contains("desktop-audio-locale-fonts-virt-gaming-smartcard\tcovered")
            && coverage.contains("flatpak-appimage\tcovered")
            && coverage.contains("lifecycle-gc-auto-upgrade\tcovered")
            && !coverage.contains("\tmissing\t"),
        "coverage: {coverage}"
    );
    let vm_gates = fs::read_to_string(generation.join("acceptance/vm-gates.json")).unwrap();
    assert!(
        vm_gates.contains("desktop-session-ready")
            && vm_gates.contains("app-modules-present")
            && vm_gates.contains("vm-acceptance-required"),
        "vm_gates: {vm_gates}"
    );
    let os_release = fs::read_to_string(generation.join("etc/os-release")).unwrap();
    let expected_os_release = "NAME=jetos\nID=jetos\nVERSION=\"26.10 (Apex)\"\nVERSION_ID=26.10\nVERSION_CODENAME=apex\nPRETTY_NAME=\"jetos 26.10 (Apex)\"\nHOME_URL=\"https://jet.dev/jetos\"\n";
    assert_eq!(os_release, expected_os_release);
    assert_eq!(
        fs::read_to_string(generation.join("usr/lib/os-release")).unwrap(),
        expected_os_release
    );
    let installed_limine = fs::read_to_string(generation.join("boot/limine.conf")).unwrap();
    assert!(
        installed_limine.contains("/jetos 26.10 (Apex) — halcyon"),
        "installed Limine title: {installed_limine}"
    );
    let wallpaper = fs::read_to_string(
        generation.join("share/backgrounds/jetos/apex.svg"),
    )
    .unwrap();
    assert!(
        wallpaper.starts_with("<svg ")
            && wallpaper.contains("jetos 26.10 Apex")
            && wallpaper.contains("linearGradient")
            && wallpaper.len() > 1_000,
        "baseline wallpaper must contain the real committed SVG bytes"
    );
    for (surface, text) in [
        ("etc/os-release", os_release.as_str()),
        ("usr/lib/os-release", expected_os_release),
        ("boot/limine.conf", installed_limine.as_str()),
        ("boot specialisation", specialisation.as_str()),
        ("wallpaper", wallpaper.as_str()),
    ] {
        assert!(
            !text.contains("NixOS") && !text.contains("Yarara"),
            "upstream identity leaked through {surface}: {text}"
        );
    }
    assert!(
        generation.join("acceptance/owner-jetos-coverage.md").is_file()
            && generation.join("sw/bin/jetos-acceptance-prove").is_file(),
        "expected acceptance artifacts"
    );
    assert!(
        !generation.join("acceptance/nixos-parity.json").exists()
            && !generation.join("acceptance/owner-nixos-diff.md").exists(),
        "legacy NixOS-named JetOS artifacts must not be generated"
    );
    let acceptance_proofs = root.path.join("acceptance-proofs");
    let acceptance_run = Command::new(generation.join("sw/bin/jetos-acceptance-prove"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_ACCEPTANCE_PROOF_DIR", &acceptance_proofs)
        .output()
        .unwrap();
    assert!(
        acceptance_run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&acceptance_run.stdout),
        String::from_utf8_lossy(&acceptance_run.stderr)
    );
    let acceptance_proof =
        fs::read_to_string(acceptance_proofs.join("acceptance-proof.json")).unwrap();
    assert!(
        acceptance_proof.contains("\"state\":\"passed\"")
            && acceptance_proof.contains("jetos-host-covered"),
        "acceptance_proof: {acceptance_proof}"
    );
    let desktop = fs::read_to_string(generation.join("desktop/facts.json")).unwrap();
    assert!(
        desktop.contains("\"session\":\"gnome-wayland\""),
        "desktop: {desktop}"
    );
    assert!(
        desktop.contains("\"display_manager\":\"gdm\"")
            && desktop.contains("\"terminal_fallback\":\"ttyS0+tty1\"")
            && desktop.contains("desktop-session-ready"),
        "desktop: {desktop}"
    );
    assert!(
        generation.join("sw/bin/jetos-desktop-session").is_file(),
        "expected desktop session launcher"
    );
    let desktop_breadth = fs::read_to_string(generation.join("desktop/breadth.json")).unwrap();
    assert!(
        desktop_breadth.contains("desktop-module-breadth-ready")
            && desktop_breadth.contains("\"audio\":true")
            && desktop_breadth.contains("plasma-wayland")
            && desktop_breadth.contains("libvirtd")
            && desktop_breadth.contains("gamemode")
            && desktop_breadth.contains("Inter"),
        "desktop_breadth: {desktop_breadth}"
    );
    for desktop_path in [
        "share/wayland-sessions/jetos-plasma.desktop",
        "etc/pipewire/jetos.conf",
        "etc/security/limits.d/99-jetos-rtkit.conf",
        "etc/locale.conf",
        "etc/vconsole.conf",
        "etc/fonts/local.conf",
        "share/applications/mimeapps.list",
        "etc/systemd/system/libvirtd.service",
        "etc/systemd/system/gamemoded.service",
        "etc/systemd/system/pcscd.service",
        "etc/binfmt.d/appimage.conf",
    ] {
        assert!(
            generation.join(desktop_path).is_file(),
            "expected desktop breadth artifact {desktop_path}"
        );
    }
    let desktop_session =
        fs::read_to_string(generation.join("sw/bin/jetos-desktop-session")).unwrap();
    assert!(
        desktop_session.contains("--jetos-proof")
            && desktop_session.contains("desktop session command gnome-session"),
        "desktop session launcher should expose proof mode: {desktop_session}"
    );
    let display_manager =
        fs::read_to_string(generation.join("sw/bin/jetos-display-manager")).unwrap();
    assert!(
        display_manager.contains("--jetos-proof")
            && display_manager.contains("display manager command gdm"),
        "display manager launcher should expose proof mode: {display_manager}"
    );
    assert!(
        generation.join("sw/bin/gdm").is_file()
            && generation.join("sw/bin/gnome-session").is_file()
            && generation.join("sw/bin/gnome-shell").is_file(),
        "expected default GNOME profile commands in system closure"
    );
    assert!(
        generation.join("sw/bin/jetos-terminal-fallback").is_file(),
        "expected terminal fallback launcher"
    );
    let terminal_fallback =
        fs::read_to_string(generation.join("sw/bin/jetos-terminal-fallback")).unwrap();
    assert!(
        terminal_fallback.contains("cat /etc/motd")
            && terminal_fallback.contains("ttyS0 and tty1 remain available"),
        "terminal_fallback: {terminal_fallback}"
    );
    assert!(
        generation
            .join("share/wayland-sessions/jetos-gnome.desktop")
            .is_file(),
        "expected GNOME Wayland session entry"
    );
    let cache = fs::read_to_string(generation.join("store/cache.json")).unwrap();
    assert!(cache.contains("jetpack-hangar"), "cache: {cache}");
    let compat = fs::read_to_string(generation.join("compat/escape-hatches.json")).unwrap();
    assert!(
        compat.contains("\"studio_visible\": \"true\""),
        "compat: {compat}"
    );
    assert!(
        generation.join("sw/bin/jetos-studio").is_file(),
        "expected installed jetos Studio launcher"
    );
    assert!(
        generation
            .join("share/applications/jetos-studio.desktop")
            .is_file(),
        "expected desktop app entry"
    );
    let studio = fs::read_to_string(generation.join("studio/app.json")).unwrap();
    assert!(
        studio.contains("\"runtime\": \"jetos-system-app\""),
        "studio: {studio}"
    );
    assert!(
        studio.contains("\"browser_fallback\": \"true\""),
        "studio: {studio}"
    );
    assert!(
        !studio.contains("Canvas"),
        "studio app projection must stay separate from Canvas: {studio}"
    );
    let studio_data = fs::read_to_string(generation.join("studio/data.json")).unwrap();
    assert!(
        studio_data.contains("\"kind\":\"jetos-studio-projection\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"artifacts\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"dashboard\"") && studio_data.contains("\"selected_host\":\"halcyon\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"page_registry\"") && studio_data.contains("\"id\":\"changeset\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"controller\":\"studio-actions\"")
            && studio_data.contains("\"model_contract\":")
            && studio_data.contains("\"read_only\":"),
        "Studio page registry must own render and action contracts: {studio_data}"
    );
    assert!(
        studio_data.contains("\"apply_gate\":\"single-source-transaction\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"secret_policy\"")
            && studio_data.contains("\"plaintext_in_projection\":false"),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"fleet\"") && studio_data.contains("\"mode\":\"adaptive\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"canvas_bridge\"")
            && studio_data.contains("\"mode\":\"separate-app-deeplink\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"first_boot\"")
            && studio_data.contains("\"role\":\"os-control-center\"")
            && studio_data.contains("\"canvas_first_surface\":false"),
        "first-boot control center must own Studio, not Canvas: {studio_data}"
    );
    assert!(
        studio_data.contains("\"openssh\""),
        "studio data: {studio_data}"
    );
    assert!(
        generation
            .join("studio/first-boot.json")
            .is_file(),
        "expected Studio first-boot control-center projection"
    );
    let first_boot = fs::read_to_string(generation.join("studio/first-boot.json")).unwrap();
    assert!(
        first_boot.contains("\"role\":\"os-control-center\"")
            && first_boot.contains("\"proof\":\"first-boot-control-center-ready\"")
            && first_boot.contains("\"first_surface\":false"),
        "first-boot: {first_boot}"
    );
    assert!(
        generation
            .join("share/xdg/autostart/jetos-studio-first-boot.desktop")
            .is_file(),
        "expected first-boot Studio autostart desktop entry"
    );
    assert!(
        generation.join("sw/bin/jetos-studio-first-boot").is_file(),
        "expected first-boot Studio launcher"
    );
    assert!(
        generation.join("studio/first-boot.pending").is_file(),
        "expected first-boot pending marker"
    );
    assert!(
        generation
            .join("root/run/current-system/studio/app.json")
            .is_file(),
        "expected Studio app in root current-system projection"
    );
    assert!(
        generation
            .join("root/run/current-system/studio/data.json")
            .is_file(),
        "expected Studio data in root current-system projection"
    );
    let studio_html = fs::read_to_string(generation.join("studio/index.html")).unwrap();
    assert!(
        studio_html.contains("jetos Studio"),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-page-registry=\"studio-pages\""),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-page-kind=\"dashboard\"")
            && studio_html.contains("Service configuration")
            && studio_html.contains("Proof/rollback status"),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-page-kind=\"settings\"")
            && studio_html.contains("data-stage-setting=\"network.hostName\""),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-page-kind=\"changeset\"")
            && studio_html.contains("data-apply-gate=\"single-source-transaction\"")
            && studio_html.contains("data-changeset-action=\"apply\"")
            && studio_html.contains("data-changeset-action=\"discard\"")
            && studio_html.contains("Impact ledger")
            && studio_html.contains("Build only"),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-changeset-tray=\"true\""),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-secret-policy=\"no-plaintext\"")
            && studio_html.contains("plaintext: never projected"),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-page-kind=\"fleet\"")
            && studio_html.contains("data-fleet-mode=\"adaptive\"")
            && studio_html.contains("proof-before-switch"),
        "studio: {studio_html}"
    );
    assert!(studio_html.contains("openssh"), "studio: {studio_html}");
    assert!(
        studio_html.contains("network.hostName"),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-stage-source=\"true\"")
            && studio_html.contains("data-pipeline=\"build-switch\""),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-run=\"proof\""),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("const PAGE_CONTROLLERS")
            && studio_html.contains("synthetic-registered-page")
            && studio_html.contains("resolvePageBinding")
            && !studio_html.contains("renderMissing"),
        "synthetic registry entry must resolve renderer, controller, and model contract: {studio_html}"
    );
    assert!(
        studio_html.contains("data-open-canvas=\"source\"")
            && studio_html.contains("Open Canvas")
            && studio_html.contains("jetos Studio"),
        "Studio may deep-link to Canvas while remaining a separate app: {studio_html}"
    );
    let secrets = fs::read_to_string(generation.join("secrets.tmpfs.manifest")).unwrap();
    assert!(
        secrets.contains("wifi\tsecrets/wifi.age"),
        "secrets: {secrets}"
    );
    let vm_proof = generation.join("vm-proof.txt");
    let vm_text = fs::read_to_string(&vm_proof).expect("risk switch writes VM proof");
    assert!(vm_text.contains("plan-sha256:"), "vm proof: {vm_text}");
    assert!(
        vm_text.contains("service-artifacts: pass"),
        "vm proof: {vm_text}"
    );
    let proof = jet()
        .args(["os", "proof", "halcyon", "--json", "--no-color"])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        proof.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&proof.stderr)
    );
    let proof_json = String::from_utf8_lossy(&proof.stdout);
    assert!(
        proof_json.contains("\"host\":\"halcyon\""),
        "proof: {proof_json}"
    );
    assert!(proof_json.contains("\"boot\":"), "proof: {proof_json}");
    assert!(
        proof_json.contains("\"provenance\":"),
        "proof: {proof_json}"
    );
    assert!(proof_json.contains("\"vm_proof\":"), "proof: {proof_json}");
}

#[test]
fn jetos_studio_headless_opens_installed_app_projection() {
    let root = Scratch::new("studio-root");
    let switch = jet()
        .args([
            "os",
            "switch",
            "halcyon",
            "--name",
            "studio-app",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        switch.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&switch.stderr)
    );
    let generation = root.path.join("systems/generations/studio-app");
    let out = jetos()
        .args(["studio", "--headless", "--no-color"])
        .env("JETOS_STUDIO_ROOT", &generation)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("studio/index.html"),
        "stdout should print app path: {stdout}"
    );

    let open_bin = root.path.join("open-bin");
    fs::create_dir_all(&open_bin).unwrap();
    write_executable(&open_bin.join("xdg-open"), "#!/bin/sh\nexit 0\n");
    let mut child = jetos()
        .args(["studio", "--no-color"])
        .env("JETOS_STUDIO_ROOT", &generation)
        .env("PATH", &open_bin)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    {
        use std::io::BufRead;
        reader.read_line(&mut line).unwrap();
    }
    let addr = line
        .trim()
        .strip_prefix("http://")
        .and_then(|s| s.strip_suffix("/studio/"))
        .expect("default Studio launch must open its local projection service");
    let page = studio_http(addr, "GET", "/studio/", "");
    assert!(page.contains("data-page-kind=\"dashboard\""), "page: {page}");
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn jetos_studio_serve_exposes_projection_json() {
    let root = Scratch::new("studio-serve-root");
    let switch = jet()
        .args([
            "os",
            "switch",
            "halcyon",
            "--name",
            "studio-serve",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        switch.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&switch.stderr)
    );
    let generation = root.path.join("systems/generations/studio-serve");
    let mut child = jetos()
        .args(["studio", "--serve", "127.0.0.1:0", "--no-color"])
        .env("JETOS_STUDIO_ROOT", &generation)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    {
        use std::io::BufRead;
        reader.read_line(&mut line).unwrap();
    }
    let addr = line
        .trim()
        .strip_prefix("http://")
        .and_then(|s| s.strip_suffix("/studio/"))
        .expect("service url");
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    {
        use std::io::Write;
        stream
            .write_all(b"GET /studio/data.json HTTP/1.1\r\nHost: local\r\n\r\n")
            .unwrap();
    }
    let mut response = String::new();
    {
        use std::io::Read;
        stream.read_to_string(&mut response).unwrap();
    }
    assert!(response.contains("200 OK"), "response: {response}");
    assert!(
        response.contains("jetos-studio-projection"),
        "response: {response}"
    );
    assert!(response.contains("openssh"), "response: {response}");
    let page = studio_http(addr, "GET", "/studio/", "");
    assert!(page.contains("200 OK"), "page: {page}");
    assert!(
        page.contains("data-page-kind=\"dashboard\"")
            && page.contains("data-page-registry=\"studio-pages\"")
            && page.contains("data-page-kind=\"changeset\""),
        "served Studio must be dashboard/sidebar/Changeset app: {page}"
    );
    assert!(
        page.contains("data-changeset-action=\"apply\"")
            && page.contains("data-changeset-action=\"discard\""),
        "served Studio must expose one Changeset apply path: {page}"
    );
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn jetos_studio_transaction_previews_and_writes_source() {
    let project = Scratch::new("studio-edit-project");
    copy_dir_recursive(&config_example_dir(), &project.path);
    let root = Scratch::new("studio-edit-root");
    let switch = jet()
        .args([
            "os",
            "switch",
            "halcyon",
            "--name",
            "studio-edit",
            "--no-color",
            "--yes",
            "--offline",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        switch.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&switch.stderr)
    );
    let generation = root.path.join("systems/generations/studio-edit");
    let mut child = jetos()
        .args([
            "studio",
            project.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--serve",
            "127.0.0.1:0",
            "--no-color",
        ])
        .env("JETOS_STUDIO_ROOT", &generation)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let server_pid = child.id();
    let stdout = child.stdout.take().unwrap();
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    {
        use std::io::BufRead;
        reader.read_line(&mut line).unwrap();
    }
    let addr = line
        .trim()
        .strip_prefix("http://")
        .and_then(|s| s.strip_suffix("/studio/"))
        .expect("service url");
    let session = studio_session(addr);
    let other_session = studio_session(addr);
    let initial_data = studio_http(addr, "GET", "/studio/data.json", "");
    assert!(initial_data.contains("live-checked-plan"), "data: {initial_data}");
    assert!(initial_data.contains("network.hostName"), "data: {initial_data}");
    for field in ["page_registry", "renderer", "system_plan", "services", "packages", "options", "proof_state", "generations"] {
        assert!(initial_data.contains(field), "missing live model `{field}`: {initial_data}");
    }
    let bypass = studio_stage_option(addr, &session, "network.hostName", "bypass", true);
    assert!(bypass.contains("400 Bad Request"), "bypass: {bypass}");
    assert!(bypass.contains("direct Studio writes are disabled"), "bypass: {bypass}");
    let inserted = studio_stage_option(addr, &session, "network.mtu", "1500", false);
    assert!(inserted.contains("@@ -1,"), "inserted: {inserted}");
    assert!(inserted.contains("+            network.mtu: 1500,"), "inserted: {inserted}");
    let inserted_owner = studio_changeset_owner(&inserted, &session);
    let _ = studio_owned_transaction(addr, "discard", &inserted_owner);
    let preview = studio_stage_option(addr, &session, "network.hostName", "aurora", false);
    assert!(preview.contains("200 OK"), "preview: {preview}");
    assert!(preview.contains("\"write\":false"), "preview: {preview}");
    assert!(preview.contains("\"state\":\"staged\""), "preview: {preview}");
    assert!(preview.contains("\"staged_count\":1"), "preview: {preview}");
    let preview_owner = studio_changeset_owner(&preview, &session);
    assert!(
        preview.contains("-            network.hostName: halcyon,"),
        "preview: {preview}"
    );
    assert!(
        preview.contains("+            network.hostName: aurora,"),
        "preview: {preview}"
    );
    let config = fs::read_to_string(project.join("config.jet")).unwrap();
    assert!(
        config.contains("network.hostName: halcyon"),
        "config: {config}"
    );
    let source = studio_http(addr, "GET", "/studio/source", "");
    assert!(
        source.contains("network.hostName: halcyon"),
        "source: {source}"
    );
    let stolen_discard = studio_http(addr, "POST", "/studio/transaction", &format!("{{\"op\":\"discard\",\"session_id\":\"{other_session}\",\"token\":\"{}\",\"base_revision\":\"{}\"}}", preview_owner.token, preview_owner.base_revision));
    assert!(stolen_discard.contains("409 Conflict"), "stolen discard: {stolen_discard}");
    let stolen_apply = studio_http(addr, "POST", "/studio/transaction", &format!("{{\"op\":\"apply\",\"session_id\":\"{other_session}\",\"token\":\"{}\",\"base_revision\":\"{}\"}}", preview_owner.token, preview_owner.base_revision));
    assert!(stolen_apply.contains("409 Conflict"), "stolen apply: {stolen_apply}");
    let staged = studio_owned_transaction(addr, "status", &preview_owner);
    assert!(staged.contains("200 OK"), "staged: {staged}");
    assert!(staged.contains("\"state\":\"staged\""), "staged: {staged}");
    assert!(staged.contains("\"staged_count\":1"), "staged: {staged}");
    assert!(staged.contains("network.hostName"), "staged: {staged}");
    let discarded = studio_owned_transaction(addr, "discard", &preview_owner);
    assert!(discarded.contains("\"state\":\"discarded\""), "discarded: {discarded}");
    let empty = studio_session_transaction(addr, "status", &session);
    assert!(empty.contains("\"state\":\"empty\""), "empty: {empty}");
    let preview = studio_stage_option(addr, &session, "network.hostName", "aurora", false);
    assert!(preview.contains("\"state\":\"staged\""), "preview: {preview}");
    let preview_owner = studio_changeset_owner(&preview, &session);
    let original = fs::read_to_string(project.join("config.jet")).unwrap();
    fs::write(project.join("config.jet"), format!("{original}// external edit\n")).unwrap();
    let stale = studio_owned_transaction(addr, "apply", &preview_owner);
    assert!(stale.contains("409 Conflict"), "stale: {stale}");
    assert!(stale.contains("changed after this Changeset"), "stale: {stale}");
    fs::write(project.join("config.jet"), &original).unwrap();
    let config_q = test_shell_quote(&project.join("config.jet"));
    let lock_path = project.join(".config.jet.studio.lock");
    let mut compliant_writer = std::process::Command::new("flock")
        .arg("-x")
        .arg(&lock_path)
        .arg("sh")
        .arg("-c")
        .arg(format!("sleep 0.05; printf '%s\\n' '// compliant external process' >> {config_q}"))
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let raced_apply = studio_owned_transaction(addr, "apply", &preview_owner);
    assert!(compliant_writer.wait().unwrap().success());
    assert!(raced_apply.contains("409 Conflict"), "cross-process CAS: {raced_apply}");
    let externally_written = fs::read_to_string(project.join("config.jet")).unwrap();
    assert!(externally_written.contains("compliant external process"), "external write was clobbered: {externally_written}");
    fs::write(project.join("config.jet"), &original).unwrap();
    let mut noncompliant_writer = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("i=0; while [ $i -lt 30 ]; do printf '%s\\n' '// noncompliant external process' >> {config_q}; i=$((i + 1)); sleep 0.01; done"))
        .spawn()
        .unwrap();
    let noncompliant_apply = studio_owned_transaction(addr, "apply", &preview_owner);
    assert!(noncompliant_writer.wait().unwrap().success());
    assert!(noncompliant_apply.contains("409 Conflict"), "noncompliant CAS: {noncompliant_apply}");
    let externally_written = fs::read_to_string(project.join("config.jet")).unwrap();
    assert!(externally_written.contains("noncompliant external process"), "noncompliant write was clobbered: {externally_written}");
    fs::write(project.join("config.jet"), &original).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let original_mode = fs::metadata(&project.path).unwrap().permissions().mode();
        fs::set_permissions(&project.path, fs::Permissions::from_mode(0o555)).unwrap();
        let failed = studio_owned_transaction(addr, "apply", &preview_owner);
        fs::set_permissions(&project.path, fs::Permissions::from_mode(original_mode)).unwrap();
        assert!(failed.contains("500 Internal Server Error"), "failed: {failed}");
        assert!(failed.contains("\"reprojected\":false"), "failed: {failed}");
    }
    let write = studio_owned_transaction(addr, "apply", &preview_owner);
    assert!(write.contains("200 OK"), "write: {write}");
    assert!(write.contains("\"state\":\"applied\""), "write: {write}");
    assert!(write.contains("\"reprojected\":true"), "write: {write}");
    assert!(write.contains("\"staged_count\":0"), "write: {write}");
    let config = fs::read_to_string(project.join("config.jet")).unwrap();
    assert!(
        config.contains("network.hostName: aurora"),
        "config: {config}"
    );
    let source = studio_http(addr, "GET", "/studio/source", "");
    assert!(
        source.contains("network.hostName: aurora"),
        "source: {source}"
    );
    let live_data = studio_http(addr, "GET", "/studio/data.json", "");
    assert!(live_data.contains("live-checked-plan"), "data: {live_data}");
    assert!(live_data.contains("aurora"), "data: {live_data}");
    assert!(live_data.contains("\"renderer\":\"dashboard\""), "data: {live_data}");
    let plan = jet()
        .args(["os", "plan", "halcyon", "--json", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        plan.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let stdout = String::from_utf8_lossy(&plan.stdout);
    assert!(stdout.contains("aurora"), "plan: {stdout}");
    let check = studio_http(addr, "POST", "/studio/run", "{\"action\":\"check\"}");
    assert!(check.contains("\"success\":true"), "check: {check}");
    let plan = studio_http(addr, "POST", "/studio/run", "{\"action\":\"plan\"}");
    assert!(plan.contains("\"success\":true"), "plan: {plan}");
    assert!(plan.contains("aurora"), "plan: {plan}");
    let build = studio_http(addr, "POST", "/studio/run", "{\"action\":\"build\"}");
    assert!(build.contains("\"success\":true"), "build: {build}");
    let unproved_switch = studio_http(addr, "POST", "/studio/run", "{\"action\":\"switch\"}");
    assert!(unproved_switch.contains("409 Conflict"), "switch: {unproved_switch}");
    let config_path = project.join("config.jet");
    let proved_source = fs::read_to_string(&config_path).unwrap();
    let raced_source = proved_source.replace("network.hostName: aurora", "network.hostName: intruder");
    let race_path = config_path.clone();
    let race = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(25));
        fs::write(race_path, raced_source).unwrap();
    });
    let raced_proof = studio_http(addr, "POST", "/studio/run", "{\"action\":\"proof\"}");
    race.join().unwrap();
    assert!(raced_proof.contains("\"source_revision\":"), "proof race: {raced_proof}");
    let raced_switch = studio_http(addr, "POST", "/studio/run", "{\"action\":\"switch\"}");
    assert!(raced_switch.contains("409 Conflict"), "proof race switch: {raced_switch}");
    let failed_proof_state = studio_http(addr, "GET", "/studio/data.json", "");
    assert!(failed_proof_state.contains("\"state\":\"unproved\""), "failed proof badge: {failed_proof_state}");
    fs::write(&config_path, &proved_source).unwrap();
    let snapshot_mutation = studio_attack_snapshot(server_pid, false);
    let mutated_proof = studio_http(addr, "POST", "/studio/run", "{\"action\":\"proof\"}");
    snapshot_mutation.join().unwrap();
    assert!(mutated_proof.contains("\"success\":true"), "sealed snapshot: {mutated_proof}");
    let snapshot_replacement = studio_attack_snapshot(server_pid, true);
    let replaced_proof = studio_http(addr, "POST", "/studio/run", "{\"action\":\"proof\"}");
    snapshot_replacement.join().unwrap();
    assert!(replaced_proof.contains("\"success\":true"), "sealed snapshot: {replaced_proof}");
    let proved_source = format!("{proved_source}// unbuilt proof-only revision\n");
    fs::write(&config_path, &proved_source).unwrap();
    let proof = studio_http(addr, "POST", "/studio/run", "{\"action\":\"proof\"}");
    assert!(proof.contains("\"success\":true"), "proof: {proof}");
    assert!(proof.contains("aurora"), "proof: {proof}");
    assert!(proof.contains("\"source_revision\":"), "proof: {proof}");
    assert!(proof.contains("source_proof"), "proof: {proof}");
    assert!(proof.contains("input_plan_sha256"), "proof: {proof}");
    let proof_response = studio_json(&proof);
    assert_eq!(
        proof_response.get("success").unwrap(),
        &jetpack::JSON::Json::Bool(true),
        "proof: {proof}"
    );
    let proof_revision = json_string(&proof_response, "source_revision");
    let proof_stdout = json_string(&proof_response, "stdout");
    let proof_artifact = jetpack::JSON::parse(proof_stdout.trim())
        .unwrap_or_else(|error| panic!("invalid Studio proof artifact: {error}: {proof_stdout}"));
    let proof_generation = json_string(&proof_artifact, "generation");
    let proof_source = proof_artifact
        .get("source_proof")
        .unwrap_or_else(|error| panic!("missing proof source binding: {error}: {proof_artifact:?}"));
    let proof_source_sha256 = json_string(proof_source, "source_sha256");
    let proof_input_plan_sha256 = json_string(proof_source, "input_plan_sha256");
    let proof_plan_sha256 = json_string(proof_source, "plan_sha256");
    assert_eq!(proof_source_sha256, proof_revision);
    let proved_projection = studio_http(addr, "GET", "/studio/data.json", "");
    assert!(proved_projection.contains("\"state\":\"proved\""), "proved badge: {proved_projection}");
    assert!(proved_projection.contains(&proof_revision), "proved revision: {proved_projection}");
    let switch_race_path = config_path.clone();
    let switch_raced_source = proved_source.replace("network.hostName: aurora", "network.hostName: unproved");
    let switch_race = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1));
        fs::write(switch_race_path, switch_raced_source).unwrap();
    });
    let switched = studio_http(addr, "POST", "/studio/run", "{\"action\":\"switch\"}");
    switch_race.join().unwrap();
    assert!(switched.contains("\"success\":false"), "switch race: {switched}");
    assert!(switched.contains("\"source_changed_after\":true"), "switch: {switched}");
    assert!(switched.contains("rolled back"), "switch race: {switched}");
    let current_after_race = fs::read_link(root.join("systems/current")).unwrap();
    assert_eq!(current_after_race.file_name().unwrap(), "studio-edit");
    fs::write(&config_path, &proved_source).unwrap();
    let candidate_plan_before = fs::read(root.join("systems/generations/zz-studio-candidate/plan.json")).unwrap();
    let switched = studio_http(addr, "POST", "/studio/run", "{\"action\":\"switch\"}");
    assert!(switched.contains("\"success\":true"), "switch: {switched}");
    let current = fs::read_link(root.join("systems/current")).unwrap();
    assert_eq!(current.file_name().unwrap(), proof_generation.as_str());
    let current_source_proof =
        fs::read_to_string(root.join("systems/current/source-proof.json")).unwrap();
    let current_source_proof = jetpack::JSON::parse(&current_source_proof)
        .unwrap_or_else(|error| panic!("invalid current generation source proof: {error}"));
    assert_eq!(
        json_string(&current_source_proof, "source_sha256"),
        proof_source_sha256
    );
    assert_eq!(
        json_string(&current_source_proof, "input_plan_sha256"),
        proof_input_plan_sha256
    );
    assert_eq!(
        json_string(&current_source_proof, "plan_sha256"),
        proof_plan_sha256
    );
    assert_eq!(
        fs::read(root.join("systems/generations/zz-studio-candidate/plan.json")).unwrap(),
        candidate_plan_before,
        "switch must not rebuild a candidate"
    );
    let generations = studio_http(addr, "POST", "/studio/run", "{\"action\":\"generations\"}");
    assert!(
        generations.contains("zz-studio-candidate"),
        "generations: {generations}"
    );
    let applied_source = proved_source.replace("// unbuilt proof-only revision\n", "");
    fs::write(&config_path, applied_source).unwrap();
    let rollback = studio_session_transaction(addr, "stage-rollback", &session);
    assert!(rollback.contains("\"state\":\"staged\""), "rollback: {rollback}");
    let rollback_owner = studio_changeset_owner(&rollback, &session);
    assert!(rollback.contains("-            network.hostName: aurora,"), "rollback: {rollback}");
    assert!(rollback.contains("+            network.hostName: halcyon,"), "rollback: {rollback}");
    let rollback_apply = studio_owned_transaction(addr, "apply", &rollback_owner);
    assert!(rollback_apply.contains("\"reprojected\":true"), "rollback: {rollback_apply}");
    let restored = studio_http(addr, "GET", "/studio/data.json", "");
    assert!(restored.contains("halcyon"), "restored: {restored}");
    let literal = "\"café \\\"lab\\\" \\\\share\"";
    let escaped = studio_stage_option(addr, &session, "network.interface", literal, false);
    assert!(escaped.contains("café"), "escaped: {escaped}");
    let escaped_owner = studio_changeset_owner(&escaped, &session);
    let escaped_apply = studio_owned_transaction(addr, "apply", &escaped_owner);
    assert!(escaped_apply.contains("\"reprojected\":true"), "escaped: {escaped_apply}");
    let escaped_source = fs::read_to_string(&config_path).unwrap();
    assert!(
        escaped_source.contains(&format!("network.interface: {literal},")),
        "escaped source: {escaped_source}"
    );
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn os_plan_prints_checked_system_contract_without_building() {
    let root = Scratch::new("os-plan-root");
    let out = jet()
        .args(["os", "plan", "halcyon", "--json", "--no-color", "--offline"])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = String::from_utf8_lossy(&out.stdout);
    assert!(json.contains("\"host\":\"halcyon\""), "plan: {json}");
    assert!(json.contains("\"loader\":\"Limine\""), "plan: {json}");
    assert!(json.contains("\"kernel\":\"CachyOS\""), "plan: {json}");
    assert!(
        json.contains("\"key\": \"users.nate.normal\""),
        "plan: {json}"
    );
    assert!(
        !root.join("systems/generations").exists(),
        "plan must not create a generation"
    );
}

#[test]
fn jetos_user_commands_use_same_generation_engine() {
    let root = Scratch::new("jetos-user-root");
    let plan = jetos()
        .args(["user", "plan", "nate", "--json", "--no-color"])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        plan.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let stdout = String::from_utf8_lossy(&plan.stdout);
    assert!(
        stdout.contains("\"kind\":\"jetos.user-generation\"")
            && stdout.contains("\"user\":\"nate\""),
        "plan: {stdout}"
    );

    let build = jetos()
        .args([
            "user",
            "build",
            "nate",
            "--name",
            "user-gen",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(
        root.path
            .join("systems/generations/user-gen/users/nate/profile.json")
            .is_file(),
        "expected user profile artifact"
    );

    let proof = jetos()
        .args(["user", "prove", "nate", "--json", "--no-color"])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        proof.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&proof.stderr)
    );
    let stdout = String::from_utf8_lossy(&proof.stdout);
    assert!(
        stdout.contains("\"user\":\"nate\"") && stdout.contains("user-gen"),
        "proof: {stdout}"
    );
}

#[test]
fn os_build_bare_host_uses_current_repo_config() {
    // D-JPK-OSHOST1=C: bare host discovers system.<host> in ./config.jet.
    let proj = Scratch::new("os-repo");
    let root = Scratch::new("os-default-root");
    // A minimal self-contained system (no packages → realizes trivially offline).
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(pkg.join("boot")).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_bootlike_cachyos_artifacts(&pkg);
    write_cachyos_source_recipe(&pkg);
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("box"), "stderr: {stderr}");
}

#[test]
fn os_cachyos_kernel_source_recipe_builds_boot_artifacts() {
    let proj = Scratch::new("os-kernel-source-build");
    let root = Scratch::new("os-kernel-source-build-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(&pkg).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_cachyos_source_recipe(&pkg);
    write_cachyos_source_builder(
        &pkg,
        "#!/bin/sh\nset -eu\nprintf 'MZ built cachyos kernel\\nHdrS\\n' > \"$JETOS_KERNEL_OUT/vmlinuz-cachyos\"\nprintf '070701 built cachyos initrd\\n' > \"$JETOS_KERNEL_OUT/initrd-cachyos\"\n",
    );
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args([
            "os",
            "build",
            "box",
            "--name",
            "kernel-source-built",
            "--no-color",
            "--offline",
        ])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let kernel = fs::read_to_string(
        root.path
            .join("systems/generations/kernel-source-built/boot/kernel"),
    )
    .unwrap();
    assert!(kernel.contains("built cachyos kernel"), "kernel: {kernel}");
    let boot = fs::read_to_string(
        root.path
            .join("systems/generations/kernel-source-built/boot/facts.json"),
    )
    .unwrap();
    assert!(
        boot.contains("\"bootstrap\":\"source-built\""),
        "boot: {boot}"
    );
}

#[test]
fn os_cachyos_kernel_source_builder_failure_is_diagnostic() {
    let proj = Scratch::new("os-kernel-source-build-fail");
    let root = Scratch::new("os-kernel-source-build-fail-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(&pkg).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_cachyos_source_recipe(&pkg);
    write_cachyos_source_builder(&pkg, "#!/bin/sh\necho compiler missing >&2\nexit 7\n");
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1286]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot("cachyos_source_build_failed", diagnostic);
}

#[test]
fn os_cachyos_kernel_requires_first_party_source() {
    let proj = Scratch::new("os-missing-kernel");
    let root = Scratch::new("os-missing-kernel-root");
    fs::write(
        proj.join("config.jet"),
        "module box {\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("missing_cachyos_kernel", &stderr);
}

#[test]
fn os_systemd_init_requires_first_party_source() {
    let proj = Scratch::new("os-missing-systemd");
    let root = Scratch::new("os-missing-systemd-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    fs::create_dir_all(pkg.join("boot")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_bootlike_cachyos_artifacts(&pkg);
    write_cachyos_source_recipe(&pkg);
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1281]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot("missing_systemd_init", diagnostic);
}

#[test]
fn os_default_gnome_desktop_requires_first_party_packages() {
    let proj = Scratch::new("os-missing-gnome-desktop");
    let root = Scratch::new("os-missing-gnome-desktop-root");
    let kernel = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(kernel.join("boot")).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        kernel.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_bootlike_cachyos_artifacts(&kernel);
    write_cachyos_source_recipe(&kernel);
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    write_executable(&systemd.join("bin/systemd"), "#!/bin/sh\nexit 0\n");
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64, options: [ services.desktop.profile: .Default ] }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1288]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot("missing_gnome_desktop", diagnostic);
}

#[test]
fn os_cachyos_kernel_requires_boot_artifacts() {
    let proj = Scratch::new("os-missing-kernel-artifacts");
    let root = Scratch::new("os-missing-kernel-artifacts-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(&pkg).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1282]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot_trimmed("missing_cachyos_boot_artifacts", diagnostic);
}

#[test]
fn os_cachyos_kernel_rejects_text_boot_artifacts() {
    let proj = Scratch::new("os-text-kernel-artifacts");
    let root = Scratch::new("os-text-kernel-artifacts-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(pkg.join("boot")).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    fs::write(pkg.join("boot/vmlinuz-cachyos"), "not a kernel\n").unwrap();
    fs::write(pkg.join("boot/initrd-cachyos"), "not an initrd\n").unwrap();
    write_cachyos_source_recipe(&pkg);
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1282]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot_trimmed("missing_cachyos_boot_artifacts", diagnostic);
}

#[test]
fn os_cachyos_kernel_requires_source_recipe() {
    let proj = Scratch::new("os-missing-kernel-source");
    let root = Scratch::new("os-missing-kernel-source-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(pkg.join("boot")).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_bootlike_cachyos_artifacts(&pkg);
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1284]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot_trimmed("missing_cachyos_source_recipe", diagnostic);
}

#[test]
fn os_systemd_init_requires_init_artifact() {
    let proj = Scratch::new("os-missing-systemd-artifact");
    let root = Scratch::new("os-missing-systemd-artifact-root");
    let kernel = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(kernel.join("boot")).unwrap();
    fs::create_dir_all(&systemd).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        kernel.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_bootlike_cachyos_artifacts(&kernel);
    write_cachyos_source_recipe(&kernel);
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1283]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot("missing_systemd_init_artifact", diagnostic);
}

#[test]
fn os_missing_host_is_friendly_and_exits_2() {
    let root = Scratch::new("os-no-host");
    let out = jet()
        .args(["os", "build", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("missing_host", &stderr);
}

#[test]
fn os_unknown_host_lists_available_systems() {
    let root = Scratch::new("os-bad-host");
    let out = jet()
        .args(["os", "build", "nope", "--no-color", "--offline"])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("unknown_host", &stderr);
}

#[test]
fn os_missing_config_file_is_friendly() {
    let root = Scratch::new("os-no-config");
    let out = jet()
        .args(["os", "build", "/definitely/not/here@box", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("missing_config", &stderr);
}

#[test]
fn os_retired_option_namespace_is_snapshot_pinned() {
    let proj = Scratch::new("os-bad-namespace");
    let root = Scratch::new("os-bad-namespace-root");
    fs::write(
        proj.join("config.jet"),
        "module box {\n    system.box: { target: linux.x64, options: [net.hostName: \"box\"] }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("retired_namespace", &stderr);
}

#[test]
fn os_generations_are_newest_first_and_rollback_activates_prior() {
    let root = Scratch::new("os-gens-root");
    for name in ["first", "second"] {
        let out = jet()
            .args([
                "os",
                "switch",
                "halcyon",
                "--name",
                name,
                "--no-color",
                "--offline",
            ])
            .current_dir(config_example_dir())
            .env("JETPACK_ROOT", &root.path)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let list = jet()
        .args(["os", "generations", "halcyon", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    let second = stdout.find("second").unwrap();
    let first = stdout.find("first").unwrap();
    assert!(
        second < first,
        "generations should be newest-first: {stdout}"
    );
    let proof = fs::read_to_string(
        root.path
            .join("systems/generations/second/activation-proof.txt"),
    )
    .unwrap();
    assert!(proof.contains("service-risk"), "proof: {proof}");
    assert!(
        proof.contains("rollback-proof: pass previous=first"),
        "proof: {proof}"
    );

    let rollback = jet()
        .args(["os", "rollback", "halcyon", "first", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        rollback.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&rollback.stderr)
    );
    assert!(
        String::from_utf8_lossy(&rollback.stderr).contains("rolled back"),
        "stderr: {}",
        String::from_utf8_lossy(&rollback.stderr)
    );
    let same = jet()
        .args(["os", "rollback", "halcyon", "first", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(same.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&same.stderr);
    assert!(
        stderr.contains("no generation is available"),
        "stderr: {stderr}"
    );
}

#[test]
fn os_vm_run_real_tier_requires_nixpkgs_pin() {
    // E1291 (D-JOS-NIXBACKEND1=C): the hidden real-tier NixOS backend
    // refuses to generate when it can't map every declaration — here
    // there is no `sources:` entry that resolves to a nixpkgs pin, so
    // `map_system_to_nixos` reports it as unmapped instead of silently
    // dropping it. `jet os vm run` hits this before any tool/media check
    // because a fresh disk always routes through `cmd_vm_run_or_build`.
    let proj = Scratch::new("os-vm-real-no-nixpkgs-pin");
    let root = Scratch::new("os-vm-real-no-nixpkgs-pin-root");
    fs::write(
        proj.join("config.jet"),
        "module halcyon {\n    sources: {}\n    system.halcyon: {\n        target: linux.x64,\n        packages: [],\n        services: {},\n        options: [\n            network.hostName: halcyon,\n        ],\n    }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args([
            "os", "vm", "run", "halcyon", "--disk", "halcyon.qcow2", "--no-color", "--offline",
        ])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("real_tier_no_nixpkgs_pin", &stderr);
}

#[test]
fn os_vm_prove_requires_pinned_media_tools() {
    let root = Scratch::new("os-vm-tools-root");
    let tools = Scratch::new("os-vm-tools-empty");
    let out = jet()
        .args([
            "os",
            "vm",
            "prove",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot_trimmed("vm_tools_missing", &stderr);
    assert!(
        !root.join("systems/vm-proofs").exists(),
        "missing tools must not write VM proof artifacts"
    );
}

#[test]
fn os_vm_run_requires_proved_installed_disk() {
    let root = Scratch::new("os-vm-run-unproven-root");
    let tools = Scratch::new("os-vm-run-unproven-tools");
    write_fake_vm_tools(&tools.path, false);
    let out = jet()
        .args([
            "os",
            "vm",
            "run",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("vm_run_unproven", &stderr);
}

#[test]
fn os_vm_prove_real_tier_rejects_fake_toolchain() {
    let root = Scratch::new("os-vm-real-root");
    let tools = Scratch::new("os-vm-real-tools");
    write_fake_vm_tools(&tools.path, true);
    let out = jet()
        .args([
            "os",
            "vm",
            "prove",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--real",
            "--name",
            "vm-real",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot_normalized(
        "vm_real_fake_tools",
        &stderr,
        &[(tools.path.to_str().unwrap(), "<tools>")],
    );
    assert!(
        !root.join("systems/vm-proofs").exists(),
        "real tier must fail before writing replacement proof with fake tools"
    );
}

#[test]
fn os_vm_prove_writes_media_bound_harness() {
    let root = Scratch::new("os-vm-proof-root");
    let tools = Scratch::new("os-vm-proof-tools");
    write_fake_vm_tools(&tools.path, false);
    let out = jet()
        .args([
            "os",
            "vm",
            "prove",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--name",
            "vm-proof",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error[E1285]"),
        "stderr should refuse harness-only proof: {stderr}"
    );
    let proof_dir = root.path.join("systems/vm-proofs");
    let proof = fs::read_dir(&proof_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
        .expect("vm proof json");
    let data = fs::read_to_string(&proof).unwrap();
    assert!(
        data.contains("\"state\":\"harness-ready\""),
        "proof should not claim guest pass before QEMU run: {data}"
    );
    assert!(
        data.contains("\"proof_tier\":\"plumbing\""),
        "fake-tool harness should be labeled plumbing tier: {data}"
    );
    assert!(data.contains("\"disk\":\"halcyon.qcow2\""), "proof: {data}");
    assert!(
        data.contains("\"media_proof\":"),
        "proof should bind installer media: {data}"
    );
    assert!(
        data.contains("\"expected_guest_proof\":"),
        "proof should name the guest proof artifact path: {data}"
    );
    assert!(
        data.contains("\"sha256\":"),
        "proof should hash tools: {data}"
    );
    assert!(
        data.contains("rollback-generation-bootable"),
        "proof should name guest assertions: {data}"
    );
    assert!(
        data.contains("terminal-login-ready")
            && data.contains("desktop-session-ready")
            && data.contains("graphical-console-ready")
            && data.contains("desktop-launchers-run"),
        "proof should require terminal and desktop readiness: {data}"
    );
    assert!(
        data.contains("\"phase\":\"boot-installer\""),
        "proof should record QEMU boot phase: {data}"
    );
    assert!(
        data.contains("\"phase\":\"boot-installed-disk\""),
        "proof should record reboot phase: {data}"
    );
    assert!(
        data.contains("\"phase\":\"boot-graphical-desktop\""),
        "proof should record graphical desktop phase: {data}"
    );
    assert!(
        data.contains("\"-cdrom\"") && data.contains("jetos-installer-halcyon.iso"),
        "installer phase should boot the ISO media: {data}"
    );
    let installed_phase = data
        .split("\"phase\":\"boot-installed-disk\"")
        .nth(1)
        .and_then(|rest| rest.split("\"phase\":\"boot-graphical-desktop\"").next())
        .expect("installed-disk command in proof");
    assert!(
        installed_phase.contains("\"-boot\",\"c\"")
            && installed_phase.contains("file=halcyon.qcow2,format=qcow2,if=ide")
            && !installed_phase.contains("\"-kernel\"")
            && !installed_phase.contains("\"-initrd\"")
            && !installed_phase.contains("\"-append\""),
        "installed-disk phase should boot firmware/disk, not direct kernel: {installed_phase}"
    );
    assert!(
        data.contains("\"-kernel\"") && data.contains("/boot/kernel"),
        "graphical proof should direct-boot the generation kernel: {data}"
    );
    assert!(
        data.contains("\"-initrd\"") && data.contains("/boot/initrd"),
        "graphical proof should direct-boot the generation initrd: {data}"
    );
    assert!(
        data.contains("jetos.mode=desktop-verify")
            && data.contains("\"-display\"")
            && data.contains("vnc=127.0.0.1:0")
            && data.contains("\"-vga\"")
            && data.contains("\"std\""),
        "graphical proof should expose a fixed VNC-backed stdvga display: {data}"
    );
    assert!(
        data.contains("qemu-xhci,id=xhci")
            && data.contains("usb-kbd,bus=xhci.0")
            && data.contains("usb-tablet,bus=xhci.0"),
        "graphical proof should expose explicit USB input devices for VNC use: {data}"
    );
    assert!(
        data.contains("rdinit=/jetos/init"),
        "graphical QEMU proof should boot the JetOS verifier overlay script: {data}"
    );
    assert!(
        data.contains("jetos.generation=vm-proof"),
        "QEMU proof should bind guest boot to the generation name: {data}"
    );
    assert!(
        data.contains("console=ttyS0"),
        "QEMU proof needs serial output for guest proof marker: {data}"
    );
    assert!(
        proof
            .with_file_name("halcyon-vm-proof-vm-proof.run")
            .join("boot-graphical-desktop.stdout")
            .is_file(),
        "vm prove should run the recorded QEMU phases"
    );
    assert!(
        root.path
            .join("systems/images/jetos-installer-halcyon.iso.proof.json")
            .is_file(),
        "vm prove should build media proof first"
    );
}

#[test]
fn os_vm_prove_runs_qemu_and_records_guest_proof() {
    let root = Scratch::new("os-vm-run-root");
    let tools = Scratch::new("os-vm-run-tools");
    write_fake_vm_tools(&tools.path, true);
    let out = jet()
        .args([
            "os",
            "vm",
            "prove",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--name",
            "vm-live",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let proof = root
        .path
        .join("systems/vm-proofs/halcyon-vm-live-vm-proof.json");
    let guest = root
        .path
        .join("systems/vm-proofs/halcyon-vm-live-vm-proof-guest-proof.json");
    let final_proof = fs::read_to_string(&proof).unwrap();
    assert!(
        final_proof.contains("\"state\":\"guest-passed\""),
        "proof: {final_proof}"
    );
    assert!(
        final_proof.contains("\"proof_tier\":\"plumbing\""),
        "regular VM proof is harness plumbing until --real passes: {final_proof}"
    );
    assert!(
        final_proof.contains("\"guest_proof_sha256\""),
        "proof: {final_proof}"
    );
    let guest_proof = fs::read_to_string(&guest).unwrap();
    assert!(
        guest_proof.contains("\"serial_report\""),
        "guest proof: {guest_proof}"
    );
    assert!(
        guest_proof.contains("halcyon") && guest_proof.contains("vm-live"),
        "guest serial report should bind host and generation: {guest_proof}"
    );
    assert!(
        guest_proof.contains("\"qemu-system-x86_64\""),
        "guest proof should bind the runner toolchain: {guest_proof}"
    );
    let boot_log = root
        .path
        .join("systems/vm-proofs/halcyon-vm-live-vm-proof.run/boot-graphical-desktop.stdout");
    assert!(
        fs::read_to_string(&boot_log)
            .unwrap()
            .contains("JETOS_GUEST_PROOF"),
        "boot log should carry guest proof marker"
    );

    let run = jet()
        .args([
            "os",
            "vm",
            "run",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("booting jetos VM halcyon generation vm-live"),
        "stderr: {stderr}"
    );
    assert!(
        stdout.contains("JETOS_GUEST_PROOF"),
        "interactive run should launch QEMU with the proved disk: {stdout}"
    );
}

#[test]
fn os_vm_test_runs_declared_scenario_and_records_proof() {
    let project = Scratch::new("os-vmtest-project");
    copy_dir_recursive(&config_example_dir(), &project.path);
    let mut config = fs::read_to_string(project.path.join("config.jet")).unwrap();
    config.push_str(
        r#"

module vmtest.ssh-smoke {
    hosts: { halcyon: system.halcyon }
    run: test {
        halcyon.wait_for_boot()
        halcyon.assert_unit_active(.openssh)
        halcyon.assert_port_open(22)
    }
}
"#,
    );
    fs::write(project.path.join("config.jet"), config).unwrap();
    let root = Scratch::new("os-vmtest-root");
    let tools = Scratch::new("os-vmtest-tools");
    write_fake_vm_tools(&tools.path, true);
    let out = jet()
        .args([
            "os",
            "vm",
            "test",
            "ssh-smoke",
            "--disk",
            "ssh-smoke.qcow2",
            "--name",
            "vmtest-proof",
            "--no-color",
            "--offline",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let proof = root
        .path
        .join("systems/vm-tests/ssh-smoke-vmtest-proof.json");
    let data = fs::read_to_string(&proof).unwrap();
    assert!(
        data.contains("\"kind\":\"jetos.vmtest.proof\"")
            && data.contains("\"state\":\"passed\"")
            && data.contains("\"name\":\"ssh-smoke\""),
        "proof: {data}"
    );
    assert!(
        data.contains("\"name\": \"halcyon\"")
            && data.contains("\"system\": \"halcyon\"")
            && data.contains("\"disk\": \"ssh-smoke.qcow2\""),
        "proof: {data}"
    );
    assert!(
        data.contains("\"wait_for_boot\"")
            && data.contains("\"assert_unit_active\"")
            && data.contains("\"assert_port_open\""),
        "proof should capture typed assertion methods: {data}"
    );
    assert!(
        data.contains("halcyon.assert_unit_active(.openssh)"),
        "proof should carry the source test body for replay: {data}"
    );
    assert!(
        root.path
            .join("systems/vm-proofs/halcyon-vmtest-proof-vm-proof.json")
            .is_file(),
        "vmtest should reuse the install/reboot VM proof harness"
    );
}

/// Scrape a string field's value out of a harness/proof JSON blob (the VM
/// proof files are flat string fields, so a split is enough).
fn harness_json_field(text: &str, key: &str) -> String {
    let needle = format!("\"{key}\":\"");
    let (_, rest) = text
        .split_once(&needle)
        .unwrap_or_else(|| panic!("harness JSON lacks `{key}`: {text}"));
    rest.split('"').next().unwrap().to_string()
}

#[test]
fn os_vm_prove_accepts_matching_guest_proof() {
    let root = Scratch::new("os-vm-guest-proof-root");
    let tools = Scratch::new("os-vm-guest-proof-tools");
    write_fake_vm_tools(&tools.path, false);
    let args = [
        "os",
        "vm",
        "prove",
        "halcyon",
        "--disk",
        "halcyon.qcow2",
        "--name",
        "vm-proof",
        "--no-color",
        "--offline",
    ];
    let first = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(2));
    let proof = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof.json");
    let guest = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof-guest-proof.json");
    let media_proof = root
        .path
        .join("systems/images/jetos-installer-halcyon.iso.proof.json");
    let harness = fs::read_to_string(&proof).unwrap();
    fs::write(
        &guest,
        format!(
            "{{\"state\":\"guest-passed\",\"host\":\"halcyon\",\"generation\":\"vm-proof\",\"disk\":\"halcyon.qcow2\",\"media_proof\":\"{}\",\"media_proof_sha256\":\"{}\",\"installer_iso_fingerprint\":\"{}\",\"assertions\":[\"current-generation-matches\",\"packages-present\",\"services-active\",\"network-up\",\"rollback-generation-bootable\",\"terminal-login-ready\",\"desktop-session-ready\",\"graphical-console-ready\",\"desktop-launchers-run\"],\"toolchain\":\"{}\"}}\n",
            test_json_escape(&media_proof.display().to_string()),
            harness_json_field(&harness, "media_proof_sha256"),
            harness_json_field(&harness, "installer_iso_fingerprint"),
            test_json_escape(&harness)
        ),
    )
    .unwrap();

    let second = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let final_proof = fs::read_to_string(&proof).unwrap();
    assert!(
        final_proof.contains("\"state\":\"guest-passed\""),
        "proof: {final_proof}"
    );
    assert!(
        final_proof.contains("\"guest_proof_sha256\""),
        "proof: {final_proof}"
    );
}

#[test]
fn os_vm_prove_rejects_incomplete_guest_proof() {
    let root = Scratch::new("os-vm-stale-guest-proof-root");
    let tools = Scratch::new("os-vm-stale-guest-proof-tools");
    write_fake_vm_tools(&tools.path, false);
    let args = [
        "os",
        "vm",
        "prove",
        "halcyon",
        "--disk",
        "halcyon.qcow2",
        "--name",
        "vm-proof",
        "--no-color",
        "--offline",
    ];
    let first = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(2));
    let proof = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof.json");
    let guest = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof-guest-proof.json");
    let media_proof = root
        .path
        .join("systems/images/jetos-installer-halcyon.iso.proof.json");
    let harness = fs::read_to_string(&proof).unwrap();
    fs::write(
        &guest,
        format!(
            "{{\"state\":\"guest-passed\",\"host\":\"halcyon\",\"generation\":\"vm-proof\",\"disk\":\"halcyon.qcow2\",\"media_proof\":\"{}\",\"media_proof_sha256\":\"{}\",\"installer_iso_fingerprint\":\"{}\",\"assertions\":[\"current-generation-matches\",\"packages-present\",\"services-active\",\"network-up\"],\"toolchain\":\"{}\"}}\n",
            test_json_escape(&media_proof.display().to_string()),
            harness_json_field(&harness, "media_proof_sha256"),
            harness_json_field(&harness, "installer_iso_fingerprint"),
            test_json_escape(&harness)
        ),
    )
    .unwrap();

    let second = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("guest assertions did not match"),
        "stderr: {stderr}"
    );
    let final_proof = fs::read_to_string(&proof).unwrap();
    assert!(
        final_proof.contains("\"state\":\"harness-ready\""),
        "proof should remain unpromoted: {final_proof}"
    );
}

#[test]
fn os_vm_prove_rejects_stale_guest_generation() {
    let root = Scratch::new("os-vm-stale-generation-root");
    let tools = Scratch::new("os-vm-stale-generation-tools");
    write_fake_vm_tools(&tools.path, false);
    let args = [
        "os",
        "vm",
        "prove",
        "halcyon",
        "--disk",
        "halcyon.qcow2",
        "--name",
        "vm-proof",
        "--no-color",
        "--offline",
    ];
    let first = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(2));
    let proof = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof.json");
    let guest = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof-guest-proof.json");
    let media_proof = root
        .path
        .join("systems/images/jetos-installer-halcyon.iso.proof.json");
    let harness = fs::read_to_string(&proof).unwrap();
    fs::write(
        &guest,
        format!(
            "{{\"state\":\"guest-passed\",\"host\":\"halcyon\",\"generation\":\"older-generation\",\"disk\":\"halcyon.qcow2\",\"media_proof\":\"{}\",\"assertions\":[\"current-generation-matches\",\"packages-present\",\"services-active\",\"network-up\",\"rollback-generation-bootable\",\"terminal-login-ready\",\"desktop-session-ready\",\"graphical-console-ready\",\"desktop-launchers-run\"],\"toolchain\":\"{}\"}}\n",
            test_json_escape(&media_proof.display().to_string()),
            test_json_escape(&harness)
        ),
    )
    .unwrap();

    let second = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("`generation` expected `vm-proof`, found `older-generation`"),
        "stderr: {stderr}"
    );
}

#[test]
fn os_image_writes_jetos_installer_media_proof() {
    let root = Scratch::new("os-image-root");
    let tools = Scratch::new("os-image-tools");
    let boot = Scratch::new("os-image-boot");
    fs::write(boot.join("kernel"), "MZ test kernel\nHdrS\n").unwrap();
    fs::write(
        boot.join("initrd"),
        b"070701 test initrd with embedded zstd magic \x28\xb5\x2f\xfd\n",
    )
    .unwrap();
    write_fake_vm_tools(&tools.path, true);
    let out = jet()
        .args([
            "os",
            "image",
            "halcyon",
            "--manual",
            "/dev/sda",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .env("JETOS_CACHYOS_KERNEL", boot.join("kernel"))
        .env("JETOS_CACHYOS_INITRD", boot.join("initrd"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let proof = root
        .path
        .join("systems/images")
        .join("jetos-installer-halcyon.iso.proof.json");
    let data = fs::read_to_string(&proof).unwrap();
    assert!(data.contains("\"brand\":\"jetos\""), "data: {data}");
    assert!(data.contains("\"kind\":\"hybrid-iso\""), "data: {data}");
    assert!(data.contains("\"state\":\"built\""), "data: {data}");
    assert!(data.contains("\"sha256\":"), "data: {data}");
    assert!(
        root.path
            .join("systems/images/jetos-installer-halcyon.iso")
            .is_file(),
        "expected real hybrid ISO artifact"
    );
    let variant_proof = root
        .path
        .join("systems/images")
        .join("jetos-image-variants-halcyon.proof.json");
    let variants = fs::read_to_string(&variant_proof).unwrap();
    assert!(
        variants.contains("\"kind\":\"jetos.image-variants\"")
            && (variants.contains("\"proof\":\"image-variants-smoke-proved\"")
                || variants.contains("\"proof\":\"image-variants-staged\""))
            && variants.contains("\"kind\": \"qcow2\"")
            && variants.contains("\"kind\": \"sd\"")
            && variants.contains("\"kind\": \"netboot-ipxe\""),
        "variants: {variants}"
    );
    // D-JOS-IMAGEPROOF1=C: sparse raw/sd markers must never claim built.
    assert!(
        variants.contains("\"kind\": \"raw\"") && variants.contains("\"state\": \"staged\""),
        "raw sparse marker must be staged: {variants}"
    );
    assert!(
        variants.contains("\"kind\": \"sd\"")
            && variants
                .split("\"kind\": \"sd\"")
                .nth(1)
                .map(|rest| rest.contains("\"state\": \"staged\""))
                .unwrap_or(false),
        "sd sparse marker must be staged: {variants}"
    );
    for artifact in [
        "jetos-halcyon.qcow2",
        "jetos-halcyon.raw",
        "jetos-halcyon-sd.img",
        "jetos-halcyon-netboot/vmlinuz",
        "jetos-halcyon-netboot/initrd",
        "jetos-halcyon-netboot/ipxe.conf",
    ] {
        assert!(
            root.path.join("systems/images").join(artifact).is_file(),
            "expected image variant artifact {artifact}"
        );
    }
    let ipxe = fs::read_to_string(
        root.path
            .join("systems/images/jetos-halcyon-netboot/ipxe.conf"),
    )
    .unwrap();
    assert!(
        ipxe.contains("kernel vmlinuz")
            && ipxe.contains("initrd initrd")
            && ipxe.contains("jetos.mode=run"),
        "ipxe: {ipxe}"
    );
    let staging = root
        .path
        .join("systems/images")
        .join("jetos-installer-halcyon.iso.d");
    let transaction = fs::read_to_string(staging.join("install/transaction.json")).unwrap();
    assert!(
        transaction.contains("\"disk\":\"/dev/sda\""),
        "tx: {transaction}"
    );
    assert!(
        transaction.contains("\"partition-gpt\"")
            && transaction.contains("\"mkfs.vfat-esp\"")
            && transaction.contains("\"install-limine-esp\""),
        "tx: {transaction}"
    );
    let install = fs::read_to_string(staging.join("install/install.sh")).unwrap();
    assert!(
        install.contains("sfdisk --wipe always"),
        "install: {install}"
    );
    assert!(
        install.contains("blockdev --rereadpt"),
        "install: {install}"
    );
    assert!(install.contains("mkfs.vfat -F 32"), "install: {install}");
    assert!(install.contains("mkfs.ext4"), "install: {install}");
    assert!(
        install.contains("EFI/BOOT/BOOTX64.EFI") && install.contains("installed-limine.conf"),
        "install: {install}"
    );
    assert!(install.contains("install-proof.json"), "install: {install}");
    let verify = fs::read_to_string(staging.join("install/guest-verify.sh")).unwrap();
    assert!(
        verify.contains("system=\"$root/var/lib/jetos/generations/")
            && verify.contains("need \"$system/plan.json\""),
        "verify: {verify}"
    );
    assert!(
        verify.contains("jetos verifier: missing $path"),
        "verify: {verify}"
    );
    assert!(
        verify.contains("LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda"),
        "verify should probe partitioned installed disks: {verify}"
    );
    assert!(
        verify.contains("terminal/facts.json")
            && verify.contains("serial-getty@ttyS0.service")
            && verify.contains("desktop/facts.json")
            && verify.contains("sw/bin/gdm")
            && verify.contains("sw/bin/gnome-session")
            && verify.contains("sw/bin/gnome-shell")
            && verify.contains("jetos-desktop-session")
            && verify.contains("jetos-studio")
            && verify.contains("--jetos-proof")
            && verify.contains("desktop-launchers-run"),
        "verify: {verify}"
    );
    assert!(
        verify.contains("for svc in openssh backup metrics"),
        "verify: {verify}"
    );
    assert!(verify.contains("\"rollback\""), "verify: {verify}");
    let initrd_bytes = fs::read(staging.join("boot/initrd")).unwrap();
    assert!(
        initrd_bytes.starts_with(b"070701"),
        "raw newc initrd with embedded compressed payload bytes must stay a raw cpio archive"
    );
    let initrd = String::from_utf8_lossy(&initrd_bytes);
    assert!(initrd.contains("jetos.mode=install"), "initrd: {initrd}");
    assert!(initrd.contains("jetos.mode=verify"), "initrd: {initrd}");
    assert!(
        initrd.contains("jetos.mode=desktop-verify"),
        "initrd: {initrd}"
    );
    assert!(
        initrd.contains("jetos/init"),
        "initrd should carry the JetOS init dispatcher: {initrd}"
    );
    assert!(
        initrd.contains("mount -t proc proc /proc")
            && initrd.contains("mount -t devtmpfs devtmpfs /dev"),
        "initrd dispatcher should prepare proc/dev before reading cmdline: {initrd}"
    );
    assert!(
        initrd.contains("LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda"),
        "initrd should probe partitioned installed disks before installer fallback: {initrd}"
    );
    assert!(
        initrd.contains("use_system_nix")
            && initrd.contains("ln -s \"$system_nix\" /nix"),
        "initrd should expose the installed generation nix store when no initrd /nix exists: {initrd}"
    );
    assert!(
        initrd.contains("jetos/tools/bin/sh")
            && initrd.contains("jetos/tools/bin/sfdisk")
            && initrd.contains("jetos/tools/bin/blockdev"),
        "initrd should carry installer partition tools: {initrd}"
    );
    assert!(
        initrd.contains("exec chroot /sysroot /run/current-system/sbin/init")
            && initrd.contains("SYSTEMD_UNIT_PATH=/etc/systemd/system")
            && initrd.contains("for top in etc sbin sw share studio init systemd lib usr network")
            && initrd.contains("ln -s \"$generation_target/$top\" \"/sysroot/$top\""),
        "initrd run mode should hand off to installed current-system, not fallback shell: {initrd}"
    );
    assert!(
        initrd.contains("jetos/modules/atkbd.ko.xz")
            && initrd.contains("jetos/modules/usbhid.ko.xz")
            && initrd.contains("jetos/modules/xhci-hcd.ko.xz"),
        "initrd should carry keyboard and USB HID modules for VNC input: {initrd}"
    );
    assert!(
        initrd.contains("JETOS_GUEST_PROOF"),
        "initrd should carry guest proof reporter: {initrd}"
    );
    let limine = fs::read_to_string(staging.join("boot/limine.conf")).unwrap();
    assert!(
        limine.contains("/Install jetos 26.10 (Apex) — halcyon")
            && limine.contains("cmdline: console=ttyS0 rdinit=/jetos/init")
            && limine.contains("jetos.disk=/dev/sda"),
        "limine: {limine}"
    );
    let installed_limine =
        fs::read_to_string(staging.join("boot/installed-limine.conf")).unwrap();
    assert!(
        installed_limine.contains("/jetos 26.10 (Apex) — halcyon verify"),
        "installed limine: {installed_limine}"
    );
    let installer_os_release = fs::read_to_string(
        staging.join("jetos/current-system/etc/os-release"),
    )
    .unwrap();
    let installer_usr_os_release = fs::read_to_string(
        staging.join("jetos/current-system/usr/lib/os-release"),
    )
    .unwrap();
    assert_eq!(installer_os_release, installer_usr_os_release);
    assert!(
        installer_os_release.contains("PRETTY_NAME=\"jetos 26.10 (Apex)\"")
            && installer_os_release.contains("VERSION_CODENAME=apex")
    );
    let installer_wallpaper = fs::read_to_string(
        staging.join("jetos/current-system/share/backgrounds/jetos/apex.svg"),
    )
    .unwrap();
    assert!(
        installer_wallpaper.contains("jetos 26.10 Apex")
            && installer_wallpaper.contains("linearGradient")
            && installer_wallpaper.len() > 1_000,
        "installer must contain projected wallpaper bytes"
    );
    for text in [
        limine.as_str(),
        installed_limine.as_str(),
        installer_os_release.as_str(),
        installer_wallpaper.as_str(),
    ] {
        assert!(!text.contains("NixOS") && !text.contains("Yarara"));
    }
    assert!(
        staging.join("boot/efiboot.img").is_file(),
        "installer media should carry a UEFI FAT ESP boot image"
    );
    assert!(
        staging.join("EFI/BOOT/BOOTX64.EFI").is_file(),
        "installer media should expose the EFI loader for target ESP install"
    );
    assert!(
        staging.join("boot/installed-limine.conf").is_file(),
        "installer media should carry installed-disk Limine config"
    );
    assert_eq!(
        fs::read_to_string(staging.join("limine.conf")).unwrap(),
        limine,
        "Limine config should be available at the ISO root and /boot"
    );
    assert!(staging.join("jetos/provenance.json").is_file());
    assert!(
        staging
            .join("jetos/current-system/etc/systemd/system/openssh.service")
            .is_file(),
        "installer media should carry the full generation"
    );
    assert!(
        !fs::symlink_metadata(staging.join("jetos/current-system/plan.json"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "installer media must be self-contained"
    );
    assert!(
        !fs::symlink_metadata(staging.join("jetos/current-system/sbin/init"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "installer media must not point back to the host root"
    );
}

#[test]
fn os_init_writes_guided_ext4_config() {
    let proj = Scratch::new("os-init");
    let out = jet()
        .args(["os", "init", "laptop", "--no-color"])
        .current_dir(&proj.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = fs::read_to_string(proj.join("config.jet")).unwrap();
    assert!(config.contains("system.laptop"), "config: {config}");
    assert!(config.contains("filesystem.layout"), "config: {config}");
    assert!(config.contains("network.hostName"), "config: {config}");
}

#[test]
fn offline_without_fixtures_errors() {
    let root = Scratch::new("root");
    let out = jetpack()
        .args(["build", "nixpkgs:fastfetch", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1276"), "stderr: {stderr}");
    assert!(stderr.contains("nixpkgs:fastfetch"), "stderr: {stderr}");
}

// ── D-JPK-FILES Phase 2b: jetpack.toml wiring ─────────────────────────────

/// The committed multi-package monorepo example dir.
fn mono_example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-mono")
}

#[test]
fn malformed_jetpack_toml_fires_e1214_from_cli() {
    // I4/D-JPK-FILES Phase 2b: E1214 must be reachable from real `jetpack`
    // usage, not just the in-module unit test. Create a scratch project whose
    // jetpack.toml has a malformed line, run `jetpack build`, and verify that
    // E1214 appears in stderr with exit code 2.
    let proj = Scratch::new("bad-toml-e1214");
    let root = Scratch::new("bad-toml-root");
    // Write a jetpack.toml with a malformed line (not a key="value" or [table]).
    fs::write(
        proj.join("jetpack.toml"),
        "[repo]\nname = \"test\"\nbad line here\n",
    )
    .unwrap();
    // Also write a minimal env.jet so the `nothing to do` error isn't hit first.
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [pkg.source(\"nixpkgs\"), pkg.packages([\"ripgrep\"])];\n}\n",
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1214"),
        "expected E1214 in stderr: {stderr}"
    );
    assert!(
        stderr.contains("jetpack.toml"),
        "expected jetpack.toml in error: {stderr}"
    );
}

#[test]
fn malformed_jetpack_toml_fires_e1215_from_cli() {
    // I4/D-JPK-FILES Phase 2b: E1215 must be reachable from real `jetpack`
    // usage. An unknown table name fires E1215 with did-you-mean.
    let proj = Scratch::new("bad-toml-e1215");
    let root = Scratch::new("bad-toml-root2");
    fs::write(proj.join("jetpack.toml"), "[workspace]\nfoo = \"bar\"\n").unwrap();
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [pkg.source(\"nixpkgs\"), pkg.packages([\"ripgrep\"])];\n}\n",
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1215"),
        "expected E1215 in stderr: {stderr}"
    );
}

#[test]
fn jetpack_toml_packages_fires_e1225_from_cli() {
    // D-WORKSPACE1: the old `[packages]` monorepo index moved to
    // `workspace.jet`; keep a real CLI test so the migration diagnostic is
    // reachable from user commands.
    let proj = Scratch::new("bad-toml-e1225");
    let root = Scratch::new("bad-toml-root3");
    fs::write(
        proj.join("jetpack.toml"),
        "[packages]\ngreeter = \"packages/greeter/pkg.jet\"\n",
    )
    .unwrap();
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [pkg.source(\"nixpkgs\"), pkg.packages([\"ripgrep\"])];\n}\n",
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1225"),
        "expected E1225 in stderr: {stderr}"
    );
    assert!(
        stderr.contains("workspace.jet"),
        "expected workspace.jet migration hint: {stderr}"
    );
}

#[test]
fn jetpack_toml_sources_merge_into_cwd_table() {
    // D-JPK-FILES Phase 2b: `[sources]` declared in jetpack.toml are folded
    // into the source table so env.jet can reference them by name. Create a
    // project whose jetpack.toml declares a named source and whose env.jet
    // references it — the build should resolve via the folded table.
    let base = Scratch::new("toml-sources");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from toml-source\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // jetpack.toml declares `mine` as a path source (no via — inferred as core).
    fs::write(
        proj.join("jetpack.toml"),
        format!("[sources]\nmine = \"path@{}\"\n", repo.to_string_lossy()),
    )
    .unwrap();
    // env.jet references `mine:hello` — the source name is resolved from jetpack.toml.
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [\n        pkg.source(\"mine\", \"path:PLACEHOLDER\", \"core\");\n        pkg.packages([\"mine:hello\"]);\n    ];\n}\n".replace(
            "path:PLACEHOLDER",
            &format!("path:{}", repo.to_string_lossy()),
        ),
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn mono_example_has_two_pkg_jet_members() {
    // D-WORKSPACE1: the committed monorepo example now uses workspace.jet
    // instead of the retired jetpack.toml [packages] index.
    let mono = mono_example_dir();
    assert!(
        mono.join("workspace.jet").exists(),
        "workspace.jet missing from mono example"
    );
    let greeter_pkg = mono.join("packages/greeter/pkg.jet");
    let logger_pkg = mono.join("packages/logger/pkg.jet");
    assert!(
        greeter_pkg.exists(),
        "packages/greeter/pkg.jet missing: {greeter_pkg:?}"
    );
    assert!(
        logger_pkg.exists(),
        "packages/logger/pkg.jet missing: {logger_pkg:?}"
    );
    let workspace_src = fs::read_to_string(mono.join("workspace.jet")).unwrap();
    assert!(
        workspace_src.contains("find(\"./packages\")"),
        "workspace.jet should use find-based member discovery"
    );
}

// ── Card #99 T4: build-from-source surface (build states / vendor / audit) ────

#[test]
fn jet_build_reports_source_states() {
    // T4: `jetpack build` reports how each package was satisfied. A first build
    // of a core package is `built`; the content-addressed re-build is `cached`.
    let (_base, proj, root) = core_hello_project("t4-build");
    let run = || {
        jetpack()
            .args(["build", "--no-color"])
            .current_dir(&proj)
            .env("JETPACK_ROOT", &root)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let out1 = String::from_utf8_lossy(&first.stderr);
    assert!(
        out1.contains("built"),
        "first build must report `built`: {out1}"
    );
    assert!(
        out1.contains("1 built"),
        "summary must count the built package: {out1}"
    );

    let second = run();
    assert!(second.status.success());
    let out2 = String::from_utf8_lossy(&second.stderr);
    assert!(
        out2.contains("cached"),
        "re-build of the same content must report `cached`: {out2}"
    );
    assert!(
        out2.contains("1 cached"),
        "summary must count the cache hit: {out2}"
    );
}

#[test]
fn jet_build_rejects_cache_after_manifest_semantics_change() {
    let (base, proj, root) = core_hello_project("truth-manifest-identity");
    let manifest = base.join("jet-pkgs/pkg.jet");
    fs::write(
        &manifest,
        "payload: { name: \"demo\", version: \"1.0.0\" }\npackages: { hello: executable }\n",
    )
    .unwrap();
    let run = || {
        jetpack()
            .args(["build", "--no-color"])
            .current_dir(&proj)
            .env("JETPACK_ROOT", &root)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    fs::write(
        &manifest,
        "payload: { name: \"demo\", version: \"2.0.0\" }\npackages: { hello: executable }\n",
    )
    .unwrap();
    let rejected = run();
    assert!(!rejected.status.success());
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("E2604"), "stderr: {stderr}");
    assert!(
        stderr.contains("recipe identity verification"),
        "stderr: {stderr}"
    );
}

#[test]
fn two_process_reverse_package_order_does_not_deadlock() {
    let base = Scratch::new("reverse-order-leases");
    let repo = base.join("repo");
    let root = base.join("root");
    for name in ["a", "b"] {
        let package = repo.join(format!("pkgs/{name}"));
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::write(package.join(format!("{name}.jet")), format!("module {name} {{ }}\n"))
            .unwrap();
        let tool = package.join(format!("bin/{name}"));
        fs::write(&tool, format!("#!/bin/sh\necho {name}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(tool, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    fs::write(
        repo.join("pkg.jet"),
        "payload: { name: \"pair\", version: \"1.0.0\" }\npackages: { a: executable, b: executable }\n",
    )
    .unwrap();
    let write_project = |name: &str, packages: &[&str]| {
        let project = base.join(name);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("env.jet"),
            format!(
                "use jetpack as pkg;\npub fn shell() -> [JSON] {{\n return [pkg.source(\"mine\", \"path:{}\", \"core\"); pkg.packages([{}]);];\n}}\n",
                repo.display(),
                packages
                    .iter()
                    .map(|package| format!("\"mine:{package}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
        .unwrap();
        project
    };
    let ab = write_project("ab", &["a", "b"]);
    let ba = write_project("ba", &["b", "a"]);
    let seeded = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&ab)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(seeded.status.success(), "stderr: {}", String::from_utf8_lossy(&seeded.stderr));
    let spawn = |project: &Path| {
        jetpack()
            .args(["enter", "--no-color", "--trust", "--", "/bin/sh", "-c", "true"])
            .current_dir(project)
            .env("JETPACK_ROOT", &root)
            .env("PATH", "/usr/bin:/bin")
            .spawn()
            .unwrap()
    };
    let first = spawn(&ab);
    let second = spawn(&ba);
    assert!(first.wait_with_output().unwrap().status.success());
    assert!(second.wait_with_output().unwrap().status.success());
}

#[test]
fn jet_build_never_reports_deleted_output_as_cached() {
    let (_base, proj, root) = core_hello_project("truth-deleted-cache");
    let run = || {
        jetpack()
            .args(["build", "--no-color"])
            .current_dir(&proj)
            .env("JETPACK_ROOT", &root)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    let roots = jetpack::Store::Roots {
        root: root.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::find_by_reference(&roots, "mine:hello").unwrap();
    make_tree_writable(Path::new(&entry.out));
    fs::remove_dir_all(&entry.out).unwrap();

    let rejected = run();
    assert!(!rejected.status.success());
    let rejected_stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(rejected_stderr.contains("E2604"), "stderr: {rejected_stderr}");
    let rebuilt = run();
    assert!(rebuilt.status.success());
    let stderr = String::from_utf8_lossy(&rebuilt.stderr);
    assert!(stderr.contains("built"), "deleted output must rebuild: {stderr}");
    assert!(
        !stderr.contains("1 cached"),
        "deleted output must never count as cache hit: {stderr}"
    );
}

#[test]
fn jet_build_never_reports_tampered_output_as_cached() {
    let (_base, proj, root) = core_hello_project("truth-tampered-cache");
    let run = || {
        jetpack()
            .args(["build", "--no-color"])
            .current_dir(&proj)
            .env("JETPACK_ROOT", &root)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    let roots = jetpack::Store::Roots {
        root: root.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::find_by_reference(&roots, "mine:hello").unwrap();
    make_tree_writable(Path::new(&entry.out));
    fs::write(Path::new(&entry.out).join("bin/hello"), "tampered").unwrap();

    let rejected = run();
    assert!(!rejected.status.success());
    let rejected_stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(rejected_stderr.contains("E2604"), "stderr: {rejected_stderr}");
    let rebuilt = run();
    assert!(rebuilt.status.success());
    let stderr = String::from_utf8_lossy(&rebuilt.stderr);
    assert!(stderr.contains("built"), "tampered output must rebuild: {stderr}");
    assert!(
        !stderr.contains("1 cached"),
        "tampered output must never count as cache hit: {stderr}"
    );
}

#[test]
fn jet_vendor_writes_pinned_sources() {
    // T4 / D-BFS1: `jetpack vendor` copies each source-built package and writes a
    // `<name>.sha256` pin (the A4 output hash) so a later build is reproducible.
    let (_base, proj, root) = core_hello_project("t4-vendor");
    // Realize first so the hangar has a source-built object.
    let built = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(built.status.success());

    let out = jetpack()
        .args(["vendor", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let pin = proj.join("vendor/hello.sha256");
    assert!(pin.is_file(), "vendor must write a per-package sha256 pin");
    let hash = fs::read_to_string(&pin).unwrap();
    assert!(
        hash.trim().starts_with("sha256-"),
        "the pin must be a content hash: {hash}"
    );
    assert!(
        proj.join("vendor/hello").is_dir(),
        "vendor must copy the package source tree"
    );
}

#[test]
fn jet_audit_reads_without_exec() {
    // T4 / D-BUILDSCOPE1: `jetpack audit` reads build provenance and executes
    // nothing — no "resolving …" / "built" build activity, just a read-only
    // report of the realized objects' provenance.
    let (_base, proj, root) = core_hello_project("t4-audit");
    let built = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(built.status.success());

    let out = jetpack()
        .args(["audit", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = String::from_utf8_lossy(&out.stderr);
    assert!(
        report.contains("read-only, no build ran"),
        "audit is read-only: {report}"
    );
    assert!(
        report.contains("provenance"),
        "audit reports provenance: {report}"
    );
    // Audit must not run a build: it never prints the realize progress line.
    assert!(
        !report.contains("resolving"),
        "audit must not realize anything: {report}"
    );
}

#[test]
fn jet_hangar_du_counts_source_built_objects() {
    // T0 exit: `jetpack hangar du` counts realized objects honestly, marking
    // source-built ones. A first-party core build shows up as a `(built)` object.
    let (_base, proj, root) = core_hello_project("t0-du");
    let built = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(built.status.success());

    let out = jetpack()
        .args(["hangar", "du", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = String::from_utf8_lossy(&out.stderr);
    assert!(
        report.contains("built"),
        "du must mark source-built objects: {report}"
    );
    assert!(
        report.contains("1 built from source"),
        "du summary must count source-built objects honestly: {report}"
    );
}
