/// Growable byte builder with one read cursor. EOF is `position == len`
/// (D-ITERTOOLS1=A / #1467). String-like ops decode UTF-8 then reuse String
/// behavior; invalid UTF-8 uses lossy replacement so beginners are not blocked.
#[derive(Clone)]
pub(crate) struct JetByteBuffer {
    pub(crate) bytes: Vec<u8>,
    pub(crate) pos: usize,
}
impl JetByteBuffer {
    pub(crate) fn new() -> Self {
        Self {
            bytes: Vec::new(),
            pos: 0,
        }
    }
    pub(crate) fn with_capacity(n: i64) -> Self {
        Self {
            bytes: Vec::with_capacity(n.max(0) as usize),
            pos: 0,
        }
    }
    pub(crate) fn from(bytes: &Vec<u8>) -> Self {
        Self {
            bytes: bytes.clone(),
            pos: 0,
        }
    }
    pub(crate) fn write_u8(&mut self, v: u8) {
        self.bytes.push(v);
    }
    pub(crate) fn write_byte(&mut self, v: u8) {
        self.write_u8(v);
    }
    pub(crate) fn write_u16_le(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn write_u16_be(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_be_bytes());
    }
    pub(crate) fn write_u32_le(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn write_u32_be(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_be_bytes());
    }
    pub(crate) fn write_u64_le(&mut self, v: u64) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn write_u64_be(&mut self, v: u64) {
        self.bytes.extend_from_slice(&v.to_be_bytes());
    }
    pub(crate) fn write_bytes(&mut self, bytes: &Vec<u8>) {
        self.bytes.extend_from_slice(bytes);
    }
    pub(crate) fn write(&mut self, bytes: &Vec<u8>) {
        self.write_bytes(bytes);
    }
    pub(crate) fn write_to(&mut self, other: &mut JetByteBuffer) {
        other.bytes.extend_from_slice(&self.bytes);
    }
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
    pub(crate) fn get_buffer(&self) -> Vec<u8> {
        self.bytes.clone()
    }
    pub(crate) fn buffer(&self) -> Vec<u8> {
        self.bytes.clone()
    }
    pub(crate) fn capacity(&self) -> i64 {
        self.bytes.capacity() as i64
    }
    pub(crate) fn len(&self) -> i64 {
        self.bytes.len() as i64
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    pub(crate) fn clear(&mut self) {
        self.bytes.clear();
        self.pos = 0;
    }
    pub(crate) fn position(&self) -> i64 {
        self.pos as i64
    }
    pub(crate) fn eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }
    pub(crate) fn rewind(&mut self) {
        self.pos = 0;
    }
    pub(crate) fn seek(&mut self, index: i64) {
        if index <= 0 {
            self.pos = 0;
        } else if (index as usize) > self.bytes.len() {
            self.pos = self.bytes.len();
        } else {
            self.pos = index as usize;
        }
    }
    pub(crate) fn get(&self, index: i64) -> JetOutcome<u8, JetAbsent> {
        if index < 0 {
            return Err(JetAbsent);
        }
        jet_outcome_of(self.bytes.get(index as usize).copied())
    }
    pub(crate) fn first(&self) -> JetOutcome<u8, JetAbsent> {
        jet_outcome_of(self.bytes.first().copied())
    }
    pub(crate) fn next(&mut self) -> JetOutcome<u8, JetAbsent> {
        self.read_byte()
    }
    pub(crate) fn read_byte(&mut self) -> JetOutcome<u8, JetAbsent> {
        if self.pos >= self.bytes.len() {
            return Err(JetAbsent);
        }
        let b = self.bytes[self.pos];
        self.pos += 1;
        Ok(b)
    }
    pub(crate) fn read_bytes(&mut self, n: i64) -> JetOutcome<Vec<u8>, JetAbsent> {
        if n < 0 {
            return Err(JetAbsent);
        }
        let n = n as usize;
        if self.pos + n > self.bytes.len() {
            return Err(JetAbsent);
        }
        let out = self.bytes[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(out)
    }
    pub(crate) fn read(&mut self) -> JetOutcome<Vec<u8>, JetAbsent> {
        if self.eof() {
            return Err(JetAbsent);
        }
        let out = self.bytes[self.pos..].to_vec();
        self.pos = self.bytes.len();
        Ok(out)
    }
    pub(crate) fn read_string(&mut self, n: i64) -> JetOutcome<String, JetAbsent> {
        self.read_bytes(n)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }
    pub(crate) fn as_text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
    pub(crate) fn to_string(&self) -> String {
        self.as_text()
    }
    pub(crate) fn string(&self) -> String {
        self.as_text()
    }
    pub(crate) fn contains(&self, needle: &String) -> bool {
        self.as_text().contains(needle.as_str())
    }
    pub(crate) fn starts_with(&self, prefix: &String) -> bool {
        self.as_text().starts_with(prefix.as_str())
    }
    pub(crate) fn ends_with(&self, suffix: &String) -> bool {
        self.as_text().ends_with(suffix.as_str())
    }
    pub(crate) fn trim(&self) -> JetByteBuffer {
        JetByteBuffer::from(&self.as_text().trim().as_bytes().to_vec())
    }
    pub(crate) fn trim_start(&self) -> JetByteBuffer {
        JetByteBuffer::from(&self.as_text().trim_start().as_bytes().to_vec())
    }
    pub(crate) fn trim_end(&self) -> JetByteBuffer {
        JetByteBuffer::from(&self.as_text().trim_end().as_bytes().to_vec())
    }
    pub(crate) fn to_lower(&self) -> JetByteBuffer {
        JetByteBuffer::from(&self.as_text().to_lowercase().into_bytes())
    }
    pub(crate) fn to_upper(&self) -> JetByteBuffer {
        JetByteBuffer::from(&self.as_text().to_uppercase().into_bytes())
    }
    pub(crate) fn to_title(&self) -> JetByteBuffer {
        let s = self.as_text();
        let mut out = String::with_capacity(s.len());
        let mut start = true;
        for ch in s.chars() {
            if ch.is_whitespace() {
                start = true;
                out.push(ch);
            } else if start {
                for c in ch.to_uppercase() {
                    out.push(c);
                }
                start = false;
            } else {
                for c in ch.to_lowercase() {
                    out.push(c);
                }
            }
        }
        JetByteBuffer::from(&out.into_bytes())
    }
    pub(crate) fn title(&self) -> JetByteBuffer {
        self.to_title()
    }
    pub(crate) fn replace(&self, from: &String, to: &String) -> JetByteBuffer {
        JetByteBuffer::from(&self.as_text().replace(from.as_str(), to.as_str()).into_bytes())
    }
    pub(crate) fn split(&self, sep: &String) -> Vec<String> {
        let text = self.as_text();
        if sep.is_empty() {
            let mut out = vec![String::new()];
            for ch in text.chars() {
                out.push(ch.to_string());
            }
            out.push(String::new());
            return out;
        }
        text.split(sep.as_str()).map(|s| s.to_string()).collect()
    }
    pub(crate) fn join(&self, parts: &Vec<String>) -> JetByteBuffer {
        JetByteBuffer::from(&parts.join(&self.as_text()).into_bytes())
    }
    pub(crate) fn lines(&self) -> Vec<String> {
        self.as_text().lines().map(|s| s.to_string()).collect()
    }
    pub(crate) fn index_of(&self, needle: &String) -> JetOutcome<i64, JetAbsent> {
        jet_outcome_of(self.as_text().find(needle.as_str()).map(|i| i as i64))
    }
    pub(crate) fn last_index_of(&self, needle: &String) -> JetOutcome<i64, JetAbsent> {
        jet_outcome_of(self.as_text().rfind(needle.as_str()).map(|i| i as i64))
    }
    pub(crate) fn is_ascii(&self) -> bool {
        self.bytes.is_ascii()
    }
    pub(crate) fn copy(&self) -> JetByteBuffer {
        self.clone()
    }
    pub(crate) fn copy_to(&self, other: &mut JetByteBuffer) {
        other.bytes = self.bytes.clone();
        other.pos = 0;
    }
    pub(crate) fn equal(&self, other: &JetByteBuffer) -> bool {
        self.bytes == other.bytes
    }
    pub(crate) fn compare(&self, other: &JetByteBuffer) -> i64 {
                match self.bytes.cmp(&other.bytes) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
    pub(crate) fn parse(&self) -> Result<i64, String> {
        self.as_text()
            .trim()
            .parse::<i64>()
            .map_err(|e| e.to_string())
    }
    pub(crate) fn flush(&mut self) {}
    pub(crate) fn close(&mut self) {
        self.clear();
    }
    pub(crate) fn shutdown(&mut self) {
        self.clear();
    }
}
