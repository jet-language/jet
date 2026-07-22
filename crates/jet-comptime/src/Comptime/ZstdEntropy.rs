//! Private std-only Zstandard entropy substrate.
//!
//! This is production-compiled but not publicly dispatched until the complete
//! frame, block, dictionary, and checksum contract has closed review.

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
        let table = Self::from_probabilities(log, &probabilities)?;
        Some((table, bits.bit.div_ceil(8)))
    }

    fn from_probabilities(log: u8, probabilities: &[i32]) -> Option<Self> {
        let size = 1usize << log;
        let total = probabilities.iter().try_fold(0usize, |sum, probability| {
            sum.checked_add(match *probability {
                -1 => 1,
                value if value >= 0 => value as usize,
                _ => return None,
            })
        })?;
        if total != size || probabilities.is_empty() || probabilities.len() > 256 {
            return None;
        }
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
        Some(Self { log, entries })
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
        if !weights.contains(&1)
            || weights.iter().filter(|weight| **weight != 0).take(2).count() < 2
        {
            return None;
        }
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
    if regenerated > BLOCK_MAX || compressed == 0 || (streams == 4 && regenerated < 6) {
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

const LL_BASE: [usize; 36] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 24,
    28, 32, 40, 48, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768,
    65536,
];
const LL_BITS: [u8; 36] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3,
    4, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
];
const ML_BASE: [usize; 53] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
    23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 37, 39, 41, 43, 47,
    51, 59, 67, 83, 99, 131, 259, 515, 1027, 2051, 4099, 8195, 16387, 32771,
    65539,
];
const ML_BITS: [u8; 53] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 4, 4, 5, 7, 8, 9, 10,
    11, 12, 13, 14, 15, 16,
];
const LL_DEFAULT: [i32; 36] = [
    4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 3, 2, 1, 1, 1, 1, 1, -1, -1, -1, -1,
];
const ML_DEFAULT: [i32; 53] = [
    1, 4, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1,
    -1, -1, -1, -1, -1, -1,
];
const OF_DEFAULT: [i32; 29] = [
    1, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    -1, -1, -1, -1, -1,
];

pub(super) struct SequenceState {
    literal_lengths: Option<FseTable>,
    offsets: Option<FseTable>,
    match_lengths: Option<FseTable>,
    recent_offsets: [usize; 3],
}

impl Default for SequenceState {
    fn default() -> Self {
        Self {
            literal_lengths: None,
            offsets: None,
            match_lengths: None,
            recent_offsets: [1, 4, 8],
        }
    }
}

fn sequence_count(source: &[u8]) -> Option<(usize, usize)> {
    match *source.first()? {
        0..=127 => Some((usize::from(source[0]), 1)),
        128..=254 => Some((
            ((usize::from(source[0]) - 128) << 8) + usize::from(*source.get(1)?),
            2,
        )),
        255 => Some((
            usize::from(*source.get(1)?) + (usize::from(*source.get(2)?) << 8) + 0x7f00,
            3,
        )),
    }
}

fn sequence_table(
    mode: u8,
    source: &[u8],
    current: &mut Option<FseTable>,
    max_log: u8,
    max_symbol: usize,
    default_log: u8,
    default: &[i32],
) -> Option<usize> {
    match mode {
        0 => {
            *current = Some(FseTable::from_probabilities(default_log, default)?);
            Some(0)
        }
        1 => {
            let symbol = *source.first()?;
            if usize::from(symbol) > max_symbol {
                return None;
            }
            *current = Some(FseTable {
                log: 0,
                entries: vec![FseEntry { baseline: 0, bits: 0, symbol }],
            });
            Some(1)
        }
        2 => {
            let (table, used) = FseTable::parse(source, max_log, max_symbol)?;
            *current = Some(table);
            Some(used)
        }
        3 => current.as_ref().map(|_| 0),
        _ => None,
    }
}

fn update_sequence_state(table: &FseTable, entry: FseEntry, bits: &mut ReverseBits) -> Option<usize> {
    let state = entry.baseline.checked_add(bits.read(entry.bits))?;
    (state < table.entries.len()).then_some(state)
}

fn resolve_offset(value: usize, literals: usize, recent: &mut [usize; 3]) -> Option<usize> {
    let actual = if literals != 0 {
        match value {
            1..=3 => recent[value - 1],
            _ => value.checked_sub(3)?,
        }
    } else {
        match value {
            1..=2 => recent[value],
            3 => recent[0].checked_sub(1)?,
            _ => value.checked_sub(3)?,
        }
    };
    if actual == 0 {
        return None;
    }
    match (literals != 0, value) {
        (true, 1) => {}
        (true, 2) | (false, 1) => {
            recent[1] = recent[0];
            recent[0] = actual;
        }
        _ => {
            recent[2] = recent[1];
            recent[1] = recent[0];
            recent[0] = actual;
        }
    }
    Some(actual)
}

fn append_match(
    out: &mut Vec<u8>,
    frame_start: usize,
    offset: usize,
    length: usize,
    window: usize,
    maximum: usize,
) -> Option<()> {
    if offset > window || out.len().checked_sub(offset)? < frame_start {
        return None;
    }
    if out.len().checked_add(length)? > maximum {
        return None;
    }
    for _ in 0..length {
        out.push(out[out.len() - offset]);
    }
    Some(())
}

pub(super) fn sequences(
    source: &[u8],
    literals: &[u8],
    state: &mut SequenceState,
    out: &mut Vec<u8>,
    frame_start: usize,
    window: usize,
    block_max: usize,
    maximum: usize,
) -> Option<()> {
    let block_start = out.len();
    let (count, mut used) = sequence_count(source)?;
    if count == 0 {
        if used != source.len() || out.len().checked_add(literals.len())? > maximum {
            return None;
        }
        out.extend_from_slice(literals);
        return (out.len() - block_start <= block_max).then_some(());
    }
    let modes = *source.get(used)?;
    used += 1;
    if modes & 3 != 0 {
        return None;
    }
    let ll_mode = modes >> 6;
    let of_mode = (modes >> 4) & 3;
    let ml_mode = (modes >> 2) & 3;
    let table_source = source.get(used..)?;
    used += sequence_table(
        ll_mode,
        table_source,
        &mut state.literal_lengths,
        9,
        35,
        6,
        &LL_DEFAULT,
    )?;
    let table_source = source.get(used..)?;
    used += sequence_table(
        of_mode,
        table_source,
        &mut state.offsets,
        8,
        31,
        5,
        &OF_DEFAULT,
    )?;
    let table_source = source.get(used..)?;
    used += sequence_table(
        ml_mode,
        table_source,
        &mut state.match_lengths,
        9,
        52,
        6,
        &ML_DEFAULT,
    )?;

    let ll_table = state.literal_lengths.as_ref()?;
    let of_table = state.offsets.as_ref()?;
    let ml_table = state.match_lengths.as_ref()?;
    let mut bits = ReverseBits::new(source.get(used..)?)?;
    let mut ll_state = bits.read(ll_table.log);
    let mut of_state = bits.read(of_table.log);
    let mut ml_state = bits.read(ml_table.log);
    if bits.remaining < 0 {
        return None;
    }
    let mut literal = 0usize;
    for index in 0..count {
        let ll_entry = *ll_table.entries.get(ll_state)?;
        let of_entry = *of_table.entries.get(of_state)?;
        let ml_entry = *ml_table.entries.get(ml_state)?;
        let ll_code = usize::from(ll_entry.symbol);
        let ml_code = usize::from(ml_entry.symbol);
        let of_code = of_entry.symbol;
        let offset_value = (1usize << of_code).checked_add(bits.read(of_code))?;
        let match_length = ML_BASE.get(ml_code)?.checked_add(bits.read(ML_BITS[ml_code]))?;
        let literal_length = LL_BASE.get(ll_code)?.checked_add(bits.read(LL_BITS[ll_code]))?;
        if bits.remaining < 0 {
            return None;
        }
        let literal_end = literal.checked_add(literal_length)?;
        let selected = literals.get(literal..literal_end)?;
        let after_sequence = out
            .len()
            .checked_add(selected.len())?
            .checked_add(match_length)?;
        if after_sequence > maximum || after_sequence.checked_sub(block_start)? > block_max {
            return None;
        }
        out.extend_from_slice(selected);
        literal = literal_end;
        let offset = resolve_offset(offset_value, literal_length, &mut state.recent_offsets)?;
        append_match(out, frame_start, offset, match_length, window, maximum)?;
        if index + 1 < count {
            ll_state = update_sequence_state(ll_table, ll_entry, &mut bits)?;
            ml_state = update_sequence_state(ml_table, ml_entry, &mut bits)?;
            of_state = update_sequence_state(of_table, of_entry, &mut bits)?;
            if bits.remaining < 0 {
                return None;
            }
        }
    }
    if bits.remaining != 0 {
        return None;
    }
    let remaining = literals.get(literal..)?;
    let end = out.len().checked_add(remaining.len())?;
    if end > maximum || end.checked_sub(block_start)? > block_max {
        return None;
    }
    out.extend_from_slice(remaining);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    fn compress(plain: &[u8]) -> Vec<u8> {
        compress_with(plain, &[])
    }

    fn compress_with(plain: &[u8], args: &[&str]) -> Vec<u8> {
        let mut child = Command::new("zstd")
            .args(["-q", "--no-check", "-c"])
            .args(args)
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

    fn sequence_repeat_plain() -> Vec<u8> {
        let mut value = 1u32;
        let base = (0..BLOCK_MAX)
            .map(|_| {
                value = value.wrapping_mul(1_103_515_245).wrapping_add(12_345) & 0x7fff_ffff;
                (value % 3) as u8
            })
            .collect::<Vec<_>>();
        let mut plain = Vec::with_capacity(BLOCK_MAX * 3);
        for shift in [0, 4, 8] {
            plain.extend(base.iter().map(|byte| byte + shift));
        }
        plain
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
    fn ordinary_multiblock_corpus_reuses_sequence_table() {
        let plain = sequence_repeat_plain();
        let frame = compress_with(&plain, &["-19"]);
        let mut huffman = HuffmanState::default();
        let mut modes = Vec::new();
        for block in blocks(&frame) {
            let (_, used) = literals(block, &mut huffman).expect("literals");
            let section = &block[used..];
            let (count, header) = sequence_count(section).expect("sequence header");
            if count != 0 {
                modes.push(section[header]);
            }
        }
        assert!(modes.iter().any(|modes| {
            [modes >> 6, (modes >> 4) & 3, (modes >> 2) & 3].contains(&3)
        }), "stock corpus must repeat an LL/ML/OF table: {modes:02x?}");
    }

    #[test]
    fn sequence_modes_and_headers_enforce_spec_bounds() {
        assert_eq!(sequence_count(&[127]), Some((127, 1)));
        assert_eq!(sequence_count(&[128, 5]), Some((5, 2)));
        assert_eq!(sequence_count(&[255, 1, 2]), Some((0x8101, 3)));

        let mut state = SequenceState::default();
        let mut out = vec![b'A'];
        assert!(sequences(
            &[1, 0x54, 1, 0, 0, 1],
            b"B",
            &mut state,
            &mut out,
            0,
            8,
            128,
            128,
        )
        .is_some());
        assert_eq!(out, b"ABBBB");
        assert!(sequences(
            &[1, 0xfc, 1],
            b"C",
            &mut state,
            &mut out,
            0,
            8,
            128,
            128,
        )
        .is_some());
        assert_eq!(out, b"ABBBBCCCC");
        let mut zero_count = Vec::new();
        assert!(sequences(
            &[128, 0],
            b"literal",
            &mut SequenceState::default(),
            &mut zero_count,
            0,
            8,
            128,
            128,
        )
        .is_some());
        assert_eq!(zero_count, b"literal");

        for (window, block_max) in [(0, 128), (8, 3)] {
            assert!(sequences(
                &[1, 0x54, 1, 0, 0, 1],
                b"B",
                &mut SequenceState::default(),
                &mut vec![b'A'],
                0,
                window,
                block_max,
                128,
            )
            .is_none());
        }

        for malformed in [
            &[0, 0][..],
            &[1, 0xfc, 1],
            &[1, 1, 1],
            &[1, 0x40, 36, 1],
            &[1, 0x10, 32, 1],
            &[1, 0x04, 53, 1],
            &[1, 0x54, 0, 0, 0, 1],
            &[1, 0x54, 1, 0, 0, 3],
        ] {
            assert!(sequences(
                malformed,
                b"B",
                &mut SequenceState::default(),
                &mut vec![b'A'],
                0,
                8,
                128,
                128,
            )
            .is_none(), "accepted malformed sequence section: {malformed:?}");
        }
    }

    #[test]
    fn entropy_headers_reject_truncation_and_oversized_counts() {
        assert!(literals(&[], &mut HuffmanState::default()).is_none());
        assert!(literals(&[0xff], &mut HuffmanState::default()).is_none());
        assert!(literals(
            &[0x12, 0xc0, 0x00, 0x80, 0x20, 0x02],
            &mut HuffmanState::default(),
        )
        .is_none());
        assert!(Huffman::parse(&[0]).is_none());
        assert!(FseTable::parse(&[0xff], 6, 255).is_none());
    }
}
