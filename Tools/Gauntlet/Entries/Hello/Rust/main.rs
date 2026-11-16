use std::io::Write;

fn main() {
    std::io::stdout().write_all(b"hello\n").expect("stdout");
}
