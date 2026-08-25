//! Card #2189 criterion 1: `env` selects the default or positional module.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn jetpack() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jetpack"))
}

fn scratch(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "jet-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn criterion_root() -> PathBuf {
    let root = PathBuf::from(std::env::var_os("HOME").unwrap())
        .join(".cache/jet-test-scratch/jetpack-criterion1-root");
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn env_enters_default_and_positional_modules() {
    let project = scratch("criterion1-project");
    let root = criterion_root();
    fs::write(
        project.join("env.jet"),
        "module env.dev { prompt: \"default-module\" }\nmodule env.full { prompt: \"full-module\" }\n",
    )
    .unwrap();

    for (args, prompt) in [
        (vec!["env", "--no-color"], "default-module"),
        (vec!["env", "full", "--no-color"], "full-module"),
    ] {
        let mut child = jetpack()
            .args(&args)
            .current_dir(&project)
            .env("JETPACK_ROOT", &root)
            .env("SHELL", "/bin/bash")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"printf '%s\\n' \"$PS1\"\nexit\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "args={args:?}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(prompt),
            "args={args:?}\nprompt={prompt}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::remove_dir_all(project).unwrap();
}
