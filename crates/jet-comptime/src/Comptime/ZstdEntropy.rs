//! Private std-only Zstandard entropy substrate.
//!
//! This is production-compiled but not publicly dispatched until sequence
//! decoding completes the full `core.compress.zstd.decompress` contract.

const BLOCK_MAX: usize = 128 * 1024;

#[derive(Clone, Copy)]
struct FseEntry {
    baseline: usize,
    bits: u8,
    symbol: u8,
}

struct ForwardBits<'a> {
    source: &'a [u8],
    bit: usize,
}

impl ForwardBits<'_> {
    fn read(&mut self, count: usize) -> Option<u32> {
        if self.bit.checked_add(count)? > self.source.len() * 8 {
            return None;
        }
        let mut value = 0u32;
        for shift in 0..count {
            value |= u32::from((self.source[self.bit / 8] >> (self.bit % 8)) & 1) << shift;
            self.bit += 1;
        }
        Some(value)
    }
}

struct ReverseBits {
    bits: Vec<u8>,
    next: usize,
    remaining: isize,
}

impl ReverseBits {
    fn new(source: &[u8]) -> Option<Self> {
        let last = *source.last()?;
        if last == 0 {
            return None;
        }
        let marker = 7usize.checked_sub(last.leading_zeros() as usize)?;
        let mut bits = Vec::with_capacity((source.len() - 1) * 8 + marker);
        for bit in (0..marker).rev() {
            bits.push((last >> bit) & 1);
        }
        for byte in source[..source.len() - 1].iter().rev() {
            for bit in (0..8).rev() {
                bits.push((byte >> bit) & 1);
            }
        }
        Some(Self { remaining: bits.len() as isize, bits, next: 0 })
    }

    fn read(&mut self, count: u8) -> usize {
        let mut value = 0usize;
        for _ in 0..count {
            value = (value << 1) | usize::from(*self.bits.get(self.next).unwrap_or(&0));
            self.next += 1;
            self.remaining -= 1;
        }
        value
    }
}

#[derive(Clone)]
struct FseTable {
    log: u8,
    entries: Vec<FseEntry>,
}

impl FseTable {
    fn parse(source: &[u8], max_log: u8, max_symbol: usize) -> Option<(Self, usize)> {
        let mut bits = ForwardBits { source, bit: 0 };
        let log = 5 + bits.read(4)? as u8;
        if log > max_log {
            return None;
        }
        let total = 1u32 << log;
        let mut sum = 0u32;
        let mut probabilities = Vec::<i32>::new();
        while sum < total {
            let remaining = total - sum + 1;
            let width = 32 - remaining.leading_zeros();
            let unchecked = bits.read(width as usize)?;
            let low = (1 << width) - 1 - remaining;
            let mask = (1 << (width - 1)) - 1;
            let small = unchecked & mask;
            let value = if small < low {
                bits.bit = bits.bit.checked_sub(1)?;
                small
            } else if unchecked > mask {
                unchecked - low
            } else {
                unchecked
            };
            let probability = value as i32 - 1;
            probabilities.push(probability);
            match probability {
                -1 => sum += 1,
                0 => loop {
                    let skip = bits.read(2)? as usize;
                    probabilities.resize(probabilities.len().checked_add(skip)?, 0);
                    if skip != 3 {
                        break;
                    }
                },
                n if n > 0 => sum = sum.checked_add(n as u32)?,
                _ => return None,
            }
            if probabilities.len() > max_symbol + 1 || sum > total {
                return None;
            }
        }
        let size = 1usize << log;
        let mut entries = vec![FseEntry { baseline: 0, bits: 0, symbol: 0 }; size];
        let mut high = size;
        for (symbol, probability) in probabilities.iter().enumerate() {
            if *probability == -1 {
                high = high.checked_sub(1)?;
                entries[high] = FseEntry { baseline: 0, bits: log, symbol: symbol as u8 };
            }
        }
        let step = (size >> 1) + (size >> 3) + 3;
        let mut position = 0usize;
        for (symbol, probability) in probabilities.iter().enumerate() {
            for _ in 0..(*probability).max(0) {
                entries[position].symbol = symbol as u8;
                position = (position + step) & (size - 1);
                while position >= high {
                    position = (position + step) & (size - 1);
                }
            }
        }
        let mut counters = vec![0u32; probabilities.len()];
        for entry in &mut entries[..high] {
            let probability = probabilities[usize::from(entry.symbol)] as u32;
            let states = probability.next_power_of_two();
            let doubled = states - probability;
            let single = probability - doubled;
            let width = size as u32 / states;
            let number = counters[usize::from(entry.symbol)];
            if number < doubled {
                entry.baseline = (single * width + number * width * 2) as usize;
                entry.bits = width.trailing_zeros() as u8 + 1;
            } else {
                entry.baseline = ((number - doubled) * width) as usize;
                entry.bits = width.trailing_zeros() as u8;
            }
            counters[usize::from(entry.symbol)] += 1;
        }
        Some((Self { log, entries }, bits.bit.div_ceil(8)))
    }

    fn weights(&self, source: &[u8]) -> Option<Vec<u8>> {
        let mut bits = ReverseBits::new(source)?;
        let mut first = bits.read(self.log) as usize;
        let mut second = bits.read(self.log) as usize;
        let mut out = Vec::new();
        loop {
            let entry = *self.entries.get(first)?;
            out.push(entry.symbol);
            first = entry.baseline + bits.read(entry.bits);
            if bits.remaining <= -1 {
                out.push(self.entries.get(second)?.symbol);
                break;
            }
            let entry = *self.entries.get(second)?;
            out.push(entry.symbol);
            second = entry.baseline + bits.read(entry.bits);
            if bits.remaining <= -1 {
                out.push(self.entries.get(first)?.symbol);
                break;
            }
            if out.len() > 255 {
                return None;
            }
        }
        Some(out)
    }
}

#[derive(Clone, Copy)]
struct HuffEntry {
    symbol: u8,
    bits: u8,
}

#[derive(Clone)]
struct Huffman {
    log: u8,
    entries: Vec<HuffEntry>,
}

impl Huffman {
    fn parse(source: &[u8]) -> Option<(Self, usize)> {
        let header = *source.first()?;
        let (mut weights, used) = if header < 128 {
            let payload = source.get(1..1 + usize::from(header))?;
            let (table, table_used) = FseTable::parse(payload, 6, 255)?;
            let weights = table.weights(payload.get(table_used..)?)?;
            (weights, 1 + usize::from(header))
        } else {
            let count = usize::from(header - 127);
            let bytes = source.get(1..1 + count.div_ceil(2))?;
            let weights = (0..count)
                .map(|index| if index % 2 == 0 { bytes[index / 2] >> 4 } else { bytes[index / 2] & 15 })
                .collect();
            (weights, 1 + count.div_ceil(2))
        };
        if weights.len() > 255 {
            return None;
        }
        let sum = weights.iter().try_fold(0u32, |sum, weight| {
            (*weight <= 11).then(|| sum + if *weight == 0 { 0 } else { 1 << (*weight - 1) })
        })?;
        if sum == 0 {
            return None;
        }
        let log = (32 - sum.leading_zeros()) as u8;
        let leftover = (1u32 << log).checked_sub(sum)?;
        if !leftover.is_power_of_two() || log > 11 {
            return None;
        }
        weights.push((32 - leftover.leading_zeros()) as u8);
        let lengths = weights
            .iter()
            .map(|weight| if *weight == 0 { 0 } else { log + 1 - *weight })
            .collect::<Vec<_>>();
        let mut ranks = vec![0usize; usize::from(log) + 1];
        for length in &lengths {
            ranks[usize::from(*length)] += 1;
        }
        let mut starts = vec![0usize; usize::from(log) + 1];
        for bits in (1..=usize::from(log)).rev() {
            starts[bits - 1] = starts[bits] + ranks[bits] * (1 << (usize::from(log) - bits));
        }
        if starts[0] != 1 << log {
            return None;
        }
        let mut entries = vec![HuffEntry { symbol: 0, bits: 0 }; 1 << log];
        for (symbol, length) in lengths.into_iter().enumerate() {
            if length == 0 {
                continue;
            }
            let span = 1 << (log - length);
            let start = starts[usize::from(length)];
            starts[usize::from(length)] += span;
            for entry in &mut entries[start..start + span] {
                *entry = HuffEntry { symbol: symbol as u8, bits: length };
            }
        }
        Some((Self { log, entries }, used))
    }

    fn stream(&self, source: &[u8], expected: usize) -> Option<Vec<u8>> {
        let mut bits = ReverseBits::new(source)?;
        let mut state = bits.read(self.log);
        let mut out = Vec::with_capacity(expected);
        while bits.remaining > -isize::from(self.log) {
            let entry = *self.entries.get(state)?;
            out.push(entry.symbol);
            state = ((state << entry.bits) & (self.entries.len() - 1)) | bits.read(entry.bits);
            if out.len() > expected {
                return None;
            }
        }
        (bits.remaining == -isize::from(self.log) && out.len() == expected).then_some(out)
    }
}

#[derive(Default)]
pub(super) struct HuffmanState(Option<Huffman>);

pub(super) fn literals(
    block: &[u8],
    state: &mut HuffmanState,
) -> Option<(Vec<u8>, usize)> {
    let first = *block.first()?;
    let kind = first & 3;
    let format = (first >> 2) & 3;
    if kind < 2 {
        let (header, size) = match format {
            0 | 2 => (1, usize::from(first >> 3)),
            1 => (2, usize::from(first >> 4) + (usize::from(*block.get(1)?) << 4)),
            _ => (
                3,
                usize::from(first >> 4)
                    + (usize::from(*block.get(1)?) << 4)
                    + (usize::from(*block.get(2)?) << 12),
            ),
        };
        if size > BLOCK_MAX {
            return None;
        }
        return if kind == 0 {
            Some((block.get(header..header + size)?.to_vec(), header + size))
        } else {
            Some((vec![*block.get(header)?; size], header + 1))
        };
    }
    let (header, streams, regenerated, compressed) = match format {
        0 | 1 => (
            3,
            if format == 0 { 1 } else { 4 },
            usize::from(first >> 4) + ((usize::from(*block.get(1)?) & 0x3f) << 4),
            usize::from(*block.get(1)? >> 6) + (usize::from(*block.get(2)?) << 2),
        ),
        2 => (
            4,
            4,
            usize::from(first >> 4)
                + (usize::from(*block.get(1)?) << 4)
                + ((usize::from(*block.get(2)?) & 3) << 12),
            (usize::from(*block.get(2)?) >> 2) + (usize::from(*block.get(3)?) << 6),
        ),
        _ => (
            5,
            4,
            usize::from(first >> 4)
                + (usize::from(*block.get(1)?) << 4)
                + ((usize::from(*block.get(2)?) & 0x3f) << 12),
            (usize::from(*block.get(2)?) >> 6)
                + (usize::from(*block.get(3)?) << 2)
                + (usize::from(*block.get(4)?) << 10),
        ),
    };
    if regenerated > BLOCK_MAX || compressed == 0 {
        return None;
    }
    let payload = block.get(header..header + compressed)?;
    let tree_used = if kind == 2 {
        let (table, used) = Huffman::parse(payload)?;
        state.0 = Some(table);
        used
    } else {
        0
    };
    let table = state.0.as_ref()?;
    let source = payload.get(tree_used..)?;
    let out = if streams == 1 {
        table.stream(source, regenerated)?
    } else {
        let one = usize::from(u16::from_le_bytes(source.get(0..2)?.try_into().ok()?));
        let two = usize::from(u16::from_le_bytes(source.get(2..4)?.try_into().ok()?));
        let three = usize::from(u16::from_le_bytes(source.get(4..6)?.try_into().ok()?));
        let source = source.get(6..)?;
        let split2 = one.checked_add(two)?;
        let split3 = split2.checked_add(three)?;
        let each = regenerated.div_ceil(4);
        let mut out = Vec::with_capacity(regenerated);
        out.extend(table.stream(source.get(..one)?, each)?);
        out.extend(table.stream(source.get(one..split2)?, each)?);
        out.extend(table.stream(source.get(split2..split3)?, each)?);
        out.extend(table.stream(source.get(split3..)?, regenerated.checked_sub(each * 3)?)?);
        out
    };
    Some((out, header + compressed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    fn compress(plain: &[u8]) -> Vec<u8> {
        let mut child = Command::new("zstd")
            .args(["-q", "--no-check", "-c"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("zstd in Jet test environment");
        child.stdin.take().unwrap().write_all(plain).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        output.stdout
    }

    fn corpus(len: usize) -> (Vec<u8>, Vec<u8>) {
        let alphabet = if len < 1_000 { 8 } else if len < 100_000 { 16 } else { 64 };
        let mut value = 1u32;
        let mut plain = Vec::with_capacity(len);
        let mut seen = HashSet::new();
        while plain.len() < len {
            value ^= value << 13;
            value ^= value >> 17;
            value ^= value << 5;
            let candidate = if value & 3 == 0 { 0 } else { 1 + (value as u8 % (alphabet - 1)) };
            if plain.len() >= 3 {
                let at = plain.len();
                let key = u32::from_le_bytes([plain[at - 3], plain[at - 2], plain[at - 1], candidate]);
                if !seen.insert(key) {
                    continue;
                }
            }
            plain.push(candidate);
        }
        let frame = compress(&plain);
        (plain, frame)
    }

    fn repeat_corpus(len: usize) -> Vec<u8> {
        let mut value = 1u32;
        let plain = (0..len)
            .map(|_| {
                value = value.wrapping_mul(1_103_515_245).wrapping_add(12_345) & 0x7fff_ffff;
                b'A' + (value % 3) as u8
            })
            .collect::<Vec<_>>();
        compress(&plain)
    }

    fn blocks(frame: &[u8]) -> Vec<&[u8]> {
        assert_eq!(&frame[..4], &[40, 181, 47, 253]);
        let descriptor = frame[4];
        let single = descriptor & 0x20 != 0;
        let dict = [0usize, 1, 2, 4][usize::from(descriptor & 3)];
        let fcs = match (descriptor >> 6, single) {
            (0, false) => 0,
            (0, true) => 1,
            (1, _) => 2,
            (2, _) => 4,
            _ => 8,
        };
        let mut offset = 5 + usize::from(!single) + dict + fcs;
        let mut out = Vec::new();
        loop {
            let header = u32::from_le_bytes([frame[offset], frame[offset + 1], frame[offset + 2], 0]);
            offset += 3;
            let size = (header >> 3) as usize;
            out.push(&frame[offset..offset + size]);
            offset += size;
            if header & 1 != 0 {
                return out;
            }
        }
    }

    #[test]
    fn ordinary_one_and_four_stream_huffman_weights_decode() {
        for (len, streams) in [(100, 1), (10_000, 4)] {
            let (plain, frame) = corpus(len);
            let block = blocks(&frame)[0];
            assert_eq!(block[0] & 3, 2, "ordinary corpus must use Huffman literals");
            assert_eq!(if (block[0] >> 2) & 3 == 0 { 1 } else { 4 }, streams);
            let (decoded, used) = literals(block, &mut HuffmanState::default()).expect("decode");
            assert_eq!(&block[used..], &[0], "corpus must have no sequences");
            assert_eq!(decoded, plain);
        }
    }

    #[test]
    fn ordinary_multiblock_corpus_reuses_huffman_table() {
        let frame = repeat_corpus(300_000);
        let mut state = HuffmanState::default();
        let mut kinds = Vec::new();
        for block in blocks(&frame) {
            if block[0] & 3 >= 2 {
                kinds.push(block[0] & 3);
                let (literals, used) = literals(block, &mut state).expect("decode");
                assert!(!literals.is_empty() && used < block.len());
            }
        }
        assert!(kinds.contains(&2) && kinds.contains(&3), "expected compressed + treeless: {kinds:?}");
    }

    #[test]
    fn entropy_headers_reject_truncation_and_oversized_counts() {
        assert!(literals(&[], &mut HuffmanState::default()).is_none());
        assert!(literals(&[0xff], &mut HuffmanState::default()).is_none());
        assert!(Huffman::parse(&[0]).is_none());
        assert!(FseTable::parse(&[0xff], 6, 255).is_none());
    }
}
