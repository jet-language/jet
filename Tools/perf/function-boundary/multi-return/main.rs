// Function-boundary cell multi-return: Rust peer.
struct Parts {
    quotient: i64,
    remainder: i64,
}

#[inline(never)]
fn split(n: i64, d: i64) -> Parts {
    Parts { quotient: n / d, remainder: n % d }
}

fn main() {
    let n: i64 = std::env::args().nth(1).expect("n").parse().expect("n");
    let mut acc: i64 = 0;
    for i in 0..n {
        let p = split(i, i % 13 + 3);
        acc += p.quotient + p.remainder;
    }
    println!("{acc}");
}
