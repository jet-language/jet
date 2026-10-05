// Function-boundary cell temp-collection: Rust peer (local Vec per call).
#[inline(never)]
fn score(seed: i64) -> i64 {
    let mut tmp = Vec::new();
    for i in 0..16 {
        tmp.push((seed * 7 + i * 13) % 101);
    }
    let mut best = 0;
    for &x in &tmp {
        if x > best {
            best = x;
        }
    }
    best + tmp[(seed % 16) as usize]
}

fn main() {
    let n: i64 = std::env::args().nth(1).expect("n").parse().expect("n");
    let mut acc: i64 = 0;
    for i in 0..n {
        acc += score(i);
    }
    println!("{acc}");
}
