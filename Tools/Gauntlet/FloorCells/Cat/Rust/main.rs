use std::io::{self, BufRead, BufWriter, Write};

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = BufWriter::with_capacity(64 * 1024, stdout.lock());
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        out.write_all(line.as_bytes()).unwrap();
        out.write_all(b"\n").unwrap();
    }
    out.flush().unwrap();
}
