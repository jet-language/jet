use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;

fn get(url: &str) -> (u16, String) {
    let rest = url.strip_prefix("http://").expect("http URL");
    let (authority, path) = rest.split_once('/').expect("URL path");
    let mut stream = TcpStream::connect(authority).expect("connect");
    write!(
        stream,
        "GET /{} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, authority
    )
    .expect("write request");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("read response");
    let response = String::from_utf8(response).expect("UTF-8 response");
    let (head, body) = response.split_once("\r\n\r\n").expect("response head");
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .expect("status");
    (status, body.to_owned())
}

fn field<'a>(body: &'a str, key: &str) -> &'a str {
    let marker = format!("\"{}\":", key);
    let start = body.find(&marker).expect("JSON field") + marker.len();
    let tail = &body[start..];
    let end = tail.find([',', '}']).expect("JSON value");
    tail[..end].trim_matches('"')
}

fn values(body: &str, key: &str) -> Vec<i32> {
    let marker = format!("\"{}\":", key);
    let mut cursor = 0;
    let mut result = Vec::new();
    while let Some(found) = body[cursor..].find(&marker) {
        let start = cursor + found + marker.len();
        let end = body[start..].find([',', '}']).expect("JSON value");
        result.push(body[start..start + end].parse().expect("JSON integer"));
        cursor = start + end + 1;
    }
    result
}

fn main() {
    let base = env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:18400".to_owned());
    let (_, list_body) = get(&format!("{base}/items"));
    let count = values(&list_body, "id").len();
    println!("items {count}");
    for id in [2, 5, 99] {
        let (status, body) = get(&format!("{base}/items/{id}"));
        if status == 404 {
            println!("item {id} missing");
        } else {
            println!(
                "item {} {} qty={}",
                field(&body, "id"),
                field(&body, "name"),
                field(&body, "qty")
            );
        }
    }
    let quantities = values(&list_body, "qty");
    println!("total-qty {}", quantities.iter().sum::<i32>());
}
