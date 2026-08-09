// D-NETSOCKET1=A: pure address value rules shared by NetHTTP and comptime.

pub(crate) fn jet_net_pure_parse_ip(
    text: &String,
) -> Result<std::net::IpAddr, std::net::AddrParseError> {
    text.parse()
}

pub(crate) fn jet_net_pure_parse_socket_addr(
    text: &String,
) -> Result<std::net::SocketAddr, std::net::AddrParseError> {
    text.parse()
}

pub(crate) fn jet_net_pure_ip_is_ipv4(address: &std::net::IpAddr) -> bool {
    address.is_ipv4()
}

pub(crate) fn jet_net_pure_socket_host(address: &std::net::SocketAddr) -> String {
    address.ip().to_string()
}

pub(crate) fn jet_net_pure_socket_port(address: &std::net::SocketAddr) -> i64 {
    i64::from(address.port())
}

pub(crate) fn jet_net_pure_socket_to_string(address: &std::net::SocketAddr) -> String {
    address.to_string()
}
