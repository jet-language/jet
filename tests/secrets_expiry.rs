mod common;

#[test]
fn expiring_secret_lends_then_zeroizes_on_expiry() {
    if !common::have_rustc() {
        return;
    }
    let _ffi_lock = common::FfiBridgeLock::acquire();

    let src = r#"
use core.crypto as crypto
use core.tasks as tasks
use core.time as time
use core.vault as vault

fn run() {
    clock := Clock.new(100)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, clock)

    if secret.with((borrowed) => borrowed.public_key()) == Ok(_) {
        print("available")
    }
    fork := ~clock
    fork.tick(1001)
    if secret.with((borrowed) => borrowed.public_key()) == Ok(_) {
        print("forked")
    }
    clock.tick(1001)
    if secret.with((borrowed) => borrowed.public_key()) == Err(_) {
        print("expired")
    }
    clock.advance(0)
    if secret.with((borrowed) => borrowed.public_key()) == Err(_) {
        print("sticky")
    }

    thread_key := crypto.SigningKey.new_random() ?? panic("thread key")
    threaded := vault.ExpiringSecret.new(^thread_key, ttl, clock)
    task := tasks.spawn(() => {
        if threaded.with((borrowed) => borrowed.public_key()) == Ok(_) {
            print("threaded")
        }
    })
    task.join()
}
"#;
    let (code, stdout, stderr) =
        common::build_and_run("jet_secrets_expiry", "expiry", src);
    assert_eq!(code, 0, "expiring secret failed: {stderr}");
    assert_eq!(stdout, "available\nforked\nexpired\nsticky\nthreaded\n");
}

#[test]
fn expiring_secret_rejects_non_secret_values() {
    let src = r#"
use core.time as time
use core.vault as vault

fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    _ := vault.ExpiringSecret.new("plaintext", ttl, clock)
}
"#;
    let diags = jet::compile(src).expect_err("plaintext must not enter ExpiringSecret");
    assert!(diags.iter().any(|diag| diag.code == "E0112"));
}

#[test]
fn expiring_secret_accepts_only_the_closed_secret_family() {
    let src = r#"
use core.crypto as crypto
use core.time as time
use core.vault as vault

fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    bytes := crypto.Secret.from_text("bytes")
    signing := crypto.SigningKey.new_random() ?? panic("signing")
    agreement := crypto.X25519SecretKey.new_random() ?? panic("agreement")
    first := vault.ExpiringSecret.new(^bytes, ttl, clock)
    second := vault.ExpiringSecret.new(^signing, ttl, clock)
    third := vault.ExpiringSecret.new(^agreement, ttl, clock)
}
"#;
    let result = jet::compile(src);
    assert!(result.is_ok(), "closed family rejected: {:?}", result.err());
}

#[test]
fn expiring_secret_loan_can_call_read_helpers() {
    let src = r#"
use core.crypto as crypto
use core.time as time
use core.vault as vault

fn inspect_key(key: crypto.SigningKey) => VerifyKey {
    return key.public_key()
}

fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, clock)
    _ := secret.with((borrowed) => inspect_key(borrowed))
}
"#;
    let result = jet::compile(src);
    assert!(
        result.is_ok(),
        "read helper rejected the temporary loan: {:?}",
        result.err()
    );

    let core_call = r#"
use core.crypto as crypto
use core.time as time
use core.vault as vault

fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, clock)
    _ := secret.with((borrowed) => crypto.sign(borrowed, [1, 2]))
}
"#;
    let result = jet::compile(core_call);
    assert!(
        result.is_ok(),
        "core read call rejected the temporary loan: {:?}",
        result.err()
    );
}

#[test]
fn expiring_secret_loan_can_call_cross_file_read_helpers() {
    let dir = common::unique_tmp("jet_expiring_secret_cross_file_read");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("helper.jet"),
        "use core.crypto as crypto\npub fn inspect_key(key: crypto.SigningKey) => crypto.VerifyKey { return key.public_key() }\n",
    )
    .unwrap();
    let src = r#"
use "helper"
use core.crypto as crypto
use core.time as time
use core.vault as vault

fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, clock)
    _ := secret.with((borrowed) => helper.inspect_key(borrowed))
}
"#;
    let main = dir.join("main.jet");
    std::fs::write(&main, src).unwrap();
    let result = jet::compile_with_path(src, &main.to_string_lossy());
    assert!(
        result.is_ok(),
        "cross-file read helper rejected the temporary loan: {:?}",
        result.err()
    );

    std::fs::write(
        dir.join("fake.jet"),
        "pub struct SigningKey {}\npub fn inspect_key(key: SigningKey) => Bool { return true }\n",
    )
    .unwrap();
    let hostile = r#"
use "fake"
use core.crypto as crypto
use core.time as time
use core.vault as vault

fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, clock)
    _ := secret.with((borrowed) => fake.inspect_key(borrowed))
}
"#;
    let hostile_main = dir.join("hostile.jet");
    std::fs::write(&hostile_main, hostile).unwrap();
    let diagnostics = jet::compile_with_path(hostile, &hostile_main.to_string_lossy())
        .expect_err("a user type with the same leaf name must not accept the loan");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0112"),
        "{diagnostics:#?}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn expiring_secret_declared_type_enforces_closed_family() {
    let src = r#"
fn bad(value: ExpiringSecret<String>) {
    print("never")
}
fn run() {}
"#;
    let diags =
        jet::compile(src).expect_err("declared ExpiringSecret types must enforce the family");
    assert!(diags.iter().any(|diag| diag.code == "E0112"), "{diags:?}");

    let shared = r#"
use core.crypto as crypto
fn bad(value: ExpiringSecret<crypto.SharedSecret>) {
    print("never")
}
fn run() {}
"#;
    let diags = jet::compile(shared)
        .expect_err("SharedSecret is not in the closed ExpiringSecret family");
    assert!(diags.iter().any(|diag| diag.code == "E0112"), "{diags:?}");
}

#[test]
fn expiring_secret_rejects_non_clock_observers_in_sema() {
    let src = r#"
use core.crypto as crypto
use core.vault as vault

fn run() {
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    _ := vault.ExpiringSecret.new(^key, ttl, 42)
}
"#;
    let diags = jet::compile(src)
        .expect_err("a non-Clock observer must fail before code generation");
    assert!(
        diags.iter().any(|diag| {
            diag.code == "E0112" && diag.what.contains("argument 3")
        }),
        "{diags:?}"
    );
}

#[test]
fn expiring_secret_system_observation_is_not_pure() {
    let src = r#"
use core.crypto as crypto
use core.vault as vault

fn inspect(secret: &ExpiringSecret<crypto.SigningKey>) =[]=> Bool {
    return secret.with((borrowed) => borrowed.public_key()) == Ok(_)
}
fn run() {
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, Clock.system())
    print(inspect(&secret))
}
"#;
    let diags =
        jet::compile(src).expect_err("a hidden system clock must remain effectful");
    assert!(diags.iter().any(|diag| diag.code == "E3403"), "{diags:?}");
}

#[test]
fn expiring_secret_requires_ownership_and_rejects_loan_escape() {
    let no_move = r#"
use core.crypto as crypto
use core.time as time
use core.vault as vault

fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    _ := vault.ExpiringSecret.new(key, ttl, clock)
}
"#;
    let diags = jet::compile(no_move).expect_err("named secret must move into the wrapper");
    assert!(
        diags.iter().any(|diag| diag.code == "E0201"),
        "wrong ownership diagnostic: {diags:?}"
    );

    let escape = r#"
use core.crypto as crypto
use core.time as time
use core.vault as vault

fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, clock)
    _ := secret.with((borrowed) => borrowed)
}
"#;
    let diags = jet::compile(escape).expect_err("callback loan must not escape");
    assert!(diags.iter().any(|diag| {
        diag.code == "E0201" && diag.what.contains("loan cannot escape")
    }));

    let closure_escape = r#"
use core.crypto as crypto
use core.time as time
use core.vault as vault

fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, clock)
    _ := secret.with((borrowed) => () => borrowed.public_key())
}
"#;
    let diags =
        jet::compile(closure_escape).expect_err("callback closure must not capture the loan");
    assert!(
        diags.iter().any(|diag| diag.code == "E0201"),
        "closure escape needs an ownership diagnostic: {diags:?}"
    );

    let storage_escape = r#"
use core.crypto as crypto
use core.time as time
use core.vault as vault

fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, clock)
    _ := secret.with((borrowed) => {
        saved := [borrowed]
        return saved
    })
}
"#;
    let diags =
        jet::compile(storage_escape).expect_err("callback loan must not enter storage");
    assert!(
        diags.iter().any(|diag| diag.code == "E0201"),
        "storage escape needs an ownership diagnostic: {diags:?}"
    );
}

#[test]
fn expiring_secret_loan_rejects_move_variadic_and_drop_paths() {
    let cases = [
        (
            "move parameter",
            r#"
use core.crypto as crypto
use core.time as time
use core.vault as vault
fn consume(value: ^crypto.SigningKey) {}
fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, clock)
    _ := secret.with((borrowed) => { consume(^borrowed); return 0 })
}
"#,
        ),
        (
            "variadic collection",
            r#"
use core.crypto as crypto
use core.time as time
use core.vault as vault
fn collect(values: ...crypto.SigningKey) {}
fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, clock)
    _ := secret.with((borrowed) => { collect(borrowed); return 0 })
}
"#,
        ),
        (
            "explicit consume/drop",
            r#"
use core.crypto as crypto
use core.time as time
use core.vault as vault
fn run() {
    clock := Clock.new(0)
    ttl := Duration.seconds(1) ?? panic("duration")
    key := crypto.SigningKey.new_random() ?? panic("key")
    secret := vault.ExpiringSecret.new(^key, ttl, clock)
    _ := secret.with((borrowed) => {
        #Unsafe("attempt to discard a loan") { consume(borrowed) }
        return 0
    })
}
"#,
        ),
    ];
    for (name, src) in cases {
        let diagnostics = jet::compile(src).expect_err(name);
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == "E0201"),
            "{name} did not reject the loan: {diagnostics:#?}"
        );
    }
}

#[test]
fn generic_ttl_remains_while_expiring_secret_replaces_rotting() {
    let compiled = jet::compile("fn run() {}").expect("empty program compiles");
    assert!(compiled.rust.contains("struct JetExpiringSecret<T>"));
    assert!(compiled.rust.contains("struct JetExpiring<T"));
    assert!(!compiled.rust.contains("struct JetRotting<T"));
}
