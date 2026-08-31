// #1414 peer adapter. Upstream identity: ripgrep 4649aa9700619f94cf9c66876e9549d83420e16c.
use std::{env, fs};

fn main() {
    let input = env::args().nth(1).expect("input");
    let raw = fs::read_to_string(input).expect("read input");
    let mut files = 0;
    let mut bytes = 0;
    let mut rejects = 0;
    let mut paths = Vec::new();
    for line in raw.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<_> = line.split('|').collect();
        if fields.len() != 3 {
            rejects += 1;
            continue;
        }
        let safe = !fields[1].is_empty()
            && !fields[1].starts_with('/')
            && !fields[1].contains("..");
        match (fields[0], safe) {
            ("file", true) => match fields[2].parse::<i64>() {
                Ok(size) => {
                    files += 1;
                    bytes += size;
                    paths.push(fields[1]);
                }
                Err(_) => rejects += 1,
            },
            ("dir" | "link", true) => {}
            _ => rejects += 1,
        }
    }
    println!("files={files}");
    println!("bytes={bytes}");
    println!("paths={}", paths.join(","));
    println!("rejects={rejects}");
}
