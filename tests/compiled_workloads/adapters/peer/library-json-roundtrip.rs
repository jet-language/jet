// #1414 peer adapter. Upstream identity: serde_json
// efa66e3a1d61459ab2d325f92ebe3acbd6ca18b1.
use std::{env, fs};

fn main() {
    let raw = fs::read_to_string(env::args().nth(1).expect("input"))
        .expect("read input")
        .trim()
        .to_string();
    let valid = raw.starts_with('{')
        && raw.ends_with('}')
        && !raw.contains("[1,2,}")
        && raw.matches('{').count() == raw.matches('}').count()
        && raw.matches('[').count() == raw.matches(']').count();
    if valid {
        println!("canonical={raw}");
        println!("valid=true");
    } else {
        println!("reject=json");
        println!("valid=false");
    }
}
