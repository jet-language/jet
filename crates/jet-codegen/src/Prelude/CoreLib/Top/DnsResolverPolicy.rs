fn jet_net_dns_socket_addr(host: &str) -> Option<String> {
    let host = host.trim().trim_matches(|c| c == '[' || c == ']');
    host.parse::<std::net::IpAddr>()
        .ok()
        .map(|ip| std::net::SocketAddr::new(ip, 53).to_string())
}

fn jet_net_dns_parse_resolv_conf(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('#').next()?.split_whitespace();
            (parts.next() == Some("nameserver"))
                .then(|| parts.next())
                .flatten()
                .and_then(jet_net_dns_socket_addr)
        })
        .collect()
}

fn jet_net_dns_parse_scutil(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let (label, value) = line.trim().split_once(':')?;
            label
                .trim()
                .starts_with("nameserver[")
                .then(|| jet_net_dns_socket_addr(value))
                .flatten()
        })
        .collect()
}

fn jet_net_dns_parse_windows_addresses(text: &str) -> Vec<String> {
    text.lines()
        .flat_map(|line| line.split(|c: char| c.is_whitespace() || c == ',' || c == '{' || c == '}'))
        .filter_map(jet_net_dns_socket_addr)
        .collect()
}
