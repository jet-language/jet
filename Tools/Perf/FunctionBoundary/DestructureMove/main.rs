// Function-boundary cell destructure-move (#4567): Rust peer.
#[inline(never)]
fn partition(seed: i64) -> (Vec<i64>, Vec<i64>) {
    let mut evens = Vec::new();
    let mut odds = Vec::new();
    for i in 0..64 {
        let v = seed + i;
        if v % 2 == 0 {
            evens.push(v);
        } else {
            odds.push(v);
        }
    }
    (evens, odds)
}

fn main() {
    let n: i64 = std::env::args().nth(1).expect("n").parse().expect("n");
    let mut acc: i64 = 0;
    for i in 0..n {
        let (evens, odds) = partition(i);
        acc += evens.len() as i64 + odds[0] + evens[31];
    }
    println!("{acc}");
}
