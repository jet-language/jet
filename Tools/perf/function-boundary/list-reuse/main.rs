// Function-boundary cell list-reuse: Rust peer (caller-reused Vec).
#[inline(never)]
fn build_into(out: &mut Vec<i64>, seed: i64, count: i64) {
    out.clear();
    for i in 0..count {
        out.push(seed + i);
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).expect("n").parse().expect("n");
    let mut acc: i64 = 0;
    let mut xs = Vec::new();
    for i in 0..n {
        build_into(&mut xs, i, 256);
        for x in &xs {
            acc += x;
        }
    }
    println!("{acc}");
}
