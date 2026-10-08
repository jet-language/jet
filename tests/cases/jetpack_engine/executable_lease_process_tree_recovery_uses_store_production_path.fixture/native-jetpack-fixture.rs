fn main() {
    if std::env::args().nth(1).as_deref() == Some("--hold") {
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    if std::env::args().nth(1).as_deref() == Some("--help") {
        println!("jetpack platform fixture");
    }
}
