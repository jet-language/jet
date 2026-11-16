use std::{env, fs, io};

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn read_u8(data: &[u8], at: &mut usize) -> io::Result<u8> {
    let value = *data.get(*at).ok_or_else(|| invalid("short frame"))?;
    *at += 1;
    Ok(value)
}

fn read_u16(data: &[u8], at: &mut usize) -> io::Result<u16> {
    let end = at.checked_add(2).ok_or_else(|| invalid("offset overflow"))?;
    let bytes = data.get(*at..end).ok_or_else(|| invalid("short frame"))?;
    *at = end;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], at: &mut usize) -> io::Result<u32> {
    let end = at.checked_add(4).ok_or_else(|| invalid("offset overflow"))?;
    let bytes = data.get(*at..end).ok_or_else(|| invalid("short frame"))?;
    *at = end;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn main() -> io::Result<()> {
    let path = env::args().nth(1).unwrap_or_else(|| "telemetry.bin".to_string());
    let data = fs::read(path)?;
    if data.get(..4) != Some(b"EDB1") {
        return Err(invalid("bad magic"));
    }
    let mut at = 4usize;
    let count = read_u32(&data, &mut at)?;
    let checksum: u64 = data.iter().map(|&byte| u64::from(byte)).sum();

    let mut valid = 0u64;
    let mut channels = [0u64; 4];
    let mut sample_min = u64::from(u16::MAX);
    let mut sample_max = 0u64;
    let mut sample_sum = 0u64;
    let mut energy_sum = 0u64;
    let mut load_sum = 0u64;
    let mut tick_sum = 0u64;

    for _ in 0..count {
        let channel = usize::from(read_u8(&data, &mut at)?);
        let flags = read_u8(&data, &mut at)?;
        let sample = u64::from(read_u16(&data, &mut at)?);
        let tick = u64::from(read_u32(&data, &mut at)?);
        let energy = u64::from(read_u16(&data, &mut at)?);
        let load = u64::from(read_u16(&data, &mut at)?);
        if channel >= channels.len() {
            return Err(invalid("bad channel"));
        }
        if flags == 0 {
            valid += 1;
            channels[channel] += 1;
            sample_min = sample_min.min(sample);
            sample_max = sample_max.max(sample);
            sample_sum += sample;
            energy_sum += energy;
            load_sum += load;
            tick_sum += tick;
        }
    }

    println!("frames {count}");
    println!("valid {valid}");
    println!("channels {} {} {} {}", channels[0], channels[1], channels[2], channels[3]);
    println!("sample {sample_min} {sample_max} {sample_sum}");
    println!("energy {energy_sum}");
    println!("load {load_sum}");
    println!("ticks {tick_sum}");
    println!("checksum {checksum}");
    Ok(())
}
