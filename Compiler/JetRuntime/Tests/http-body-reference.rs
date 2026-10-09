// Fixture-only Rust-tier reference appended to fresh same-tree allparts.
// Never compiled into a native pack. The seed HTTPBody kernel remains the
// reference until the coordinated Core/L9 cutover and Rust-tier retirement.
fn main() {
    let path = std::env::args().nth(1).expect("1GiB input path");
    let file = std::fs::File::open(path).expect("open perf input");
    let length = file.metadata().expect("stat perf input").len() as usize;
    assert_eq!(length, 1 << 30);
    let body = JetHTTPBody::file(file, length);
    let chunks = body.chunks(65536).unwrap_or_else(|_| panic!("reference take failed"));
    let mut count = 0u64;
    let mut checksum = 0u64;
    for chunk in chunks {
        let chunk = chunk.unwrap_or_else(|_| panic!("reference read failed"));
        // CAbiRoutes List output widens u8 -> u64. Materialize that same
        // carrier: comparing only a u8 Vec would omit the adapter's work.
        let words: Vec<u64> = chunk.into_iter().map(u64::from).collect();
        let words = std::hint::black_box(words);
        count += words.len() as u64;
        for word in words { checksum += word; }
    }
    assert_eq!(count, length as u64);
    assert!(body.is_drained());
    println!("bytes={} checksum={}", count, checksum);
}
