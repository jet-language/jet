// Function-boundary cell string-reuse: Rust peer (caller-reused String).
#[inline(never)]
fn render_into(out: &mut String, seed: i64) {
    out.clear();
    for i in 0..(32 + seed % 32) {
        if (seed + i) % 2 == 0 {
            out.push_str("ab");
        } else {
            out.push_str("xyz");
        }
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).expect("n").parse().expect("n");
    let mut acc: i64 = 0;
    let mut text = String::new();
    for i in 0..n {
        render_into(&mut text, i);
        acc += text.len() as i64;
    }
    println!("{acc}");
}
