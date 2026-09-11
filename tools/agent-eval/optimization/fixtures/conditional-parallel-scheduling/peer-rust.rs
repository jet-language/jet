fn main() {
    let values: Vec<i64> = (0..1024).collect();
    let results: Vec<i64> = values.iter().map(|value| value * value).collect();
    println!("{}:{}", results[0], results[1023]);
}
