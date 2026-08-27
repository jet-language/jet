use std::collections::BTreeMap;
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

fn response(stream: &mut TcpStream, status: u16, reason: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        reason,
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn handle(mut stream: TcpStream, values: &mut BTreeMap<String, String>) -> bool {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return false;
    }
    let parts: Vec<&str> = line.trim_end().split_whitespace().collect();
    if parts.len() != 3 {
        return false;
    }
    let method = parts[0].to_string();
    let path = parts[1].split('?').next().unwrap_or(parts[1]).to_string();
    let mut content_length = 0usize;
    loop {
        line.clear();
        if reader.read_line(&mut line).is_err() {
            return false;
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; content_length];
    if reader.read_exact(&mut body).is_err() {
        return false;
    }
    let key = path.strip_prefix("/kv/");
    match (method.as_str(), path.as_str(), key) {
        ("GET", "/health", _) => response(&mut stream, 200, "OK", "{\"status\":\"ok\"}"),
        ("PUT", _, Some(key)) => {
            let value = String::from_utf8_lossy(&body).into_owned();
            values.insert(key.to_string(), value);
            response(&mut stream, 200, "OK", &format!("{{\"stored\":\"{}\"}}", key));
        }
        ("GET", _, Some(key)) => match values.get(key) {
            Some(value) => response(&mut stream, 200, "OK", &format!("{{\"key\":\"{}\",\"value\":\"{}\"}}", key, value)),
            None => response(&mut stream, 404, "Not Found", "{\"error\":\"not found\"}"),
        },
        ("GET", "/shutdown", _) => {
            response(&mut stream, 200, "OK", "{\"bye\":true}");
            return true;
        }
        _ => response(&mut stream, 404, "Not Found", "{\"error\":\"not found\"}"),
    }
    false
}

fn main() {
    let port = env::args().nth(1).unwrap_or_else(|| "18080".to_string());
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
    let mut values = BTreeMap::new();
    for stream in listener.incoming() {
        if handle(stream.unwrap(), &mut values) {
            break;
        }
    }
}
