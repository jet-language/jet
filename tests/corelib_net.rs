#![allow(dead_code, unused_imports)]
mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;
include!("corelib_parts/support.rs");
include!("corelib_parts/net.rs");

#[test]
fn core_net_socket_address_values_match_aot_default_and_interpreter() {
    let scratch = common::Scratch::new("core_net_socket_address_tiers");
    let dir = scratch.path.clone();
    let source = r#"
use core.net as net

fn run() -[IO, Net, Time.Wait]> {
    address :: net.socket_addr("::1", 53) ?? panic("socket address")
    print(net.socket_to_string(address))
    if net.socket_addr("::1", 65536) == {
        .Ok(_) -> panic("out-of-range port succeeded")
        .Err(error) -> {
            print(net.error_operation(error))
            print(net.error_address(error) ?? "missing address")
            print(net.error_message(error))
            if error == {
                .InvalidInput(_) -> { print("invalid-input") }
                else -> { panic("wrong network error kind") }
            }
        }
    }
}
"#;
    let expected = "[::1]:53\nsocket_addr\n::1\nport must be in 0..=65535\ninvalid-input\n";
    let (code, aot_stdout, stderr) =
        build_and_run(&dir, "net_socket_address_tiers", source, &[], None);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(aot_stdout, expected);

    let file = dir.join("net_socket_address_tiers.jet");
    std::fs::write(&file, source).unwrap();
    std::fs::write(
        dir.join("package.jet"),
        "name: \"net_socket_address_tiers\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO, Mem.Alloc, Net, Time.Wait] } }\n",
    )
    .unwrap();
    let default_stdout = match crate::common::dev_run(
        file.to_str().unwrap(),
        false,
        false,
    ) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout.as_str(), stderr.as_str()), (0, expected, ""));
            stdout
        }
        other => panic!("Core.net socket address failed in default dev: {other:?}"),
    };
    let interpreter_stdout = match crate::common::dev_run(
        file.to_str().unwrap(),
        false,
        true,
    ) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout.as_str(), stderr.as_str()), (0, expected, ""));
            stdout
        }
        other => panic!("Core.net socket address failed in forced interpreter: {other:?}"),
    };
    assert_eq!(default_stdout, aot_stdout);
    assert_eq!(interpreter_stdout, aot_stdout);
}

#[test]
fn core_url_private_components_use_accessors_in_all_execution_tiers() {
    let scratch = common::Scratch::new("core_url_accessor_tiers");
    let dir = scratch.path.clone();
    let source = r#"
use core.net.url as url

fn run() -[IO, Mem.Alloc]> {
    parsed :: url.parse("https://alice:secret@example.test:8443/a?x=1#frag") ?? panic("URL parse")
    typed :: URL{"https://alice:secret@example.test:8443/a?x=1#frag"}
    values :: [URL]{parsed, typed}
    loop value in values {
        print(value.scheme())
        print(value.host() ?? "missing")
        print(value.port() ?? -1)
        print(value.path())
        print(value.query())
        print(value.fragment() ?? "missing")
        print(value.username())
        print(value.password())
    }
}
"#;
    let expected = "https\nexample.test\n8443\n/a\nx=1\nfrag\nalice\nsecret\n".repeat(2);
    let (code, stdout, stderr) =
        build_and_run(&dir, "url_accessor_tiers", source, &[], None);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, expected.as_str(), ""));
    // The AOT build above writes its binary at `dir/url_accessor_tiers`; the
    // dev tiers read the source from its own `.jet` file.
    let file = dir.join("url_accessor_tiers.jet");
    fs::write(&file, source).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"url_accessor_tiers\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n",
    )
    .unwrap();
    for interpreted in [false, true] {
        match common::dev_run(file.to_str().unwrap(), false, interpreted) {
            jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
                assert_eq!((exit_code, stdout.as_str(), stderr.as_str()), (0, expected.as_str(), ""));
            }
            other => panic!("Core URL accessor tier failed: {other:?}"),
        }
    }
}

#[test]
fn core_url_literals_load_source_owner_without_import() {
    let scratch = common::Scratch::new("core_url_source_literal");
    let source = r#"
fn run() {
    hole :: "a/b?x=1#frag"
    value :: URL{"https://example.test/root/{hole}?q={hole}#{hole}"}
    print(value.to_string())
    print(value.path_segments().join("|"))
    print(value.add_query("n", "2").to_string())
    print(URL{"data:text/plain,{hole}"}.to_string())
}
"#;
    let expected = concat!(
        "https://example.test/root/a%2Fb%3Fx%3D1%23frag?q=a%2Fb%3Fx%3D1%23frag#a%2Fb%3Fx%3D1%23frag\n",
        "root|a/b?x=1#frag\n",
        "https://example.test/root/a%2Fb%3Fx%3D1%23frag?q=a%2Fb%3Fx%3D1%23frag&n=2#a%2Fb%3Fx%3D1%23frag\n",
        "data:text/plain,a%2Fb%3Fx%3D1%23frag\n",
    );
    let (code, stdout, stderr) =
        build_and_run(&scratch.path, "url_source_literal.jet", source, &[], None);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, expected, ""));
}

#[test]
fn core_ip_and_socket_carriers_cross_live_udp_on_all_tiers() {
    let scratch = common::Scratch::new("core_net_source_native_carriers");
    let dir = scratch.path.clone();
    // Rust-tier seed/parity driver only; the assertions live in Jet and this
    // comparison harness dies with the Rust compiler at cutover.
    let source = include_str!("net_carrier_interop.jet");
    let expected = "::ffff:192.0.2.1\n[fe80::1%4294967295]:65535\n4\n127.0.0.1\n127.0.0.1\ntrue\nA�B\n4\ntrue\n";
    let (code, stdout, stderr) =
        build_and_run(&dir, "net_carrier_tiers.jet", source, &[], None);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, expected, ""));
    let file = dir.join("net_carrier_tiers.jet");
    fs::write(&file, source).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"net_carrier_tiers\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [Net, Time.Wait, IO, Mem.Alloc] } }\n",
    )
    .unwrap();
    for interpreted in [false, true] {
        match common::dev_run(file.to_str().unwrap(), false, interpreted) {
            jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
                assert_eq!((exit_code, stdout.as_str(), stderr.as_str()), (0, expected, ""));
            }
            other => panic!("Core native carrier tier failed: {other:?}"),
        }
    }
}
