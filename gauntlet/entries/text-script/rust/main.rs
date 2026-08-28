use std::{env, fs};

fn main() {
    let path = env::args().nth(1).unwrap_or_else(|| "notes.txt".to_string());
    let input = fs::read_to_string(path).expect("read failed");
    let mut lines: Vec<String> = input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    lines.sort();
    println!("lines {}", lines.len());
    for line in lines {
        println!("{line}");
    }
}
