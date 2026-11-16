// Function-boundary cell list-fresh: Rust peer (fresh Vec per call).
#[inline(never)]
fn build(seed: i64, count: i64) -> Vec<i64> {
    let mut out = Vec::new();
    for i in 0..count {
        out.push(seed + i);
    }
    out
}

fn main() {
    let n: i64 = std::env::args().nth(1).expect("n").parse().expect("n");
    let mut acc: i64 = 0;
    for i in 0..n {
        let xs = build(i, 256);
        for x in &xs {
            acc += x;
        }
    }
    println!("{acc}");
}
