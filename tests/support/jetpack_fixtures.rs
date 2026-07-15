//! Shared fixture helpers for the split `tests/jetpack_*.rs` suites
//! (Tower card #367 slice 6). Each jetpack test binary includes this file
//! via `#[path = "support/jetpack_fixtures.rs"] mod jetpack_fixtures;` and
//! compiles its own copy, mirroring `tests/common/mod.rs` — hence the
//! file-level `allow(dead_code)` (not every suite uses every helper).
//!
//! Extracted byte-identical from the original `tests/jetpack.rs` (module
//! item bodies unchanged; only added `pub` for cross-module visibility).

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::{jetos_bin, jetpack_bin};

pub fn jetpack() -> Command {
    Command::new(jetpack_bin())
}


pub fn jet() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jet"))
}


pub fn jetos() -> Command {
    Command::new(jetos_bin())
}


pub fn copy_dir_recursive(src: &Path, dst: &Path) {
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


pub fn studio_http(addr: &str, method: &str, path: &str, body: &str) -> String {
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


pub fn studio_json_string(response: &str, key: &str) -> String {
    let needle = format!("\"{key}\":\"");
    response
        .split_once(&needle)
        .and_then(|(_, rest)| rest.split_once('\"').map(|(value, _)| value.to_string()))
        .unwrap_or_else(|| panic!("missing Studio JSON string `{key}`: {response}"))
}


pub fn studio_json(response: &str) -> jetpack::JSON::Json {
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or(response);
    jetpack::JSON::parse(body.trim())
        .unwrap_or_else(|error| panic!("invalid Studio JSON response: {error}: {response}"))
}


pub fn json_string(json: &jetpack::JSON::Json, key: &str) -> String {
    json.get(key)
        .and_then(jetpack::JSON::Json::as_str)
        .unwrap_or_else(|error| panic!("invalid JSON string `{key}`: {error}: {json:?}"))
        .to_string()
}


#[derive(Clone)]
pub struct StudioTestOwner {
    pub session_id: String,
    pub token: String,
    pub base_revision: String,
}


pub fn studio_session(addr: &str) -> String {
    let response = studio_http(
        addr,
        "POST",
        "/studio/transaction",
        "{\"op\":\"session\"}",
    );
    studio_json_string(&response, "session_id")
}


pub fn studio_changeset_owner(response: &str, session_id: &str) -> StudioTestOwner {
    StudioTestOwner {
        session_id: session_id.to_string(),
        token: studio_json_string(response, "token"),
        base_revision: studio_json_string(response, "base_revision"),
    }
}


pub fn studio_owned_transaction(addr: &str, op: &str, owner: &StudioTestOwner) -> String {
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


pub fn studio_session_transaction(addr: &str, op: &str, session_id: &str) -> String {
    studio_http(
        addr,
        "POST",
        "/studio/transaction",
        &format!("{{\"op\":\"{op}\",\"session_id\":\"{session_id}\"}}"),
    )
}


pub fn studio_stage_option(
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


pub fn studio_attack_snapshot(server_pid: u32, truncate: bool) -> std::thread::JoinHandle<()> {
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
pub struct Scratch {
    pub path: PathBuf,
}


impl Scratch {
    pub fn new(tag: &str) -> Scratch {
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
    pub fn join(&self, p: &str) -> PathBuf {
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
pub fn make_tree_writable(path: &Path) {
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


pub fn assert_no_ephemeral_links(path: &Path) {
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
pub fn make_tree_writable(path: &Path) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    let mut permissions = meta.permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).unwrap();
}


pub fn example_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-project/fixtures")
}


/// The committed jetpack project fixture (`env.jet` + `jet-pkgs/`).
pub fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-project")
}


fn seed_hangar_object(root: &Path, staging_dir: &Path) -> PathBuf {
    jetpack::Store::seal_local_output(staging_dir).unwrap();
    let digest = jetpack::Envelope::try_output_hash_of(&staging_dir.to_string_lossy()).unwrap();
    let out_dir = root.join("hangar").join("objects").join(&digest);
    fs::create_dir_all(out_dir.parent().unwrap()).unwrap();
    let mut staging_permissions = fs::metadata(staging_dir).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        staging_permissions.set_mode(staging_permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    staging_permissions.set_readonly(false);
    fs::set_permissions(staging_dir, staging_permissions).unwrap();
    fs::rename(staging_dir, &out_dir).unwrap();
    jetpack::Store::seal_local_output(&out_dir).unwrap();
    assert_eq!(
        jetpack::Envelope::try_output_hash_of(&out_dir.to_string_lossy()).unwrap(),
        digest,
        "published fixture must retain its content-addressed identity"
    );
    out_dir
}


/// Write a provider fixture whose `out` is a real content-addressed Hangar
/// object, so closure validation can prove it before executing its binary.
pub fn write_runnable_fixture(fixtures: &Path, root: &Path, staging_dir: &Path) -> PathBuf {
    fs::create_dir_all(fixtures).unwrap();
    let bin = staging_dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let greet = bin.join("greet");
    fs::write(&greet, "#!/bin/sh\necho hello from jetpack\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out_dir = seed_hangar_object(root, staging_dir);
    let json = format!(
        "[{{\"drvPath\":\"/nix/store/0fixture00000000000000000000-greet.drv\",\"outputs\":{{\"out\":{:?}}}}}]",
        out_dir.to_string_lossy()
    );
    fs::write(fixtures.join("nixpkgs-greet.json"), json).unwrap();
    out_dir
}


/// Write a `nixpkgs:fastfetch` fixture whose `out` points at a real directory
/// we control (see `write_runnable_fixture`). The committed
/// `tests/fixtures/jetpack-project/fixtures` set uses placeholder
/// `/nix/store/...` paths that never exist on disk — fine for tests that only
/// check `jetpack build`'s ledger output, but Store's fail-closed leasing
/// (`snapshot_lease`) refuses to hand a consumer any path whose `out` doesn't
/// exist, so a test that enters the composed env (`run`/`dev` with no
/// explicit command consuming the package) needs a real backing tree.
pub fn write_fastfetch_fixture(fixtures: &Path, root: &Path, staging_dir: &Path) -> PathBuf {
    fs::create_dir_all(fixtures).unwrap();
    let bin = staging_dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let fastfetch = bin.join("fastfetch");
    fs::write(&fastfetch, "#!/bin/sh\necho fastfetch stub\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fastfetch, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out_dir = seed_hangar_object(root, staging_dir);
    let json = format!(
        "[{{\"drvPath\":\"/nix/store/0fixture00000000000000000000-fastfetch.drv\",\"outputs\":{{\"out\":{:?}}}}}]",
        out_dir.to_string_lossy()
    );
    fs::write(fixtures.join("nixpkgs-fastfetch.json"), json).unwrap();
    out_dir
}


pub fn write_channel_fixture(fixtures: &Path, base: &str, channel: &str, exact: &str) {
    fs::create_dir_all(fixtures).unwrap();
    fs::write(
        fixtures.join("channels.txt"),
        format!("{base} {channel} {exact} 240000000\n"),
    )
    .unwrap();
}


pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}


pub fn test_json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}


pub fn test_shell_quote(value: &Path) -> String {
    format!("'{}'", value.display().to_string().replace('\'', "'\\''"))
}


pub fn write_executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}


pub fn write_fake_vm_tools(bin: &Path, guest_passes: bool) {
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


/// Register a real, contract-valid hangar object via the production
/// `ingest_tree` API.
///
/// Card #420's producer-record refactor made `jetpack clean`/`closure_graph`
/// fail-closed: every hangar entry it walks must decode a valid
/// `ProducerRecord` and pass `store_validates_complete_closure` (real content
/// re-hashed against the recorded digest — see
/// `crates/jetpack/src/Store/Closure.rs::{normalize_legacy_entry,
/// store_validates_complete_closure}`). A hand-written `meta.json` with a
/// fictitious `/nix/store/...` `out` and blank `CacheIdentity` can no longer
/// satisfy that — it always fails with "legacy package lacks immutable
/// producer facts" or "has no dependency references or store-validated
/// closure proof". `ingest_tree` (provider `hangar-ingest`) is the only
/// producer-record-issuing constructor reachable from outside the `jetpack`
/// crate (the `store-record`/`core` provider path is `#[cfg(test)]`-gated
/// inside `jetpack` itself), so fixtures now go through it to get a real
/// digest over real bytes.
///
/// The digest is no longer caller-chosen (it's the real hash of the fixture
/// payload), so this returns `(hangar_dir, envelope_output_hash)` — callers
/// that need to cross-reference the digest elsewhere (e.g. a lockfile's
/// `output-hash`) read it back from the second element.
///
/// `last_used_at`, when given, is backdated via the card #650 `test-seam`
/// (`jetpack::Store::test_backdate_last_used_at`) — a plain `meta.json` text
/// edit doesn't survive the next hangar operation (the closure journal is
/// the real source of truth and gets re-materialized over it).
pub fn write_hangar_meta(
    root: &Path,
    id: &str,
    name: &str,
    version: &str,
    last_used_at: Option<u64>,
) -> (PathBuf, String) {
    let roots = jetpack::Store::Roots {
        root: root.to_path_buf(),
        dev_mode: false,
    };
    let src = root.join(format!("hangar-meta-src-{id}"));
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("payload"), format!("fixture payload: {id}")).unwrap();
    let entry = jetpack::Store::ingest_tree(
        &roots,
        &jetpack::Store::IngestRequest {
            name: name.to_string(),
            version: version.to_string(),
            reference: format!("fixture:{id}"),
            cache_identity: jetpack::Store::CacheIdentity {
                source_fingerprint: format!("sha256-fixture-source-{id}"),
                recipe_fingerprint: format!("sha256-fixture-recipe-{id}"),
                policy_fingerprint: "policy=fixture".to_string(),
                platform: jetpack::Envelope::host_platform(),
            },
            references: Vec::new(),
            outputs: std::collections::BTreeMap::from([("out".to_string(), src.clone())]),
            signature: String::new(),
            provenance: "fixture".to_string(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap()
    .entry;
    fs::remove_dir_all(&src).ok();
    if let Some(ts) = last_used_at {
        jetpack::Store::test_backdate_last_used_at(&roots, &entry.id, ts).unwrap();
    }
    let dir = root.join("hangar").join(&entry.id);
    (dir, entry.envelope.output_hash)
}


pub fn write_lock_with_live_output(project: &Path, name: &str, version: &str, output_hash: &str) {
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


/// Build a scratch project whose `env.jet` pulls a first-party `core` package
/// (`hello`) from a local repo. Returns `(base, proj, root)` so a test can run
/// a jetpack command in `proj` with `JETPACK_ROOT=root` and no nix on PATH.
pub fn core_hello_project(tag: &str) -> (Scratch, PathBuf, PathBuf) {
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


/// The committed jetpack-config fixture dir.
pub fn config_example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-config")
}


pub fn assert_jetos_stderr_snapshot(name: &str, stderr: &str) {
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


pub fn assert_jetos_stderr_snapshot_trimmed(name: &str, stderr: &str) {
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


pub fn assert_jetos_stderr_snapshot_normalized(name: &str, stderr: &str, replacements: &[(&str, &str)]) {
    let mut normalized = stderr.to_string();
    for (from, to) in replacements {
        normalized = normalized.replace(from, to);
    }
    assert_jetos_stderr_snapshot(name, &normalized);
}


pub fn write_cachyos_source_recipe(pkg: &Path) {
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


pub fn write_cachyos_source_builder(pkg: &Path, body: &str) {
    write_executable(&pkg.join("source/build.sh"), body);
}


pub fn write_bootlike_cachyos_artifacts(pkg: &Path) {
    fs::create_dir_all(pkg.join("boot")).unwrap();
    fs::write(pkg.join("boot/vmlinuz-cachyos"), "MZ test kernel\nHdrS\n").unwrap();
    fs::write(pkg.join("boot/initrd-cachyos"), "070701 test initrd\n").unwrap();
}


/// Stage a flake-root fixture plus a stub `nix` on PATH whose `eval` prints
/// the canned live-extractor result (or fails when `output` is None).
pub fn write_live_import_fixture(src: &Path, tools: &Path, output: Option<&str>) {
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


pub const LIVE_IMPORT_EVAL_JSON: &str = r#"{"host":"halcyon","stateVersion":"26.05","tz":"America/New_York","locale":"en_US.UTF-8","keyboard":"us","desktopGnome":false,"desktopPlasma":true,"dmGdm":false,"dmSddm":true,"loaderLimine":true,"loaderSystemdBoot":false,"efiTouch":false,"kernelName":"linux-cachyos","kernelParams":["quiet"],"sysctl":{"vm.swappiness":10},"firewallTcp":[22,443],"firewallUdp":[53317],"nameservers":["1.1.1.1"],"networkmanager":true,"zramEnable":true,"zramPercent":25,"svcOpenssh":true,"svcPipewire":true,"svcRtkit":true,"svcTailscale":true,"svcLibvirtd":true,"svcDocker":true,"svcFlatpak":false,"svcSteam":true,"svcGamemode":true,"svcPcscd":true,"svcBluetooth":false,"stylix":true,"packages":["git","ripgrep","jetbrains.idea-ultimate"],"users":[{"name":"nate","home":"/home/nate","groups":["wheel","networkmanager"],"shell":"fish"}],"hm":[{"name":"nate","packages":["ghostty"],"programs":["git","starship"]}]}"#;


/// Scrape a string field's value out of a harness/proof JSON blob (the VM
/// proof files are flat string fields, so a split is enough).
pub fn harness_json_field(text: &str, key: &str) -> String {
    let needle = format!("\"{key}\":\"");
    let (_, rest) = text
        .split_once(&needle)
        .unwrap_or_else(|| panic!("harness JSON lacks `{key}`: {text}"));
    rest.split('"').next().unwrap().to_string()
}


/// The committed multi-package monorepo example dir.
pub fn mono_example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-mono")
}

