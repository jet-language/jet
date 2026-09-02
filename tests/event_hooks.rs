mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc};

#[cfg(unix)]
use common::Scratch;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::process::{Command, Stdio};

#[cfg(unix)]
fn json_string_at(text: &str, start: usize) -> (String, usize) {
    let bytes = text.as_bytes();
    assert_eq!(bytes.get(start), Some(&b'"'), "JSON string must start with quote");
    let mut index = start + 1;
    let mut decoded = String::new();
    while index < bytes.len() {
        match bytes[index] {
            b'"' => return (decoded, index + 1),
            b'\\' => {
                index += 1;
                let escaped = *bytes
                    .get(index)
                    .expect("JSON string cannot end after an escape");
                let character = match escaped {
                    b'"' => '"',
                    b'\\' => '\\',
                    b'/' => '/',
                    b'b' => '\u{8}',
                    b'f' => '\u{c}',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    b'u' => {
                        let end = index + 5;
                        let digits = text
                            .get(index + 1..end)
                            .expect("JSON unicode escape must have four digits");
                        let code = u16::from_str_radix(digits, 16)
                            .expect("JSON unicode escape must be hexadecimal");
                        index = end - 1;
                        char::from_u32(code as u32).expect("JSON unicode escape must be valid")
                    }
                    other => panic!("unsupported JSON escape {other:?}"),
                };
                decoded.push(character);
                index += 1;
            }
            _ => {
                let character = text[index..]
                    .chars()
                    .next()
                    .expect("JSON string byte must be valid UTF-8");
                decoded.push(character);
                index += character.len_utf8();
            }
        }
    }
    panic!("unterminated JSON string");
}

#[cfg(unix)]
fn hook_commands(settings: &str) -> Vec<String> {
    let bytes = settings.as_bytes();
    let mut commands = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'"' {
            index += 1;
            continue;
        }
        let (key, after_key) = json_string_at(settings, index);
        index = after_key;
        if key != "command" {
            continue;
        }
        while bytes
            .get(index)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            index += 1;
        }
        if bytes.get(index) != Some(&b':') {
            continue;
        }
        index += 1;
        while bytes
            .get(index)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            index += 1;
        }
        let (command, after_command) = json_string_at(settings, index);
        commands.push(command);
        index = after_command;
    }
    commands
}


#[test]
fn decision_hook_outcomes_transform_and_short_circuit() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run(
        "decision_hook_outcomes",
        r#"
use core.event as event

fn show(outcome: HookOutcome<String, String>) {
    if outcome == {
        .Continue(value) -> print("continue {value}")
        .Cancel -> print("cancel")
        .Fail(error) -> print("fail {error}")
    }
}

fn run() {
    zero :: event.decision_hook<String, String>(HookPolicy.FirstCancelElseTransform)
    show(zero.run("original"))

    scope :: event.scope()
    transformed :: event.decision_hook<String, String>(HookPolicy.FirstCancelElseTransform)
    transformed.on_priority(scope, 10, (value: String) -> {
        print("first {value}")
        HookDecision.Transform("{value}-one")
    })
    transformed.on(scope, (value: String) -> {
        print("second {value}")
        HookDecision.Continue
    })
    show(transformed.run("start"))

    cancelled :: event.decision_hook<String, String>(HookPolicy.FirstCancelElseTransform)
    cancelled.on_priority(scope, 10, (value: String) -> HookDecision.Cancel)
    cancelled.on(scope, (value: String) -> {
        print("must not run {value}")
        HookDecision.Continue
    })
    show(cancelled.run("stop"))

    failed :: event.decision_hook<String, String>(HookPolicy.FirstCancelElseTransform)
    failed.on_priority(scope, 10, (value: String) -> HookDecision.Fail("denied {value}"))
    failed.on(scope, (value: String) -> {
        print("must not run {value}")
        HookDecision.Continue
    })
    show(failed.run("save"))
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "continue original\nfirst start\nsecond start-one\ncontinue start-one\ncancel\nfail denied save\n"
    );
}

#[test]
fn decision_hook_lifetime_order_once_mutation_and_reentrancy() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run(
        "decision_hook_lifetime",
        r#"
use core.event as event

fn run() {
    scope :: event.scope()
    hook :: event.decision_hook<Int, String>(HookPolicy.FirstCancelElseTransform)
    late :: hook.on(scope, (value: Int) -> {
        print("late {value}")
        HookDecision.Continue
    })
    hook.on_priority(scope, 10, (value: Int) -> {
        print("first {value}")
        late.unsubscribe()
        HookDecision.Transform(value + 1)
    })
    hook.once(scope, (value: Int) -> {
        print("once {value}")
        if value == 2 { hook.run(10) }
        HookDecision.Continue
    })
    hook.on(scope, (value: Int) -> {
        print("last {value}")
        HookDecision.Continue
    })
    hook.run(1)
    hook.run(2)
    print("active {scope.active_count()}")
    scope.cancel()
    hook.run(3)

    owned :: event.decision_hook<Int, String>(HookPolicy.FirstCancelElseTransform)
    if true {
        owner :: event.scope()
        owned.on(owner, (value: Int) -> {
            print("leaked {value}")
            HookDecision.Continue
        })
    }
    owned.run(9)
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "first 1\nonce 2\nfirst 10\nlast 11\nlast 2\nfirst 2\nlast 3\nactive 2\n"
    );
}

#[cfg(unix)]
#[test]
fn claude_hooks_follow_project_dir_for_hostile_paths() {
    let settings_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".claude/settings.json");
    let settings =
        fs::read_to_string(&settings_path).expect("Claude hook settings must be readable");
    let commands = hook_commands(&settings);
    let script_names = [
        "clean-nix-tmp.sh",
        "check-worktree-layout.sh",
        "require-clean-tree.sh",
    ];
    assert_eq!(
        commands.len(),
        script_names.len(),
        "Claude settings must keep one command for each guarded hook"
    );

    let scratch = Scratch::new("claude-hostile-project-path");
    let project = scratch.join(r#"project with spaces;$(touch "$HOOK_INJECTION_MARKER")"#);
    let attacker = scratch.join("attacker cwd");
    let project_scripts = project.join("scripts/agent");
    let attacker_scripts = attacker.join("scripts/agent");
    fs::create_dir_all(&project_scripts).expect("create project hook directory");
    fs::create_dir_all(&attacker_scripts).expect("create attacker hook directory");
    let marker = scratch.join("claude-hook-origin");
    let injection_marker = scratch.join("claude-hook-injected");

    for script_name in script_names {
        fs::write(
            project_scripts.join(script_name),
            "printf 'project\\n' >> \"$HOOK_MARKER\"\n",
        )
        .expect("write project hook fixture");
        fs::write(
            attacker_scripts.join(script_name),
            "printf 'attacker\\n' >> \"$HOOK_MARKER\"\n",
        )
        .expect("write attacker hook fixture");
    }

    for script_name in script_names {
        let command = commands
            .iter()
            .find(|command| command.contains(script_name))
            .unwrap_or_else(|| panic!("missing configured hook {script_name}"));
        let output = Command::new("bash")
            .current_dir(&attacker)
            .args(["-c", command])
            .env("CLAUDE_PROJECT_DIR", &project)
            .env("HOOK_MARKER", &marker)
            .env("HOOK_INJECTION_MARKER", &injection_marker)
            .stdin(Stdio::null())
            .output()
            .expect("run Claude hook command");
        assert!(
            output.status.success(),
            "Claude hook {script_name} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert_eq!(
        fs::read_to_string(&marker).expect("project hook marker"),
        "project\nproject\nproject\n",
        "hooks must execute the project scripts, not cwd-relative attacker scripts"
    );
    assert!(
        !injection_marker.exists(),
        "CLAUDE_PROJECT_DIR was reparsed as shell syntax"
    );
}
