fn source_map(offset: usize) -> (usize, usize, usize, usize) {
    if offset < 11 {
        return (offset, offset, 1, offset + 1);
    }
    (offset, offset + 5, 2, offset - 10)
}

fn main() {
    let source = "fn run() {\n    pirnt(\"hi\")\n}";
    let offset = source.find("pirnt").expect("fixture token");
    let (start, end, line, col) = source_map(offset);
    println!(
        "diagnostic=E0102 line={} col={} span={}..{}",
        line, col, start, end
    );
    println!("fix=print source_map=stable");
}
