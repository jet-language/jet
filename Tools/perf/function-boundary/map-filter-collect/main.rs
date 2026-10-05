// Function-boundary cell map-filter-collect: Rust peer (iterator chain + collect).
fn main() {
    let n: i64 = std::env::args().nth(1).expect("n").parse().expect("n");
    let xs: Vec<i64> = (0..100000).collect();
    let mut acc: i64 = 0;
    for r in 0..n {
        let ys: Vec<i64> = xs.iter().map(|x| x * 3 + r).filter(|x| x % 2 == 0).map(|x| x + 1).collect();
        for y in &ys {
            acc += y;
        }
    }
    println!("{acc}");
}
