mod parser {
    pub fn token_count() -> usize {
        3
    }
}

mod typer {
    pub fn value_type() -> &'static str {
        "Int"
    }
}

fn main() {
    println!("packages=2 parser={} typer={}", parser::token_count(), typer::value_type());
    println!("build=single-graph artifacts=3");
}
