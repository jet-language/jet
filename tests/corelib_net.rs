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
    jet_jit::reset_jit_trace_for_test();
    let default_stdout = match jet::Interpreter::dev_iteration(
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
    let interpreter_stdout = match jet::Interpreter::dev_iteration(
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
