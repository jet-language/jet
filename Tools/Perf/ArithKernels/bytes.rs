// The Rust peer of bytes.jet: the same payload, the same two kernels (the
// frame loop reads fields the way the embedded-data Rust entry does), the
// same rounds and the same output lines.
//
//   rustc -O -C target-cpu=native -o /tmp/bytes Tools/Perf/ArithKernels/bytes.rs && /tmp/bytes [FRAMES]
use std::env;
use std::hint::black_box;
use std::time::Instant;

const FRAMES: i64 = 1_000_000;
const ROUNDS: usize = 5;

fn push_le(out: &mut Vec<u8>, value: i64, width: usize) {
    let mut rest = value;
    for _ in 0..width {
        out.push((rest % 256) as u8);
        rest /= 256;
    }
}

fn payload(frames: i64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"EDB1");
    push_le(&mut out, frames, 4);
    let mut seed: i64 = 12345;
    for index in 0..frames {
        seed = (seed * 1_103_515_245 + 12345) % 2_147_483_648;
        out.push((index % 4) as u8);
        out.push(if seed % 16 == 0 { 1 } else { 0 });
        push_le(&mut out, seed % 65536, 2);
        push_le(&mut out, index, 4);
        push_le(&mut out, (seed / 65536) % 65536, 2);
        push_le(&mut out, (seed / 7) % 65536, 2);
    }
    out
}

fn read_u8(data: &[u8], at: &mut usize) -> Option<u8> {
    let value = *data.get(*at)?;
    *at += 1;
    Some(value)
}

fn read_u16(data: &[u8], at: &mut usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    let bytes = data.get(*at..end)?;
    *at = end;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], at: &mut usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let bytes = data.get(*at..end)?;
    *at = end;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn take_pattern_kernel(data: &[u8]) -> Option<u64> {
    if data.get(..4) != Some(b"EDB1") {
        return None;
    }
    let mut at = 4usize;
    let count = read_u32(data, &mut at)?;
    let mut check = 0u64;
    for _ in 0..count {
        let channel = u64::from(read_u8(data, &mut at)?);
        let flags = u64::from(read_u8(data, &mut at)?);
        let sample = u64::from(read_u16(data, &mut at)?);
        let tick = u64::from(read_u32(data, &mut at)?);
        let energy = u64::from(read_u16(data, &mut at)?);
        let load = u64::from(read_u16(data, &mut at)?);
        check += channel + flags + sample + tick + energy + load;
    }
    Some(check)
}

fn byte_checksum_kernel(data: &[u8]) -> u64 {
    data.iter().map(|&byte| u64::from(byte)).sum()
}

fn main() {
    let frames: i64 = env::args().nth(1).and_then(|text| text.parse().ok()).unwrap_or(FRAMES);
    let data = payload(frames);

    let mut best = u128::MAX;
    let mut check = 0u64;
    for _ in 0..ROUNDS {
        let started = Instant::now();
        check = take_pattern_kernel(black_box(&data)).unwrap_or(0);
        best = best.min(started.elapsed().as_nanos());
    }
    println!("take_pattern frames={frames} best_ns={best} check={check}");

    best = u128::MAX;
    for _ in 0..ROUNDS {
        let started = Instant::now();
        check = byte_checksum_kernel(black_box(&data));
        best = best.min(started.elapsed().as_nanos());
    }
    println!("byte_checksum bytes={} best_ns={best} check={check}", data.len());
}
