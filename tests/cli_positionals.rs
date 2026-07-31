//! D-CLI-POS1=A: focused proof for derive positionals (owned by #748).
//! Kept out of tests/cli.rs (foreign-dirty under another task).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet-cli-pos-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_todo(dir: &std::path::Path) {
    fs::write(
        dir.join("todo.jet"),
        r#"#CLI
struct AddArgs {
    text: String
    due: String = ""
}

#CLI
struct LoginArgs {
    #Flag token: String
}

enum Cmd {
    Add(AddArgs)
    Login(LoginArgs)
}

fn run(cmd: Cmd) {
    if cmd == {
        .Add(a) -> {
            print(a.text)
            print(a.due)
        }
        .Login(a) -> {
            print(a.token)
        }
    }
}
"#,
    )
    .unwrap();
}

#[test]
fn positional_bare_form_fills_required_field() {
    let dir = scratch("bare");
    write_todo(&dir);
    let out = Command::new(jet())
        .args(["run", "--release", "todo.jet", "--", "add", "buy-milk"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("buy-milk"), "{stdout}");
}

#[test]
fn named_form_still_works_and_optional_stays_flag() {
    let dir = scratch("named");
    write_todo(&dir);
    let out = Command::new(jet())
        .args([
            "run",
            "--release",
            "todo.jet",
            "--",
            "add",
            "--text",
            "buy-milk",
            "--due",
            "fri",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("buy-milk"), "{stdout}");
    assert!(stdout.contains("fri"), "{stdout}");
}

#[test]
fn named_wins_over_positional() {
    let dir = scratch("named-wins");
    write_todo(&dir);
    let out = Command::new(jet())
        .args([
            "run",
            "--release",
            "todo.jet",
            "--",
            "add",
            "bare-loses",
            "--text",
            "named-wins",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("named-wins"), "{stdout}");
    assert!(!stdout.contains("bare-loses"), "{stdout}");
}

#[test]
fn flag_marker_rejects_bare_token() {
    let dir = scratch("flag-only");
    write_todo(&dir);
    let bare = Command::new(jet())
        .args(["run", "--release", "todo.jet", "--", "login", "secret"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        !bare.status.success(),
        "bare token must fail for #[Flag] field"
    );
    let named = Command::new(jet())
        .args(["run", "--release", "todo.jet", "--", "login", "--token", "secret"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        named.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&named.stderr)
    );
    assert!(String::from_utf8_lossy(&named.stdout).contains("secret"));
}

#[test]
fn help_lists_arguments_before_options() {
    let dir = scratch("help");
    write_todo(&dir);
    let out = Command::new(jet())
        .args(["run", "--release", "todo.jet", "--", "add", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let args_at = stdout.find("Arguments:").expect("Arguments section");
    let opts_at = stdout.find("Options:").expect("Options section");
    assert!(args_at < opts_at, "{stdout}");
    assert!(stdout.contains("text"), "{stdout}");
}

#[test]
fn dossier_exposes_positional_order() {
    let dir = scratch("dossier");
    write_todo(&dir);
    let out = Command::new(jet())
        .args(["inspect", "dossier", "todo.jet", "run", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let body = String::from_utf8_lossy(&out.stdout);
    assert!(body.contains("\"positional\":0"), "{body}");
    assert!(body.contains("\"shape\":\"positional\""), "{body}");
    assert!(body.contains("\"flag\":\"--token\""), "{body}");
}
