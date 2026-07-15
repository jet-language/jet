#![no_std]
#![forbid(unsafe_code)]
#![deny(clippy::disallowed_methods)]
#![deny(clippy::disallowed_types)]

extern crate std as host;

pub fn installed_nix_escape() {
    let _command = host::process::Command::new("nix-instantiate");
}
