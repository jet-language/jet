// Function-boundary cell split-join: Rust peer (split, collect, join).
fn main() {
    let n: i64 = std::env::args().nth(1).expect("n").parse().expect("n");
    let words: Vec<String> = (0..20000).map(|i| format!("w{}", i % 1000)).collect();
    let text = words.join(",");
    let mut acc: i64 = 0;
    for r in 0..n {
        let joined = text.split(',').collect::<Vec<&str>>().join(";");
        acc += joined.len() as i64 + r;
    }
    println!("{acc}");
}
