#![allow(dead_code, unused_imports)]

mod common;
include!("corelib_parts/support.rs");

#[test]
fn network_core_contract_example_covers_reserved_cidr_and_mime_boundaries() {
    let dir = common::unique_tmp("network_core_contract");
    let source = include_str!("../Examples/features/net/network_core_contract.jet");
    let (code, stdout, stderr) = build_and_run(&dir, "network_core_contract", source, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "reserved-global:false\nnetwork:192.0.2.0/24\nbroadcast:192.0.2.255\nmime:rejected\n"
    );
}
