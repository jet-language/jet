// Function-boundary cell string-fresh: Rust peer (fresh String per call).
#[inline(never)]
fn render(seed: i64) -> String {
    let mut out = String::new();
    for i in 0..(32 + seed % 32) {
        if (seed + i) % 2 == 0 {
            out.push_str("ab");
        } else {
            out.push_str("xyz");
        }
    }
    out
}

fn main() {
    let n: i64 = std::env::args().nth(1).expect("n").parse().expect("n");
    let mut acc: i64 = 0;
    for i in 0..n {
        let text = render(i);
        acc += text.len() as i64;
    }
    println!("{acc}");
}
