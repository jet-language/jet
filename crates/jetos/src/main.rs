//! The `jetos` binary — direct front door for JetOS system tools.
//!
//! Card #367 / D-PRODUCT-SPLIT1=C: `jetos` owns OS workflows as its own
//! crate/binary boundary. Today it still dispatches through `jetpack`'s `os`
//! verb (JetOS realization logic hasn't moved out of `jetpack::JetOS` yet —
//! that split is a separate, larger slice: pulling the JetOS engine out from
//! under the Jetpack provider/store engine it currently shares). This is the
//! boundary-prep step: a real `jetos` crate/binary exists and owns the entry
//! point, without changing what running `jetos <args>` does.

#![allow(non_snake_case)]

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    args.insert(0, "os".to_string());
    std::process::exit(jetpack::run(args));
}
