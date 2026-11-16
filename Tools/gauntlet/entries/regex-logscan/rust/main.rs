use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader};

fn main() -> io::Result<()> {
    let path = env::args().nth(1).expect("missing input");
    let input = BufReader::new(File::open(path)?);
    let mut counts = HashMap::<String, usize>::new();
    let mut matches = 0usize;

    for line in input.lines() {
        let line = line?;
        let Some(request_start) = line.find("\"GET ") else { continue };
        let request = &line[request_start + 5..];
        let Some(path) = request.split_whitespace().next() else { continue };
        if !path.starts_with("/api/") {
            continue;
        }
        let Some(status_start) = line.rfind("\" ").map(|i| i + 2) else { continue };
        let Some(status) = line[status_start..].split_whitespace().next() else { continue };
        if !status.starts_with('5') || status.len() != 3 {
            continue;
        }
        let ip = &line[..line.find(' ').unwrap_or(0)];
        *counts.entry(ip.to_string()).or_default() += 1;
        matches += 1;
    }

    println!("matches {matches}");
    let mut top = Vec::new();
    for _ in 0..5 {
        let best = counts
            .iter()
            .filter(|(ip, _)| !top.iter().any(|seen| seen == *ip))
            .max_by(|(ip_a, count_a), (ip_b, count_b)| count_a.cmp(count_b).then_with(|| ip_b.cmp(ip_a)))
            .map(|(ip, count)| (ip.clone(), *count));
        if let Some((ip, count)) = best {
            println!("{count} {ip}");
            top.push(ip);
        }
    }
    Ok(())
}
