#![no_std]
#![forbid(unsafe_code)]
#![deny(clippy::disallowed_methods)]
#![deny(clippy::disallowed_types)]

extern crate std as host;

pub fn installed_nix_escape() {
    let _command = host::process::Command::new("nix-instantiate");
}

pub fn tcp_escape() {
    use host::net::TcpStream as Wire;

    let _wire = Wire::connect("127.0.0.1:9");
}

pub fn dns_escape() {
    use host::net::ToSocketAddrs as Resolve;

    let _addresses = ("localhost", 9).to_socket_addrs();
}

#[cfg(unix)]
pub fn unix_socket_escape() {
    use host::os::unix::net::UnixStream as Wire;

    let _wire = Wire::connect("/tmp/nix-daemon.socket");
}
