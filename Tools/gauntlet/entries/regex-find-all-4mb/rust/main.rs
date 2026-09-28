use std::env;
use std::fs;
use std::io;

fn main() -> io::Result<()> {
    let path = env::args().nth(1).expect("missing input");
    let text = fs::read_to_string(path)?;
    let matches = text.match_indices("struct").count();
    println!("matches {matches}");
    Ok(())
}
