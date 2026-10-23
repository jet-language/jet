fn main() {
    let values = [1i64, 2, 3, 4];
    let total: i64 = values.iter().map(|value| value * 2).sum();
    println!("{total}");
}
