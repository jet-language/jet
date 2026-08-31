use std::fs;
use std::process;

const RECORD_BYTES: usize = 64;
const METADATA_BYTES: usize = 4;
const PAYLOAD_BYTES: usize = RECORD_BYTES - METADATA_BYTES;
const KIND_COUNT: usize = 5;

fn main() {
    let data = fs::read("fuzz-input.bin").expect("fuzz input");
    if data.is_empty() || data.len() % RECORD_BYTES != 0 {
        eprintln!("invalid fuzz corpus");
        process::exit(2);
    }
    let mut counts = [0usize; KIND_COUNT];
    let mut checksum = 0u32;
    let mut semantic = 0u32;
    for record in data.chunks_exact(RECORD_BYTES) {
        let kind = usize::from(record[0]);
        if kind >= KIND_COUNT {
            eprintln!("invalid fuzz case kind");
            process::exit(2);
        }
        let declared_length = usize::from(record[1]);
        let requested_index = usize::from(record[2]);
        let bounded_length = declared_length.min(PAYLOAD_BYTES);
        let safe_index = requested_index.min(PAYLOAD_BYTES);
        let mut value = 0u32;
        counts[kind] += 1;
        for byte in record {
            checksum = checksum.wrapping_add(u32::from(*byte));
        }
        if kind == 0 || kind == 2 {
            for byte in &record[METADATA_BYTES..METADATA_BYTES + bounded_length] {
                value = value.wrapping_add(u32::from(*byte));
            }
        } else if requested_index < PAYLOAD_BYTES {
            let selected = record[METADATA_BYTES + requested_index];
            value = u32::from(selected);
            if kind == 4 {
                value ^= 0xa5;
            }
        }
        semantic = semantic.wrapping_add(value);
        semantic = semantic.wrapping_add(((kind + 1) * 257 + bounded_length + safe_index) as u32);
    }
    println!(
        "cases {} valid {} boundary {} oob {} use_after_free {} wrong_output {} bytes {} checksum {} semantic {}",
        data.len() / RECORD_BYTES,
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        counts[4],
        data.len(),
        checksum,
        semantic
    );
}
