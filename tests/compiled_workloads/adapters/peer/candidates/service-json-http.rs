use std::{env, fs, process};

fn sum_numbers(body: &str) -> Option<i64> {
    let start = body.find('[')? + 1;
    let end = body[start..].find(']')? + start;
    let values = &body[start..end];
    if values.trim().is_empty() { return Some(0); }
    values.split(',').map(|part| part.trim().parse::<i64>().ok()).sum()
}

fn main() {
    let input = match env::args().nth(1) {
        Some(path) => path,
        None => process::exit(64),
    };
    let raw = match fs::read_to_string(input) {
        Ok(value) => value,
        Err(_) => process::exit(2),
    };
    let mut ready = false;
    let mut body_limit: i64 = 64;
    let mut requests = 0i64;
    let mut stopped = false;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let fields: Vec<&str> = line.split('|').collect();
        match fields.first().copied().unwrap_or("") {
            "CONFIG" => {
                if fields.len() >= 8 {
                    body_limit = fields[6].parse().unwrap_or(64);
                }
            }
            "WAIT" => ready = true,
            "GET" | "POST" | "PUT" | "PATCH" | "DELETE" => {
                requests += 1;
                let method = fields[0];
                let route = fields.get(1).copied().unwrap_or("");
                let body = fields.get(2).copied().unwrap_or("");
                match (method, route) {
                    ("GET", "/health") => println!("health=200"),
                    ("GET", "/ready") => println!("ready={}",&if ready { 200 } else { 503 }),
                    ("POST", "/sum") => {
                        if body.len() as i64 > body_limit { println!("oversize=413"); }
                        else if let Some(sum) = sum_numbers(body) { println!("sum={sum}"); }
                        else { println!("error=malformed-json"); }
                    }
                    (_, _) => println!("missing=404"),
                }
            }
            "CONCURRENT" => {
                let count = fields.get(3).and_then(|value| value.parse::<i64>().ok()).unwrap_or(0);
                requests += count;
                println!("concurrent={count}");
            }
            "MALFORMED" => { requests += 1; println!("malformed=400"); }
            "OVERSIZE" => { requests += 1; println!("oversize=413"); }
            "SLOW" => { requests += 1; println!("slow=timeout"); }
            "SHUTDOWN" => { stopped = true; println!("shutdown=clean"); }
            _ => process::exit(1),
        }
    }
    if !stopped { println!("shutdown=clean"); }
    println!("requests={requests}");
}
