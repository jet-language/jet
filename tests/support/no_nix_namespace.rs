//! Rootless Linux harness for tests that must start without a host Nix store.

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    Enabled,
    Disabled,
}

const CHILD_MARKER: &str = "JET_NO_NIX_NAMESPACE_CHILD";

pub fn run_in_no_nix_namespace<F>(exact_test_name: &str, network_mode: NetworkMode, child_body: F)
where
    F: FnOnce(),
{
    if std::env::var_os(CHILD_MARKER).is_some() {
        assert_no_host_store();
        child_body();
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (exact_test_name, network_mode);
        panic!("no-host-store namespace proof requires Linux");
    }

    #[cfg(target_os = "linux")]
    {
        let unshare = find_helper("unshare");
        let mount = find_helper("mount");
        let shell = find_helper("sh");
        let mkdir = find_helper("mkdir");
        let test_binary = std::env::current_exe().expect("current test executable");
        let script = r#"set -eu
mount="$1"
mkdir="$2"
test_binary="$3"
test_name="$4"
"$mkdir" -p /nix
"$mount" -t tmpfs -o mode=0755 jet-no-nix-test /nix
exec "$test_binary" "$test_name" --exact --nocapture
"#;

        let mut command = Command::new(unshare);
        command
            .args([
                "--user",
                "--map-root-user",
                "--mount",
                "--propagation",
                "private",
            ])
            .args((network_mode == NetworkMode::Disabled).then_some("--net"))
            .arg(shell)
            .args(["-c", script, "jet-no-nix-namespace"])
            .arg(mount)
            .arg(mkdir)
            .arg(test_binary)
            .arg(exact_test_name)
            .env(CHILD_MARKER, "1");
        let status = command
            .status()
            .unwrap_or_else(|error| panic!("starting no-host-store namespace: {error}"));
        assert!(
            status.success(),
            "no-host-store namespace proof failed for `{exact_test_name}`: {status}"
        );
    }
}

fn assert_no_host_store() {
    match std::fs::symlink_metadata("/nix/store") {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => panic!("test child started with a visible /nix/store"),
        Err(error) => panic!("could not inspect /nix/store before Jetpack: {error}"),
    }
}

#[cfg(target_os = "linux")]
fn find_helper(name: &str) -> PathBuf {
    [
        PathBuf::from(format!("/run/current-system/sw/bin/{name}")),
        PathBuf::from(format!("/usr/bin/{name}")),
        PathBuf::from(format!("/bin/{name}")),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .unwrap_or_else(|| panic!("required namespace helper `{name}` is not installed"))
}
