use std::{env, fs::File, io::{self, Read}};

fn main() -> io::Result<()> {
    let path = env::args().nth(1).unwrap_or_else(|| "records.bin".into());
    let mut data = Vec::new();
    File::open(path)?.read_to_end(&mut data)?;
    if data.get(..4) != Some(b"JGB1") { return Err(io::Error::new(io::ErrorKind::InvalidData, "bad magic")); }
    let count = u32::from_le_bytes(data[4..8].try_into().unwrap());
    let mut at = 8usize;
    let mut sum = 0.0f64;
    let mut hash = 0xcbf29ce484222325u64;
    for _ in 0..count {
        let id = u32::from_le_bytes(data[at..at + 4].try_into().unwrap()); at += 4;
        let value = f64::from_le_bytes(data[at..at + 8].try_into().unwrap()); at += 8;
        let name_len = u16::from_le_bytes(data[at..at + 2].try_into().unwrap()) as usize; at += 2;
        if id % 7 == 0 { sum += value; }
        for &byte in &data[at..at + name_len] { hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3); }
        at += name_len;
    }
    println!("records {count}");
    println!("sum7 {sum:.6}");
    println!("fnv {hash:016x}");
    Ok(())
}
