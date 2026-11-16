// Function-boundary cell fixed-return-4k: Rust peer (4096-byte array return).
const K: usize = 512;

#[inline(never)]
fn fill(seed: i64) -> [i64; K] {
    let mut out = [0i64; K];
    for i in 0..K {
        out[i] = seed + i as i64;
    }
    out
}

fn main() {
    let n: i64 = std::env::args().nth(1).expect("n").parse().expect("n");
    let mut acc: i64 = 0;
    for i in 0..n {
        let block = fill(i);
        for x in block {
            acc += x;
        }
    }
    println!("{acc}");
}
