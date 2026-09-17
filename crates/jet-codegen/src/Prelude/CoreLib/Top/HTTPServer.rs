// ── D-HTTPLIB1=A / D-HTTPLIB2=B: core.http.server — function-first mux ───────
// Plain HTTP is pure std. On wasm32-wasip2, Rust std delegates socket calls to
// the canonical wasi:sockets adapter. D-TLSSERVE1=A routes server TLS through
// the hidden rustls bridge only when the named `tls:` option is used.

const JET_HTTP_KEEPALIVE_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const JET_HTTP_MAX_REQUESTS_PER_CONNECTION: usize = 1000;
const JET_HTTP_MAX_CHUNK_FRAMING_BYTES: usize = 32 * 1024;

type JetHTTPMiddleware = std::sync::Arc<dyn Fn(JetHTTPHandler) -> JetHTTPHandler + Send + Sync>;

#[derive(Clone)]
struct JetHTTPMuxRoute {
    method: String,
    pattern: String,
    handler: JetHTTPHandler,
    /// Checked endpoint contract carried through registration for inspection.
    contract_json: String,
}
#[derive(Clone)]
struct JetHTTPMuxRouteCacheEntry {
    route: JetHTTPMuxRoute,
    pattern: Option<JetHTTPRoutePattern>,
}

struct JetHTTPMuxRouteCache {
    routes: Vec<JetHTTPMuxRouteCacheEntry>,
    validation_error: Option<String>,
}

#[derive(Clone)]
pub(crate) struct JetHTTPMux(
    std::sync::Arc<std::sync::Mutex<Vec<JetHTTPMuxRoute>>>,
    std::sync::Arc<std::sync::Mutex<Vec<JetHTTPMiddleware>>>,
    std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<JetHTTPMuxRouteCache>>>>,
);

#[derive(Clone)]
struct JetHTTPServerTls {
    cert_pem: String,
    key_pem: String,
}

#[derive(Debug)]
struct JetHTTPReadError {
    status: i64,
    message: &'static str,
}

#[derive(Clone, Copy, Debug)]
enum JetHTTPRequestFraming {
    ContentLength(usize),
    Chunked,
}

#[derive(Clone, Debug)]
struct JetHTTPRequestHead {
    framing: JetHTTPRequestFraming,
    expect_continue: bool,
    method: String,
    version: String,
    target: String,
    headers: JetHTTPHeaders,
    content_encoding_layers: usize,
    trailer_names: Vec<String>,
}


struct JetHTTPDeflateBits<'a> {
    input: &'a [u8],
    bit: usize,
}

impl JetHTTPDeflateBits<'_> {
    fn read(&mut self, count: usize) -> Result<u32, JetHTTPReadError> {
        let value = self.peek(count)?;
        self.bit += count;
        Ok(value)
    }

    fn peek(&self, count: usize) -> Result<u32, JetHTTPReadError> {
        if self.bit.saturating_add(count) > self.input.len().saturating_mul(8) {
            return Err(jet_http_gzip_invalid());
        }
        let mut value = 0u32;
        for offset in 0..count {
            let bit = self.bit + offset;
            value |= u32::from((self.input[bit / 8] >> (bit % 8)) & 1) << offset;
        }
        Ok(value)
    }

    fn align(&mut self) {
        self.bit = self.bit.saturating_add(7) & !7;
    }
}

struct JetHTTPDeflateTable {
    entries: Vec<(u16, u8)>,
    max_bits: usize,
}

impl JetHTTPDeflateTable {
    fn new(lengths: &[u8], require_complete: bool) -> Result<Self, JetHTTPReadError> {
        let max_bits = usize::from(lengths.iter().copied().max().unwrap_or(0));
        if max_bits > 15 {
            return Err(jet_http_gzip_invalid());
        }
        if max_bits == 0 {
            return if require_complete {
                Err(jet_http_gzip_invalid())
            } else {
                Ok(Self { entries: Vec::new(), max_bits: 0 })
            };
        }
        let mut counts = [0usize; 16];
        for &length in lengths {
            counts[usize::from(length)] += usize::from(length != 0);
        }
        let mut left = 1isize;
        for count in counts.iter().skip(1) {
            left = left.saturating_mul(2) - *count as isize;
            if left < 0 {
                return Err(jet_http_gzip_invalid());
            }
        }
        if left > 0 && (require_complete || max_bits != 1) {
            return Err(jet_http_gzip_invalid());
        }
        let mut next = [0usize; 16];
        let mut code = 0usize;
        for bits in 1..=15 {
            code = (code + counts[bits - 1]) << 1;
            next[bits] = code;
        }
        let mut entries = vec![(0u16, 0u8); 1usize << max_bits];
        for (symbol, &length) in lengths.iter().enumerate() {
            let length = usize::from(length);
            if length == 0 {
                continue;
            }
            let canonical = next[length];
            next[length] += 1;
            let mut reversed = 0usize;
            for bit in 0..length {
                reversed |= ((canonical >> bit) & 1) << (length - bit - 1);
            }
            for suffix in 0..(1usize << (max_bits - length)) {
                let entry = &mut entries[reversed | (suffix << length)];
                if entry.1 != 0 {
                    return Err(jet_http_gzip_invalid());
                }
                *entry = (symbol as u16, length as u8);
            }
        }
        Ok(Self { entries, max_bits })
    }

    fn symbol(&self, bits: &mut JetHTTPDeflateBits<'_>) -> Result<usize, JetHTTPReadError> {
        if self.entries.is_empty() {
            return Err(jet_http_gzip_invalid());
        }
        let remaining = bits.input.len().saturating_mul(8).saturating_sub(bits.bit);
        let available = remaining.min(self.max_bits);
        if available == 0 {
            return Err(jet_http_gzip_invalid());
        }
        let (symbol, length) = self.entries[bits.peek(available)? as usize];
        let length = usize::from(length);
        if length == 0 || length > available {
            return Err(jet_http_gzip_invalid());
        }
        bits.bit += length;
        Ok(usize::from(symbol))
    }
}

fn jet_http_gzip_invalid() -> JetHTTPReadError {
    JetHTTPReadError { status: 400, message: "gzip request body is malformed" }
}

fn jet_http_gzip_too_large() -> JetHTTPReadError {
    JetHTTPReadError { status: 413, message: "decoded request body is too large" }
}

fn jet_http_deflate_tables(
    bits: &mut JetHTTPDeflateBits<'_>,
    kind: u32,
) -> Result<(JetHTTPDeflateTable, JetHTTPDeflateTable), JetHTTPReadError> {
    if kind == 1 {
        let mut literals = vec![0u8; 288];
        literals[..144].fill(8);
        literals[144..256].fill(9);
        literals[256..280].fill(7);
        literals[280..].fill(8);
        return Ok((
            JetHTTPDeflateTable::new(&literals, false)?,
            JetHTTPDeflateTable::new(&[5; 32], false)?,
        ));
    }
    if kind != 2 {
        return Err(jet_http_gzip_invalid());
    }
    let literal_count = bits.read(5)? as usize + 257;
    let distance_count = bits.read(5)? as usize + 1;
    let code_count = bits.read(4)? as usize + 4;
    if literal_count > 286 || distance_count > 32 {
        return Err(jet_http_gzip_invalid());
    }
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut code_lengths = [0u8; 19];
    for index in 0..code_count {
        code_lengths[ORDER[index]] = bits.read(3)? as u8;
    }
    let code_table = JetHTTPDeflateTable::new(&code_lengths, true)?;
    let total = literal_count + distance_count;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        match code_table.symbol(bits)? {
            length @ 0..=15 => lengths.push(length as u8),
            16 => {
                let previous = *lengths.last().ok_or_else(jet_http_gzip_invalid)?;
                let repeats = bits.read(2)? as usize + 3;
                if lengths.len().saturating_add(repeats) > total {
                    return Err(jet_http_gzip_invalid());
                }
                lengths.extend(std::iter::repeat_n(previous, repeats));
            }
            17 => {
                let repeats = bits.read(3)? as usize + 3;
                if lengths.len().saturating_add(repeats) > total {
                    return Err(jet_http_gzip_invalid());
                }
                lengths.extend(std::iter::repeat_n(0, repeats));
            }
            18 => {
                let repeats = bits.read(7)? as usize + 11;
                if lengths.len().saturating_add(repeats) > total {
                    return Err(jet_http_gzip_invalid());
                }
                lengths.extend(std::iter::repeat_n(0, repeats));
            }
            _ => return Err(jet_http_gzip_invalid()),
        }
    }
    if lengths.get(256).copied().unwrap_or(0) == 0 {
        return Err(jet_http_gzip_invalid());
    }
    Ok((
        JetHTTPDeflateTable::new(&lengths[..literal_count], false)?,
        JetHTTPDeflateTable::new(&lengths[literal_count..], false)?,
    ))
}

fn jet_http_deflate_decode(
    input: &[u8],
    output: &mut Vec<u8>,
    member_start: usize,
    limit: usize,
) -> Result<usize, JetHTTPReadError> {
    const LENGTH_BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59,
        67, 83, 99, 115, 131, 163, 195, 227, 258,
    ];
    const LENGTH_EXTRA: [usize; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4,
        4, 4, 5, 5, 5, 5, 0,
    ];
    const DISTANCE_BASE: [usize; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385,
        513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DISTANCE_EXTRA: [usize; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9,
        10, 10, 11, 11, 12, 12, 13, 13,
    ];
    let mut bits = JetHTTPDeflateBits { input, bit: 0 };
    loop {
        let final_block = bits.read(1)? != 0;
        let kind = bits.read(2)?;
        if kind == 0 {
            bits.align();
            let length = bits.read(16)? as usize;
            let complement = bits.read(16)? as u16;
            if (length as u16) != !complement {
                return Err(jet_http_gzip_invalid());
            }
            if output.len().saturating_add(length) > limit {
                return Err(jet_http_gzip_too_large());
            }
            for _ in 0..length {
                output.push(bits.read(8)? as u8);
            }
        } else {
            let (literals, distances) = jet_http_deflate_tables(&mut bits, kind)?;
            loop {
                let symbol = literals.symbol(&mut bits)?;
                if symbol < 256 {
                    if output.len() == limit {
                        return Err(jet_http_gzip_too_large());
                    }
                    output.push(symbol as u8);
                } else if symbol == 256 {
                    break;
                } else {
                    let length_index = symbol.checked_sub(257).filter(|index| *index < 29)
                        .ok_or_else(jet_http_gzip_invalid)?;
                    let length = LENGTH_BASE[length_index]
                        + bits.read(LENGTH_EXTRA[length_index])? as usize;
                    let distance_symbol = distances.symbol(&mut bits)?;
                    if distance_symbol >= 30 {
                        return Err(jet_http_gzip_invalid());
                    }
                    let distance = DISTANCE_BASE[distance_symbol]
                        + bits.read(DISTANCE_EXTRA[distance_symbol])? as usize;
                    if distance == 0 || distance > output.len().saturating_sub(member_start) {
                        return Err(jet_http_gzip_invalid());
                    }
                    if output.len().saturating_add(length) > limit {
                        return Err(jet_http_gzip_too_large());
                    }
                    for _ in 0..length {
                        output.push(output[output.len() - distance]);
                    }
                }
            }
        }
        if final_block {
            bits.align();
            return Ok(bits.bit / 8);
        }
    }
}

fn jet_http_crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn jet_http_gzip_decode(input: &[u8], limit: usize) -> Result<Vec<u8>, JetHTTPReadError> {
    let mut output = Vec::new();
    let mut cursor = 0usize;
    while cursor < input.len() {
        let member = cursor;
        let header = input.get(cursor..cursor + 10).ok_or_else(jet_http_gzip_invalid)?;
        if header[..3] != [31, 139, 8] || header[3] & 0xe0 != 0 {
            return Err(jet_http_gzip_invalid());
        }
        let flags = header[3];
        cursor += 10;
        if flags & 4 != 0 {
            let length = input.get(cursor..cursor + 2).ok_or_else(jet_http_gzip_invalid)?;
            cursor += 2;
            let length = usize::from(u16::from_le_bytes([length[0], length[1]]));
            cursor = cursor.checked_add(length).filter(|end| *end <= input.len())
                .ok_or_else(jet_http_gzip_invalid)?;
        }
        for flag in [8u8, 16u8] {
            if flags & flag != 0 {
                let end = input[cursor..].iter().position(|byte| *byte == 0)
                    .ok_or_else(jet_http_gzip_invalid)?;
                cursor += end + 1;
            }
        }
        if flags & 2 != 0 {
            let expected = input.get(cursor..cursor + 2).ok_or_else(jet_http_gzip_invalid)?;
            let expected = u16::from_le_bytes([expected[0], expected[1]]);
            if jet_http_crc32(&input[member..cursor]) as u16 != expected {
                return Err(jet_http_gzip_invalid());
            }
            cursor += 2;
        }
        let member_output = output.len();
        let compressed = jet_http_deflate_decode(&input[cursor..], &mut output, member_output, limit)?;
        cursor += compressed;
        let trailer = input.get(cursor..cursor + 8).ok_or_else(jet_http_gzip_invalid)?;
        let crc = u32::from_le_bytes(trailer[..4].try_into().map_err(|_| jet_http_gzip_invalid())?);
        let size = u32::from_le_bytes(trailer[4..].try_into().map_err(|_| jet_http_gzip_invalid())?);
        if jet_http_crc32(&output[member_output..]) != crc
            || output.len().saturating_sub(member_output) as u32 != size
        {
            return Err(jet_http_gzip_invalid());
        }
        cursor += 8;
    }
    if cursor == 0 {
        return Err(jet_http_gzip_invalid());
    }
    Ok(output)
}

fn jet_http_decode_request_bytes(
    mut bytes: Vec<u8>,
    layers: usize,
    limit: usize,
) -> Result<Vec<u8>, JetHTTPReadError> {
    if layers == 0 {
        return Ok(bytes);
    }
    if bytes.len() > limit {
        return Err(jet_http_gzip_too_large());
    }
    for _ in 0..layers {
        bytes = jet_http_gzip_decode(&bytes, limit)?;
    }
    Ok(bytes)
}

fn jet_http_decode_request_body(
    body: JetHTTPBody,
    layers: usize,
    limit: usize,
) -> Result<JetHTTPBody, JetHTTPReadError> {
    if layers == 0 {
        return Ok(body);
    }
    let bytes = body.bytes(limit).map_err(|error| match error {
        JetHTTPError::BodyTooLarge { .. } => jet_http_gzip_too_large(),
        _ => JetHTTPReadError { status: 400, message: "encoded request body could not be read" },
    })?;
    Ok(JetHTTPBody::from_bytes(jet_http_decode_request_bytes(bytes, layers, limit)?))
}

#[derive(Clone, Copy)]
enum JetHTTPChunkPhase {
    Size,
    Data(usize),
    DataCrlf,
    Trailers,
}

struct JetHTTPChunkState {
    cursor: usize,
    decoded_len: usize,
    framing_len: usize,
    chunks: Vec<(usize, usize)>,
    phase: JetHTTPChunkPhase,
    limit: usize,
    trailer_names: Vec<String>,
    trailers: JetHTTPHeaders,
}

impl JetHTTPChunkState {
    fn new(limit: usize, trailer_names: Vec<String>) -> Self {
        Self {
            cursor: 0,
            decoded_len: 0,
            framing_len: 0,
            chunks: Vec::new(),
            phase: JetHTTPChunkPhase::Size,
            limit,
            trailer_names,
            trailers: JetHTTPHeaders::new(),
        }
    }

    fn add_framing(&mut self, amount: usize) -> Result<(), JetHTTPReadError> {
        self.framing_len = self.framing_len.saturating_add(amount);
        if self.framing_len > JET_HTTP_MAX_CHUNK_FRAMING_BYTES {
            return Err(JetHTTPReadError {
                status: 413,
                message: "chunked request framing is too large",
            });
        }
        Ok(())
    }

    fn advance(&mut self, body: &[u8]) -> Result<Option<usize>, JetHTTPReadError> {
        loop {
            match self.phase {
                JetHTTPChunkPhase::Size => {
                    let Some(line_len) = body[self.cursor..]
                        .windows(2)
                        .position(|bytes| bytes == b"\r\n")
                    else {
                        if body.len().saturating_sub(self.cursor)
                            > JET_HTTP_MAX_CHUNK_FRAMING_BYTES.saturating_sub(self.framing_len)
                        {
                            return Err(JetHTTPReadError {
                                status: 413,
                                message: "chunked request framing is too large",
                            });
                        }
                        return Ok(None);
                    };
                    let line_end = self.cursor + line_len;
                    let size = jet_http_chunk_size(&body[self.cursor..line_end])?;
                    self.add_framing(line_len + 2)?;
                    self.cursor = line_end + 2;
                    if size == 0 {
                        self.phase = JetHTTPChunkPhase::Trailers;
                    } else {
                        self.decoded_len = self.decoded_len.checked_add(size).ok_or(JetHTTPReadError {
                            status: 413,
                            message: "request body is too large",
                        })?;
                        if self.decoded_len > self.limit {
                            return Err(JetHTTPReadError {
                                status: 413,
                                message: "request body is too large",
                            });
                        }
                        self.chunks.push((self.cursor, size));
                        self.phase = JetHTTPChunkPhase::Data(size);
                    }
                }
                JetHTTPChunkPhase::Data(remaining) => {
                    let available = body.len().saturating_sub(self.cursor).min(remaining);
                    self.cursor += available;
                    if available < remaining {
                        self.phase = JetHTTPChunkPhase::Data(remaining - available);
                        return Ok(None);
                    }
                    self.phase = JetHTTPChunkPhase::DataCrlf;
                }
                JetHTTPChunkPhase::DataCrlf => {
                    if body.len().saturating_sub(self.cursor) < 2 {
                        return Ok(None);
                    }
                    if &body[self.cursor..self.cursor + 2] != b"\r\n" {
                        return Err(JetHTTPReadError {
                            status: 400,
                            message: "chunk data is not followed by CRLF",
                        });
                    }
                    self.cursor += 2;
                    self.add_framing(2)?;
                    self.phase = JetHTTPChunkPhase::Size;
                }
                JetHTTPChunkPhase::Trailers => {
                    let Some(line_len) = body[self.cursor..]
                        .windows(2)
                        .position(|bytes| bytes == b"\r\n")
                    else {
                        if body.len().saturating_sub(self.cursor)
                            > JET_HTTP_MAX_CHUNK_FRAMING_BYTES.saturating_sub(self.framing_len)
                        {
                            return Err(JetHTTPReadError {
                                status: 413,
                                message: "chunked request framing is too large",
                            });
                        }
                        return Ok(None);
                    };
                    let line_end = self.cursor + line_len;
                    self.add_framing(line_len + 2)?;
                    if line_len == 0 {
                        self.cursor += 2;
                        return Ok(Some(self.cursor));
                    }
                    jet_http_parse_trailer_line(
                        &body[self.cursor..line_end],
                        &self.trailer_names,
                        &mut self.trailers,
                    )?;
                    self.cursor = line_end + 2;
                }
            }
        }
    }
}

#[derive(Clone)]
struct JetHTTPServerOptions {
    workers: usize,
    admission_queue: usize,
    read_header_timeout: std::time::Duration,
    read_idle_timeout: std::time::Duration,
    read_body_timeout: std::time::Duration,
    write_idle_timeout: std::time::Duration,
    shutdown_grace: std::time::Duration,
    max_body_bytes: usize,
    max_connections: usize,
    max_connections_per_ip: usize,
}

impl JetHTTPServerOptions {
    fn safe() -> Self {
        Self {
            workers: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            admission_queue: 256,
            read_header_timeout: std::time::Duration::from_secs(5),
            read_idle_timeout: std::time::Duration::from_secs(30),
            read_body_timeout: std::time::Duration::from_secs(30),
            write_idle_timeout: std::time::Duration::from_secs(30),
            shutdown_grace: std::time::Duration::from_secs(30),
            max_body_bytes: JET_HTTP_MAX_BODY_BYTES,
            max_connections: 10_000,
            max_connections_per_ip: 256,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct JetHTTPShutdownReport {
    user_accepted: i64,
    user_overloaded: i64,
    user_completed: i64,
    user_cancelled: i64,
}

fn jet_http_shutdown_report_field(
    report: &JetHTTPShutdownReport,
    field: i64,
) -> i64 {
    match field {
        0 => report.user_accepted,
        1 => report.user_overloaded,
        2 => report.user_completed,
        3 => report.user_cancelled,
        _ => 0,
    }
}

#[derive(Clone)]
struct JetHTTPServer {
    inner: std::sync::Arc<JetHTTPServerState>,
}

type JetHTTPTlsReader = Box<dyn std::io::Read + Send>;
type JetHTTPTlsWriter = Box<dyn std::io::Write + Send>;
type JetHTTPTlsTimeout = Box<
    dyn Fn(Option<std::time::Duration>) -> Result<(), String> + Send,
>;
type JetHTTPTlsH2 = Box<
    dyn FnOnce(
            JetHTTPTlsReader,
            JetHTTPTlsWriter,
            JetHTTPTlsTimeout,
            JetHTTPTlsTimeout,
        ) -> Result<(), String>
        + Send,
>;

type JetHTTPTlsConn = std::sync::Arc<
    dyn Fn(
            std::net::TcpStream,
            std::sync::Arc<std::sync::atomic::AtomicBool>,
            JetHTTPServerOptions,
            Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
            std::sync::Arc<std::sync::atomic::AtomicU64>,
        ) -> Result<(), String>
        + Send
        + Sync,
>;

struct JetHTTPServerState {
    listener: std::sync::Mutex<Option<std::net::TcpListener>>,
    mux: JetHTTPMux,
    local_addr: String,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    shutdown_called: std::sync::atomic::AtomicBool,
    grace_ms: std::sync::Arc<std::sync::atomic::AtomicU64>,
    drain_deadline_ms: std::sync::Arc<std::sync::atomic::AtomicU64>,
    lifecycle: std::sync::atomic::AtomicU8,
    report: std::sync::Mutex<Option<JetHTTPShutdownReport>>,
    report_ready: std::sync::Condvar,
    tls_conn: Option<JetHTTPTlsConn>,
}

impl std::fmt::Display for JetHTTPReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl JetHTTPMux {
    fn new() -> Self {
        JetHTTPMux(
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            std::sync::Arc::new(std::sync::Mutex::new(None)),
        )
    }

    fn route_cache(&self) -> std::sync::Arc<JetHTTPMuxRouteCache> {
        let mut cached = self.2.lock().unwrap();
        if let Some(cache) = cached.as_ref().cloned() {
            return cache;
        }
        let routes = self.0.lock().unwrap();
        let mut seen = std::collections::BTreeSet::new();
        let mut validation_error = None;
        let mut cached_routes = Vec::with_capacity(routes.len());
        for route in routes.iter() {
            match jet_http_route_parse(&route.pattern) {
                Ok(pattern) => {
                    if validation_error.is_none() {
                        let key = (route.method.clone(), jet_http_route_shape(&pattern));
                        if !seen.insert(key) {
                            validation_error = Some(format!(
                                "E2804: HTTP route conflict for {} `{}`",
                                route.method, route.pattern
                            ));
                        }
                    }
                    cached_routes.push(JetHTTPMuxRouteCacheEntry {
                        route: route.clone(),
                        pattern: Some(pattern),
                    });
                }
                Err(error) => {
                    if validation_error.is_none() {
                        validation_error = Some(error);
                    }
                    cached_routes.push(JetHTTPMuxRouteCacheEntry {
                        route: route.clone(),
                        pattern: None,
                    });
                }
            }
        }
        let cache = std::sync::Arc::new(JetHTTPMuxRouteCache {
            routes: cached_routes,
            validation_error,
        });
        *cached = Some(cache.clone());
        cache
    }

    fn add<F>(&self, method: &str, pattern: &str, f: F)
    where
        F: Fn(JetHTTPRequest) -> Result<JetHTTPResponse, JetHTTPError> + Send + Sync + 'static,
    {
        let mut cached = self.2.lock().unwrap();
        let mut routes = self.0.lock().unwrap();
        routes.push(JetHTTPMuxRoute {
            method: method.to_string(),
            pattern: pattern.to_string(),
            handler: std::sync::Arc::new(f) as JetHTTPHandler,
            contract_json: String::new(),
        });
        *cached = None;
    }

    fn add_handler(&self, method: &str, pattern: &str, handler: JetHTTPHandler) {
        self.add_handler_with_contract(method, pattern, handler, String::new());
    }

    fn add_handler_with_contract(
        &self,
        method: &str,
        pattern: &str,
        handler: JetHTTPHandler,
        contract_json: String,
    ) {
        let mut cached = self.2.lock().unwrap();
        let mut routes = self.0.lock().unwrap();
        routes.push(JetHTTPMuxRoute {
            method: method.to_string(),
            pattern: pattern.to_string(),
            handler,
            contract_json,
        });
        *cached = None;
    }

}

fn jet_http_mux_middleware(mux: &JetHTTPMux, middleware: JetHTTPMiddleware) {
    mux.1.lock().unwrap().push(middleware);
}

fn jet_http_mux_new() -> JetHTTPMux {
    JetHTTPMux::new()
}

fn jet_http_srv_tls(cert_pem: &String, key_pem: &String) -> JetHTTPServerTls {
    JetHTTPServerTls {
        cert_pem: cert_pem.clone(),
        key_pem: key_pem.clone(),
    }
}

fn jet_http_mux_add<F>(mux: &JetHTTPMux, method: &str, pattern: &str, f: F)
where
    F: Fn(JetHTTPRequest) -> JetHTTPResponse + Send + Sync + 'static,
{
    mux.add(method, pattern, move |request| Ok(f(request)));
}

fn jet_http_mux_add_handler(mux: &JetHTTPMux, method: &str, pattern: &str, handler: JetHTTPHandler) {
    mux.add_handler(method, pattern, handler);
}

fn jet_http_mux_add_zero_handler(
    mux: &JetHTTPMux,
    method: &str,
    pattern: &str,
    handler: std::sync::Arc<
        dyn Fn() -> Result<JetHTTPResponse, JetHTTPError> + Send + Sync + 'static,
    >,
) {
    mux.add_handler(method, pattern, std::sync::Arc::new(move |_| handler()));
}
fn jet_http_route_query_param(req: &JetHTTPRequest, name: &str) -> Result<Option<String>, ()> {
    let Some(query) = req.path.split_once('?').map(|(_, query)| query) else {
        return Ok(None);
    };
    for pair in query.split('&') {
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = raw_key.replace('+', " ");
        let key = jet_http_route_decode_path_segment(&key).map_err(|_| ())?.into_owned();
        if key != name {
            continue;
        }
        let value = raw_value.replace('+', " ");
        return Ok(Some(
            jet_http_route_decode_path_segment(&value)
                .map_err(|_| ())?
                .into_owned(),
        ));
    }
    Ok(None)
}
fn jet_http_route_raw(req: &JetHTTPRequest, name: &str) -> Result<Option<String>, ()> {
    if let Some(value) = req.params.get(name) {
        return Ok(Some(value.clone()));
    }
    jet_http_route_query_param(req, name)
}
fn jet_http_route_param<T: __jet_Decode>(
    req: &JetHTTPRequest,
    name: &str,
) -> Result<Option<T>, ()> {
    let Some(raw) = jet_http_route_raw(req, name)? else {
        return Ok(None);
    };
    let text = jet_std::DataTree::Text(raw.clone());
    if let Ok(value) = T::jet_decode(&text) {
        return Ok(Some(value));
    }
    let tree = jet_std::parse_json_typed_datatree(&raw).map_err(|_| ())?;
    T::jet_decode(&tree).map(Some).map_err(|_| ())
}

fn jet_http_route_optional<T: __jet_Decode>(
    req: &JetHTTPRequest,
    name: &str,
) -> Result<T, ()> {
    let Some(raw) = jet_http_route_raw(req, name)? else {
        return T::jet_decode(&jet_std::DataTree::Null).map_err(|_| ());
    };
    let text = jet_std::DataTree::Text(raw.clone());
    if let Ok(value) = T::jet_decode(&text) {
        return Ok(value);
    }
    let tree = jet_std::parse_json_typed_datatree(&raw).map_err(|_| ())?;
    T::jet_decode(&tree).map_err(|_| ())
}

fn jet_http_contract_object(value: &jet_std::DataTree) -> Option<&[(String, jet_std::DataTree)]> {
    match value {
        jet_std::DataTree::Object(entries) => Some(entries.as_slice()),
        _ => None,
    }
}

fn jet_http_contract_field<'a>(
    object: &'a [(String, jet_std::DataTree)],
    name: &str,
) -> Option<&'a jet_std::DataTree> {
    object
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
}

fn jet_http_contract_text<'a>(
    object: &'a [(String, jet_std::DataTree)],
    name: &str,
) -> Option<&'a str> {
    match jet_http_contract_field(object, name)? {
        jet_std::DataTree::Text(value) | jet_std::DataTree::TypedText(value) => Some(value),
        _ => None,
    }
}

pub(crate) fn jet_http_route_schema_matches(
    value: &jet_std::DataTree,
    schema: &jet_std::DataTree,
) -> bool {
    let Some(schema_object) = jet_http_contract_object(schema) else {
        return false;
    };
    if let Some(any_of) = jet_http_contract_field(schema_object, "anyOf") {
        let jet_std::DataTree::Array(items) = any_of else {
            return false;
        };
        return items
            .iter()
            .any(|candidate| jet_http_route_schema_matches(value, candidate));
    }
    if let Some(one_of) = jet_http_contract_field(schema_object, "oneOf") {
        let jet_std::DataTree::Array(items) = one_of else {
            return false;
        };
        return items
            .iter()
            .filter(|candidate| jet_http_route_schema_matches(value, candidate))
            .count()
            == 1;
    }
    let Some(kind) = jet_http_contract_text(schema_object, "type") else {
        return true;
    };
    match kind {
        "null" => matches!(value, jet_std::DataTree::Null),
        "boolean" => matches!(value, jet_std::DataTree::Bool(_)),
        "integer" => match value {
            jet_std::DataTree::Int(_) => true,
            jet_std::DataTree::Number(number) => number.parse::<i64>().is_ok(),
            _ => false,
        },
        "number" => matches!(
            value,
            jet_std::DataTree::Int(_)
                | jet_std::DataTree::Float(_)
                | jet_std::DataTree::Number(_)
        ),
        "string" => matches!(
            value,
            jet_std::DataTree::Text(_) | jet_std::DataTree::TypedText(_)
        ),
        "array" => {
            let jet_std::DataTree::Array(items) = value else {
                return false;
            };
            let Some(item_schema) = jet_http_contract_field(schema_object, "items") else {
                return false;
            };
            items
                .iter()
                .all(|item| jet_http_route_schema_matches(item, item_schema))
        }
        "object" => {
            let jet_std::DataTree::Object(fields) = value else {
                return false;
            };
            let Some(properties) = jet_http_contract_field(schema_object, "properties")
                .and_then(jet_http_contract_object)
            else {
                return false;
            };
            let additional_properties = match jet_http_contract_field(
                schema_object,
                "additionalProperties",
            ) {
                Some(jet_std::DataTree::Bool(value)) => *value,
                None => true,
                _ => false,
            };
            let required = match jet_http_contract_field(schema_object, "required") {
                None => Vec::new(),
                Some(jet_std::DataTree::Array(names)) => names
                    .iter()
                    .filter_map(|name| match name {
                        jet_std::DataTree::Text(name)
                        | jet_std::DataTree::TypedText(name) => Some(name.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                Some(_) => return false,
            };
            if required
                .iter()
                .any(|name| !fields.iter().any(|(field, _)| field == *name))
            {
                return false;
            }
            fields.iter().all(|(name, field_value)| {
                let Some((_, field_schema)) =
                    properties.iter().find(|(property, _)| property == name)
                else {
                    return additional_properties;
                };
                jet_http_route_schema_matches(field_value, field_schema)
            })
        }
        _ => false,
    }
}

fn jet_http_route_validate_body(
    mut request: JetHTTPRequest,
    contract_json: &str,
) -> Result<JetHTTPRequest, ()> {
    let contract = jet_std::parse_json_strict(contract_json).map_err(|_| ())?;
    let contract_object = jet_http_contract_object(&contract).ok_or(())?;
    let request_body = jet_http_contract_field(contract_object, "request_body").ok_or(())?;
    let request_body_object = match request_body {
        jet_std::DataTree::Null => return Ok(request),
        value => jet_http_contract_object(value).ok_or(())?,
    };
    let required = match jet_http_contract_field(request_body_object, "required") {
        Some(jet_std::DataTree::Bool(value)) => *value,
        _ => return Err(()),
    };
    let schema = jet_http_contract_field(request_body_object, "schema").ok_or(())?;
    let content_type = request.body.content_type();
    let bytes = request
        .body
        .bytes(jet_http_default_body_limit() as usize)
        .map_err(|_| ())?;
    if bytes.is_empty() && !required {
        request.body = JetHTTPBody::from_bytes_with_content_type(bytes, content_type);
        return Ok(request);
    }
    let text = String::from_utf8(bytes.clone()).map_err(|_| ())?;
    let value = jet_std::parse_json_typed_datatree(&text).map_err(|_| ())?;
    if !jet_http_route_schema_matches(&value, schema) {
        return Err(());
    }
    request.body = JetHTTPBody::from_bytes_with_content_type(bytes, content_type);
    Ok(request)
}

fn jet_http_require_route_contract(
    method: &str,
    pattern: &str,
    contract_json: &str,
    file: &str,
    line: u32,
    allow_empty_responses: bool,
) {
    if contract_json.trim().is_empty() {
        jet_panic(
            file,
            line,
            &format!(
                "E2805: route `{} {}` has no checked endpoint contract",
                method, pattern
            ),
        );
    }
    let contract = match jet_std::parse_json_strict(contract_json) {
        Ok(contract) => contract,
        Err(_) => jet_panic(
            file,
            line,
            &format!(
                "E2805: route `{} {}` has an invalid checked endpoint contract",
                method, pattern
            ),
        ),
    };
    let Some(contract_object) = jet_http_contract_object(&contract) else {
        jet_panic(
            file,
            line,
            &format!(
                "E2805: route `{} {}` has a non-object endpoint contract",
                method, pattern
            ),
        );
    };
    let required_fields = [
        "method",
        "pattern",
        "path",
        "operation_id",
        "summary",
        "parameters",
        "request_body",
        "responses",
        "security",
        "provenance",
    ];
    if required_fields
        .iter()
        .any(|field| jet_http_contract_field(contract_object, field).is_none())
    {
        jet_panic(
            file,
            line,
            &format!(
                "E2805: route `{} {}` has an incomplete endpoint contract",
                method, pattern
            ),
        );
    }
    if jet_http_contract_text(contract_object, "method") != Some(method)
        || jet_http_contract_text(contract_object, "pattern") != Some(pattern)
    {
        jet_panic(
            file,
            line,
            &format!(
                "E2805: route `{} {}` does not match its endpoint contract",
                method, pattern
            ),
        );
    }
    let Some(jet_std::DataTree::Array(responses)) =
        jet_http_contract_field(contract_object, "responses")
    else {
        jet_panic(
            file,
            line,
            &format!(
                "E2805: route `{} {}` has invalid endpoint responses",
                method, pattern
            ),
        );
    };
    if responses.is_empty() && !allow_empty_responses {
        jet_panic(
            file,
            line,
            &format!(
                "E2805: route `{} {}` has no endpoint responses",
                method, pattern
            ),
        );
    }
    let Some(jet_std::DataTree::Array(_)) =
        jet_http_contract_field(contract_object, "parameters")
    else {
        jet_panic(
            file,
            line,
            &format!(
                "E2805: route `{} {}` has invalid endpoint parameters",
                method, pattern
            ),
        );
    };
    let Some(jet_std::DataTree::Array(_)) =
        jet_http_contract_field(contract_object, "security")
    else {
        jet_panic(
            file,
            line,
            &format!(
                "E2805: route `{} {}` has invalid endpoint security policy",
                method, pattern
            ),
        );
    };
    match jet_http_contract_field(contract_object, "request_body") {
        Some(jet_std::DataTree::Null) => {}
        Some(jet_std::DataTree::Object(body)) => {
            let valid_required = matches!(
                jet_http_contract_field(body, "required"),
                Some(jet_std::DataTree::Bool(_))
            );
            let valid_content_type = jet_http_contract_text(body, "content_type")
                .is_some_and(|value| !value.is_empty());
            let valid_schema = jet_http_contract_field(body, "schema")
                .is_some_and(|schema| jet_http_contract_object(schema).is_some());
            if !(valid_required && valid_content_type && valid_schema) {
                jet_panic(
                    file,
                    line,
                    &format!(
                        "E2805: route `{} {}` has an invalid endpoint request body",
                        method, pattern
                    ),
                );
            }
        }
        Some(_) | None => jet_panic(
            file,
            line,
            &format!(
                "E2805: route `{} {}` has an invalid endpoint request body",
                method, pattern
            ),
        ),
    }
}

fn jet_http_mux_register(
    mux: &JetHTTPMux,
    method: String,
    pattern: String,
    handler: JetHTTPHandler,
    file: &str,
    line: u32,
    contract_json: String,
) {
    jet_http_require_route_contract(&method, &pattern, &contract_json, file, line, true);
    let segments = match jet_http_router_parse_pattern(&pattern) {
        Ok(segments) => segments,
        Err(message) => jet_panic(file, line, &message),
    };
    let shape = jet_http_route_shape(&JetHTTPRoutePattern {
        segments: segments.clone(),
    });
    let duplicate = mux
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|route| route.method == method)
        .filter_map(|route| {
            let existing = jet_http_router_parse_pattern(&route.pattern).ok()?;
            Some(jet_http_route_shape(&JetHTTPRoutePattern { segments: existing }))
        })
        .any(|existing| existing == shape);
    if duplicate {
        jet_panic(
            file,
            line,
            &format!("E2804: duplicate route `{} {}`", method, pattern),
        );
    }
    mux.add_handler_with_contract(&method, &pattern, handler, contract_json);
}

trait JetHTTPStatusCode {
    fn jet_http_status_code(self) -> i64;
}

impl JetHTTPStatusCode for i64 {
    fn jet_http_status_code(self) -> i64 {
        self
    }
}

impl JetHTTPStatusCode for jet_foundation::Numeric::JetInt {
    fn jet_http_status_code(self) -> i64 {
        self.to_i64().unwrap_or(500)
    }
}

fn jet_http_srv_response<S: JetHTTPStatusCode>(status: S, body: &String) -> JetHTTPResponse {
    jet_http_srv_response_owned(status.jet_http_status_code(), body.clone())
}
fn jet_http_srv_response_owned(status: i64, body: String) -> JetHTTPResponse {
    let (status, body) = if !(100..=599).contains(&status) {
        (500, "500 Internal Server Error".to_string())
    } else {
        (status, body)
    };
    JetHTTPResponse {
        status,
        version: "HTTP/1.1".to_string(),
        body: JetHTTPBody::from_text(body),
        headers: JetHTTPHeaders::new(),
        trailers: JetHTTPHeaders::new(),
        head_content_length: None,
        suppress_body: false,
        protocol: "HTTP/1.1".to_string(),
        remote_address: String::new(),
        redirect_history: Vec::new(),
        timings_ms: Vec::new(),
        reused_connection: false,
        raw_content_encoding: None,
    }
}

const JET_ERROR_PAGE_MAX_ID_BYTES: usize = 128;
const JET_ERROR_PAGE_MAX_MESSAGE_BYTES: usize = 2048;
const JET_ERROR_PAGE_MAX_SOURCE_BYTES: usize = 512;
const JET_ERROR_PAGE_MAX_CONTEXT: usize = 16;
const JET_ERROR_PAGE_MAX_CONTEXT_BYTES: usize = 4096;
const JET_ERROR_PAGE_MAX_LINKS: usize = 8;
const JET_ERROR_PAGE_MAX_LINK_BYTES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetErrorPageSourceFrame {
    fn_name: String,
    file: String,
    line: u32,
    note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetErrorPageContext {
    text: String,
    file: String,
    line: u32,
}

/// One normalized `jet.err/v1` report projected for a service boundary. The
/// projection owns both view-independent facts and the policy that decides
/// whether local source context is allowed to leave the process.
#[derive(Clone, Debug, PartialEq, Eq)]
struct JetErrorPageProjection {
    status: i64,
    failure_id: String,
    request_id: String,
    message: String,
    code: Option<String>,
    source_frame: Option<JetErrorPageSourceFrame>,
    context: Vec<JetErrorPageContext>,
    correlation_links: Vec<String>,
}

fn jet_error_page_status(status: i64) -> i64 {
    if (100..=599).contains(&status) { status } else { 500 }
}

fn jet_error_page_bounded(value: &str, limit: usize) -> String {
    let mut output = String::new();
    for character in value.chars() {
        let character = if character.is_control() || character == '\u{7f}' {
            ' '
        } else {
            character
        };
        if output.len().saturating_add(character.len_utf8()) > limit {
            break;
        }
        output.push(character);
    }
    output.trim().to_string()
}

fn jet_error_page_sensitive(value: &str) -> bool {
    const MARKERS: &[&str] = &[
        "authorization",
        "proxy-authorization",
        "set-cookie",
        "cookie",
        "password",
        "passwd",
        "secret",
        "token",
        "api_key",
        "api-key",
        "apikey",
        "private_key",
        "private-key",
        "credential",
        "session",
        "jwt",
        "bearer",
        "access_key",
        "access-key",
        "refresh_token",
        "refresh-token",
        "client_secret",
        "client-secret",
        "signature",
        "unpublished",
    ];
    let lower = value.to_ascii_lowercase();
    MARKERS.iter().any(|marker| lower.contains(marker))
}

fn jet_error_page_safe_text(value: &str, limit: usize) -> String {
    if jet_error_page_sensitive(value) {
        "[redacted]".to_string()
    } else {
        jet_error_page_bounded(value, limit)
    }
}

fn jet_error_page_identifier(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty()
        || value.len() > JET_ERROR_PAGE_MAX_ID_BYTES
        || jet_error_page_sensitive(value)
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_graphic() || matches!(byte, b'"' | b'\\'))
    {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn jet_error_page_failure_id(
    supplied: &str,
    status: i64,
    report: &JetErrorReport,
) -> String {
    let supplied = jet_error_page_identifier(supplied, "");
    if !supplied.is_empty() {
        return supplied;
    }
    let canonical = report.to_json();
    let mut hash = 0xcbf29ce484222325u64;
    for byte in status
        .to_le_bytes()
        .into_iter()
        .chain(canonical.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fail-{hash:016x}")
}

fn jet_error_page_source(report: &JetErrorReport) -> Option<JetErrorPageSourceFrame> {
    report
        .source_journey
        .first()
        .map(|frame| JetErrorPageSourceFrame {
            fn_name: jet_error_page_safe_text(&frame.fn_name, JET_ERROR_PAGE_MAX_SOURCE_BYTES),
            file: jet_error_page_bounded(&frame.file, JET_ERROR_PAGE_MAX_SOURCE_BYTES),
            line: frame.line,
            note: jet_error_page_safe_text(&frame.note, JET_ERROR_PAGE_MAX_MESSAGE_BYTES),
        })
        .or_else(|| {
            report
                .context_frames
                .first()
                .map(|frame| JetErrorPageSourceFrame {
                    fn_name: String::new(),
                    file: jet_error_page_bounded(&frame.file, JET_ERROR_PAGE_MAX_SOURCE_BYTES),
                    line: frame.line,
                    note: jet_error_page_safe_text(
                        &frame.text,
                        JET_ERROR_PAGE_MAX_MESSAGE_BYTES,
                    ),
                })
        })
}

fn jet_error_page_context(report: &JetErrorReport) -> Vec<JetErrorPageContext> {
    let mut context = Vec::new();
    let mut bytes = 0usize;
    for frame in report.context_frames.iter().take(JET_ERROR_PAGE_MAX_CONTEXT) {
        let text = jet_error_page_safe_text(&frame.text, JET_ERROR_PAGE_MAX_MESSAGE_BYTES);
        let file = jet_error_page_bounded(&frame.file, JET_ERROR_PAGE_MAX_SOURCE_BYTES);
        let item_bytes = text.len().saturating_add(file.len()).saturating_add(16);
        if bytes.saturating_add(item_bytes) > JET_ERROR_PAGE_MAX_CONTEXT_BYTES {
            break;
        }
        bytes = bytes.saturating_add(item_bytes);
        context.push(JetErrorPageContext {
            text,
            file,
            line: frame.line,
        });
    }
    context
}

fn jet_error_page_link(value: &str) -> Option<String> {
    if value.chars().any(char::is_control) || jet_error_page_sensitive(value) {
        return None;
    }
    let link = jet_error_page_bounded(value, JET_ERROR_PAGE_MAX_LINK_BYTES);
    let lower = link.to_ascii_lowercase();
    if link.is_empty()
        || lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("file:")
        || !(link.starts_with('/')
            || lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("jet://"))
    {
        None
    } else {
        Some(link)
    }
}

fn jet_error_page_json_quote(value: &str) -> String {
    let mut output = String::with_capacity(value.len().saturating_add(2));
    output.push('"');
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn jet_error_page_html_escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            character => output.push(character),
        }
    }
    output
}

fn jet_error_page_status_text(status: i64) -> &'static str {
    match status {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ if (400..=499).contains(&status) => "Client Error",
        _ if (500..=599).contains(&status) => "Server Error",
        _ => "HTTP Error",
    }
}

impl JetErrorPageProjection {
    fn json(&self) -> String {
        let code = self
            .code
            .as_deref()
            .map(jet_error_page_json_quote)
            .unwrap_or_else(|| "null".to_string());
        let source_frame = self
            .source_frame
            .as_ref()
            .map(|frame| {
                format!(
                    "{{\"fn_name\":{},\"file\":{},\"line\":{},\"note\":{}}}",
                    jet_error_page_json_quote(&frame.fn_name),
                    jet_error_page_json_quote(&frame.file),
                    frame.line,
                    jet_error_page_json_quote(&frame.note)
                )
            })
            .unwrap_or_else(|| "null".to_string());
        let context = self
            .context
            .iter()
            .map(|frame| {
                format!(
                    "{{\"text\":{},\"file\":{},\"line\":{}}}",
                    jet_error_page_json_quote(&frame.text),
                    jet_error_page_json_quote(&frame.file),
                    frame.line
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let links = self
            .correlation_links
            .iter()
            .map(|link| jet_error_page_json_quote(link))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema\":\"jet.err/v1\",\"status\":{},\"failure_id\":{},\
\"request_id\":{},\"message\":{},\"code\":{},\"source_frame\":{},\
\"context\":[{}],\"correlation_links\":[{}]}}",
            self.status,
            jet_error_page_json_quote(&self.failure_id),
            jet_error_page_json_quote(&self.request_id),
            jet_error_page_json_quote(&self.message),
            code,
            source_frame,
            context,
            links
        )
    }

    fn html(&self) -> String {
        let status = format!(
            "{} {}",
            self.status,
            jet_error_page_html_escape(jet_error_page_status_text(self.status))
        );
        let mut html = String::from(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>",
        );
        html.push_str(&status);
        html.push_str(
            "</title><style>body{background:#101418;color:#e8edf2;font:16px/1.5 system-ui,sans-serif;margin:0}\
main{box-sizing:border-box;margin:0 auto;max-width:860px;padding:48px 24px}\
h1{font-size:2rem;margin:0 0 24px}h2{font-size:1rem;margin:28px 0 8px}\
dl{display:grid;grid-template-columns:max-content 1fr;gap:6px 18px;margin:0}\
dt{color:#9aa8b5}dd{margin:0;overflow-wrap:anywhere}code{font-family:ui-monospace,monospace}\
pre{white-space:pre-wrap;overflow-wrap:anywhere;margin:0}a{color:#8bc8ff}\
.kind{color:#ffb454;text-transform:uppercase;letter-spacing:.08em;font-size:.75rem}</style>\
</head><body><main><div class=\"kind\">Jet service failure</div><h1>",
        );
        html.push_str(&status);
        html.push_str("</h1><dl><dt>Request ID</dt><dd><code>");
        html.push_str(&jet_error_page_html_escape(&self.request_id));
        html.push_str("</code></dd><dt>Failure ID</dt><dd><code>");
        html.push_str(&jet_error_page_html_escape(&self.failure_id));
        html.push_str("</code></dd></dl><h2>Message</h2><p>");
        html.push_str(&jet_error_page_html_escape(&self.message));
        html.push_str("</p>");
        if let Some(code) = &self.code {
            html.push_str("<p><strong>Code:</strong> <code>");
            html.push_str(&jet_error_page_html_escape(code));
            html.push_str("</code></p>");
        }
        if let Some(frame) = &self.source_frame {
            html.push_str("<h2>Source frame</h2><p><code>");
            if !frame.fn_name.is_empty() {
                html.push_str(&jet_error_page_html_escape(&frame.fn_name));
                html.push_str(" (");
            }
            html.push_str(&jet_error_page_html_escape(&frame.file));
            html.push(':');
            html.push_str(&frame.line.to_string());
            if !frame.fn_name.is_empty() {
                html.push(')');
            }
            html.push_str("</code>");
            if !frame.note.is_empty() {
                html.push_str(" — ");
                html.push_str(&jet_error_page_html_escape(&frame.note));
            }
            html.push_str("</p>");
        }
        if !self.context.is_empty() {
            html.push_str("<h2>Context</h2><ul>");
            for frame in &self.context {
                html.push_str("<li><code>");
                html.push_str(&jet_error_page_html_escape(&frame.file));
                html.push(':');
                html.push_str(&frame.line.to_string());
                html.push_str("</code> ");
                html.push_str(&jet_error_page_html_escape(&frame.text));
                html.push_str("</li>");
            }
            html.push_str("</ul>");
        }
        if !self.correlation_links.is_empty() {
            html.push_str("<h2>Correlation</h2><ul>");
            for link in &self.correlation_links {
                let escaped = jet_error_page_html_escape(link);
                html.push_str("<li><a href=\"");
                html.push_str(&escaped);
                html.push_str("\">");
                html.push_str(&escaped);
                html.push_str("</a></li>");
            }
            html.push_str("</ul>");
        }
        html.push_str("</main></body></html>");
        html
    }
}

/// Build the one service error projection. `local` is a server-side trust
/// decision; release builds and non-local callers always receive the bounded
/// generic view, even when a caller accidentally asks for local details.
fn jet_error_page_projection(
    status: i64,
    failure_id: &str,
    request_id: &str,
    report: &JetErrorReport,
    correlation_links: &[String],
    local: bool,
) -> JetErrorPageProjection {
    let status = jet_error_page_status(status);
    let failure_id = jet_error_page_failure_id(failure_id, status, report);
    let request_id = jet_error_page_identifier(request_id, "request-unknown");
    let local = local && cfg!(debug_assertions);
    let (message, code, source_frame, context, correlation_links) = if local {
        let message = {
            let message = jet_error_page_safe_text(
                &report.message,
                JET_ERROR_PAGE_MAX_MESSAGE_BYTES,
            );
            if message.is_empty() {
                jet_error_page_status_text(status).to_string()
            } else {
                message
            }
        };
        let code = report
            .code
            .as_deref()
            .map(|code| jet_error_page_identifier(code, "ERR"));
        let links = correlation_links
            .iter()
            .take(JET_ERROR_PAGE_MAX_LINKS)
            .filter_map(|link| jet_error_page_link(link))
            .collect();
        (
            message,
            code,
            jet_error_page_source(report),
            jet_error_page_context(report),
            links,
        )
    } else {
        (
            jet_error_page_status_text(status).to_string(),
            None,
            None,
            Vec::new(),
            Vec::new(),
        )
    };
    JetErrorPageProjection {
        status,
        failure_id,
        request_id,
        message,
        code,
        source_frame,
        context,
        correlation_links,
    }
}

fn jet_http_accept_quality(value: &str) -> u16 {
    let value = value.trim();
    if value == "1" {
        return 1000;
    }
    if let Some(fraction) = value.strip_prefix("1.") {
        return if !fraction.is_empty() && fraction.bytes().all(|byte| byte == b'0') {
            1000
        } else {
            0
        };
    }
    let Some(fraction) = value.strip_prefix("0.") else {
        return 0;
    };
    if fraction.is_empty() || fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return 0;
    }
    let mut quality = 0u16;
    for byte in fraction.bytes() {
        quality = quality.saturating_mul(10).saturating_add(u16::from(byte - b'0'));
    }
    quality * match fraction.len() {
        1 => 100,
        2 => 10,
        _ => 1,
    }
}

/// Select one representation from the request's Accept list. Explicit media
/// types outrank wildcards; equal-quality explicit types keep client order.
fn jet_http_error_page_accepts_json(accept: Option<&String>) -> bool {
    let Some(accept) = accept else {
        return false;
    };
    let mut found = false;
    let mut best_quality = 0u16;
    let mut best_specificity = 0u8;
    let mut best_json = false;
    for item in accept.split(',') {
        let mut parameters = item.split(';');
        let media = parameters.next().unwrap_or("").trim().to_ascii_lowercase();
        let mut quality = 1000u16;
        for parameter in parameters {
            let Some((name, value)) = parameter.trim().split_once('=') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("q") {
                quality = jet_http_accept_quality(value);
            }
        }
        if quality == 0 {
            continue;
        }
        let (supported, specificity, is_json) = match media.as_str() {
            "application/json" => (true, 2, true),
            "text/html" => (true, 2, false),
            "application/*" => (true, 1, true),
            "text/*" => (true, 1, false),
            "*/*" => (true, 0, false),
            _ => (false, 0, false),
        };
        if !supported {
            continue;
        }
        if !found || quality > best_quality || (quality == best_quality && specificity > best_specificity) {
            found = true;
            best_quality = quality;
            best_specificity = specificity;
            best_json = is_json;
        }
    }
    found && best_json
}

/// Negotiate and render one service failure. The caller supplies the
/// normalized `JetErrorReport`; both media types are rendered from the same
/// bounded projection, and request metadata is copied into either response.
fn jet_http_error_page_response(
    request: &JetHTTPRequest,
    status: i64,
    failure_id: &str,
    report: &JetErrorReport,
    correlation_links: &[String],
    local: bool,
) -> JetHTTPResponse {
    let request_id = match request.headers.get("x-request-id") {
        Some(value) if jet_http_request_id_valid(value) => value.clone(),
        _ => jet_http_new_request_id(),
    };
    jet_http_devtools_publish_exception_report(&request_id, report);
    let projection = jet_error_page_projection(
        status,
        failure_id,
        &request_id,
        report,
        correlation_links,
        local,
    );
    let wants_json = jet_http_error_page_accepts_json(request.headers.get("accept"));
    let (body, content_type) = if wants_json {
        (projection.json(), "application/json; charset=utf-8")
    } else {
        (projection.html(), "text/html; charset=utf-8")
    };
    let mut response = jet_http_srv_response_owned(projection.status, body);
    let _ = response.headers.set("content-type", content_type);
    let _ = response.headers.set("cache-control", "no-store");
    let _ = response.headers.set("vary", "Accept");
    let _ = response.headers.set("x-request-id", &projection.request_id);
    response
}


fn jet_http_srv_response_with_headers(
    status: i64,
    body: &str,
    headers: JetHTTPHeaders,
) -> JetHTTPResponse {
    let mut response = jet_http_srv_response_owned(status, body.to_string());
    response.headers = headers;
    response
}

fn jet_http_srv_response_header(
    mut resp: JetHTTPResponse,
    name: &String,
    value: &String,
) -> JetHTTPResponse {
    if resp.headers.append(name, value).is_err() {
        resp.status = 500;
        resp.body = JetHTTPBody::from_text("500 Internal Server Error".to_string());
        resp.headers = JetHTTPHeaders::new();
    }
    resp
}
fn jet_http_srv_response_status(resp: &JetHTTPResponse) -> i64 { resp.status }
fn jet_http_srv_response_body(resp: &JetHTTPResponse) -> JetHTTPBody {
    resp.body.clone()
}

fn jet_http_mux_serve(
    addr: &String,
    mux: JetHTTPMux,
    tls: Option<JetHTTPServerTls>,
    deadline: Option<jet_std::Duration>,
) -> Result<(), JetHTTPError> {
    if tls.is_some() {
        return Err(JetHTTPError::IO {
            operation: "TLS server requires the native TLS adapter".to_string(),
        });
    }
    let mux = jet_http_mux_with_optional_deadline(mux, deadline.as_ref());
    jet_http_mux_validate(&mux).map_err(|operation| JetHTTPError::IO { operation })?;
    let listener = std::net::TcpListener::bind(addr.as_str()).map_err(|error| {
        JetHTTPError::IO {
            operation: format!("bind on `{addr}` failed: {error}"),
        }
    })?;
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    jet_http_server_run_listener(
        listener,
        mux,
        JetHTTPServerOptions::safe(),
        shutdown,
        None,
        None,
        None,
    )
    .map(|_| ())
    .map_err(|operation| JetHTTPError::IO { operation })
}

fn jet_http_server_bind(
    addr: &String,
    mux: JetHTTPMux,
    tls: Option<JetHTTPServerTls>,
    deadline: Option<jet_std::Duration>,
) -> Result<JetHTTPServer, JetHTTPError> {
    if tls.is_some() {
        return Err(JetHTTPError::IO {
            operation: "TLS server requires the native TLS adapter".to_string(),
        });
    }
    let mux = jet_http_mux_with_optional_deadline(mux, deadline.as_ref());
    jet_http_server_bind_with_tls(addr, mux, None)
}

/// Bind the beginner HTTP server on the safe loopback address and install the
/// canonical per-request deadline middleware before any request is accepted.
fn jet_http_server_default(
    mux: &JetHTTPMux,
    deadline: &jet_std::Duration,
) -> JetHTTPServer {
    let mux = jet_http_mux_with_deadline(mux.clone(), deadline);
    let address = "127.0.0.1:8080".to_string();
    jet_http_server_bind(&address, mux, None, None)
        .unwrap_or_else(|error| panic!("HTTP server bind failed: {error}"))
}

/// `HTTPServer.wait()` is the blocking lifecycle operation. Keeping the
/// implementation on the existing serve kernel makes signal cancellation and
/// request draining identical for the explicit and beginner surfaces.
fn jet_http_server_wait(server: &JetHTTPServer) -> Result<JetHTTPShutdownReport, JetHTTPError> {
    jet_http_server_serve(server)
}


fn jet_http_server_bind_with_tls(
    addr: &String,
    mux: JetHTTPMux,
    tls_conn: Option<JetHTTPTlsConn>,
) -> Result<JetHTTPServer, JetHTTPError> {
    // D-HTTP-UNSUPPORTED1=A: refuse before bind I/O on unsupported targets.
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "windows",
        all(target_os = "wasi", target_env = "p2", target_arch = "wasm32")
    )))]
    {
        let _ = (addr, mux, tls_conn);
        return Err(JetHTTPError::UnsupportedTarget {
            operation: JetHTTPOperation::ServerBind,
        });
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "windows",
        all(target_os = "wasi", target_env = "p2", target_arch = "wasm32")
    ))]
    {
        jet_http_mux_validate(&mux).map_err(|operation| JetHTTPError::IO { operation })?;
        let listener = std::net::TcpListener::bind(addr.as_str()).map_err(|error| {
            JetHTTPError::IO {
                operation: format!("bind on `{addr}` failed: {error}"),
            }
        })?;
        let local_addr = listener.local_addr().map_err(|error| JetHTTPError::IO {
            operation: format!("local address failed: {error}"),
        })?.to_string();
        Ok(JetHTTPServer {
            inner: std::sync::Arc::new(JetHTTPServerState {
                listener: std::sync::Mutex::new(Some(listener)),
                mux,
                local_addr,
                shutdown: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                shutdown_called: std::sync::atomic::AtomicBool::new(false),
                grace_ms: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(30_000)),
                drain_deadline_ms: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                lifecycle: std::sync::atomic::AtomicU8::new(0),
                report: std::sync::Mutex::new(None),
                report_ready: std::sync::Condvar::new(),
                tls_conn,
            }),
        })
    }
}

fn jet_http_server_local_addr(server: &JetHTTPServer) -> Result<String, JetHTTPError> {
    Ok(server.inner.local_addr.clone())
}

fn jet_http_server_serve(server: &JetHTTPServer) -> Result<JetHTTPShutdownReport, JetHTTPError> {
    use std::sync::atomic::Ordering;
    server.inner.lifecycle.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| JetHTTPError::IO {
            operation: "HTTP server can only be served once".to_string(),
        })?;
    let listener = server.inner.listener.lock().unwrap().take().ok_or_else(|| {
        JetHTTPError::IO {
            operation: "HTTP server listener was already consumed".to_string(),
        }
    })?;
    let result = jet_http_server_run_listener(
        listener,
        server.inner.mux.clone(),
        JetHTTPServerOptions::safe(),
        server.inner.shutdown.clone(),
        Some(server.inner.grace_ms.clone()),
        Some(server.inner.drain_deadline_ms.clone()),
        server.inner.tls_conn.clone(),
    )
    .map_err(|operation| JetHTTPError::IO { operation });
    server.inner.lifecycle.store(2, Ordering::Release);
    if let Ok(report) = result {
        *server.inner.report.lock().unwrap() = Some(report);
        server.inner.report_ready.notify_all();
        Ok(report)
    } else {
        server.inner.report_ready.notify_all();
        result
    }
}

fn jet_http_server_wait_for_report(
    server: &JetHTTPServer,
) -> Result<JetHTTPShutdownReport, JetHTTPError> {
    use std::sync::atomic::Ordering;
    let mut report = server.inner.report.lock().unwrap();
    while report.is_none() && server.inner.lifecycle.load(Ordering::Acquire) == 1 {
        report = server.inner.report_ready.wait(report).unwrap();
    }
    (*report).ok_or_else(|| JetHTTPError::IO {
        operation: "HTTP server stopped without a shutdown report".to_string(),
    })
}

fn jet_http_server_shutdown(
    server: &JetHTTPServer,
    grace: &jet_std::Duration,
) -> Result<JetHTTPShutdownReport, JetHTTPError> {
    use std::sync::atomic::Ordering;
    let already_requested = server.inner.shutdown_called.swap(true, Ordering::AcqRel);
    if already_requested {
        return jet_http_server_wait_for_report(server);
    }
    // Claim the terminal state for an unserved server. Otherwise teardown can
    // clear the JIT handles while a racing `serve` still wins the 0 -> 1 CAS
    // and starts a worker with callbacks from the old resident image.
    if server.inner.lifecycle.compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire).is_ok() {
        return Err(JetHTTPError::IO {
            operation: "HTTP server is not serving".to_string(),
        });
    }
    if server.inner.lifecycle.load(Ordering::Acquire) != 1 {
        return Err(JetHTTPError::IO {
            operation: "HTTP server is not serving".to_string(),
        });
    }
    let grace_ms = grace.as_millis().max(0) as u64;
    // Publish the absolute drain deadline before the shutdown flag so H2 and the
    // accept loop share one clock instead of each computing now+grace.
    let deadline_ms = jet_http_unix_now_ms().saturating_add(grace_ms);
    server.inner.drain_deadline_ms.store(deadline_ms, Ordering::Release);
    server.inner.grace_ms.store(grace_ms, Ordering::Release);
    server.inner.shutdown.store(true, Ordering::Release);
    jet_http_server_wait_for_report(server)
}


/// Run every native HTTP listener under the same root task-control scope that
/// `process.on_signal` cancels. Existing task scopes remain unchanged because
/// `jet_scheduler_with_root_control` is a no-op when a caller already owns one.
fn jet_http_server_run_listener(
    listener: std::net::TcpListener,
    mux: JetHTTPMux,
    options: JetHTTPServerOptions,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    dynamic_grace_ms: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    drain_deadline_ms: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    tls_conn: Option<JetHTTPTlsConn>,
) -> Result<JetHTTPShutdownReport, String> {
    jet_scheduler_with_root_control(|| {
        jet_http_server_run_listener_inner(
            listener,
            mux,
            options,
            shutdown,
            dynamic_grace_ms,
            drain_deadline_ms,
            tls_conn,
        )
    })
}

fn jet_http_server_run_listener_inner(
    listener: std::net::TcpListener,
    mux: JetHTTPMux,
    options: JetHTTPServerOptions,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    dynamic_grace_ms: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    drain_deadline_ms: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    tls_conn: Option<JetHTTPTlsConn>,
) -> Result<JetHTTPShutdownReport, String> {
    #[cfg(all(target_os = "wasi", target_env = "p2", target_arch = "wasm32"))]
    {
        return jet_http_server_run_listener_wasip2(
            listener,
            mux,
            options,
            shutdown,
            dynamic_grace_ms,
            drain_deadline_ms,
            tls_conn,
        );
    }

    use std::io::{Read, Write};
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::{Receiver, SyncSender, TrySendError};

    listener.set_nonblocking(true).map_err(|error| format!("http listener setup failed: {error}"))?;
    let (tx, rx): (SyncSender<(std::net::TcpStream, std::net::IpAddr)>, Receiver<(std::net::TcpStream, std::net::IpAddr)>) =
        std::sync::mpsc::sync_channel(options.admission_queue);
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
    let active = std::sync::Arc::new(std::sync::Mutex::new(Vec::<std::net::TcpStream>::new()));
    let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let force_cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let connection_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let per_ip = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::<std::net::IpAddr, usize>::new()));
    // Absolute UNIX-ms drain deadline shared by accept-loop wait and H2 serve.
    // Server.shutdown publishes it before the flag; other callers set it here.
    let drain_deadline_ms = drain_deadline_ms
        .unwrap_or_else(|| std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)));
    let mut workers = Vec::new();
    for _ in 0..options.workers.max(1) {
        let worker_rx = rx.clone();
        let worker_mux = mux.clone();
        let worker_options = options.clone();
        let worker_active = active.clone();
        let worker_completed = completed.clone();
        let worker_force_cancel = force_cancel.clone();
        let worker_shutdown = shutdown.clone();
        let worker_grace = dynamic_grace_ms.clone();
        let worker_deadline = drain_deadline_ms.clone();
        let worker_connection_count = connection_count.clone();
        let worker_per_ip = per_ip.clone();
        let worker_tls = tls_conn.clone();
        workers.push(std::thread::spawn(move || loop {
            let received = worker_rx.lock().unwrap().recv();
            let Ok((mut stream, peer_ip)) = received else { break };
            if worker_force_cancel.load(Ordering::Acquire) {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            } else {
                let peer = stream.peer_addr().ok();
                if let Ok(tracked) = stream.try_clone() { worker_active.lock().unwrap().push(tracked); }
                jet_scheduler_with_root_control(|| {
                    if let Some(tls) = worker_tls.as_ref() {
                        let _ = tls(
                            stream,
                            worker_shutdown.clone(),
                            worker_options.clone(),
                            worker_grace.clone(),
                            worker_deadline.clone(),
                        );
                    } else {
                        jet_http_server_handle_stream(
                            &mut stream,
                            &worker_mux,
                            &worker_options,
                            &worker_shutdown,
                            worker_grace.as_deref(),
                            Some(worker_deadline.as_ref()),
                        );
                    }
                });
                worker_completed.fetch_add(1, Ordering::Relaxed);
                worker_active.lock().unwrap().retain(|tracked| tracked.peer_addr().ok() != peer);
            }
            worker_connection_count.fetch_sub(1, Ordering::AcqRel);
            let mut counts = worker_per_ip.lock().unwrap();
            if let Some(count) = counts.get_mut(&peer_ip) {
                *count -= 1;
                if *count == 0 { counts.remove(&peer_ip); }
            }
        }));
    }

    let mut report = JetHTTPShutdownReport::default();
    let mut accept_error = None;
    while !shutdown.load(Ordering::Acquire) {
        if jet_scheduler_wait_point_cancelled() {
            shutdown.store(true, Ordering::Release);
            break;
        }
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let peer_ip = peer.ip();
                let globally_full = connection_count.load(Ordering::Acquire) >= options.max_connections;
                let ip_full = per_ip.lock().unwrap().get(&peer_ip).copied().unwrap_or(0) >= options.max_connections_per_ip;
                if globally_full || ip_full {
                    report.user_overloaded += 1;
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(10)));
                    let mut discard = [0u8; 8192];
                    let _ = stream.read(&mut discard);
                    let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    let _ = stream.flush();
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                    continue;
                }
                connection_count.fetch_add(1, Ordering::AcqRel);
                *per_ip.lock().unwrap().entry(peer_ip).or_insert(0) += 1;
                match tx.try_send((stream, peer_ip)) {
                    Ok(()) => report.user_accepted += 1,
                    Err(TrySendError::Full((mut stream, peer_ip))) => {
                        connection_count.fetch_sub(1, Ordering::AcqRel);
                        let mut counts = per_ip.lock().unwrap();
                        if let Some(count) = counts.get_mut(&peer_ip) {
                            *count -= 1;
                            if *count == 0 { counts.remove(&peer_ip); }
                        }
                        drop(counts);
                        report.user_overloaded += 1;
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(10)));
                        let mut discard = [0u8; 8192];
                        let _ = stream.read(&mut discard);
                        let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        let _ = stream.flush();
                        let _ = stream.shutdown(std::net::Shutdown::Write);
                    }
                    Err(TrySendError::Disconnected((_stream, peer_ip))) => {
                        connection_count.fetch_sub(1, Ordering::AcqRel);
                        let mut counts = per_ip.lock().unwrap();
                        if let Some(count) = counts.get_mut(&peer_ip) {
                            *count -= 1;
                            if *count == 0 { counts.remove(&peer_ip); }
                        }
                        break;
                    }
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(2)),
            Err(error) => {
                accept_error = Some(format!("http accept failed: {error}"));
                force_cancel.store(true, Ordering::Release);
                for stream in active.lock().unwrap().iter() {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                }
                break;
            }
        }
    }
    drop(tx);
    let grace = dynamic_grace_ms.as_ref()
        .map(|value| std::time::Duration::from_millis(value.load(Ordering::Acquire)))
        .unwrap_or(options.shutdown_grace);
    let deadline_ms = {
        let existing = drain_deadline_ms.load(Ordering::Acquire);
        if existing > 0 {
            existing
        } else {
            let computed = jet_http_unix_now_ms().saturating_add(grace.as_millis() as u64);
            drain_deadline_ms.store(computed, Ordering::Release);
            computed
        }
    };
    let deadline = jet_http_instant_from_unix_ms(deadline_ms);
    while accept_error.is_none()
        && completed.load(Ordering::Acquire) < report.user_accepted as usize
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // H2 may observe the shared deadline only after its poll read times out.
    let observe = deadline + std::time::Duration::from_millis(30);
    while accept_error.is_none()
        && completed.load(Ordering::Acquire) < report.user_accepted as usize
        && std::time::Instant::now() < observe
    {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    report.user_completed = completed
        .load(Ordering::Acquire)
        .min(report.user_accepted as usize) as i64;
    report.user_cancelled = report.user_accepted.saturating_sub(report.user_completed);
    if report.user_cancelled > 0 {
        force_cancel.store(true, Ordering::Release);
        for stream in active.lock().unwrap().iter() { let _ = stream.shutdown(std::net::Shutdown::Both); }
    }
    for worker in workers {
        let _ = worker.join();
    }
    if let Some(error) = accept_error {
        return Err(error);
    }
    Ok(report)
}

#[cfg(all(target_os = "wasi", target_env = "p2", target_arch = "wasm32"))]
fn jet_http_server_run_listener_wasip2(
    listener: std::net::TcpListener,
    mux: JetHTTPMux,
    options: JetHTTPServerOptions,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    dynamic_grace_ms: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    drain_deadline_ms: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    tls_conn: Option<JetHTTPTlsConn>,
) -> Result<JetHTTPShutdownReport, String> {
    use std::sync::atomic::Ordering;

    listener
        .set_nonblocking(true)
        .map_err(|error| format!("http listener setup failed: {error}"))?;
    let drain_deadline_ms = drain_deadline_ms
        .unwrap_or_else(|| std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)));
    let mut report = JetHTTPShutdownReport::default();

    // WASI Preview 2 runtimes do not provide `std::thread::spawn`. Keep the
    // same Prelude protocol, limits, and request handler, but serve one
    // connection at a time on the component's main thread.
    while !shutdown.load(Ordering::Acquire) {
        if jet_scheduler_wait_point_cancelled() {
            shutdown.store(true, Ordering::Release);
            break;
        }
        match listener.accept() {
            Ok((mut stream, _peer)) => {
                report.user_accepted += 1;
                stream
                    .set_nonblocking(false)
                    .map_err(|error| format!("http connection setup failed: {error}"))?;
                if let Some(tls) = tls_conn.as_ref() {
                    if let Err(error) = tls(
                        stream,
                        shutdown.clone(),
                        options.clone(),
                        dynamic_grace_ms.clone(),
                        drain_deadline_ms.clone(),
                    ) {
                        eprintln!("E2801: Connection failed: {error}");
                    }
                } else {
                    jet_http_server_handle_stream_wasip2(&mut stream, &mux, &options);
                }
                report.user_completed += 1;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(error) => return Err(format!("http accept failed: {error}")),
        }
    }

    report.user_cancelled = report
        .user_accepted
        .saturating_sub(report.user_completed);
    Ok(report)
}

#[cfg(all(target_os = "wasi", target_env = "p2", target_arch = "wasm32"))]
fn jet_http_server_handle_stream_wasip2(
    stream: &mut std::net::TcpStream,
    mux: &JetHTTPMux,
    options: &JetHTTPServerOptions,
) {
    use std::io::Write;

    let raw = match jet_http_srv_read_with_limits(stream, options) {
        Ok(raw) => raw,
        Err(error) => {
            let _ = stream.write_all(jet_http_srv_read_error_response(&error).as_bytes());
            return;
        }
    };
    let request = match jet_http_srv_parse(&raw) {
        Ok(request) => request,
        Err(error) => {
            let _ = stream.write_all(jet_http_srv_read_error_response(&error).as_bytes());
            return;
        }
    };
    let response = jet_http_mux_dispatch(mux, request).unwrap_or_else(jet_http_srv_error_response);
    let _ = jet_http_srv_write_response(stream, &response, "HTTP/1.1", true);
}

fn jet_http_unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn jet_http_instant_from_unix_ms(deadline_ms: u64) -> std::time::Instant {
    let now = std::time::Instant::now();
    let now_ms = jet_http_unix_now_ms();
    if deadline_ms <= now_ms {
        now
    } else {
        now + std::time::Duration::from_millis(deadline_ms - now_ms)
    }
}

fn jet_http_server_handle_stream(
    stream: &mut std::net::TcpStream,
    mux: &JetHTTPMux,
    options: &JetHTTPServerOptions,
    shutdown: &std::sync::atomic::AtomicBool,
    dynamic_grace_ms: Option<&std::sync::atomic::AtomicU64>,
    drain_deadline_ms: Option<&std::sync::atomic::AtomicU64>,
) {
    use std::io::Write;
    use std::sync::atomic::Ordering;

    let _ = stream.set_write_timeout(Some(options.write_idle_timeout));
    if jet_http2_is_preface(stream, options.read_header_timeout) {
        let (result, last_stream) = jet_http2_serve_with_last_stream(
            stream,
            mux,
            options,
            shutdown,
            dynamic_grace_ms,
            drain_deadline_ms,
        );
        if result.is_err() {
            let _ = jet_http2_write_frame(
                stream,
                7,
                0,
                0,
                &jet_http2_goaway_payload(last_stream, 1),
            );
            let _ = std::io::Write::flush(stream);
        }
        let _ = stream.shutdown(std::net::Shutdown::Both);
        return;
    }
    for request_index in 0..JET_HTTP_MAX_REQUESTS_PER_CONNECTION {
        if request_index > 0 && shutdown.load(Ordering::Acquire) {
            return;
        }
        let (request, request_version) = match jet_http_srv_read_streaming(
            stream,
            options,
            request_index > 0,
            (request_index > 0).then_some(shutdown),
        ) {
            Ok(Some(request)) => request,
            Ok(None) => return,
            Err(error) => {
                let _ = stream.write_all(jet_http_srv_read_error_response(&error).as_bytes());
                jet_http_srv_finish_close(stream);
                return;
            }
        };
        if request_index > 0 && shutdown.load(Ordering::Acquire) {
            return;
        }
        let body = request.body.clone();
        let close = !jet_http_srv_request_keep_alive(&request_version, &request.headers)
            || request_index + 1 == JET_HTTP_MAX_REQUESTS_PER_CONNECTION;
        if request_index > 0 && shutdown.load(Ordering::Acquire) {
            return;
        }
        // D-WS1=B: expose this connection to core.net.ws.upgrade during dispatch.
        let _ws_guard = JetWsStreamGuard::install(stream);
        let response = jet_http_mux_dispatch(mux, request)
            .unwrap_or_else(jet_http_srv_error_response);
        if jet_ws_take_upgraded() {
            // Handler completed a WebSocket upgrade on a cloned stream handle.
            return;
        }
        let close = close
            || !body.is_drained()
            || (request_version == "HTTP/1.0" && response.body.length().is_none());
        if jet_http_srv_write_response(stream, &response, &request_version, close).is_err() {
            return;
        }
        if close {
            jet_http_srv_finish_close(stream);
            return;
        }
    }
}

fn jet_http_srv_empty_response(status: i64) -> JetHTTPResponse {
    let mut response = jet_http_srv_response_owned(status, String::new());
    response.body = JetHTTPBody::empty();
    response
}

fn jet_http_srv_finish_close(stream: &mut std::net::TcpStream) {
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(10)));
    let mut discarded = 0usize;
    let mut buffer = [0u8; 4096];
    while discarded < 64 * 1024 {
        match std::io::Read::read(stream, &mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => discarded += read,
        }
    }
}

const JET_HTTP2_PREFACE: &[u8; 24] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const JET_HTTP2_MAX_FRAME: usize = 16 * 1024;
const JET_HTTP2_MAX_HEADER_LIST: usize = 32 * 1024;

trait JetHTTP2Transport: std::io::Read + std::io::Write {
    fn jet_http_set_read_timeout(
        &mut self,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), String>;
    fn jet_http_set_write_timeout(
        &mut self,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), String>;
}

impl JetHTTP2Transport for std::net::TcpStream {
    fn jet_http_set_read_timeout(
        &mut self,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), String> {
        self.set_read_timeout(timeout)
            .map_err(|_| "HTTP/2 read timeout setup failed".to_string())
    }

    fn jet_http_set_write_timeout(
        &mut self,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), String> {
        self.set_write_timeout(timeout)
            .map_err(|_| "HTTP/2 write timeout setup failed".to_string())
    }
}

struct JetHTTP2TlsTransport {
    reader: JetHTTPTlsReader,
    writer: JetHTTPTlsWriter,
    set_read_timeout: JetHTTPTlsTimeout,
    set_write_timeout: JetHTTPTlsTimeout,
}

impl std::io::Read for JetHTTP2TlsTransport {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(output)
    }
}

impl std::io::Write for JetHTTP2TlsTransport {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        self.writer.write(input)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl JetHTTP2Transport for JetHTTP2TlsTransport {
    fn jet_http_set_read_timeout(
        &mut self,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), String> {
        (self.set_read_timeout)(timeout)
    }

    fn jet_http_set_write_timeout(
        &mut self,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), String> {
        (self.set_write_timeout)(timeout)
    }
}

static JET_HTTP2_HUFFMAN: std::sync::OnceLock<std::collections::HashMap<(u8, u32), u8>> = std::sync::OnceLock::new();
const JET_HTTP2_HUFFMAN_LENGTHS: [u8; 256] = [
    13, 23, 28, 28, 28, 28, 28, 28, 28, 24, 30, 28, 28, 30, 28, 28,
    28, 28, 28, 28, 28, 28, 30, 28, 28, 28, 28, 28, 28, 28, 28, 28,
    6, 10, 10, 12, 13, 6, 8, 11, 10, 10, 8, 11, 8, 6, 6, 6,
    5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 7, 8, 15, 6, 12, 10,
    13, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    7, 7, 7, 7, 7, 7, 7, 7, 8, 7, 8, 13, 19, 13, 14, 6,
    15, 5, 6, 5, 6, 5, 6, 6, 6, 5, 7, 7, 6, 6, 6, 5,
    6, 7, 6, 5, 5, 6, 7, 7, 7, 7, 7, 15, 11, 14, 13, 28,
    20, 22, 20, 20, 22, 22, 22, 23, 22, 23, 23, 23, 23, 23, 24, 23,
    24, 24, 22, 23, 24, 23, 23, 23, 23, 21, 22, 23, 22, 23, 23, 24,
    22, 21, 20, 22, 22, 23, 23, 21, 23, 22, 22, 24, 21, 22, 23, 23,
    21, 21, 22, 21, 23, 22, 23, 23, 20, 22, 22, 22, 23, 22, 22, 23,
    26, 26, 20, 19, 22, 23, 22, 25, 26, 26, 26, 27, 27, 26, 24, 25,
    19, 21, 26, 27, 27, 26, 27, 24, 21, 21, 26, 26, 28, 27, 27, 27,
    20, 24, 20, 21, 22, 21, 21, 23, 22, 22, 25, 25, 24, 24, 26, 23,
    26, 27, 26, 26, 27, 27, 27, 27, 27, 28, 27, 27, 27, 27, 27, 26,
];

struct JetHTTP2Frame {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: Vec<u8>,
}

fn jet_http2_is_preface(stream: &std::net::TcpStream, timeout: std::time::Duration) -> bool {
    let started = std::time::Instant::now();
    let _ = stream.set_read_timeout(Some(timeout));
    let mut bytes = [0u8; 24];
    while started.elapsed() < timeout {
        match stream.peek(&mut bytes) {
            Ok(read) if read > 0 => {
                if bytes[..read] != JET_HTTP2_PREFACE[..read] { return false; }
                if read == bytes.len() { return true; }
            }
            Ok(_) => return false,
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return false,
            Err(_) => return false,
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    false
}

fn jet_http2_read_frame(reader: &mut impl std::io::Read) -> Result<JetHTTP2Frame, String> {
    let mut header = [0u8; 9];
    reader.read_exact(&mut header).map_err(|error| {
        if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
            "HTTP/2 read timed out".to_string()
        } else { "HTTP/2 frame header ended early".to_string() }
    })?;
    let length = (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
    if length > JET_HTTP2_MAX_FRAME { return Err("HTTP/2 frame exceeds the advertised maximum".to_string()); }
    if header[5] & 0x80 != 0 { return Err("HTTP/2 reserved stream bit is set".to_string()); }
    let stream = u32::from_be_bytes([header[5], header[6], header[7], header[8]]);
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).map_err(|error| {
        if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
            "HTTP/2 frame payload timed out".to_string()
        } else { "HTTP/2 frame payload ended early".to_string() }
    })?;
    Ok(JetHTTP2Frame { kind: header[3], flags: header[4], stream, payload })
}

fn jet_http2_write_frame(
    writer: &mut impl std::io::Write,
    kind: u8,
    flags: u8,
    stream: u32,
    payload: &[u8],
) -> Result<(), String> {
    if payload.len() > 0x00ff_ffff { return Err("HTTP/2 frame payload is too large".to_string()); }
    let length = payload.len();
    let stream = (stream & 0x7fff_ffff).to_be_bytes();
    let header = [
        (length >> 16) as u8, (length >> 8) as u8, length as u8, kind, flags,
        stream[0], stream[1], stream[2], stream[3],
    ];
    writer.write_all(&header).and_then(|()| writer.write_all(payload))
        .map_err(|_| "HTTP/2 write failed".to_string())
}

fn jet_http2_goaway_payload(last_stream: u32, error_code: u32) -> [u8; 8] {
    let mut payload = [0u8; 8];
    payload[..4].copy_from_slice(&(last_stream & 0x7fff_ffff).to_be_bytes());
    payload[4..].copy_from_slice(&error_code.to_be_bytes());
    payload
}

fn jet_http2_integer(input: &[u8], cursor: &mut usize, prefix: u8) -> Result<usize, String> {
    let first = *input.get(*cursor).ok_or_else(|| "HPACK integer ended early".to_string())?;
    *cursor += 1;
    let mask = (1u16 << prefix) as u8 - 1;
    let mut value = usize::from(first & mask);
    if value < usize::from(mask) { return Ok(value); }
    let mut shift = 0;
    loop {
        let byte = *input.get(*cursor).ok_or_else(|| "HPACK integer ended early".to_string())?;
        *cursor += 1;
        if shift >= usize::BITS as usize || usize::from(byte & 0x7f) > (usize::MAX - value) >> shift {
            return Err("HPACK integer overflow".to_string());
        }
        value += usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 { return Ok(value); }
        shift += 7;
    }
}

fn jet_http2_huffman(input: &[u8]) -> Result<String, String> {
    let table = JET_HTTP2_HUFFMAN.get_or_init(|| {
        let mut entries = std::collections::HashMap::with_capacity(256);
        let mut symbols = (0u16..256).collect::<Vec<_>>();
        symbols.sort_by_key(|symbol| (JET_HTTP2_HUFFMAN_LENGTHS[*symbol as usize], *symbol));
        let mut code = 0u32;
        let mut prior = 0u8;
        for symbol in symbols {
            let length = JET_HTTP2_HUFFMAN_LENGTHS[symbol as usize];
            code <<= u32::from(length - prior);
            entries.insert((length, code), symbol as u8);
            code += 1;
            prior = length;
        }
        entries
    });
    let mut output = Vec::new();
    let mut code = 0u32;
    let mut length = 0u8;
    for byte in input {
        for shift in (0..8).rev() {
            code = (code << 1) | u32::from((byte >> shift) & 1);
            length += 1;
            if let Some(symbol) = table.get(&(length, code)) {
                output.push(*symbol);
                code = 0;
                length = 0;
            } else if length == 30 {
                return Err("HPACK Huffman code is invalid".to_string());
            }
        }
    }
    if length > 7 || code != (1u32 << length) - 1 { return Err("HPACK Huffman padding is invalid".to_string()); }
    String::from_utf8(output).map_err(|_| "HPACK string is not UTF-8".to_string())
}

fn jet_http2_string(input: &[u8], cursor: &mut usize) -> Result<String, String> {
    let huffman = input.get(*cursor).is_some_and(|byte| byte & 0x80 != 0);
    let length = jet_http2_integer(input, cursor, 7)?;
    let end = cursor.checked_add(length).ok_or_else(|| "HPACK string length overflow".to_string())?;
    let bytes = input.get(*cursor..end).ok_or_else(|| "HPACK string ended early".to_string())?;
    *cursor = end;
    if huffman { jet_http2_huffman(bytes) } else {
        std::str::from_utf8(bytes).map(str::to_string).map_err(|_| "HPACK string is not UTF-8".to_string())
    }
}

fn jet_http2_static(index: usize) -> Option<(&'static str, &'static str)> {
    const NAMES: [&str; 47] = [
        "accept-charset", "accept-encoding", "accept-language", "accept-ranges", "accept",
        "access-control-allow-origin", "age", "allow", "authorization", "cache-control",
        "content-disposition", "content-encoding", "content-language", "content-length",
        "content-location", "content-range", "content-type", "cookie", "date", "etag", "expect",
        "expires", "from", "host", "if-match", "if-modified-since", "if-none-match", "if-range",
        "if-unmodified-since", "last-modified", "link", "location", "max-forwards",
        "proxy-authenticate", "proxy-authorization", "range", "referer", "refresh", "retry-after",
        "server", "set-cookie", "strict-transport-security", "transfer-encoding", "user-agent",
        "vary", "via", "www-authenticate",
    ];
    match index {
        1 => Some((":authority", "")), 2 => Some((":method", "GET")), 3 => Some((":method", "POST")),
        4 => Some((":path", "/")), 5 => Some((":path", "/index.html")),
        6 => Some((":scheme", "http")), 7 => Some((":scheme", "https")),
        8 => Some((":status", "200")), 9 => Some((":status", "204")), 10 => Some((":status", "206")),
        11 => Some((":status", "304")), 12 => Some((":status", "400")), 13 => Some((":status", "404")),
        14 => Some((":status", "500")),
        15..=61 => Some((NAMES[index - 15], if index == 16 { "gzip, deflate" } else { "" })),
        _ => None,
    }
}

struct JetHTTP2Hpack {
    dynamic: Vec<(String, String)>,
    dynamic_size: usize,
    max_size: usize,
}

impl JetHTTP2Hpack {
    fn new() -> Self { Self { dynamic: Vec::new(), dynamic_size: 0, max_size: 4096 } }

    fn field(&self, index: usize) -> Option<(String, String)> {
        jet_http2_static(index).map(|(name, value)| (name.to_string(), value.to_string()))
            .or_else(|| self.dynamic.get(index.checked_sub(62)?).cloned())
    }

    fn resize(&mut self, size: usize) -> Result<(), String> {
        if size > 4096 { return Err("HPACK dynamic table size exceeds server limit".to_string()); }
        self.max_size = size;
        while self.dynamic_size > self.max_size {
            let Some((name, value)) = self.dynamic.pop() else { break };
            self.dynamic_size -= name.len() + value.len() + 32;
        }
        Ok(())
    }

    fn insert(&mut self, name: String, value: String) {
        let size = name.len() + value.len() + 32;
        if size > self.max_size {
            self.dynamic.clear();
            self.dynamic_size = 0;
            return;
        }
        while self.dynamic_size + size > self.max_size {
            let Some((old_name, old_value)) = self.dynamic.pop() else { break };
            self.dynamic_size -= old_name.len() + old_value.len() + 32;
        }
        self.dynamic.insert(0, (name, value));
        self.dynamic_size += size;
    }
}

fn jet_http2_decode_headers(decoder: &mut JetHTTP2Hpack, block: &[u8]) -> Result<Vec<(String, String)>, String> {
    let mut headers = Vec::new();
    let mut cursor = 0;
    let mut allow_size_update = true;
    let mut list_size = 0usize;
    while cursor < block.len() {
        let byte = block[cursor];
        if byte & 0x80 != 0 {
            let index = jet_http2_integer(block, &mut cursor, 7)?;
            let (name, value) = decoder.field(index).ok_or_else(|| "HPACK index is invalid".to_string())?;
            list_size = list_size.saturating_add(name.len() + value.len() + 32);
            headers.push((name, value));
            allow_size_update = false;
        } else if byte & 0xe0 == 0x20 {
            if !allow_size_update { return Err("HPACK table size update follows a header".to_string()); }
            let size = jet_http2_integer(block, &mut cursor, 5)?;
            decoder.resize(size)?;
        } else {
            let indexed = byte & 0x40 != 0;
            let prefix = if indexed { 6 } else { 4 };
            let name_index = jet_http2_integer(block, &mut cursor, prefix)?;
            let name = if name_index == 0 { jet_http2_string(block, &mut cursor)? }
                else { decoder.field(name_index).map(|field| field.0).ok_or_else(|| "HPACK name index is invalid".to_string())? };
            let value = jet_http2_string(block, &mut cursor)?;
            list_size = list_size.saturating_add(name.len() + value.len() + 32);
            if indexed { decoder.insert(name.clone(), value.clone()); }
            headers.push((name, value));
            allow_size_update = false;
        }
        if list_size > JET_HTTP2_MAX_HEADER_LIST {
            return Err("HTTP/2 header list is too large".to_string());
        }
        if headers.len() > 100 { return Err("HTTP/2 request has too many headers".to_string()); }
    }
    Ok(headers)
}

fn jet_http2_encode_integer(output: &mut Vec<u8>, value: usize, prefix: u8, bits: u8) {
    let mask = (1usize << prefix) - 1;
    if value < mask { output.push(bits | value as u8); return; }
    output.push(bits | mask as u8);
    let mut rest = value - mask;
    while rest >= 128 { output.push((rest as u8 & 0x7f) | 0x80); rest >>= 7; }
    output.push(rest as u8);
}

fn jet_http2_encode_string(output: &mut Vec<u8>, value: &str) {
    jet_http2_encode_integer(output, value.len(), 7, 0);
    output.extend_from_slice(value.as_bytes());
}

fn jet_http2_encode_response_headers(response: &JetHTTPResponse, length: Option<usize>) -> Vec<u8> {
    let mut output = Vec::new();
    let status_index = match response.status { 200 => Some(8), 204 => Some(9), 206 => Some(10), 304 => Some(11), 400 => Some(12), 404 => Some(13), 500 => Some(14), _ => None };
    if let Some(index) = status_index { jet_http2_encode_integer(&mut output, index, 7, 0x80); }
    else {
        jet_http2_encode_integer(&mut output, 8, 4, 0);
        jet_http2_encode_string(&mut output, &response.status.to_string());
    }
    if let Some(length) = length {
        jet_http2_encode_integer(&mut output, 28, 4, 0);
        jet_http2_encode_string(&mut output, &length.to_string());
    }
    let connection_headers = response.headers.all("connection").into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    for (name, value) in &response.headers {
        if matches!(name.to_ascii_lowercase().as_str(), "connection" | "content-length" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade")
            || connection_headers.iter().any(|candidate| name.eq_ignore_ascii_case(candidate))
        { continue; }
        jet_http2_encode_integer(&mut output, 0, 4, 0);
        jet_http2_encode_string(&mut output, &name.to_ascii_lowercase());
        jet_http2_encode_string(&mut output, value);
    }
    output
}

fn jet_http2_encode_trailers(trailers: &JetHTTPHeaders) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    for (name, value) in trailers {
        if !jet_http_trailer_name_allowed(name) {
            return Err("HTTP/2 response trailer is forbidden".to_string());
        }
        jet_http2_encode_integer(&mut output, 0, 4, 0);
        jet_http2_encode_string(&mut output, &name.to_ascii_lowercase());
        jet_http2_encode_string(&mut output, value);
    }
    if output.len() > JET_HTTP2_MAX_HEADER_LIST {
        return Err("HTTP/2 response trailer list is too large".to_string());
    }
    Ok(output)
}

fn jet_http2_request_trailers(headers: Vec<(String, String)>) -> Result<JetHTTPHeaders, String> {
    let mut trailers = JetHTTPHeaders::new();
    for (name, value) in headers {
        if name.starts_with(':') {
            return Err("HTTP/2 trailer contains a pseudo-header".to_string());
        }
        if name.bytes().any(|byte| byte.is_ascii_uppercase()) {
            return Err("HTTP/2 trailer name contains uppercase".to_string());
        }
        if !jet_http_trailer_name_allowed(&name) {
            return Err("HTTP/2 request trailer is forbidden".to_string());
        }
        trailers
            .append(&name, &value)
            .map_err(|_| "HTTP/2 trailer is invalid".to_string())?;
    }
    Ok(trailers)
}

fn jet_http2_request_with_trailers(
    headers: Vec<(String, String)>,
    body: JetHTTPBody,
    trailers: std::sync::Arc<std::sync::Mutex<JetHTTPHeaders>>,
    end_stream: bool,
) -> Result<(JetHTTPRequest, Option<usize>), String> {
    let mut method = None;
    let mut path = None;
    let mut scheme = None;
    let mut authority = None;
    let mut regular = JetHTTPHeaders::new();
    let mut saw_regular = false;
    for (name, value) in headers {
        if name.bytes().any(|byte| byte.is_ascii_uppercase()) { return Err("HTTP/2 header name contains uppercase".to_string()); }
        if name.starts_with(':') {
            if saw_regular { return Err("HTTP/2 pseudo-header follows regular headers".to_string()); }
            let slot = match name.as_str() { ":method" => &mut method, ":path" => &mut path, ":scheme" => &mut scheme, ":authority" => &mut authority, _ => return Err("HTTP/2 pseudo-header is invalid".to_string()) };
            if slot.replace(value).is_some() { return Err("HTTP/2 pseudo-header is duplicated".to_string()); }
        } else {
            saw_regular = true;
            if matches!(name.as_str(), "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade") { return Err("HTTP/2 connection-specific header is forbidden".to_string()); }
            if name == "te" && !value.eq_ignore_ascii_case("trailers") { return Err("HTTP/2 TE value is invalid".to_string()); }
            regular.append(&name, &value).map_err(|_| "HTTP/2 header is invalid".to_string())?;
        }
    }
    let method = method.ok_or_else(|| "HTTP/2 method is missing".to_string())?;
    if !JetHTTPHeaders::valid_name(&method) {
        return Err("HTTP/2 request target is invalid".to_string());
    }
    let path = if method == "CONNECT" {
        // RFC 9113 §8.5: CONNECT omits :scheme and :path; :authority is the target.
        if path.is_some() {
            return Err("HTTP/2 CONNECT must omit :path".to_string());
        }
        if scheme.is_some() {
            return Err("HTTP/2 CONNECT must omit :scheme".to_string());
        }
        let authority = authority.ok_or_else(|| "HTTP/2 CONNECT requires :authority".to_string())?;
        if jet_http_trim_ows(&authority) != authority {
            return Err("HTTP/2 authority is invalid".to_string());
        }
        let parsed = jet_http_parse_authority(&authority)
            .ok_or_else(|| "HTTP/2 authority is invalid".to_string())?;
        if let Some(host) = regular.get("host") {
            let host = jet_http_parse_authority(host)
                .ok_or_else(|| "HTTP/2 authority does not match host".to_string())?;
            if parsed.host != host.host || parsed.port != host.port {
                return Err("HTTP/2 authority does not match host".to_string());
            }
        } else {
            regular
                .append("host", &jet_http_format_authority(&parsed))
                .map_err(|_| "HTTP/2 authority is invalid".to_string())?;
        }
        jet_http_format_authority(&parsed)
    } else {
        let path = path.ok_or_else(|| "HTTP/2 path is missing".to_string())?;
        if !matches!(scheme.as_deref(), Some("http" | "https")) {
            return Err("HTTP/2 scheme is invalid".to_string());
        }
        if let Some(authority) = authority {
            jet_http_parse_authority(&authority)
                .ok_or_else(|| "HTTP/2 authority is invalid".to_string())?;
            if regular
                .get("host")
                .is_some_and(|host| !host.eq_ignore_ascii_case(&authority))
            {
                return Err("HTTP/2 authority does not match host".to_string());
            }
            if regular.get("host").is_none() {
                regular
                    .append("host", &authority)
                    .map_err(|_| "HTTP/2 authority is invalid".to_string())?;
            }
        }
        if !(jet_http_path_query_valid(&path) || method == "OPTIONS" && path == "*") {
            return Err("HTTP/2 request target is invalid".to_string());
        }
        path
    };
    let mut content_length = None;
    for value in regular.all("content-length") {
        content_length = Some(jet_http_parse_content_length(value, content_length).map_err(|error| error.to_string())?);
    }
    if let Ok(mut state) = body.state.lock() {
        state.length = content_length;
        state.drained.store(end_stream, std::sync::atomic::Ordering::Release);
    }
    Ok((JetHTTPRequest::server_body_with_trailers(&method, path, body, regular, trailers), content_length))
}

fn jet_http2_request(
    headers: Vec<(String, String)>,
    body: JetHTTPBody,
) -> Result<(JetHTTPRequest, Option<usize>), String> {
    jet_http2_request_with_trailers(
        headers,
        body,
        std::sync::Arc::new(std::sync::Mutex::new(JetHTTPHeaders::new())),
        true,
    )
}

enum JetHTTP2BodyPart {
    Data { bytes: Vec<u8>, flow_bytes: usize },
    End,
}

struct JetHTTPSchedulerBlockingWait;

impl JetHTTPSchedulerBlockingWait {
    fn enter() -> Self {
        jet_scheduler_blocking_wait_enter();
        Self
    }
}

impl Drop for JetHTTPSchedulerBlockingWait {
    fn drop(&mut self) { jet_scheduler_blocking_wait_leave(); }
}

struct JetHTTP2BodyReader {
    receiver: std::sync::mpsc::Receiver<JetHTTP2BodyPart>,
    consumed: std::sync::mpsc::Sender<(u32, usize)>,
    stream_id: u32,
    current: Option<(std::io::Cursor<Vec<u8>>, usize)>,
    ended: bool,
}

impl std::io::Read for JetHTTP2BodyReader {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if let Some((current, flow_bytes)) = &mut self.current {
                let read = std::io::Read::read(current, output)?;
                let done = current.position() == current.get_ref().len() as u64;
                if done {
                    let flow_bytes = *flow_bytes;
                    self.current = None;
                    let _ = self.consumed.send((self.stream_id, flow_bytes));
                }
                if read != 0 { return Ok(read); }
            }
            if self.ended { return Ok(0); }
            let part = match self.receiver.try_recv() {
                Ok(part) => Ok(part),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(std::sync::mpsc::RecvError),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    let _wait = JetHTTPSchedulerBlockingWait::enter();
                    self.receiver.recv()
                }
            };
            if jet_scheduler_wait_point_cancelled() { jet_task_deliver_cancel(); }
            match part {
                Ok(JetHTTP2BodyPart::Data { bytes, flow_bytes }) => {
                    self.current = Some((std::io::Cursor::new(bytes), flow_bytes));
                }
                Ok(JetHTTP2BodyPart::End) | Err(_) => self.ended = true,
            }
        }
    }
}

struct JetHTTP2RequestStream {
    sender: std::sync::mpsc::SyncSender<JetHTTP2BodyPart>,
    pending: std::collections::VecDeque<JetHTTP2BodyPart>,
    received: usize,
    unconsumed_flow: usize,
    expected: Option<usize>,
    inbound_closed: bool,
    response_done: bool,
    control: Option<std::sync::Arc<JetTaskControl>>,
    body_started: std::time::Instant,
    trailers: std::sync::Arc<std::sync::Mutex<JetHTTPHeaders>>,
}

impl JetHTTP2RequestStream {
    fn pump(&mut self) {
        while let Some(part) = self.pending.pop_front() {
            match self.sender.try_send(part) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(part)) => {
                    self.pending.push_front(part);
                    break;
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    self.pending.clear();
                    break;
                }
            }
        }
    }
}

impl Drop for JetHTTP2RequestStream {
    fn drop(&mut self) {
        if let Some(control) = &self.control { control.cancel(); }
    }
}

struct JetHTTP2DispatchTasks {
    tasks: Vec<(JetSchedulerJoin<()>, std::sync::Arc<JetTaskControl>)>,
}

impl JetHTTP2DispatchTasks {
    fn push(&mut self, task: JetSchedulerJoin<()>, control: std::sync::Arc<JetTaskControl>) {
        self.tasks.push((task, control));
    }

    fn drain(&mut self) {
        for (task, control) in std::mem::take(&mut self.tasks) {
            control.cancel();
            task.drain();
        }
    }
}

impl Default for JetHTTP2DispatchTasks {
    fn default() -> Self {
        Self { tasks: Vec::new() }
    }
}

impl Drop for JetHTTP2DispatchTasks {
    fn drop(&mut self) {
        self.drain();
    }
}

struct JetHTTP2Outgoing {
    receiver: Option<std::sync::mpsc::Receiver<JetHTTP2ResponsePart>>,
    chunk: Vec<u8>,
    offset: usize,
    expected: Option<usize>,
    sent: usize,
    control: std::sync::Arc<JetTaskControl>,
    source_closer: Option<std::sync::Arc<JetHTTPBodyCloser>>,
    producer: Option<JetSchedulerJoin<()>>,
    trailer_block: Vec<u8>,
}

enum JetHTTP2ResponsePart {
    Chunk(Vec<u8>),
    Error,
    End,
}

impl Drop for JetHTTP2Outgoing {
    fn drop(&mut self) {
        // Disconnect the queue before joining. A producer may be blocked in
        // `send` while the connection is being reset; leaving this receiver
        // alive would make the cancellation join wait forever.
        self.receiver.take();
        self.control.cancel();
        if let Some(closer) = &self.source_closer { closer.close(); }
        if let Some(producer) = self.producer.take() { producer.drain(); }
    }
}

fn jet_http2_write_header_block(
    stream: &mut impl std::io::Write,
    stream_id: u32,
    flags: u8,
    block: &[u8],
    max_frame: usize,
) -> Result<(), String> {
    if block.len() <= max_frame { return jet_http2_write_frame(stream, 1, flags | 0x4, stream_id, block); }
    let mut chunks = block.chunks(max_frame).peekable();
    let first = chunks.next().expect("non-empty HPACK block");
    jet_http2_write_frame(stream, 1, flags & !0x4, stream_id, first)?;
    while let Some(chunk) = chunks.next() {
        jet_http2_write_frame(stream, 9, if chunks.peek().is_none() { 0x4 } else { 0 }, stream_id, chunk)?;
    }
    Ok(())
}

fn jet_http2_start_response(
    stream: &mut impl std::io::Write,
    stream_id: u32,
    response: JetHTTPResponse,
    max_frame: usize,
) -> Result<Option<JetHTTP2Outgoing>, String> {
    let body_forbidden = (100..200).contains(&response.status) || matches!(response.status, 204 | 304);
    let reset_content = response.status == 205;
    let head = response.head_content_length.is_some();
    let trailer_block = jet_http2_encode_trailers(&response.trailers)?;
    let has_trailers = !trailer_block.is_empty();
    if has_trailers && (body_forbidden || reset_content || head) {
        return Err("HTTP/2 response trailers are invalid for this response".to_string());
    }
    let length = if body_forbidden { None } else if reset_content { Some(0) }
        else { response.head_content_length.or_else(|| response.body.length()) };
    let empty = body_forbidden || reset_content || head || length == Some(0);
    let headers = jet_http2_encode_response_headers(&response, length);
    if headers.len() > JET_HTTP2_MAX_HEADER_LIST { return Err("HTTP/2 response header list is too large".to_string()); }
    let mut chunks = if empty {
        None
    } else {
        let chunks = response.body.chunks(JET_HTTP2_MAX_FRAME).map_err(|error| error.to_string())?;
        if !chunks.h2_cancellable() {
            return Err("HTTP/2 streaming response body must be bounded or cancellable".to_string());
        }
        Some(chunks)
    };
    jet_http2_write_header_block(stream, stream_id, if empty && !has_trailers { 0x1 } else { 0 }, &headers, max_frame)?;
    std::io::Write::flush(stream).map_err(|_| "HTTP/2 flush failed".to_string())
        .and_then(|()| if empty && !has_trailers { Ok(None) } else {
            if empty {
                let (sender, receiver) = std::sync::mpsc::sync_channel(1);
                sender.send(JetHTTP2ResponsePart::End).map_err(|_| "HTTP/2 trailer queue failed".to_string())?;
                drop(sender);
                return Ok(Some(JetHTTP2Outgoing {
                    receiver: Some(receiver),
                    chunk: Vec::new(),
                    offset: 0,
                    expected: Some(0),
                    sent: 0,
                    control: JetTaskControl::new(),
                    source_closer: None,
                    producer: None,
                    trailer_block,
                }));
            }
            let mut chunks = chunks.take().expect("non-empty HTTP/2 response has chunks");
            let source_closer = chunks.closer();
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let control = JetTaskControl::new();
            let task_control = control.clone();
            let producer = jet_scheduler_spawn_blocking_with_control(move || loop {
                let part = {
                    let _wait = JetHTTPSchedulerBlockingWait::enter();
                    match chunks.next() {
                        Some(Ok(chunk)) => JetHTTP2ResponsePart::Chunk(chunk),
                        Some(Err(_)) => JetHTTP2ResponsePart::Error,
                        None => JetHTTP2ResponsePart::End,
                    }
                };
                if jet_scheduler_wait_point_cancelled() { break; }
                let done = matches!(part, JetHTTP2ResponsePart::Error | JetHTTP2ResponsePart::End);
                let sent = match sender.try_send(part) {
                    Ok(()) => true,
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => false,
                    Err(std::sync::mpsc::TrySendError::Full(part)) => {
                        let _wait = JetHTTPSchedulerBlockingWait::enter();
                        sender.send(part).is_ok()
                    }
                };
                if done || !sent { break; }
            }, task_control);
            Ok(Some(JetHTTP2Outgoing {
                receiver: Some(receiver),
                chunk: Vec::new(),
                offset: 0,
                expected: length,
                sent: 0,
                control,
                source_closer,
                producer: Some(producer),
                trailer_block,
            }))
        })
}

fn jet_http2_flush_body(
    stream: &mut impl std::io::Write,
    stream_id: u32,
    outgoing: &mut JetHTTP2Outgoing,
    connection_window: &mut i64,
    stream_window: &mut i64,
    max_frame: usize,
) -> Result<bool, String> {
    loop {
        if outgoing.offset == outgoing.chunk.len() {
            match outgoing
                .receiver
                .as_ref()
                .expect("HTTP/2 response receiver missing")
                .try_recv()
            {
                Ok(JetHTTP2ResponsePart::Chunk(chunk)) => {
                    outgoing.chunk = chunk;
                    outgoing.offset = 0;
                    if outgoing.chunk.is_empty() { continue; }
                }
                Ok(JetHTTP2ResponsePart::Error) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    jet_http2_write_frame(stream, 3, 0, stream_id, &2u32.to_be_bytes())?;
                    std::io::Write::flush(stream).map_err(|_| "HTTP/2 flush failed".to_string())?;
                    return Ok(true);
                }
                Ok(JetHTTP2ResponsePart::End) => {
                    if outgoing.expected.is_some_and(|expected| expected != outgoing.sent) {
                        jet_http2_write_frame(stream, 3, 0, stream_id, &2u32.to_be_bytes())?;
                        std::io::Write::flush(stream).map_err(|_| "HTTP/2 flush failed".to_string())?;
                        return Ok(true);
                    }
                    if outgoing.trailer_block.is_empty() {
                        jet_http2_write_frame(stream, 0, 0x1, stream_id, &[])?;
                    } else {
                        jet_http2_write_header_block(stream, stream_id, 0x1, &outgoing.trailer_block, max_frame)?;
                    }
                    std::io::Write::flush(stream).map_err(|_| "HTTP/2 flush failed".to_string())?;
                    return Ok(true);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(false),
            }
        }
        if *connection_window <= 0 || *stream_window <= 0 { return Ok(false); }
        let remaining = outgoing.expected
            .map(|expected| expected.saturating_sub(outgoing.sent))
            .unwrap_or(usize::MAX);
        if remaining == 0 {
            jet_http2_write_frame(stream, 3, 0, stream_id, &2u32.to_be_bytes())?;
            std::io::Write::flush(stream).map_err(|_| "HTTP/2 flush failed".to_string())?;
            return Ok(true);
        }
        let length = (outgoing.chunk.len() - outgoing.offset)
            .min(max_frame).min(*connection_window as usize).min(*stream_window as usize).min(remaining);
        jet_http2_write_frame(stream, 0, 0, stream_id,
            &outgoing.chunk[outgoing.offset..outgoing.offset + length])?;
        outgoing.offset += length;
        outgoing.sent += length;
        *connection_window -= length as i64;
        *stream_window -= length as i64;
    }
}

fn jet_http2_dispatch(
    mux: &JetHTTPMux,
    request: JetHTTPRequest,
) -> Result<JetHTTPResponse, String> {
    Ok(jet_http_mux_dispatch(mux, request).unwrap_or_else(jet_http_srv_error_response))
}

fn jet_http2_queue_response(
    stream: &mut impl std::io::Write,
    stream_id: u32,
    response: JetHTTPResponse,
    outgoing: &mut std::collections::BTreeMap<u32, JetHTTP2Outgoing>,
    stream_windows: &mut std::collections::BTreeMap<u32, i64>,
    connection_window: &mut i64,
    initial_window: i64,
    max_frame: usize,
) -> Result<bool, String> {
    let Some(mut response) = jet_http2_start_response(stream, stream_id, response, max_frame)? else { return Ok(true) };
    let window = stream_windows.entry(stream_id).or_insert(initial_window);
    if !jet_http2_flush_body(stream, stream_id, &mut response, connection_window, window, max_frame)? {
        outgoing.insert(stream_id, response);
        return Ok(false);
    }
    Ok(true)
}

fn jet_http2_serve(
    stream: &mut impl JetHTTP2Transport,
    mux: &JetHTTPMux,
    options: &JetHTTPServerOptions,
    shutdown: &std::sync::atomic::AtomicBool,
    dynamic_grace_ms: Option<&std::sync::atomic::AtomicU64>,
    drain_deadline_ms: Option<&std::sync::atomic::AtomicU64>,
) -> Result<(), String> {
    jet_http2_serve_with_last_stream(
        stream,
        mux,
        options,
        shutdown,
        dynamic_grace_ms,
        drain_deadline_ms,
    ).0
}

fn jet_http2_serve_with_last_stream(
    stream: &mut impl JetHTTP2Transport,
    mux: &JetHTTPMux,
    options: &JetHTTPServerOptions,
    shutdown: &std::sync::atomic::AtomicBool,
    dynamic_grace_ms: Option<&std::sync::atomic::AtomicU64>,
    drain_deadline_ms: Option<&std::sync::atomic::AtomicU64>,
) -> (Result<(), String>, u32) {
    let mut last_stream = 0u32;
    let result = jet_http2_serve_inner(
        stream,
        mux,
        options,
        shutdown,
        dynamic_grace_ms,
        drain_deadline_ms,
        &mut last_stream,
    );
    (result, last_stream)
}

fn jet_http2_serve_inner(
    stream: &mut impl JetHTTP2Transport,
    mux: &JetHTTPMux,
    options: &JetHTTPServerOptions,
    shutdown: &std::sync::atomic::AtomicBool,
    dynamic_grace_ms: Option<&std::sync::atomic::AtomicU64>,
    drain_deadline_ms: Option<&std::sync::atomic::AtomicU64>,
    last_stream: &mut u32,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    stream.jet_http_set_read_timeout(Some(options.read_header_timeout))?;
    stream.jet_http_set_write_timeout(Some(options.write_idle_timeout))?;
    let mut preface = [0u8; 24];
    stream.read_exact(&mut preface).map_err(|_| "HTTP/2 preface ended early".to_string())?;
    if &preface != JET_HTTP2_PREFACE { return Err("HTTP/2 preface is invalid".to_string()); }
    let max_streams = options.workers.saturating_add(options.admission_queue).max(1).min(u32::MAX as usize);
    let mut settings = Vec::with_capacity(18);
    settings.extend_from_slice(&3u16.to_be_bytes());
    settings.extend_from_slice(&(max_streams as u32).to_be_bytes());
    settings.extend_from_slice(&4u16.to_be_bytes());
    settings.extend_from_slice(&65_535u32.to_be_bytes());
    settings.extend_from_slice(&6u16.to_be_bytes());
    settings.extend_from_slice(&(JET_HTTP2_MAX_HEADER_LIST as u32).to_be_bytes());
    jet_http2_write_frame(stream, 4, 0, 0, &settings)?;
    stream.flush().map_err(|_| "HTTP/2 settings write failed".to_string())?;
    let poll_timeout = options.read_idle_timeout.min(std::time::Duration::from_millis(10));
    stream.jet_http_set_read_timeout(Some(poll_timeout.max(std::time::Duration::from_millis(1))))?;
    let mut dispatch_tasks = JetHTTP2DispatchTasks::default();
    let mut requests = std::collections::BTreeMap::<u32, JetHTTP2RequestStream>::new();
    let mut outgoing = std::collections::BTreeMap::<u32, JetHTTP2Outgoing>::new();
    let mut stream_windows = std::collections::BTreeMap::<u32, i64>::new();
    let (completed_tx, completed_rx) = std::sync::mpsc::channel::<(u32, Result<JetHTTPResponse, String>)>();
    let (consumed_tx, consumed_rx) = std::sync::mpsc::channel::<(u32, usize)>();
    let mut decoder = JetHTTP2Hpack::new();
    let mut last_activity = std::time::Instant::now();
    let mut connection_send_window = 65_535i64;
    let mut initial_send_window = 65_535i64;
    let mut peer_max_frame = JET_HTTP2_MAX_FRAME;
    let mut connection_receive_window = 65_535i64;
    let mut stream_receive_windows = std::collections::BTreeMap::<u32, i64>::new();
    let mut saw_client_settings = false;
    let mut going_away = false;
    let mut goaway_sent = false;
    let mut grace_deadline = None;
    loop {
        if shutdown.load(Ordering::Acquire) && !going_away {
            going_away = true;
            let shared = drain_deadline_ms
                .map(|value| value.load(Ordering::Acquire))
                .filter(|value| *value > 0);
            grace_deadline = Some(if let Some(deadline_ms) = shared {
                jet_http_instant_from_unix_ms(deadline_ms)
            } else {
                let grace = dynamic_grace_ms
                    .map(|value| std::time::Duration::from_millis(value.load(Ordering::Acquire)))
                    .unwrap_or(options.shutdown_grace);
                std::time::Instant::now() + grace
            });
            jet_http2_write_frame(stream, 7, 0, 0, &jet_http2_goaway_payload(*last_stream, 0))?;
            stream.flush().map_err(|_| "HTTP/2 GOAWAY write failed".to_string())?;
            goaway_sent = true;
        }
        if grace_deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
            break;
        }
        while let Ok((stream_id, flow_bytes)) = consumed_rx.try_recv() {
            let Some(request) = requests.get_mut(&stream_id) else { continue };
            let increment = flow_bytes.min(request.unconsumed_flow);
            if increment == 0 { continue; }
            request.unconsumed_flow -= increment;
            connection_receive_window += increment as i64;
            *stream_receive_windows.entry(stream_id).or_insert(0) += increment as i64;
            let increment = (increment as u32).to_be_bytes();
            jet_http2_write_frame(stream, 8, 0, 0, &increment)?;
            jet_http2_write_frame(stream, 8, 0, stream_id, &increment)?;
            request.pump();
        }
        while let Ok((stream_id, response)) = completed_rx.try_recv() {
            let Some(request) = requests.get_mut(&stream_id) else { continue };
            request.control = None;
            let response = response.unwrap_or_else(|_| jet_http_srv_response_owned(500, "500 Internal Server Error".to_string()));
            request.response_done = jet_http2_queue_response(
                stream, stream_id, response, &mut outgoing, &mut stream_windows,
                &mut connection_send_window, initial_send_window, peer_max_frame,
            )?;
        }
        for id in outgoing.keys().copied().collect::<Vec<_>>() {
            let Some(mut body) = outgoing.remove(&id) else { continue };
            let stream_window = stream_windows.get_mut(&id)
                .ok_or_else(|| "HTTP/2 response stream lost its window".to_string())?;
            if jet_http2_flush_body(
                stream, id, &mut body, &mut connection_send_window, stream_window, peer_max_frame,
            )? {
                if let Some(request) = requests.get_mut(&id) { request.response_done = true; }
            } else {
                outgoing.insert(id, body);
            }
        }
        let closed = requests.iter()
            .filter_map(|(id, request)| (request.inbound_closed && request.response_done).then_some(*id))
            .collect::<Vec<_>>();
        for id in closed {
            if let Some(request) = requests.remove(&id) {
                if request.unconsumed_flow > 0 {
                    connection_receive_window += request.unconsumed_flow as i64;
                    jet_http2_write_frame(stream, 8, 0, 0, &(request.unconsumed_flow as u32).to_be_bytes())?;
                }
            }
            outgoing.remove(&id);
            stream_windows.remove(&id);
            stream_receive_windows.remove(&id);
        }
        if going_away && requests.is_empty() && outgoing.is_empty() {
            break;
        }
        let now = std::time::Instant::now();
        let expired = requests.iter()
            .filter_map(|(id, request)| (!request.inbound_closed
                && now.duration_since(request.body_started) >= options.read_body_timeout).then_some(*id))
            .collect::<Vec<_>>();
        for id in expired {
            jet_http2_write_frame(stream, 3, 0, id, &8u32.to_be_bytes())?;
            if let Some(request) = requests.remove(&id) {
                if request.unconsumed_flow > 0 {
                    connection_receive_window += request.unconsumed_flow as i64;
                    jet_http2_write_frame(stream, 8, 0, 0, &(request.unconsumed_flow as u32).to_be_bytes())?;
                }
            }
            outgoing.remove(&id);
            stream_windows.remove(&id);
            stream_receive_windows.remove(&id);
        }
        let frame = match jet_http2_read_frame(stream) {
            Ok(frame) => { last_activity = std::time::Instant::now(); frame }
            Err(error) if error == "HTTP/2 read timed out" => {
                let now = std::time::Instant::now();
                if going_away { continue; }
                if now.duration_since(last_activity) >= options.read_idle_timeout { return Ok(()); }
                continue;
            }
            Err(error) if error.contains("ended early") => return Ok(()),
            Err(error) => return Err(error),
        };
        if !saw_client_settings && !(frame.kind == 4 && frame.stream == 0 && frame.flags & 0x1 == 0) {
            return Err("HTTP/2 client SETTINGS must be the first frame".to_string());
        }
        match frame.kind {
            0 => {
                if frame.stream == 0 { return Err("HTTP/2 DATA uses stream zero".to_string()); }
                // RFC 7540 §6.8: after GOAWAY, frames for streams > last-stream-id
                // (and other unknown streams) may be discarded. Never abort drain
                // with PROTOCOL_ERROR just because the peer kept writing.
                if !requests.contains_key(&frame.stream) {
                    if going_away {
                        continue;
                    }
                    return Err("HTTP/2 DATA has no open request".to_string());
                }
                let request = requests.get_mut(&frame.stream).expect("checked above");
                if request.inbound_closed { return Err("HTTP/2 DATA follows end of stream".to_string()); }
                let (start, padding) = if frame.flags & 0x8 != 0 {
                    (1, usize::from(*frame.payload.first().ok_or_else(|| "HTTP/2 padded DATA is empty".to_string())?))
                } else { (0, 0) };
                if start + padding > frame.payload.len() { return Err("HTTP/2 DATA padding is invalid".to_string()); }
                let data = &frame.payload[start..frame.payload.len() - padding];
                let flow_bytes = frame.payload.len();
                connection_receive_window -= flow_bytes as i64;
                let receive_window = stream_receive_windows.entry(frame.stream).or_insert(65_535);
                *receive_window -= flow_bytes as i64;
                if connection_receive_window < 0 || *receive_window < 0 { return Err("HTTP/2 receive flow-control window exceeded".to_string()); }
                request.received = request.received.saturating_add(data.len());
                request.unconsumed_flow = request.unconsumed_flow.saturating_add(flow_bytes);
                if request.received > options.max_body_bytes { return Err("HTTP/2 request body is too large".to_string()); }
                if flow_bytes > 0 { request.pending.push_back(JetHTTP2BodyPart::Data { bytes: data.to_vec(), flow_bytes }); }
                if frame.flags & 0x1 != 0 {
                    if request.expected.is_some_and(|expected| expected != request.received) {
                        return Err("HTTP/2 body does not match content-length".to_string());
                    }
                    request.inbound_closed = true;
                    request.pending.push_back(JetHTTP2BodyPart::End);
                }
                request.pump();
            }
            1 => {
                let trailing = requests.contains_key(&frame.stream);
                if going_away && !trailing
                    || frame.stream == 0
                    || frame.stream % 2 == 0
                    || !trailing && frame.stream <= *last_stream
                {
                    if going_away && frame.stream != 0 {
                        jet_http2_write_frame(stream, 3, 0, frame.stream, &7u32.to_be_bytes())?;
                        stream.flush().map_err(|_| "HTTP/2 RST write failed".to_string())?;
                        continue;
                    }
                    return Err("HTTP/2 HEADERS stream id is invalid".to_string());
                }
                if trailing && requests.get(&frame.stream).is_some_and(|request| request.inbound_closed) {
                    return Err("HTTP/2 trailing HEADERS follows end of stream".to_string());
                }
                if !trailing {
                    if requests.len() >= max_streams { return Err("HTTP/2 concurrent stream limit exceeded".to_string()); }
                    *last_stream = frame.stream;
                }
                let mut offset = 0usize;
                let padding = if frame.flags & 0x8 != 0 { offset = 1; usize::from(*frame.payload.first().ok_or_else(|| "HTTP/2 padded HEADERS is empty".to_string())?) } else { 0 };
                if frame.flags & 0x20 != 0 { offset += 5; }
                if offset + padding > frame.payload.len() { return Err("HTTP/2 HEADERS padding is invalid".to_string()); }
                let mut block = frame.payload[offset..frame.payload.len() - padding].to_vec();
                if frame.flags & 0x4 == 0 {
                    stream.jet_http_set_read_timeout(Some(options.read_header_timeout))?;
                    loop {
                        let continuation = jet_http2_read_frame(stream)?;
                        if continuation.kind != 9 || continuation.stream != frame.stream { return Err("HTTP/2 header block was interrupted".to_string()); }
                        block.extend_from_slice(&continuation.payload);
                        if block.len() > JET_HTTP2_MAX_HEADER_LIST { return Err("HTTP/2 header block is too large".to_string()); }
                        if continuation.flags & 0x4 != 0 { break; }
                    }
                    stream.jet_http_set_read_timeout(Some(poll_timeout.max(std::time::Duration::from_millis(1))))?;
                }
                let headers = jet_http2_decode_headers(&mut decoder, &block)?;
                if trailing {
                    if frame.flags & 0x1 == 0 {
                        return Err("HTTP/2 trailing HEADERS must end the stream".to_string());
                    }
                    let trailers = jet_http2_request_trailers(headers)?;
                    let request = requests.get_mut(&frame.stream).expect("checked above");
                    if request.expected.is_some_and(|expected| expected != request.received) {
                        return Err("HTTP/2 body does not match content-length".to_string());
                    }
                    *request.trailers.lock().map_err(|_| "HTTP/2 trailer store failed".to_string())? = trailers;
                    request.inbound_closed = true;
                    request.pending.push_back(JetHTTP2BodyPart::End);
                    request.pump();
                    continue;
                }
                stream_receive_windows.insert(frame.stream, 65_535);
                stream_windows.insert(frame.stream, initial_send_window);
                let (body_tx, body_rx) = std::sync::mpsc::sync_channel(1);
                let body = JetHTTPBody::reader(JetHTTP2BodyReader {
                    receiver: body_rx,
                    consumed: consumed_tx.clone(),
                    stream_id: frame.stream,
                    current: None,
                    ended: false,
                }, None);
                let inbound_closed = frame.flags & 0x1 != 0;
                let trailers = std::sync::Arc::new(std::sync::Mutex::new(JetHTTPHeaders::new()));
                let (request, expected) = jet_http2_request_with_trailers(headers, body, trailers.clone(), inbound_closed)?;
                if expected.is_some_and(|length| length > options.max_body_bytes) {
                    return Err("HTTP/2 request body is too large".to_string());
                }
                if inbound_closed && expected.is_some_and(|length| length != 0) {
                    return Err("HTTP/2 body does not match content-length".to_string());
                }
                let control = JetTaskControl::new();
                let task_control = control.clone();
                let task_mux = mux.clone();
                let task_completed = completed_tx.clone();
                let stream_id = frame.stream;
                let task = jet_scheduler_spawn_blocking_with_control(move || {
                    let result = jet_http2_dispatch(&task_mux, request);
                    let _ = task_completed.send((stream_id, result));
                }, task_control);
                dispatch_tasks.push(task, control.clone());
                let mut request = JetHTTP2RequestStream {
                    sender: body_tx,
                    pending: std::collections::VecDeque::new(),
                    received: 0,
                    unconsumed_flow: 0,
                    expected,
                    inbound_closed,
                    response_done: false,
                    control: Some(control),
                    body_started: std::time::Instant::now(),
                    trailers,
                };
                if inbound_closed { request.pending.push_back(JetHTTP2BodyPart::End); }
                request.pump();
                requests.insert(frame.stream, request);
            }
            2 if frame.stream != 0 && frame.payload.len() == 5 => {}
            3 if frame.stream != 0 && frame.payload.len() == 4 => {
                if let Some(request) = requests.remove(&frame.stream) {
                    if request.unconsumed_flow > 0 {
                        connection_receive_window += request.unconsumed_flow as i64;
                        jet_http2_write_frame(stream, 8, 0, 0, &(request.unconsumed_flow as u32).to_be_bytes())?;
                    }
                }
                outgoing.remove(&frame.stream);
                stream_windows.remove(&frame.stream);
                stream_receive_windows.remove(&frame.stream);
            }
            4 => {
                if frame.stream != 0 || frame.payload.len() % 6 != 0 || frame.flags & 0x1 != 0 && !frame.payload.is_empty() { return Err("HTTP/2 SETTINGS is malformed".to_string()); }
                if frame.flags & 0x1 == 0 {
                    saw_client_settings = true;
                    for setting in frame.payload.chunks_exact(6) {
                        let id = u16::from_be_bytes([setting[0], setting[1]]);
                        let value = u32::from_be_bytes([setting[2], setting[3], setting[4], setting[5]]);
                        match id {
                            2 if value > 1 => return Err("HTTP/2 ENABLE_PUSH setting is invalid".to_string()),
                            4 if value > 0x7fff_ffff => return Err("HTTP/2 initial window is invalid".to_string()),
                            4 => {
                                let change = i64::from(value) - initial_send_window;
                                initial_send_window = i64::from(value);
                                for window in stream_windows.values_mut() { *window += change; }
                            }
                            5 if !(16_384..=16_777_215).contains(&value) => return Err("HTTP/2 maximum frame size is invalid".to_string()),
                            5 => peer_max_frame = value as usize,
                            _ => {}
                        }
                    }
                    jet_http2_write_frame(stream, 4, 0x1, 0, &[])?;
                }
            }
            6 => {
                if frame.stream != 0 || frame.payload.len() != 8 { return Err("HTTP/2 PING is malformed".to_string()); }
                if frame.flags & 0x1 == 0 { jet_http2_write_frame(stream, 6, 0x1, 0, &frame.payload)?; }
            }
            7 => {
                if frame.stream != 0 || frame.payload.len() < 8 { return Err("HTTP/2 GOAWAY is malformed".to_string()); }
                return Ok(());
            }
            8 => {
                if frame.payload.len() != 4 { return Err("HTTP/2 WINDOW_UPDATE is malformed".to_string()); }
                let increment = u32::from_be_bytes(frame.payload[..4].try_into().unwrap()) & 0x7fff_ffff;
                if increment == 0 { return Err("HTTP/2 WINDOW_UPDATE is malformed".to_string()); }
                if frame.stream != 0 && !stream_windows.contains_key(&frame.stream) { continue; }
                let window = if frame.stream == 0 { &mut connection_send_window }
                    else { stream_windows.get_mut(&frame.stream).ok_or_else(|| "HTTP/2 WINDOW_UPDATE uses an idle stream".to_string())? };
                *window = window.checked_add(i64::from(increment)).filter(|value| *value <= 0x7fff_ffff)
                    .ok_or_else(|| "HTTP/2 flow-control window overflow".to_string())?;
                let ids = if frame.stream == 0 { outgoing.keys().copied().collect::<Vec<_>>() } else { vec![frame.stream] };
                for id in ids {
                    let Some(mut body) = outgoing.remove(&id) else { continue };
                    let stream_window = stream_windows.get_mut(&id).ok_or_else(|| "HTTP/2 response stream lost its window".to_string())?;
                    if jet_http2_flush_body(stream, id, &mut body, &mut connection_send_window, stream_window, peer_max_frame)? {
                        if let Some(request) = requests.get_mut(&id) { request.response_done = true; }
                    } else {
                        outgoing.insert(id, body);
                    }
                }
            }
            9 => {
                // CONTINUATION is only legal mid header-block (consumed inline under
                // HEADERS). An orphan frame on an unknown/idle stream after GOAWAY
                // must be discarded like post-GOAWAY DATA — never abort drain.
                if going_away {
                    continue;
                }
                return Err("HTTP/2 CONTINUATION has no open header block".to_string());
            }
            _ => {}
        }
    }
    for request in requests.values() {
        if let Some(control) = &request.control { control.cancel(); }
    }
    drop(requests);
    dispatch_tasks.drain();
    if !goaway_sent {
        jet_http2_write_frame(stream, 7, 0, 0, &jet_http2_goaway_payload(*last_stream, 0))?;
    }
    let _ = stream.flush();
    Ok(())
}

struct JetHTTPContinueReader<R> {
    inner: R,
    stream: Option<std::net::TcpStream>,
    timeout_stream: Option<std::net::TcpStream>,
    deadline: std::time::Instant,
}

fn jet_http_body_deadline_remaining(
    deadline: std::time::Instant,
) -> std::io::Result<std::time::Duration> {
    deadline
        .checked_duration_since(std::time::Instant::now())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "request body timed out"))
}

impl<R: std::io::Read> std::io::Read for JetHTTPContinueReader<R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let remaining = jet_http_body_deadline_remaining(self.deadline)?;
        if let Some(stream) = self.timeout_stream.as_ref() {
            stream.set_read_timeout(Some(remaining))?;
        }
        if let Some(mut stream) = self.stream.take() {
            std::io::Write::write_all(&mut stream, b"HTTP/1.1 100 Continue\r\n\r\n")?;
            std::io::Write::flush(&mut stream)?;
        }
        std::io::Read::read(&mut self.inner, output)
    }
}

struct JetHTTPChunkedSocketReader {
    stream: std::net::TcpStream,
    deadline: std::time::Instant,
    remaining: usize,
    need_crlf: bool,
    done: bool,
    framing: usize,
    decoded: usize,
    limit: usize,
    trailer_names: Vec<String>,
    trailers: std::sync::Arc<std::sync::Mutex<JetHTTPHeaders>>,
}

impl JetHTTPChunkedSocketReader {
    fn invalid(message: &'static str) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, message)
    }

    fn read_exact_framing(&mut self, bytes: &mut [u8]) -> std::io::Result<()> {
        for byte in bytes {
            let remaining = jet_http_body_deadline_remaining(self.deadline)?;
            self.stream.set_read_timeout(Some(remaining))?;
            std::io::Read::read_exact(&mut self.stream, std::slice::from_mut(byte))?;
            self.framing = self.framing.saturating_add(1);
            if self.framing > JET_HTTP_MAX_CHUNK_FRAMING_BYTES {
                return Err(std::io::Error::new(std::io::ErrorKind::OutOfMemory, "chunk framing is too large"));
            }
        }
        Ok(())
    }

    fn next_size(&mut self) -> std::io::Result<usize> {
        let mut line = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            self.read_exact_framing(&mut byte)?;
            line.push(byte[0]);
            if line.ends_with(b"\r\n") {
                line.truncate(line.len() - 2);
                return jet_http_chunk_size(&line).map_err(|error| {
                    if error.status == 413 {
                        std::io::Error::new(std::io::ErrorKind::OutOfMemory, error.message)
                    } else {
                        Self::invalid("chunk size is malformed")
                    }
                });
            }
        }
    }

    fn read_trailers(&mut self) -> std::io::Result<()> {
        let mut trailers = JetHTTPHeaders::new();
        loop {
            let mut line = Vec::new();
            loop {
                let mut byte = [0u8; 1];
                self.read_exact_framing(&mut byte)?;
                line.push(byte[0]);
                if line.ends_with(b"\r\n") {
                    line.truncate(line.len() - 2);
                    break;
                }
            }
            if line.is_empty() {
                *self
                    .trailers
                    .lock()
                    .map_err(|_| Self::invalid("request trailer lock failed"))? = trailers;
                return Ok(());
            }
            jet_http_parse_trailer_line(&line, &self.trailer_names, &mut trailers).map_err(
                |error| {
                    if error.status == 413 || error.status == 431 {
                        std::io::Error::new(std::io::ErrorKind::OutOfMemory, error.message)
                    } else {
                        Self::invalid(error.message)
                    }
                },
            )?;
        }
    }
}

impl std::io::Read for JetHTTPChunkedSocketReader {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if self.done || output.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            if self.need_crlf {
                let mut crlf = [0u8; 2];
                self.read_exact_framing(&mut crlf)?;
                if crlf != *b"\r\n" {
                    return Err(Self::invalid("chunk data is not followed by CRLF"));
                }
                self.need_crlf = false;
            }
            self.remaining = self.next_size()?;
            self.decoded = self.decoded.checked_add(self.remaining)
                .filter(|decoded| *decoded <= self.limit)
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::OutOfMemory, "request body is too large"))?;
            if self.remaining == 0 {
                self.read_trailers()?;
                self.done = true;
                return Ok(0);
            }
        }
        let wanted = output.len().min(self.remaining);
        let remaining = jet_http_body_deadline_remaining(self.deadline)?;
        self.stream.set_read_timeout(Some(remaining))?;
        let read = self.stream.read(&mut output[..wanted])?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "chunk ended early",
            ));
        }
        self.remaining -= read;
        self.need_crlf = self.remaining == 0;
        Ok(read)
    }
}

fn jet_http_srv_read_streaming(
    stream: &mut std::net::TcpStream,
    options: &JetHTTPServerOptions,
    keep_alive: bool,
    shutdown: Option<&std::sync::atomic::AtomicBool>,
) -> Result<Option<(JetHTTPRequest, String)>, JetHTTPReadError> {
    use std::io::Read;
    use std::sync::atomic::Ordering;
    const MAX_HEADER_BYTES: usize = 32 * 1024;
    let started = std::time::Instant::now();
    let timeout = if keep_alive {
        JET_HTTP_KEEPALIVE_IDLE_TIMEOUT
    } else {
        options.read_header_timeout
    };
    let read_timeout = if keep_alive && shutdown.is_some() {
        std::time::Duration::from_millis(20)
    } else {
        timeout
    };
    stream.set_read_timeout(Some(read_timeout)).map_err(|_| JetHTTPReadError {
        status: 400,
        message: "request read failed",
    })?;
    let mut header = [0u8; MAX_HEADER_BYTES + 1];
    let mut header_len = 0usize;
    loop {
        if shutdown.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Ok(None);
        }
        let deadline = if keep_alive && header_len > 0 {
            options.read_idle_timeout
        } else {
            timeout
        };
        if started.elapsed() >= deadline {
            return if keep_alive && header_len == 0 {
                Ok(None)
            } else {
                Err(JetHTTPReadError { status: 408, message: "request timed out" })
            };
        }
        match stream.peek(&mut header) {
            Ok(0) if header_len == 0 => return Ok(None),
            Ok(0) => return Err(JetHTTPReadError {
                status: 400,
                message: "request headers ended early",
            }),
            Ok(available) if available > header_len => {
                header_len = available;
                if let Some(end) = jet_http_header_end(&header[..header_len]) {
                    let request_header_len = end + 4;
                    if request_header_len > MAX_HEADER_BYTES {
                        return Err(JetHTTPReadError {
                            status: 431,
                            message: "request headers are too large",
                        });
                    }
                    header_len = request_header_len;
                    break;
                }
                if header_len > MAX_HEADER_BYTES {
                    return Err(JetHTTPReadError {
                        status: 431,
                        message: "request headers are too large",
                    });
                }
            }
            Ok(_) => std::thread::sleep(std::time::Duration::from_millis(1)),
            Err(_) => return Err(JetHTTPReadError { status: 400, message: "request read failed" }),
        }
    }
    if let Err(error) = stream.read_exact(&mut header[..header_len]) {
        return Err(if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
            JetHTTPReadError { status: 408, message: "request timed out" }
        } else {
            JetHTTPReadError { status: 400, message: "request read failed" }
        });
    }
    let header_end = header_len - 4;
    let head = jet_http_validate_headers(&header[..header_end])?;
    let body_already_arrived = if head.expect_continue {
        let _ = stream.set_nonblocking(true);
        let mut byte = [0u8; 1];
        let arrived = stream.peek(&mut byte).is_ok_and(|read| read > 0);
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(options.read_body_timeout));
        arrived
    } else {
        false
    };
    let JetHTTPRequestHead {
        framing,
        expect_continue,
        method,
        version,
        target,
        mut headers,
        content_encoding_layers,
        trailer_names,
    } = head;
    if matches!(framing, JetHTTPRequestFraming::ContentLength(0)) && content_encoding_layers == 0 {
        let trailers = std::sync::Arc::new(std::sync::Mutex::new(JetHTTPHeaders::new()));
        return Ok(Some((
            JetHTTPRequest::server_body_with_trailers(
                &method,
                target,
                JetHTTPBody::empty(),
                headers,
                trailers,
            ),
            version,
        )));
    }
    stream.set_read_timeout(Some(options.read_body_timeout)).map_err(|_| JetHTTPReadError {
        status: 400,
        message: "request read failed",
    })?;
    let body_stream = stream.try_clone().map_err(|_| JetHTTPReadError {
        status: 500,
        message: "request stream could not be cloned",
    })?;
    let timeout_stream = body_stream.try_clone().map_err(|_| JetHTTPReadError {
        status: 500,
        message: "request timeout stream could not be cloned",
    })?;
    let body_deadline = std::time::Instant::now() + options.read_body_timeout;
    let continue_stream = if expect_continue && !body_already_arrived {
        Some(stream.try_clone().map_err(|_| JetHTTPReadError {
            status: 500,
            message: "continue response stream could not be cloned",
        })?)
    } else {
        None
    };
    let trailers = std::sync::Arc::new(std::sync::Mutex::new(JetHTTPHeaders::new()));
    let body = match framing {
        JetHTTPRequestFraming::ContentLength(length) => {
            if length > options.max_body_bytes {
                return Err(JetHTTPReadError { status: 413, message: "request body is too large" });
            }
            JetHTTPBody::reader(JetHTTPContinueReader {
                inner: body_stream.take(length as u64),
                stream: continue_stream,
                timeout_stream: Some(timeout_stream),
                deadline: body_deadline,
            }, Some(length))
        }
        JetHTTPRequestFraming::Chunked => JetHTTPBody::reader(
            JetHTTPContinueReader {
                inner: JetHTTPChunkedSocketReader {
                    stream: body_stream,
                    remaining: 0,
                    need_crlf: false,
                    done: false,
                    framing: 0,
                    decoded: 0,
                    limit: options.max_body_bytes,
                    trailer_names,
                    trailers: trailers.clone(),
                    deadline: body_deadline,
                },
                stream: continue_stream,
                timeout_stream: Some(timeout_stream),
                deadline: body_deadline,
            },
            None,
        ),
    };
    let body = jet_http_decode_request_body(
        body,
        content_encoding_layers,
        options.max_body_bytes,
    )?;
    if content_encoding_layers > 0 {
        headers.remove("content-encoding");
        headers.remove("content-length");
    }
    Ok(Some((
        JetHTTPRequest::server_body_with_trailers(&method, target, body, headers, trailers),
        version,
    )))
}

fn jet_http_mux_serve_once_raw(addr: &String, mux: JetHTTPMux) -> Result<(), String> {
    jet_http_mux_validate(&mux)?;
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    jet_http_mux_serve_once_listener_raw(&JetTCPListener { inner: listener }, &mux)
}

fn jet_http_mux_serve_once(addr: &String, mux: JetHTTPMux) -> Result<(), JetHTTPError> {
    jet_http_mux_serve_once_raw(addr, mux).map_err(|operation| JetHTTPError::IO { operation })
}

fn jet_http_mux_serve_once_listener_raw(
    listener: &JetTCPListener,
    mux: &JetHTTPMux,
) -> Result<(), String> {
    use std::io::Write;
    let route_cache = mux.route_cache();
    if let Some(error) = route_cache.validation_error.clone() {
        return Err(error);
    }
    let (mut stream, _peer) = jet_http_accept_once(listener, std::time::Duration::from_secs(5))?;
    let (req, version) = match jet_http_srv_read_streaming(
        &mut stream,
        &JetHTTPServerOptions::safe(),
        false,
        None,
    ) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(error) => {
            stream
                .write_all(jet_http_srv_read_error_response(&error).as_bytes())
                .map_err(|e| format!("http write failed: {}", e))?;
            return Ok(());
        }
    };
    let resp = jet_scheduler_with_root_control(|| {
        jet_http_mux_dispatch_cached(mux, req, &route_cache)
    })
    .unwrap_or_else(jet_http_srv_error_response);
    jet_http_srv_write_response(&mut stream, &resp, &version, true)
        .map_err(|error| error.to_string())
}

fn jet_http_mux_serve_once_listener(
    listener: &JetTCPListener,
    mux: &JetHTTPMux,
) -> Result<(), JetHTTPError> {
    jet_http_mux_serve_once_listener_raw(listener, mux)
        .map_err(|operation| JetHTTPError::IO { operation })
}


fn jet_http_accept_once(
    listener: &JetTCPListener,
    timeout: std::time::Duration,
) -> Result<(std::net::TcpStream, std::net::SocketAddr), String> {
    let started = std::time::Instant::now();
    loop {
        match listener.inner.accept() {
            Ok(connection) => return Ok(connection),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= timeout {
                    return Err("HTTP serve_once accept timed out".to_string());
                }
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
            Err(error) => return Err(format!("accept failed: {error}")),
        }
    }
}

fn jet_http_srv_read(stream: &mut std::net::TcpStream) -> Result<Vec<u8>, JetHTTPReadError> {
    jet_http_srv_read_with_limits(stream, &JetHTTPServerOptions::safe())
}

fn jet_http_srv_read_with_limits(stream: &mut std::net::TcpStream, options: &JetHTTPServerOptions) -> Result<Vec<u8>, JetHTTPReadError> {
    let mut pending = Vec::new();
    jet_http_srv_read_buffered(stream, options, &mut pending, false, None)?.ok_or(JetHTTPReadError {
        status: 400,
        message: "request ended before its declared framing was complete",
    })
}

fn jet_http_srv_read_buffered(
    stream: &mut std::net::TcpStream,
    options: &JetHTTPServerOptions,
    pending: &mut Vec<u8>,
    keep_alive: bool,
    shutdown: Option<&std::sync::atomic::AtomicBool>,
) -> Result<Option<Vec<u8>>, JetHTTPReadError> {
    use std::io::{Read, Write};
    use std::sync::atomic::Ordering;
    const MAX_HEADER_BYTES: usize = 32 * 1024;
    const SHUTDOWN_POLL: std::time::Duration = std::time::Duration::from_millis(20);
    let mut buf = [0u8; 8192];
    let mut reading_body = false;
    let mut continue_sent = false;
    let mut chunked = None;
    let mut body_deadline = None;
    let started = std::time::Instant::now();
    let mut header_deadline = (!keep_alive || !pending.is_empty()).then(|| started + options.read_header_timeout);
    let mut idle_deadline = started
        + if header_deadline.is_some() {
            options.read_idle_timeout
        } else {
            JET_HTTP_KEEPALIVE_IDLE_TIMEOUT
        };
    let mut parsed_head: Option<JetHTTPRequestHead> = None;
    loop {
        if let Some(header_end) = jet_http_header_end(pending) {
            if header_end > MAX_HEADER_BYTES {
                return Err(JetHTTPReadError { status: 431, message: "request headers are too large" });
            }
            if parsed_head.is_none() {
                parsed_head = Some(jet_http_validate_headers(&pending[..header_end])?);
            }
            let head = parsed_head.as_mut().expect("validated HTTP request head");
            if !reading_body {
                reading_body = true;
                let deadline = std::time::Instant::now() + options.read_body_timeout;
                body_deadline = Some(deadline);
                idle_deadline = std::time::Instant::now()
                    + options.read_body_timeout.min(options.read_idle_timeout);
            }
            let body_start = header_end + 4;
            let request_end = match head.framing {
                JetHTTPRequestFraming::ContentLength(content_len) => {
                    if content_len > options.max_body_bytes {
                        return Err(JetHTTPReadError { status: 413, message: "request body is too large" });
                    }
                    let request_end = body_start + content_len;
                    (pending.len() >= request_end).then_some(request_end)
                }
                JetHTTPRequestFraming::Chunked => chunked
                    .get_or_insert_with(|| {
                        JetHTTPChunkState::new(
                            options.max_body_bytes,
                            std::mem::take(&mut head.trailer_names),
                        )
                    })
                    .advance(&pending[body_start..])?
                    .map(|body_end| body_start + body_end),
            };
            if let Some(request_end) = request_end {
                return Ok(Some(pending.drain(..request_end).collect()));
            }
            if head.expect_continue && !continue_sent {
                stream
                    .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                    .map_err(|_| JetHTTPReadError {
                        status: 400,
                        message: "continue response write failed",
                    })?;
                continue_sent = true;
            }
        } else if pending.len() > MAX_HEADER_BYTES {
            return Err(JetHTTPReadError { status: 431, message: "request headers are too large" });
        }

        let deadline = if reading_body {
            body_deadline
                .unwrap_or(idle_deadline)
                .min(idle_deadline)
        } else if let Some(deadline) = header_deadline {
            deadline.min(idle_deadline)
        } else {
            idle_deadline
        };
        let timeout = deadline.saturating_duration_since(std::time::Instant::now());
        if timeout.is_zero() {
            let between_requests = keep_alive && pending.is_empty() && header_deadline.is_none();
            if between_requests {
                return Ok(None);
            }
            return Err(JetHTTPReadError { status: 408, message: "request timed out" });
        }
        let socket_timeout = if shutdown.is_some() { timeout.min(SHUTDOWN_POLL) } else { timeout };
        stream.set_read_timeout(Some(socket_timeout)).map_err(|_| JetHTTPReadError { status: 400, message: "request read failed" })?;
        let n = match stream.read(&mut buf) {
            Ok(n) => n,
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                let between_requests = keep_alive && pending.is_empty() && header_deadline.is_none();
                if between_requests && shutdown.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                    return Ok(None);
                }
                if std::time::Instant::now() < deadline {
                    continue;
                }
                if between_requests {
                    return Ok(None);
                }
                return Err(JetHTTPReadError { status: 408, message: "request timed out" });
            }
            Err(_) => return Err(JetHTTPReadError { status: 400, message: "request read failed" }),
        };
        if n == 0 {
            if pending.is_empty() {
                return Ok(None);
            }
            return Err(JetHTTPReadError {
                status: 400,
                message: "request ended before its declared framing was complete",
            });
        }
        pending.extend_from_slice(&buf[..n]);
        let now = std::time::Instant::now();
        if header_deadline.is_none() {
            header_deadline = Some(now + options.read_header_timeout);
        }
        idle_deadline = now
            + if reading_body {
                options.read_body_timeout.min(options.read_idle_timeout)
            } else {
                options.read_idle_timeout
            };
    }
}

fn jet_http_srv_request_version(raw: &[u8]) -> &str {
    raw.windows(2)
        .position(|bytes| bytes == b"\r\n")
        .and_then(|end| std::str::from_utf8(&raw[..end]).ok())
        .and_then(|line| line.split(' ').nth(2))
        .filter(|version| matches!(*version, "HTTP/1.0" | "HTTP/1.1"))
        .unwrap_or("HTTP/1.1")
}

fn jet_http_connection_options(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(jet_http_trim_ows).filter(|token| !token.is_empty())
}

fn jet_http_parse_content_length(
    value: &str,
    mut expected: Option<usize>,
) -> Result<usize, JetHTTPReadError> {
    for member in value.split(',').map(jet_http_trim_ows) {
        if member.is_empty() || !member.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(JetHTTPReadError { status: 400, message: "content-length is malformed" });
        }
        let parsed = member.parse::<usize>()
            .map_err(|_| JetHTTPReadError { status: 400, message: "content-length is malformed" })?;
        if expected.is_some_and(|old| old != parsed) {
            return Err(JetHTTPReadError { status: 400, message: "conflicting content-length headers" });
        }
        expected = Some(parsed);
    }
    expected.ok_or(JetHTTPReadError { status: 400, message: "content-length is malformed" })
}

fn jet_http_srv_request_keep_alive(version: &str, headers: &JetHTTPHeaders) -> bool {
    let mut close = false;
    let mut keep_alive = false;
    for value in headers.all("connection") {
        for token in jet_http_connection_options(value) {
            close |= token.eq_ignore_ascii_case("close");
            keep_alive |= token.eq_ignore_ascii_case("keep-alive");
        }
    }
    !close && (version == "HTTP/1.1" || (version == "HTTP/1.0" && keep_alive))
}

fn jet_http_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn jet_http_trim_ows(value: &str) -> &str {
    value.trim_matches(|character| matches!(character, ' ' | '\t'))
}

fn jet_http_trim_ows_start(value: &str) -> &str {
    value.trim_start_matches(|character| matches!(character, ' ' | '\t'))
}

fn jet_http_trailer_name_allowed(name: &str) -> bool {
    !matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "connection"
            | "content-encoding"
            | "content-length"
            | "content-range"
            | "content-type"
            | "cookie"
            | "host"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "set-cookie"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn jet_http_parse_trailer_names(values: &[String]) -> Result<Vec<String>, JetHTTPReadError> {
    let mut names = Vec::new();
    for name in values
        .iter()
        .flat_map(|value| value.split(','))
        .map(jet_http_trim_ows)
    {
        if !JetHTTPHeaders::valid_name(name) || !jet_http_trailer_name_allowed(name) {
            return Err(JetHTTPReadError {
                status: 400,
                message: "trailer declaration is malformed or forbidden",
            });
        }
        if !names.iter().any(|old: &String| old.eq_ignore_ascii_case(name)) {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

fn jet_http_parse_trailer_line(
    line: &[u8],
    declared: &[String],
    trailers: &mut JetHTTPHeaders,
) -> Result<(), JetHTTPReadError> {
    if trailers.entries.len() >= 100 || line.starts_with(b" ") || line.starts_with(b"\t") {
        return Err(JetHTTPReadError {
            status: 431,
            message: "request has too many or folded trailers",
        });
    }
    let line = std::str::from_utf8(line).map_err(|_| JetHTTPReadError {
        status: 400,
        message: "request trailer is not valid UTF-8",
    })?;
    let (name, value) = line.split_once(':').ok_or(JetHTTPReadError {
        status: 400,
        message: "request trailer is malformed",
    })?;
    if !jet_http_trailer_name_allowed(name)
        || !declared.iter().any(|declared| declared.eq_ignore_ascii_case(name))
    {
        return Err(JetHTTPReadError {
            status: 400,
            message: "request trailer was forbidden or undeclared",
        });
    }
    trailers
        .append(name, jet_http_trim_ows(value))
        .map_err(|_| JetHTTPReadError {
            status: 400,
            message: "request trailer is malformed",
        })
}

fn jet_http_host_port_valid(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

fn jet_http_reg_name_valid(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    let bytes = host.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b';' | b'=')
        {
            index += 1;
        } else if byte == b'%'
            && bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            index += 3;
        } else {
            return false;
        }
    }
    true
}

fn jet_http_ipv_future_valid(host: &str) -> bool {
    let Some((version, address)) = host.get(1..).and_then(|rest| rest.split_once('.')) else {
        return false;
    };
    (host.starts_with('v') || host.starts_with('V'))
        && !version.is_empty()
        && version.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !address.is_empty()
        && address.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':')
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetHTTPAuthority {
    host: String,
    port: Option<u16>,
}

fn jet_http_normalize_reg_name(host: &str) -> String {
    let mut normalized = String::with_capacity(host.len());
    let bytes = host.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            normalized.push('%');
            normalized.push((bytes[index + 1] as char).to_ascii_uppercase());
            normalized.push((bytes[index + 2] as char).to_ascii_uppercase());
            index += 3;
        } else {
            normalized.push((bytes[index] as char).to_ascii_lowercase());
            index += 1;
        }
    }
    normalized
}

fn jet_http_parse_authority(value: &str) -> Option<JetHTTPAuthority> {
    let authority = jet_http_trim_ows(value);
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, suffix) = bracketed.split_once(']')?;
        let host = if let Ok(address) = host.parse::<std::net::Ipv6Addr>() {
            address.to_string()
        } else if jet_http_ipv_future_valid(host) {
            host.to_ascii_lowercase()
        } else {
            return None;
        };
        let port = if suffix.is_empty() {
            None
        } else {
            let port = suffix.strip_prefix(':')?;
            if !jet_http_host_port_valid(port) {
                return None;
            }
            Some(port.parse().unwrap())
        };
        return Some(JetHTTPAuthority { host: format!("[{host}]"), port });
    }
    let mut parts = authority.split(':');
    let host = parts.next().unwrap_or("");
    let port = parts.next();
    if parts.next().is_some() || !jet_http_reg_name_valid(host) {
        return None;
    }
    let port = match port {
        Some(port) if jet_http_host_port_valid(port) => Some(port.parse().unwrap()),
        Some(_) => return None,
        None => None,
    };
    Some(JetHTTPAuthority {
        host: jet_http_normalize_reg_name(host),
        port,
    })
}

fn jet_http_path_query_valid(target: &str) -> bool {
    let bytes = target.as_bytes();
    if bytes.first() != Some(&b'/') {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if !bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
                || !bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit)
            {
                return false;
            }
            index += 3;
            continue;
        }
        if byte == b'?' {
            index += 1;
            continue;
        }
        if !(byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')'
                    | b'*' | b'+' | b',' | b';' | b'=' | b':' | b'@' | b'/'
            ))
        {
            return false;
        }
        index += 1;
    }
    true
}

fn jet_http_format_authority(authority: &JetHTTPAuthority) -> String {
    match authority.port {
        Some(port) => format!("{}:{}", authority.host, port),
        None => authority.host.clone(),
    }
}

fn jet_http_absolute_target(
    method: &str,
    target: &str,
    host: Option<&JetHTTPAuthority>,
    host_required: bool,
) -> Result<String, JetHTTPReadError> {
    if target == "*" {
        return if method == "OPTIONS" {
            Ok(target.to_string())
        } else {
            Err(JetHTTPReadError { status: 400, message: "asterisk request target requires OPTIONS" })
        };
    }
    if method == "CONNECT" {
        if target.starts_with('/') || target.contains("://") {
            return Err(JetHTTPReadError {
                status: 400,
                message: "CONNECT requires authority-form request target",
            });
        }
        if jet_http_trim_ows(target) != target {
            return Err(JetHTTPReadError {
                status: 400,
                message: "CONNECT authority is malformed",
            });
        }
        let authority = jet_http_parse_authority(target).ok_or(JetHTTPReadError {
            status: 400,
            message: "CONNECT authority is malformed",
        })?;
        if let Some(host) = host {
            if authority.host != host.host || authority.port != host.port {
                return Err(JetHTTPReadError {
                    status: 400,
                    message: "CONNECT authority does not match host",
                });
            }
        } else if host_required {
            return Err(JetHTTPReadError {
                status: 400,
                message: "CONNECT request target requires a host header",
            });
        }
        return Ok(jet_http_format_authority(&authority));
    }
    let path_query = if target.starts_with('/') {
        target.to_string()
    } else {
        let Some(scheme_end) = target.find("://") else {
            return Err(JetHTTPReadError { status: 400, message: "request target form is not supported" });
        };
        let scheme = &target[..scheme_end];
        if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
            return Err(JetHTTPReadError { status: 400, message: "absolute request target is malformed" });
        }
        let remainder = &target[scheme_end + 3..];
        let authority_end = remainder.find(['/', '?']).unwrap_or(remainder.len());
        let raw_authority = &remainder[..authority_end];
        if jet_http_trim_ows(raw_authority) != raw_authority {
            return Err(JetHTTPReadError { status: 400, message: "absolute request authority is malformed" });
        }
        let authority = jet_http_parse_authority(raw_authority).ok_or(JetHTTPReadError {
            status: 400,
            message: "absolute request authority is malformed",
        })?;
        let default_port = if scheme.eq_ignore_ascii_case("http") { 80 } else { 443 };
        if let Some(host) = host {
            if authority.host != host.host
                || authority.port.unwrap_or(default_port) != host.port.unwrap_or(default_port)
            {
                return Err(JetHTTPReadError { status: 400, message: "absolute request authority does not match host" });
            }
        } else if host_required {
            return Err(JetHTTPReadError { status: 400, message: "absolute request target requires a host header" });
        }
        let suffix = &remainder[authority_end..];
        if suffix.is_empty() {
            "/".to_string()
        } else if suffix.starts_with('?') {
            format!("/{suffix}")
        } else {
            suffix.to_string()
        }
    };
    if !jet_http_path_query_valid(&path_query) {
        return Err(JetHTTPReadError { status: 400, message: "request target path or query is malformed" });
    }
    Ok(path_query)
}

fn jet_http_chunk_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.'
                | b'^' | b'_' | b'`' | b'|' | b'~'
        )
}

fn jet_http_chunk_extensions_valid(mut input: &[u8]) -> bool {
    while !input.is_empty() {
        while input.first().is_some_and(|byte| matches!(byte, b' ' | b'\t')) {
            input = &input[1..];
        }
        if input.first() != Some(&b';') {
            return false;
        }
        input = &input[1..];
        while input.first().is_some_and(|byte| matches!(byte, b' ' | b'\t')) {
            input = &input[1..];
        }
        let name_len = input.iter().take_while(|byte| jet_http_chunk_token_byte(**byte)).count();
        if name_len == 0 {
            return false;
        }
        input = &input[name_len..];
        while input.first().is_some_and(|byte| matches!(byte, b' ' | b'\t')) {
            input = &input[1..];
        }
        if input.first() != Some(&b'=') {
            continue;
        }
        input = &input[1..];
        while input.first().is_some_and(|byte| matches!(byte, b' ' | b'\t')) {
            input = &input[1..];
        }
        if input.first() == Some(&b'"') {
            input = &input[1..];
            let mut closed = false;
            while let Some((&byte, rest)) = input.split_first() {
                input = rest;
                if byte == b'"' {
                    closed = true;
                    break;
                }
                if byte == b'\\' {
                    let Some((&escaped, rest)) = input.split_first() else { return false };
                    if !(escaped == b'\t' || escaped == b' ' || escaped.is_ascii_graphic()) {
                        return false;
                    }
                    input = rest;
                } else if !(byte == b'\t' || byte == b' ' || matches!(byte, 0x21 | 0x23..=0x5b | 0x5d..=0x7e)) {
                    return false;
                }
            }
            if !closed {
                return false;
            }
        } else {
            let value_len = input.iter().take_while(|byte| jet_http_chunk_token_byte(**byte)).count();
            if value_len == 0 {
                return false;
            }
            input = &input[value_len..];
        }
    }
    true
}

fn jet_http_chunk_size(line: &[u8]) -> Result<usize, JetHTTPReadError> {
    let digits = line.iter().take_while(|byte| byte.is_ascii_hexdigit()).count();
    if digits == 0 || !jet_http_chunk_extensions_valid(&line[digits..]) {
        return Err(JetHTTPReadError {
            status: 400,
            message: "chunk size is malformed",
        });
    }
    let mut size = 0usize;
    for byte in &line[..digits] {
        let digit = (*byte as char).to_digit(16).unwrap() as usize;
        size = size.checked_mul(16).and_then(|value| value.checked_add(digit)).ok_or(
            JetHTTPReadError {
                status: 413,
                message: "request body is too large",
            },
        )?;
    }
    Ok(size)
}

fn jet_http_decode_chunked_body(
    body: &[u8],
    trailer_names: Vec<String>,
) -> Result<(Vec<u8>, JetHTTPHeaders), JetHTTPReadError> {
    let mut state = JetHTTPChunkState::new(JET_HTTP_MAX_BODY_BYTES, trailer_names);
    let end = state.advance(body)?.ok_or(JetHTTPReadError {
        status: 400,
        message: "request ended before its chunked framing was complete",
    })?;
    if end != body.len() {
        return Err(JetHTTPReadError {
            status: 400,
            message: "request body exceeds its chunked framing",
        });
    }
    let mut decoded = Vec::with_capacity(state.decoded_len);
    for (start, len) in state.chunks {
        decoded.extend_from_slice(&body[start..start + len]);
    }
    Ok((decoded, state.trailers))
}

fn jet_http_validate_headers(header: &[u8]) -> Result<JetHTTPRequestHead, JetHTTPReadError> {
    let text = std::str::from_utf8(header)
        .map_err(|_| JetHTTPReadError { status: 400, message: "request headers are not valid UTF-8" })?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut request_parts = request_line.split(' ');
    let request_shape = (request_parts.next(), request_parts.next(), request_parts.next(), request_parts.next());
    let (Some(method), Some(target), Some(version), None) = request_shape else {
        return Err(JetHTTPReadError { status: 400, message: "request line is malformed" });
    };
    if request_line.len() > 8 * 1024 || target.is_empty() {
        return Err(JetHTTPReadError { status: 400, message: "request line is malformed" });
    }
    if !JetHTTPHeaders::valid_name(method) {
        return Err(JetHTTPReadError { status: 400, message: "request method is malformed" });
    }
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(JetHTTPReadError { status: 505, message: "HTTP version is not supported" });
    }
    let mut count = 0usize;
    let mut content_length = None;
    let mut transfer_encoding = None;
    let mut content_encoding_layers = 0usize;
    let mut expectation = None;
    let mut host = None;
    let mut trailer_values = Vec::new();
    let mut headers = JetHTTPHeaders::new();
    for line in lines {
        count += 1;
        if count > 100 {
            return Err(JetHTTPReadError { status: 431, message: "request has too many headers" });
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            return Err(JetHTTPReadError { status: 400, message: "folded request headers are not allowed" });
        }
        let (name, value) = line.split_once(':')
            .ok_or(JetHTTPReadError { status: 400, message: "request header is malformed" })?;
        if !JetHTTPHeaders::valid_name(name) {
            return Err(JetHTTPReadError { status: 400, message: "request header name is malformed" });
        }
        if !JetHTTPHeaders::valid_value(value) {
            return Err(JetHTTPReadError { status: 400, message: "request header value is malformed" });
        }
        headers.append(name, jet_http_trim_ows_start(value)).map_err(|_| JetHTTPReadError {
            status: 400,
            message: "request header is malformed",
        })?;
        if name.eq_ignore_ascii_case("connection") {
            if !jet_http_connection_options(value).all(JetHTTPHeaders::valid_name) {
                return Err(JetHTTPReadError {
                    status: 400,
                    message: "connection option is malformed",
                });
            }
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(jet_http_parse_content_length(value, content_length)?);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if transfer_encoding.replace(jet_http_trim_ows(value)).is_some() {
                return Err(JetHTTPReadError {
                    status: 400,
                    message: "multiple transfer-encoding headers are not allowed",
                });
            }
        } else if name.eq_ignore_ascii_case("content-encoding") {
            for coding in value.split(',') {
                let coding = jet_http_trim_ows(coding);
                if coding.is_empty() {
                    return Err(JetHTTPReadError {
                        status: 400,
                        message: "content-encoding is malformed",
                    });
                }
                if !coding.eq_ignore_ascii_case("gzip") {
                    return Err(JetHTTPReadError {
                        status: 415,
                        message: "content encoding is not supported",
                    });
                }
                content_encoding_layers += 1;
                if content_encoding_layers > 4 {
                    return Err(JetHTTPReadError {
                        status: 415,
                        message: "too many content encodings",
                    });
                }
            }
        } else if name.eq_ignore_ascii_case("expect") {
            if expectation.replace(jet_http_trim_ows(value)).is_some() {
                return Err(JetHTTPReadError {
                    status: 417,
                    message: "multiple expect headers are not supported",
                });
            }
        } else if name.eq_ignore_ascii_case("host") {
            if host.replace(value).is_some() {
                return Err(JetHTTPReadError {
                    status: 400,
                    message: "multiple host headers are not allowed",
                });
            }
        } else if name.eq_ignore_ascii_case("trailer") {
            trailer_values.push(jet_http_trim_ows(value).to_string());
        }
    }
    let host = match host {
        Some(value) => Some(jet_http_parse_authority(value).ok_or(JetHTTPReadError {
            status: 400,
            message: "host authority is malformed",
        })?),
        None if version == "HTTP/1.1" => {
            return Err(JetHTTPReadError {
                status: 400,
                message: "HTTP/1.1 requires one host header",
            });
        }
        None => None,
    };
    let target = jet_http_absolute_target(method, target, host.as_ref(), version == "HTTP/1.1")?;
    if transfer_encoding.is_some() && content_length.is_some() {
        return Err(JetHTTPReadError { status: 400, message: "content-length and transfer-encoding cannot be combined" });
    }
    let framing = if let Some(encoding) = transfer_encoding {
        if version != "HTTP/1.1" || !encoding.eq_ignore_ascii_case("chunked") {
            return Err(JetHTTPReadError { status: 400, message: "transfer-encoding is not supported" });
        }
        JetHTTPRequestFraming::Chunked
    } else {
        JetHTTPRequestFraming::ContentLength(content_length.unwrap_or(0))
    };
    let trailer_names = jet_http_parse_trailer_names(&trailer_values)?;
    if !trailer_names.is_empty() && !matches!(framing, JetHTTPRequestFraming::Chunked) {
        return Err(JetHTTPReadError {
            status: 400,
            message: "request trailers require chunked framing",
        });
    }
    let expect_continue = match expectation {
        None => false,
        Some(value) if version == "HTTP/1.1" && value.eq_ignore_ascii_case("100-continue") => true,
        Some(_) => {
            return Err(JetHTTPReadError {
                status: 417,
                message: "request expectation is not supported",
            });
        }
    };
    Ok(JetHTTPRequestHead {
        framing,
        expect_continue,
        method: method.to_string(),
        version: version.to_string(),
        target,
        headers,
        content_encoding_layers,
        trailer_names,
    })
}

fn jet_http_srv_read_error_response(error: &JetHTTPReadError) -> String {
    let reason = match error.status {
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        417 => "Expectation Failed",
        431 => "Request Header Fields Too Large",
        408 => "Request Timeout",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "Bad Request",
    };
    format!(
        "HTTP/1.1 {} {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        error.status, reason
    )
}

fn jet_http_mux_serve_tls<V, S>(
    addr: &String,
    mux: JetHTTPMux,
    tls: JetHTTPServerTls,
    validate: V,
    session: S,
) -> Result<(), String>
where
    V: Fn(&String, &String) -> Result<(), String>,
    S: Fn(
            &String,
            &String,
            std::net::TcpStream,
            Box<dyn FnMut(&[u8], bool) -> Result<(Vec<u8>, bool), String> + Send>,
            JetHTTPTlsH2,
            Box<dyn Fn() -> bool + Send>,
        ) -> Result<(), String>
        + Clone
        + Send
        + Sync
        + 'static,
{
    validate(&tls.cert_pem, &tls.key_pem)?;
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let tls_conn = jet_http_tls_conn_handler(mux.clone(), tls, session);
    jet_http_server_run_listener(
        listener,
        mux,
        JetHTTPServerOptions::safe(),
        shutdown,
        None,
        None,
        Some(tls_conn),
    )
    .map(|_| ())
}

fn jet_http_server_bind_tls<V, S>(
    addr: &String,
    mux: JetHTTPMux,
    tls: JetHTTPServerTls,
    validate: V,
    session: S,
) -> Result<JetHTTPServer, JetHTTPError>
where
    V: Fn(&String, &String) -> Result<(), String>,
    S: Fn(
            &String,
            &String,
            std::net::TcpStream,
            Box<dyn FnMut(&[u8], bool) -> Result<(Vec<u8>, bool), String> + Send>,
            JetHTTPTlsH2,
            Box<dyn Fn() -> bool + Send>,
        ) -> Result<(), String>
        + Clone
        + Send
        + Sync
        + 'static,
{
    validate(&tls.cert_pem, &tls.key_pem).map_err(|operation| JetHTTPError::IO { operation })?;
    let tls_conn = jet_http_tls_conn_handler(mux.clone(), tls, session);
    jet_http_server_bind_with_tls(addr, mux, Some(tls_conn))
}

fn jet_http_tls_conn_handler<S>(mux: JetHTTPMux, tls: JetHTTPServerTls, session: S) -> JetHTTPTlsConn
where
    S: Fn(
            &String,
            &String,
            std::net::TcpStream,
            Box<dyn FnMut(&[u8], bool) -> Result<(Vec<u8>, bool), String> + Send>,
            JetHTTPTlsH2,
            Box<dyn Fn() -> bool + Send>,
        ) -> Result<(), String>
        + Clone
        + Send
        + Sync
        + 'static,
{
    std::sync::Arc::new(move |stream, shutdown, options, dynamic_grace_ms, drain_deadline_ms| {
        let m = mux.clone();
        let h2_mux = mux.clone();
        let tls_cfg = tls.clone();
        let session = session.clone();
        let stop = shutdown.clone();
        let h2_stop = shutdown.clone();
        session(
            &tls_cfg.cert_pem,
            &tls_cfg.key_pem,
            stream,
            Box::new(move |raw, force_close| {
                match jet_http_srv_parse(raw) {
                    Ok(req) => {
                        let version = jet_http_srv_request_version(raw).to_string();
                        let keep = jet_http_srv_request_keep_alive(&version, &req.headers);
                        let response = jet_http_mux_dispatch(&m, req)
                            .unwrap_or_else(jet_http_srv_error_response);
                        let close = !keep
                            || force_close
                            || stop.load(std::sync::atomic::Ordering::Acquire)
                            || (version == "HTTP/1.0" && response.body.length().is_none());
                        Ok((
                            jet_http_srv_format_connection(&response, &version, close).into_bytes(),
                            !close,
                        ))
                    }
                    Err(error) => Ok((jet_http_srv_read_error_response(&error).into_bytes(), false)),
                }
            }),
            Box::new(move |reader, writer, set_read_timeout, set_write_timeout| {
                let mut transport = JetHTTP2TlsTransport {
                    reader,
                    writer,
                    set_read_timeout,
                    set_write_timeout,
                };
                let (result, last_stream) = jet_http2_serve_with_last_stream(
                    &mut transport,
                    &h2_mux,
                    &options,
                    &h2_stop,
                    dynamic_grace_ms.as_deref(),
                    Some(drain_deadline_ms.as_ref()),
                );
                if result.is_err() {
                    let _ = jet_http2_write_frame(
                        &mut transport,
                        7,
                        0,
                        0,
                        &jet_http2_goaway_payload(last_stream, 1),
                    );
                    let _ = std::io::Write::flush(&mut transport);
                }
                result
            }),
            Box::new(move || shutdown.load(std::sync::atomic::Ordering::Acquire)),
        )
    })
}

fn jet_http_srv_parse(raw: &[u8]) -> Result<JetHTTPRequest, JetHTTPReadError> {
    let sep = jet_http_header_end(raw).ok_or(JetHTTPReadError {
        status: 400,
        message: "request headers are incomplete",
    })?;
    if sep > 32 * 1024 {
        return Err(JetHTTPReadError {
            status: 431,
            message: "request headers are too large",
        });
    }
    let header_part = &raw[..sep];
    let mut head = jet_http_validate_headers(header_part)?;
    let encoded_body = &raw[sep + 4..];
    let (body, trailers) = match head.framing {
        JetHTTPRequestFraming::ContentLength(content_length) => {
            if content_length > JET_HTTP_MAX_BODY_BYTES {
                return Err(JetHTTPReadError {
                    status: 413,
                    message: "request body is too large",
                });
            }
            if encoded_body.len() != content_length {
                return Err(JetHTTPReadError {
                    status: 400,
                    message: "request body does not match content-length",
                });
            }
            (encoded_body.to_vec(), JetHTTPHeaders::new())
        }
        JetHTTPRequestFraming::Chunked => {
            jet_http_decode_chunked_body(encoded_body, std::mem::take(&mut head.trailer_names))?
        }
    };
    let body = jet_http_decode_request_bytes(
        body,
        head.content_encoding_layers,
        JET_HTTP_MAX_BODY_BYTES,
    )?;
    let method = head.method;
    let path = head.target;
    let mut headers = head.headers;
    if head.content_encoding_layers > 0 {
        headers.remove("content-encoding");
        headers.remove("content-length");
    }
    Ok(JetHTTPRequest::server_body_with_trailers(
        &method,
        path,
        JetHTTPBody::from_bytes(body),
        headers,
        std::sync::Arc::new(std::sync::Mutex::new(trailers)),
    ))
}

const JET_HTTP_DEVTOOLS_SOURCE: &str = "core.http.server";

fn jet_http_devtools_publish_fact(fact: JetDevtoolsRequestPanelFact) {
    if let Ok(event) = fact.to_protocol_event(JET_HTTP_DEVTOOLS_SOURCE) {
        jet_devtools_publish_event(event);
    }
}

fn jet_http_devtools_request_id(req: &JetHTTPRequest) -> String {
    match req.headers.get("x-request-id") {
        Some(value) if jet_http_request_id_valid(value) => value.clone(),
        _ => jet_http_new_request_id(),
    }
}

fn jet_http_devtools_publish_request(
    req: &JetHTTPRequest,
    request_id: &str,
    started_at_ms: u64,
) {
    let path = req.path.split('?').next().unwrap_or(req.path.as_str());
    let route = req.route_template.as_deref().unwrap_or(path);
    let mut fact = JetDevtoolsRequestFact::new(
        request_id,
        req.method.clone(),
        route,
        started_at_ms,
    );
    for (name, _) in req
        .params
        .iter()
        .take(JET_DEVTOOLS_REQUEST_PANEL_MAX_ROUTE_INPUTS)
    {
        fact = fact.with_route_input(JetDevtoolsRouteInput::new(name.clone()));
    }
    jet_http_devtools_publish_fact(JetDevtoolsRequestPanelFact::Request(fact));
}

fn jet_http_devtools_publish_response(
    request_id: &str,
    response: &JetHTTPResponse,
    started: &std::time::Instant,
) {
    let Some(status) = u16::try_from(response.status).ok() else {
        return;
    };
    let size_bytes = response
        .head_content_length
        .or_else(|| response.body.length())
        .and_then(|length| u64::try_from(length).ok())
        .unwrap_or(0);
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let fact = JetDevtoolsResponseFact::new(request_id, status, size_bytes, duration_ms);
    jet_http_devtools_publish_fact(JetDevtoolsRequestPanelFact::Response(fact));
}

fn jet_http_devtools_error_code(error: &JetHTTPError) -> &'static str {
    match error {
        JetHTTPError::InvalidMethod => "InvalidMethod",
        JetHTTPError::InvalidUrl => "InvalidUrl",
        JetHTTPError::InvalidHeader => "InvalidHeader",
        JetHTTPError::InvalidStatus => "InvalidStatus",
        JetHTTPError::BodyConsumed => "BodyConsumed",
        JetHTTPError::BodyTooLarge { .. } => "BodyTooLarge",
        JetHTTPError::InvalidFraming => "InvalidFraming",
        JetHTTPError::UnsupportedEncoding => "UnsupportedEncoding",
        JetHTTPError::Resolve { .. } => "Resolve",
        JetHTTPError::Connect { .. } => "Connect",
        JetHTTPError::TLS { .. } => "TLS",
        JetHTTPError::Timeout { .. } => "Timeout",
        JetHTTPError::Proxy { .. } => "Proxy",
        JetHTTPError::Redirect { .. } => "Redirect",
        JetHTTPError::Protocol { .. } => "Protocol",
        JetHTTPError::IO { .. } => "IO",
        JetHTTPError::Policy { .. } => "Policy",
        JetHTTPError::Cancelled => "Cancelled",
        JetHTTPError::ResourceUnavailable { .. } => "ResourceUnavailable",
        JetHTTPError::UnsupportedTarget { .. } => "UnsupportedTarget",
        JetHTTPError::Internal { .. } => "Internal",
    }
}

fn jet_http_devtools_publish_exception_code(request_id: &str, code: &str) {
    let id = format!("{request_id}:exception:{code}");
    let fact = JetDevtoolsExceptionFact::new(
        id,
        request_id,
        "HTTPError",
        code,
        jet_http_unix_now_ms(),
    );
    jet_http_devtools_publish_fact(JetDevtoolsRequestPanelFact::Exception(fact));
}


fn jet_http_devtools_source_text(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(limit)
        .collect()
}

fn jet_http_devtools_publish_exception_report(request_id: &str, report: &JetErrorReport) {
    let code = report
        .code
        .as_deref()
        .filter(|value| {
            !value.is_empty()
                && value
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
        })
        .unwrap_or("HTTPError");
    let mut fact = JetDevtoolsExceptionFact::new(
        format!("{request_id}:exception:{code}"),
        request_id,
        "HTTPError",
        code,
        jet_http_unix_now_ms(),
    );
    if report.source_journey.is_empty() {
        if let Some(frame) = jet_error_page_source(report) {
            let file = jet_http_devtools_source_text(&frame.file, JET_ERROR_PAGE_MAX_SOURCE_BYTES);
            if !file.is_empty() {
                fact = fact.with_source_frame(JetDevtoolsSourceFrame::new(
                    format!("{request_id}:frame:0"),
                    request_id,
                    jet_http_devtools_source_text(
                        &frame.fn_name,
                        JET_ERROR_PAGE_MAX_SOURCE_BYTES,
                    ),
                    file,
                    frame.line,
                    0,
                    0,
                ));
            }
        }
    } else {
        for (index, frame) in report
            .source_journey
            .iter()
            .take(JET_DEVTOOLS_REQUEST_PANEL_MAX_SOURCE_FRAMES)
            .enumerate()
        {
            let file = jet_http_devtools_source_text(&frame.file, JET_ERROR_PAGE_MAX_SOURCE_BYTES);
            if file.is_empty() {
                continue;
            }
            fact = fact.with_source_frame(JetDevtoolsSourceFrame::new(
                format!("{request_id}:frame:{index}"),
                request_id,
                jet_http_devtools_source_text(
                    &frame.fn_name,
                    JET_ERROR_PAGE_MAX_SOURCE_BYTES,
                ),
                file,
                frame.line,
                0,
                u32::try_from(index).unwrap_or(u32::MAX),
            ));
        }
    }
    jet_http_devtools_publish_fact(JetDevtoolsRequestPanelFact::Exception(fact));
}

fn jet_http_mux_dispatch(
    mux: &JetHTTPMux,
    req: JetHTTPRequest,
) -> Result<JetHTTPResponse, JetHTTPError> {
    let route_cache = mux.route_cache();
    jet_http_mux_dispatch_cached(mux, req, &route_cache)
}

fn jet_http_mux_dispatch_cached(
    mux: &JetHTTPMux,
    mut req: JetHTTPRequest,
    route_cache: &JetHTTPMuxRouteCache,
) -> Result<JetHTTPResponse, JetHTTPError> {
    let started = std::time::Instant::now();
    let started_at_ms = jet_http_unix_now_ms();
    let request_id = jet_http_devtools_request_id(&req);
    let _ = req.headers.set("x-request-id", &request_id);
    let requested_method = req.method.as_str();
    let is_head = requested_method == "HEAD";
    if requested_method == "OPTIONS" && req.path == "*" {
        let allow = jet_http_allowed_methods(
            route_cache.routes.iter().map(|route| route.route.method.as_str()),
        );
        let handler: JetHTTPHandler = std::sync::Arc::new(move |_| {
            Ok(jet_http_srv_response_with_headers(
                204,
                "",
                [("Allow".to_string(), allow.clone())].into_iter().collect(),
            ))
        });
        jet_http_devtools_publish_request(&req, &request_id, started_at_ms);
        let response = jet_http_mux_run_handler(mux, req, handler);
        jet_http_devtools_publish_response(&request_id, &response, &started);
        return Ok(response);
    }
    // CONNECT authority-form has no path; route against "/{authority}" while
    // leaving req.path as the normalized authority for handlers.
    let route_target = if requested_method == "CONNECT" {
        std::borrow::Cow::Owned(format!("/{}", req.path))
    } else {
        std::borrow::Cow::Borrowed(req.path.as_str())
    };
    if jet_http_route_validate_path(route_target.as_ref()).is_err() {
        let handler: JetHTTPHandler = std::sync::Arc::new(|_| {
            Ok(jet_http_srv_response_owned(400, "400 Bad Request".to_string()))
        });
        jet_http_devtools_publish_request(&req, &request_id, started_at_ms);
        let response = jet_http_mux_run_handler(mux, req, handler);
        let response = jet_http_srv_head_response(response, is_head);
        jet_http_devtools_publish_response(&request_id, &response, &started);
        return Ok(response);
    }
    // Route lookup uses an immutable validated snapshot. Never retain the
    // registry lock while composing middleware or running user code: handlers
    // may overlap and may register another route on this same mux.
    let route_path = route_target.as_ref();
    let mut path_match_count = 0usize;
    let mut has_head = false;
    let mut selected = None;
    let mut selected_get = None;
    for (order, route) in route_cache.routes.iter().enumerate() {
        let Some(pattern) = route.pattern.as_ref() else {
            continue;
        };
        if !jet_http_route_matches_path(pattern, route_path) {
            continue;
        }
        path_match_count += 1;
        has_head |= route.route.method == "HEAD";
        if route.route.method == requested_method {
            let replace = match selected {
                None => true,
                Some((left_order, _, left_pattern)) => {
                    jet_http_route_selection_cmp(pattern, order, left_pattern, left_order)
                        == std::cmp::Ordering::Greater
                }
            };
            if replace {
                selected = Some((order, route, pattern));
            }
        }
        if requested_method == "HEAD" && route.route.method == "GET" {
            let replace = match selected_get {
                None => true,
                Some((left_order, _, left_pattern)) => {
                    jet_http_route_selection_cmp(pattern, order, left_pattern, left_order)
                        == std::cmp::Ordering::Greater
                }
            };
            if replace {
                selected_get = Some((order, route, pattern));
            }
        }
    }
    let selected = if requested_method == "HEAD" && !has_head {
        selected_get
    } else {
        selected
    };
    if let Some((_, route, pattern)) = selected {
        req.params = jet_http_route_params_path(pattern, route_path)
            .expect("validated HTTP route path");
        req.route_template = Some(route.route.pattern.clone());
        jet_http_devtools_publish_request(&req, &request_id, started_at_ms);
        let response = jet_http_mux_run_handler_ref(mux, req, &route.route.handler);
        let response = jet_http_srv_head_response(response, is_head);
        jet_http_devtools_publish_response(&request_id, &response, &started);
        return Ok(response);
    }
    if path_match_count > 0 {
        let allow = jet_http_allowed_methods(route_cache.routes.iter().filter_map(|route| {
            let pattern = route.pattern.as_ref()?;
            jet_http_route_matches_path(pattern, route_path).then_some(route.route.method.as_str())
        }));
        let handler: JetHTTPHandler = std::sync::Arc::new(move |_| {
            Ok(jet_http_srv_response_with_headers(
                405,
                "405 Method Not Allowed",
                [("Allow".to_string(), allow.clone())].into_iter().collect(),
            ))
        });
        jet_http_devtools_publish_request(&req, &request_id, started_at_ms);
        let response = jet_http_mux_run_handler(mux, req, handler);
        let response = jet_http_srv_head_response(response, is_head);
        jet_http_devtools_publish_response(&request_id, &response, &started);
        return Ok(response);
    }
    let handler: JetHTTPHandler = std::sync::Arc::new(|_| {
        Ok(jet_http_srv_response_owned(404, "404 Not Found".to_string()))
    });
    jet_http_devtools_publish_request(&req, &request_id, started_at_ms);
    let response = jet_http_mux_run_handler(mux, req, handler);
    let response = jet_http_srv_head_response(response, is_head);
    jet_http_devtools_publish_response(&request_id, &response, &started);
    Ok(response)
}

fn jet_http_mux_run_handler_ref(
    mux: &JetHTTPMux,
    req: JetHTTPRequest,
    handler: &JetHTTPHandler,
) -> JetHTTPResponse {
    let request_id = jet_http_devtools_request_id(&req);
    let _db_request_scope = JetDbRequestScope::enter(Some(request_id.clone()));
    let middlewares_empty = match mux.1.lock() {
        Ok(middlewares) => middlewares.is_empty(),
        Err(_) => {
            jet_http_devtools_publish_exception_code(&request_id, "middleware_lock");
            return jet_http_srv_internal_response();
        }
    };
    if middlewares_empty {
        return match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(req))) {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                jet_http_devtools_publish_exception_code(
                    &request_id,
                    jet_http_devtools_error_code(&error),
                );
                jet_http_srv_error_response(error)
            }
            Err(_) => {
                jet_http_devtools_publish_exception_code(&request_id, "panic");
                jet_http_srv_internal_response()
            }
        };
    }
    jet_http_mux_run_handler(mux, req, handler.clone())
}


fn jet_http_mux_run_handler(
    mux: &JetHTTPMux,
    req: JetHTTPRequest,
    handler: JetHTTPHandler,
) -> JetHTTPResponse {
    let request_id = jet_http_devtools_request_id(&req);
    let _db_request_scope = JetDbRequestScope::enter(Some(request_id.clone()));
    let middlewares = match mux.1.lock() {
        Ok(middlewares) => middlewares.clone(),
        Err(_) => {
            jet_http_devtools_publish_exception_code(&request_id, "middleware_lock");
            return jet_http_srv_internal_response();
        }
    };
    if middlewares.is_empty() {
        return match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(req))) {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                jet_http_devtools_publish_exception_code(
                    &request_id,
                    jet_http_devtools_error_code(&error),
                );
                jet_http_srv_error_response(error)
            }
            Err(_) => {
                jet_http_devtools_publish_exception_code(&request_id, "panic");
                jet_http_srv_internal_response()
            }
        };
    }
    let mut handler = jet_http_mux_total_handler(handler);
    for middleware in middlewares.iter().rev() {
        let next = handler.clone();
        handler = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| middleware(next))) {
            Ok(handler) => jet_http_mux_total_handler(handler),
            Err(_) => std::sync::Arc::new(|req| {
                let request_id = jet_http_devtools_request_id(&req);
                jet_http_devtools_publish_exception_code(&request_id, "middleware_panic");
                Ok(jet_http_srv_internal_response())
            }),
        };
    }
    match handler(req) {
        Ok(response) => response,
        Err(error) => {
            jet_http_devtools_publish_exception_code(
                &request_id,
                jet_http_devtools_error_code(&error),
            );
            jet_http_srv_error_response(error)
        }
    }

}
fn jet_http_mux_total_handler(handler: JetHTTPHandler) -> JetHTTPHandler {
    std::sync::Arc::new(move |req| {
        let request_id = jet_http_devtools_request_id(&req);
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(req))) {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) => {
                jet_http_devtools_publish_exception_code(
                    &request_id,
                    jet_http_devtools_error_code(&error),
                );
                Ok(jet_http_srv_error_response(error))
            }
            Err(_) => {
                jet_http_devtools_publish_exception_code(&request_id, "panic");
                Ok(jet_http_srv_internal_response())
            }
        }
    })
}

fn jet_http_srv_internal_response() -> JetHTTPResponse {
    jet_http_srv_response_owned(500, "500 Internal Server Error".to_string())
}

fn jet_http_srv_error_response(error: JetHTTPError) -> JetHTTPResponse {
    match error {
        JetHTTPError::BodyTooLarge { .. } => jet_http_srv_empty_response(413),
        JetHTTPError::InvalidFraming => jet_http_srv_empty_response(400),
        JetHTTPError::UnsupportedEncoding => jet_http_srv_empty_response(415),
        _ => jet_http_srv_internal_response(),
    }
}

fn jet_http_srv_head_response(mut response: JetHTTPResponse, is_head: bool) -> JetHTTPResponse {
    if is_head {
        response.suppress_body = true;
        response.head_content_length = response.body.length();
        response.body = JetHTTPBody::empty();
        response.trailers = JetHTTPHeaders::new();
    }
    response
}

fn jet_http_allowed_methods<'a>(registered: impl Iterator<Item = &'a str>) -> String {
    let mut methods = std::collections::BTreeSet::new();
    methods.extend(registered.map(str::to_string));
    if methods.contains("GET") { methods.insert("HEAD".to_string()); }
    methods.insert("OPTIONS".to_string());
    methods.into_iter().collect::<Vec<_>>().join(", ")
}

fn jet_http_mux_validate(mux: &JetHTTPMux) -> Result<(), String> {
    mux.route_cache()
        .validation_error
        .clone()
        .map_or(Ok(()), Err)
}

fn jet_http_srv_format(resp: &JetHTTPResponse) -> String {
    jet_http_srv_format_connection(resp, "HTTP/1.1", true)
}

fn jet_http_srv_format_connection(resp: &JetHTTPResponse, version: &str, close: bool) -> String {
    let mut bytes = Vec::new();
    jet_http_srv_write_response(&mut bytes, resp, version, close)
        .expect("in-memory HTTP response formatting cannot fail");
    String::from_utf8(bytes).expect("text compatibility response contains UTF-8")
}
fn jet_http_srv_connection_nominates(headers: &JetHTTPHeaders, name: &str) -> bool {
    for (candidate, value) in headers {
        if candidate.eq_ignore_ascii_case("connection")
            && value
                .split(',')
                .map(str::trim)
                .any(|nominated| nominated.eq_ignore_ascii_case(name))
        {
            return true;
        }
    }
    false
}


fn jet_http_srv_write_bytes_response(
    writer: &mut impl std::io::Write,
    header: &str,
    body: &JetHTTPBody,
    expected_length: Option<usize>,
) -> Result<Option<usize>, JetHTTPError> {
    use std::io::Write;
    let mut state = body.state.lock().map_err(|_| JetHTTPError::Internal {
        incident_id: "http-body-lock".to_string(),
    })?;
    let Some(source) = state.source.take() else {
        return Ok(None);
    };
    let JetHTTPBodySource::Bytes(reader) = source else {
        state.source = Some(source);
        return Ok(None);
    };
    let position = reader.position();
    let bytes = reader.into_inner();
    let start = usize::try_from(position).unwrap_or(bytes.len()).min(bytes.len());
    let bytes = &bytes[start..];
    let total = header.len().saturating_add(bytes.len());
    let slices = [std::io::IoSlice::new(header.as_bytes()), std::io::IoSlice::new(bytes)];
    let first = writer.write_vectored(&slices).map_err(|_| JetHTTPError::IO {
        operation: "write response body".to_string(),
    })?;
    if first < total {
        if first < header.len() {
            writer.write_all(&header.as_bytes()[first..]).map_err(|_| JetHTTPError::IO {
                operation: "write response headers".to_string(),
            })?;
            writer.write_all(bytes).map_err(|_| JetHTTPError::IO {
                operation: "write response body".to_string(),
            })?;
        } else {
            writer.write_all(&bytes[first - header.len()..]).map_err(|_| JetHTTPError::IO {
                operation: "write response body".to_string(),
            })?;
        }
    }
    state.drained.store(true, std::sync::atomic::Ordering::Release);
    if expected_length.is_some_and(|length| length != bytes.len()) {
        return Err(JetHTTPError::InvalidFraming);
    }
    Ok(Some(bytes.len()))
}

fn jet_http_srv_write_response(
    writer: &mut impl std::io::Write,
    resp: &JetHTTPResponse,
    version: &str,
    close: bool,
) -> Result<(), JetHTTPError> {
    use std::fmt::Write as _;
    let reason = match resp.status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        500 => "Internal Server Error",
        505 => "HTTP Version Not Supported",
        _ => "OK",
    };
    let body_forbidden = (100..200).contains(&resp.status) || matches!(resp.status, 204 | 304);
    let reset_content = resp.status == 205;
    let mut trailer_names = Vec::new();
    for (name, _) in &resp.trailers {
        if !jet_http_trailer_name_allowed(name) {
            return Err(JetHTTPError::InvalidHeader);
        }
        if !trailer_names.iter().any(|old: &String| old.eq_ignore_ascii_case(name)) {
            trailer_names.push(name.clone());
        }
    }
    if !trailer_names.is_empty()
        && (version != "HTTP/1.1" || body_forbidden || reset_content || resp.head_content_length.is_some())
    {
        return Err(JetHTTPError::InvalidFraming);
    }
    let known_length = resp.head_content_length.or_else(|| resp.body.length());
    let chunked = version == "HTTP/1.1"
        && !resp.suppress_body
        && !body_forbidden
        && !reset_content
        && (known_length.is_none() || !trailer_names.is_empty());
    let close_delimited = version == "HTTP/1.0"
        && !resp.suppress_body
        && !body_forbidden
        && !reset_content
        && known_length.is_none();
    let mut out = String::with_capacity(128);
    write!(&mut out, "{} {} {}\r\n", version, resp.status, reason).expect("response formatting cannot fail");
    if !body_forbidden {
        if reset_content {
            out.push_str("Content-Length: 0\r\n");
        } else if resp.suppress_body {
            if let Some(length) = known_length {
                write!(&mut out, "Content-Length: {length}\r\n")
                    .expect("response formatting cannot fail");
            }
        } else if chunked {
            out.push_str("Transfer-Encoding: chunked\r\n");
            if !trailer_names.is_empty() {
                write!(&mut out, "Trailer: {}\r\n", trailer_names.join(", "))
                    .expect("response formatting cannot fail");
            }
        } else if !close_delimited {
            write!(&mut out, "Content-Length: {}\r\n", known_length.unwrap_or(0))
                .expect("response formatting cannot fail");
        }
    }
    write!(
        &mut out,
        "Connection: {}\r\n",
        if close || close_delimited { "close" } else { "keep-alive" },
    )
    .expect("response formatting cannot fail");
    for (name, value) in &resp.headers {
        let framing = name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("transfer-encoding")
            || name.eq_ignore_ascii_case("trailer")
            || name.eq_ignore_ascii_case("connection");
        let nominated = jet_http_srv_connection_nominates(&resp.headers, name);
        if !framing && !nominated {
            write!(&mut out, "{}: {}\r\n", name, value)
                .expect("response formatting cannot fail");
        }
    }
    out.push_str("\r\n");
    let direct_body = if !resp.suppress_body && !body_forbidden && !reset_content && !chunked {
        jet_http_srv_write_bytes_response(writer, &out, &resp.body, known_length)?
    } else {
        None
    };
    if direct_body.is_none() {
        writer.write_all(out.as_bytes()).map_err(|_| JetHTTPError::IO {
            operation: "write response headers".to_string(),
        })?;
    }
    if direct_body.is_some() {
        return Ok(());
    }
    if !resp.suppress_body && !body_forbidden && !reset_content {
        let mut written = 0usize;
        for chunk in resp.body.chunks(64 * 1024)? {
            let chunk = chunk?;
            written = written.saturating_add(chunk.len());
            if chunked {
                write!(writer, "{:x}\r\n", chunk.len()).map_err(|_| JetHTTPError::IO {
                    operation: "write response chunk framing".to_string(),
                })?;
            }
            writer.write_all(&chunk).map_err(|_| JetHTTPError::IO {
                operation: "write response body".to_string(),
            })?;
            if chunked {
                writer.write_all(b"\r\n").map_err(|_| JetHTTPError::IO {
                    operation: "write response chunk framing".to_string(),
                })?;
            }
        }
        if known_length.is_some_and(|length| length != written) {
            return Err(JetHTTPError::InvalidFraming);
        }
        if chunked {
            writer.write_all(b"0\r\n").map_err(|_| JetHTTPError::IO {
                operation: "write response chunk terminator".to_string(),
            })?;
            for (name, value) in &resp.trailers {
                write!(writer, "{name}: {value}\r\n").map_err(|_| JetHTTPError::IO {
                    operation: "write response trailers".to_string(),
                })?;
            }
            writer.write_all(b"\r\n").map_err(|_| JetHTTPError::IO {
                operation: "write response trailer terminator".to_string(),
            })?;
        }
    }
    Ok(())
}

fn jet_http_srv_req_method(req: &JetHTTPRequest) -> String {
    req.method.clone()
}
fn jet_http_srv_req_path(req: &JetHTTPRequest) -> String {
    req.path.clone()
}
fn jet_http_srv_req_param(req: &JetHTTPRequest, name: &String) -> JetOutcome<String, JetAbsent> {
    jet_outcome_of(req.params.get(name).cloned())
}
fn jet_http_srv_req_body(req: &JetHTTPRequest) -> JetHTTPBody {
    req.body.clone()
}
fn jet_http_srv_req_trailers(req: &JetHTTPRequest) -> Result<JetHTTPHeaders, JetHTTPError> {
    if !req.body.is_drained() {
        return Err(JetHTTPError::InvalidFraming);
    }
    req.trailers.lock().map(|trailers| trailers.clone()).map_err(|_| {
        JetHTTPError::Internal {
            incident_id: "request-trailers-lock".to_string(),
        }
    })
}
fn jet_http_srv_req_header(req: &JetHTTPRequest, name: &String) -> JetOutcome<String, JetAbsent> {
    jet_outcome_of(req.headers.get(name).cloned())
}

fn jet_http_srv_req_body_len(req: &JetHTTPRequest) -> i64 {
    req.body.length().unwrap_or(0) as i64
}

fn jet_http_srv_req_under_limit(req: &JetHTTPRequest, max_bytes: i64) -> bool {
    max_bytes >= 0 && req.body.length().is_some_and(|length| length as i64 <= max_bytes)
}

fn jet_http_srv_sse(data: &String) -> JetHTTPResponse {
    let resp = jet_http_srv_response(200, &format!("data: {}\n\n", data));
    let resp = jet_http_srv_response_header(
        resp,
        &"content-type".to_string(),
        &"text/event-stream".to_string(),
    );
    jet_http_srv_response_header(resp, &"cache-control".to_string(), &"no-cache".to_string())
}

fn jet_http_srv_response_trailers(
    mut response: JetHTTPResponse,
    trailers: JetHTTPHeaders,
) -> Result<JetHTTPResponse, JetHTTPError> {
    if (&trailers).into_iter().any(|(name, _)| !jet_http_trailer_name_allowed(name)) {
        return Err(JetHTTPError::InvalidHeader);
    }
    response.trailers = trailers;
    Ok(response)
}

fn jet_http_srv_static_file(path: &String, mime: &String) -> Result<JetHTTPResponse, String> {
    let candidate = std::path::Path::new(path);
    let parent = candidate.parent().filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let root = std::fs::canonicalize(parent).map_err(|error| format!("static file `{path}` failed: {error}"))?;
    let Some((file, metadata, _)) = jet_http_static_open(&root, candidate) else {
        return Err(format!("static file `{path}` could not be opened with held identity"));
    };
    let length = usize::try_from(metadata.len()).map_err(|_| format!("static file `{path}` is too large"))?;
    let mut response = jet_http_srv_response(200, &String::new());
    response.body = JetHTTPBody::file(file, length);
    Ok(jet_http_srv_response_header(response, &"content-type".to_string(), mime))
}

fn jet_http_srv_static_file_range(
    req: &JetHTTPRequest,
    path: &String,
    mime: &String,
) -> Result<JetHTTPResponse, String> {
    let candidate = std::path::Path::new(path);
    let parent = candidate.parent().filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let root = std::fs::canonicalize(parent).map_err(|error| format!("static file `{path}` failed: {error}"))?;
    let Some((mut file, metadata, _)) = jet_http_static_open(&root, candidate) else {
        return Err(format!("static file `{path}` could not be opened with held identity"));
    };
    let file_len = usize::try_from(metadata.len()).map_err(|_| format!("static file `{path}` is too large"))?;
    let Ok(range) = jet_http_srv_req_header(req, &"range".to_string()) else {
        let mut response = jet_http_srv_response(200, &String::new());
        response.body = JetHTTPBody::file(file, file_len);
        return Ok(jet_http_srv_response_header(response, &"content-type".to_string(), mime));
    };
    let Some((start, end)) = jet_http_static_range(&range, file_len) else {
        return Ok(jet_http_srv_response_owned(416, "range not satisfiable".to_string()));
    };
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(start as u64))
        .map_err(|error| format!("static file `{path}` seek failed: {error}"))?;
    let length = end - start + 1;
    let mut response = jet_http_srv_response(206, &String::new());
    response.body = JetHTTPBody::file(file, length);
    let resp = jet_http_srv_response_header(
        response,
        &"content-type".to_string(),
        mime,
    );
    Ok(jet_http_srv_response_header(
        resp,
        &"content-range".to_string(),
        &format!("bytes {start}-{end}/{file_len}"),
    ))
}

fn jet_http_static_range(value: &str, len: usize) -> Option<(usize, usize)> {
    let spec = value.strip_prefix("bytes=")?;
    if len == 0 || spec.contains(',') { return None; }
    let (first, last) = spec.split_once('-')?;
    match (first.is_empty(), last.is_empty()) {
        (true, true) => None,
        (true, false) => {
            let suffix = last.parse::<usize>().ok()?;
            (suffix > 0).then_some((len.saturating_sub(suffix), len - 1))
        }
        (false, true) => {
            let start = first.parse::<usize>().ok()?;
            (start < len).then_some((start, len - 1))
        }
        (false, false) => {
            let start = first.parse::<usize>().ok()?;
            let end = last.parse::<usize>().ok()?.min(len - 1);
            (start < len && start <= end).then_some((start, end))
        }
    }
}

fn jet_http_static_mime(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(std::ffi::OsStr::to_str).unwrap_or("").to_ascii_lowercase().as_str() {
        "css" => "text/css; charset=utf-8",
        "gif" => "image/gif",
        "html" | "htm" => "text/html; charset=utf-8",
        "jpeg" | "jpg" => "image/jpeg",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn jet_http_days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * shifted_month + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn jet_http_civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn jet_http_date(seconds: i64) -> String {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days = seconds.div_euclid(86_400);
    let within = seconds.rem_euclid(86_400);
    let (year, month, day) = jet_http_civil_from_days(days);
    format!("{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        WEEKDAYS[(days + 4).rem_euclid(7) as usize], day, MONTHS[(month - 1) as usize], year,
        within / 3600, within / 60 % 60, within % 60)
}

fn jet_http_date_parse(value: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let parts = value.split_ascii_whitespace().collect::<Vec<_>>();
    if parts.len() != 6 || !parts[0].ends_with(',') || parts[5] != "GMT" { return None; }
    let day = parts[1].parse::<i64>().ok()?;
    let month = MONTHS.iter().position(|month| *month == parts[2])? as i64 + 1;
    let year = parts[3].parse::<i64>().ok()?;
    let time = parts[4].split(':').map(str::parse::<i64>).collect::<Result<Vec<_>, _>>().ok()?;
    if time.len() != 3 || !(0..24).contains(&time[0]) || !(0..60).contains(&time[1]) || !(0..60).contains(&time[2]) { return None; }
    let seconds = jet_http_days_from_civil(year, month, day).checked_mul(86_400)?
        .checked_add(time[0] * 3600 + time[1] * 60 + time[2])?;
    (jet_http_date(seconds) == value).then_some(seconds)
}

fn jet_http_static_open(
    root: &std::path::Path,
    candidate: &std::path::Path,
) -> Option<(std::fs::File, std::fs::Metadata, std::path::PathBuf)> {
    // std exposes neither final-path-by-handle nor an openat-style no-reparse
    // walk on Windows. Pathname revalidation is not an identity guarantee, so
    // static serving fails closed there until the native bridge owns this open.
    #[cfg(windows)]
    {
        let _ = (root, candidate);
        return None;
    }
    #[cfg(not(windows))]
    {
    let canonical = std::fs::canonicalize(candidate).ok()?;
    if !canonical.starts_with(root) { return None; }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        const O_NOFOLLOW: i32 = 0o400000;
        options.custom_flags(O_NOFOLLOW);
    }
    let file = options.open(&canonical).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() { return None; }
    if std::fs::canonicalize(candidate).ok()? != canonical { return None; }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::fd::AsRawFd;
        let held = std::fs::canonicalize(format!("/proc/self/fd/{}", file.as_raw_fd())).ok()?;
        if !held.starts_with(root) { return None; }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let current = std::fs::metadata(&canonical).ok()?;
        if current.dev() != metadata.dev() || current.ino() != metadata.ino() { return None; }
    }
        Some((file, metadata, canonical))
    }
}

/// D-HTTP-STATIC-FILES1=A: the policy one static mount serves under.
/// `safe()` is what `static_files(mux, prefix, root)` installs: serve
/// `index.html` for a directory request, hide dot-files, and refuse symbolic
/// links. A resolved path always has to stay under the root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JetHTTPStaticOptions {
    index: bool,
    dotfiles: bool,
    follow_links: bool,
}

impl JetHTTPStaticOptions {
    fn safe() -> Self {
        Self { index: true, dotfiles: false, follow_links: false }
    }
}

/// Remove a mount prefix from a request path. `None` means the path is not
/// under the prefix, and the caller answers 404.
fn jet_http_static_relative<'a>(prefix: &str, path: &'a str) -> Option<&'a str> {
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() {
        return Some(path);
    }
    let rest = path.strip_prefix(prefix)?;
    if rest.is_empty() || rest.starts_with('/') { Some(rest) } else { None }
}

fn jet_http_srv_static_files(
    req: &JetHTTPRequest,
    prefix: &str,
    root: &std::path::Path,
    options: JetHTTPStaticOptions,
) -> Result<JetHTTPResponse, String> {
    let not_found = || Ok(jet_http_srv_empty_response(404));
    if !matches!(req.method.as_str(), "GET" | "HEAD") { return Ok(jet_http_srv_empty_response(405)); }
    let root = match std::fs::canonicalize(root) { Ok(root) => root, Err(_) => return not_found() };
    let path = req.path.split('?').next().unwrap_or(&req.path);
    let Some(path) = jet_http_static_relative(prefix, path) else { return not_found() };
    let mut candidate = root.clone();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        let Ok(segment) = jet_http_route_decode_segment(segment) else { return not_found() };
        if segment == "." || segment == ".." || segment.contains(std::path::MAIN_SEPARATOR) { return not_found(); }
        if !options.dotfiles && segment.starts_with('.') { return not_found(); }
        candidate.push(segment);
        let Ok(metadata) = std::fs::symlink_metadata(&candidate) else { return not_found() };
        if !options.follow_links && metadata.file_type().is_symlink() { return not_found(); }
    }
    if candidate.is_dir() {
        if !options.index { return not_found(); }
        candidate.push("index.html");
        let Ok(metadata) = std::fs::symlink_metadata(&candidate) else { return not_found() };
        if !options.follow_links && metadata.file_type().is_symlink() { return not_found(); }
    }
    let Some((mut file, metadata, canonical)) = jet_http_static_open(&root, &candidate) else { return not_found() };
    let file_len = usize::try_from(metadata.len()).map_err(|_| "static file is too large".to_string())?;
    let modified_seconds = metadata.modified().ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64).unwrap_or(0);
    let last_modified = jet_http_date(modified_seconds);
    let etag = format!("\"{:x}-{:x}\"", file_len, modified_seconds);
    if req.headers.all("if-none-match").iter().any(|value| *value == &etag || *value == "*") {
        let mut response = jet_http_srv_empty_response(304);
        response.headers.append("etag", &etag).expect("generated ETag is valid");
        response.headers.append("last-modified", &last_modified).expect("generated date is valid");
        return Ok(response);
    }
    if req.headers.get("if-none-match").is_none()
        && req.headers.get("if-modified-since").and_then(|value| jet_http_date_parse(value)).is_some_and(|since| modified_seconds <= since)
    {
        let mut response = jet_http_srv_empty_response(304);
        response.headers.append("etag", &etag).expect("generated ETag is valid");
        response.headers.append("last-modified", &last_modified).expect("generated date is valid");
        return Ok(response);
    }
    let range = req.headers.get("range").cloned().filter(|_| {
        req.headers.get("if-range").is_none_or(|value| value == &etag || value == &last_modified)
    });
    let (status, start, length, content_range) = if let Some(range) = range {
        let Some((start, end)) = jet_http_static_range(&range, file_len) else {
            let mut response = jet_http_srv_empty_response(416);
            response.headers.append("content-range", &format!("bytes */{file_len}")).expect("generated range is valid");
            return Ok(response);
        };
        (206, start, end - start + 1, Some(format!("bytes {start}-{end}/{file_len}")))
    } else {
        (200, 0, file_len, None)
    };
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(start as u64))
        .map_err(|_| "static file seek failed".to_string())?;
    let mut response = jet_http_srv_empty_response(status);
    response.body = JetHTTPBody::file(file, length);
    response.headers.append("content-type", jet_http_static_mime(&canonical)).expect("static MIME is valid");
    response.headers.append("accept-ranges", "bytes").expect("static header is valid");
    response.headers.append("etag", &etag).expect("generated ETag is valid");
    response.headers.append("last-modified", &last_modified).expect("generated date is valid");
    if let Some(content_range) = content_range {
        response.headers.append("content-range", &content_range).expect("generated range is valid");
    }
    Ok(response)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetHTTPAccessEvent {
    request_id: String,
    method: String,
    path: String,
    route_template: String,
    status: i64,
    bytes: i64,
    duration_ms: i64,
    peer: String,
    protocol: String,
    tls: bool,
}

impl std::fmt::Display for JetHTTPAccessEvent {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "request_id={} method={} path={} route={} status={} bytes={} duration_ms={} peer={} protocol={} tls={}",
            self.request_id, self.method, self.path, self.route_template, self.status,
            self.bytes, self.duration_ms, self.peer, self.protocol, self.tls)
    }
}

fn jet_http_srv_access_event(
    req: &JetHTTPRequest,
    status: i64,
    bytes: i64,
    duration_ms: i64,
    peer: &str,
    protocol: &str,
    tls: bool,
) -> JetHTTPAccessEvent {
    let path = req.path.split('?').next().unwrap_or(&req.path).to_string();
    JetHTTPAccessEvent {
        request_id: req.headers.get("x-request-id").cloned().unwrap_or_default(),
        method: req.method.clone(),
        route_template: req.route_template.clone().unwrap_or_else(|| path.clone()),
        path,
        status,
        bytes,
        duration_ms,
        peer: peer.to_string(),
        protocol: protocol.to_string(),
        tls,
    }
}

fn jet_http_srv_access_log(req: &JetHTTPRequest, status: i64) -> String {
    let route = req.route_template.as_deref().unwrap_or_else(|| req.path.split('?').next().unwrap_or(&req.path));
    format!("{} {} {}", req.method, route, status)
}

/// D-HTTP-SERVER2 built-in `request_id` middleware: ordinary Handler wrapper.
/// Keeps a valid inbound `x-request-id`, otherwise assigns one, and echoes it
/// on responses reached through its declaration-ordered layer.
fn jet_http_request_id_valid(value: &str) -> bool {
    let len = value.len();
    (1..=128).contains(&len) && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn jet_http_new_request_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("req-{nanos:x}-{seq:x}")
}

fn jet_http_srv_request_id(next: JetHTTPHandler) -> JetHTTPHandler {
    std::sync::Arc::new(move |mut request| {
        let id = match request.headers.get("x-request-id") {
            Some(value) if jet_http_request_id_valid(value) => value.clone(),
            _ => jet_http_new_request_id(),
        };
        let _ = request.headers.set("x-request-id", &id);
        let mut response = next(request)?;
        if response.headers.get("x-request-id").is_none() {
            let _ = response.headers.set("x-request-id", &id);
        }
        Ok(response)
    })
}

fn jet_http_srv_install_request_id(mux: &JetHTTPMux) {
    jet_http_mux_middleware(
        mux,
        std::sync::Arc::new(jet_http_srv_request_id) as JetHTTPMiddleware,
    );
}

// ── D-HTTP-HANDLER-MW1=A: nested core.http.middleware Handler wrappers ───────

/// D-HTTP-CORS1=A: which origins a CORS policy answers for.
#[derive(Clone, Debug)]
enum JetHTTPCorsOrigins {
    Any,
    List(Vec<String>),
}

#[derive(Clone, Debug)]
struct JetHTTPCorsPolicy {
    origins: JetHTTPCorsOrigins,
    allow_methods: String,
    allow_headers: String,
    credentials: bool,
    max_age_secs: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetHTTPCompressEncoding {
    Gzip,
}

const JET_HTTP_CORS_DEFAULT_METHODS: &str = "GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS";
const JET_HTTP_CORS_DEFAULT_HEADERS: &str = "content-type, authorization, x-request-id";

/// D-HTTP-CORS1=A: build a CORS policy. An `.Any` origin with credentials is
/// refused here, because that combination would open the API to every website.
fn jet_http_cors_policy(
    origins: &JetHTTPCorsOrigins,
    methods: &Vec<String>,
    headers: &Vec<String>,
    credentials: bool,
    max_age: i64,
) -> Result<JetHTTPCorsPolicy, JetHTTPError> {
    if credentials && matches!(origins, JetHTTPCorsOrigins::Any) {
        return Err(JetHTTPError::Policy {
            reason: "CORS credentials need named origins. An `.Any` origin with credentials \
                     would let every website read this API, and browsers refuse the pair. \
                     List the origins you trust, or set credentials to false."
                .to_string(),
        });
    }
    let join = |values: &Vec<String>, fallback: &str| {
        if values.is_empty() { fallback.to_string() } else { values.join(", ") }
    };
    Ok(JetHTTPCorsPolicy {
        origins: origins.clone(),
        allow_methods: join(methods, JET_HTTP_CORS_DEFAULT_METHODS),
        allow_headers: join(headers, JET_HTTP_CORS_DEFAULT_HEADERS),
        credentials,
        max_age_secs: max_age,
    })
}

fn jet_http_cors_policy_defaulted(
    origins: &JetHTTPCorsOrigins,
    methods: Option<&Vec<String>>,
    headers: Option<&Vec<String>>,
    credentials: Option<bool>,
    max_age: Option<i64>,
) -> Result<JetHTTPCorsPolicy, JetHTTPError> {
    let empty_methods = Vec::new();
    let empty_headers = Vec::new();
    jet_http_cors_policy(
        origins,
        methods.unwrap_or(&empty_methods),
        headers.unwrap_or(&empty_headers),
        credentials.unwrap_or(false),
        max_age.unwrap_or(86_400),
    )
}

/// The value for `access-control-allow-origin`, or `None` when this request
/// origin is not in the policy and no CORS header may be sent.
fn jet_http_cors_allow_origin(policy: &JetHTTPCorsPolicy, origin: &str) -> Option<String> {
    match &policy.origins {
        JetHTTPCorsOrigins::Any => Some("*".to_string()),
        JetHTTPCorsOrigins::List(list) => list
            .iter()
            .any(|allowed| allowed == origin)
            .then(|| origin.to_string()),
    }
}

fn jet_http_request_deadline_report(rendered: &str) -> JetErrorReport {
    let message = rendered
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("Error [E3003]: "))
        .filter(|message| !message.is_empty())
        .unwrap_or("Deadline exceeded while waiting in HTTP request handler")
        .to_string();
    JetErrorReport {
        code: Some("E3003".to_string()),
        message,
        typed_identity: None,
        causes: Vec::new(),
        context_frames: Vec::new(),
        source_journey: Vec::new(),
        conversion_history: Vec::new(),
        details: None,
    }
}

fn jet_http_request_deadline_response(
    request: &JetHTTPRequest,
    rendered: &str,
) -> JetHTTPResponse {
    let report = jet_http_request_deadline_report(rendered);
    jet_http_error_page_response(request, 503, "", &report, &[], true)
}

/// Run one route inside the shared `#Context(deadline: …)` and task-control
/// boundary. The child owns the deadline guard; the parent only waits for its
/// typed completion, so a CPU-bound handler observes expiry at its next wait
/// point instead of receiving a second, detached timeout signal.
fn jet_http_request_context_deadline(
    request: JetHTTPRequest,
    budget_ms: i64,
    next: JetHTTPHandler,
) -> Result<JetHTTPResponse, JetHTTPError> {
    let request_for_error = request.clone();
    let configured_deadline = jet_std_time_now().saturating_add(budget_ms.max(0));
    let deadline = jet_ctx_deadline_ms()
        .map_or(configured_deadline, |ambient| ambient.min(configured_deadline));
    let control = JetTaskControl::new();
    let join = jet_scheduler_spawn_blocking_with_control(
        move || {
            let _deadline = jet_ctx_push_deadline(deadline);
            next(request)
        },
        control.clone(),
    );
    let _wait = JetHTTPSchedulerBlockingWait::enter();
    loop {
        if let Some(result) = join.try_recv() {
            return match result {
                JetSchedulerResult::Value(response) => response,
                JetSchedulerResult::Cancelled => Err(JetHTTPError::Cancelled),
                JetSchedulerResult::Panicked(_) => {
                    let request_id = jet_http_devtools_request_id(&request_for_error);
                    jet_http_devtools_publish_exception_code(&request_id, "panic");
                    Ok(jet_http_srv_internal_response())
                },
                JetSchedulerResult::Deadline(rendered) => {
                    Ok(jet_http_request_deadline_response(&request_for_error, &rendered))
                }
            };
        }
        if jet_scheduler_wait_point_cancelled() {
            control.cancel();
            join.drain();
            jet_task_deliver_cancel();
            return Err(JetHTTPError::Cancelled);
        }
        std::thread::yield_now();
    }
}

fn jet_http_mw_timeout(duration: &jet_std::Duration, next: JetHTTPHandler) -> JetHTTPHandler {
    let budget_ms = duration.as_millis().max(0);
    std::sync::Arc::new(move |request| {
        jet_http_request_context_deadline(request, budget_ms, next.clone())
    })
}

/// Clone a mux and attach the shared Context deadline wrapper. The duration is
/// owned by the middleware so the request handler remains `'static` on every
/// serving tier.
fn jet_http_mux_with_deadline(
    mux: JetHTTPMux,
    deadline: &jet_std::Duration,
) -> JetHTTPMux {
    let deadline = deadline.clone();
    jet_http_mux_middleware(
        &mux,
        std::sync::Arc::new(move |next| jet_http_mw_timeout(&deadline, next)),
    );
    mux
}
/// Apply a request deadline only when the named option is present. Keeping
/// this branch beside `jet_http_mux_with_deadline` makes explicit-address
/// servers use the same per-request Context kernel as `http.serve`.
fn jet_http_mux_with_optional_deadline(
    mux: JetHTTPMux,
    deadline: Option<&jet_std::Duration>,
) -> JetHTTPMux {
    deadline.map_or(mux.clone(), |deadline| {
        jet_http_mux_with_deadline(mux.clone(), deadline)
    })
}
fn jet_http_mw_body_limit(max_bytes: i64, next: JetHTTPHandler) -> JetHTTPHandler {
    std::sync::Arc::new(move |req| {
        if !jet_http_srv_req_under_limit(&req, max_bytes) {
            return Err(JetHTTPError::BodyTooLarge { limit: max_bytes });
        }
        next.clone()(req)
    })
}

/// Keep any existing `Vary` tokens and add `origin` once. CORS must not erase a
/// handler's own vary list.
fn jet_http_cors_stamp_vary(headers: &mut JetHTTPHeaders) {
    match headers.get("vary") {
        Some(existing) => {
            let already = existing
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("origin"));
            if !already {
                let merged = format!("{existing}, origin");
                let _ = headers.set("vary", &merged);
            }
        }
        None => {
            let _ = headers.set("vary", "origin");
        }
    }
}

fn jet_http_mw_cors(policy: &JetHTTPCorsPolicy, next: JetHTTPHandler) -> JetHTTPHandler {
    let policy = policy.clone();
    std::sync::Arc::new(move |req| {
        let origin = req.headers.get("origin").cloned();
        let allow = origin
            .as_deref()
            .and_then(|origin| jet_http_cors_allow_origin(&policy, origin));
        // A preflight is an OPTIONS request that names the method it is asking
        // about. Every other OPTIONS request keeps its normal route answer.
        let preflight = req.method == "OPTIONS"
            && req.headers.get("access-control-request-method").is_some();
        if preflight {
            let mut response = jet_http_srv_empty_response(204);
            jet_http_cors_stamp_vary(&mut response.headers);
            if let Some(allow) = allow {
                let _ = response.headers.set("access-control-allow-origin", &allow);
                let _ = response.headers.set("access-control-allow-methods", &policy.allow_methods);
                let _ = response.headers.set("access-control-allow-headers", &policy.allow_headers);
                if policy.credentials {
                    let _ = response.headers.set("access-control-allow-credentials", "true");
                }
                if policy.max_age_secs > 0 {
                    let _ = response.headers.set("access-control-max-age", &policy.max_age_secs.to_string());
                }
            }
            return Ok(response);
        }
        let mut response = next.clone()(req)?;
        if let Some(allow) = allow {
            let _ = response.headers.set("access-control-allow-origin", &allow);
            jet_http_cors_stamp_vary(&mut response.headers);
            if policy.credentials {
                let _ = response.headers.set("access-control-allow-credentials", "true");
            }
        }
        Ok(response)
    })
}

/// D-HTTP-CORS1=A: install the policy on a mux. No call means no CORS headers.
fn jet_http_srv_install_cors(mux: &JetHTTPMux, policy: &JetHTTPCorsPolicy) {
    let policy = policy.clone();
    jet_http_mux_middleware(
        mux,
        std::sync::Arc::new(move |next| jet_http_mw_cors(&policy, next)) as JetHTTPMiddleware,
    );
}

fn jet_http_gzip_stored(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() + 32);
    out.extend_from_slice(&[0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0x00, 0xff]);
    let mut pos = 0usize;
    while pos < input.len() {
        let chunk = (input.len() - pos).min(65_535);
        let final_block = pos + chunk >= input.len();
        out.push(if final_block { 0x01 } else { 0x00 });
        let len = chunk as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&input[pos..pos + chunk]);
        pos += chunk;
    }
    out.extend_from_slice(&jet_http_crc32(input).to_le_bytes());
    out.extend_from_slice(&(input.len() as u32).to_le_bytes());
    out
}

fn jet_http_mw_compress(encoding: JetHTTPCompressEncoding, next: JetHTTPHandler) -> JetHTTPHandler {
    std::sync::Arc::new(move |req| {
        let accepts_gzip = req
            .headers
            .get("accept-encoding")
            .is_some_and(|value| value.split(',').any(|part| part.trim().eq_ignore_ascii_case("gzip")));
        let mut response = next.clone()(req)?;
        if !accepts_gzip || encoding != JetHTTPCompressEncoding::Gzip {
            return Ok(response);
        }
        if response.suppress_body {
            return Ok(response);
        }
        let plain = match response.body.bytes(JET_HTTP_MAX_BODY_BYTES) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(response),
        };
        if plain.is_empty() {
            return Ok(response);
        }
        let compressed = jet_http_gzip_stored(&plain);
        response.body = JetHTTPBody::from_bytes(compressed);
        let _ = response.headers.set("content-encoding", "gzip");
        response.headers.remove("content-length");
        Ok(response)
    })
}

fn jet_http_mw_access_log(next: JetHTTPHandler) -> JetHTTPHandler {
    std::sync::Arc::new(move |req| {
        let started = std::time::Instant::now();
        let method = req.method.clone();
        let path = req.path.split('?').next().unwrap_or(&req.path).to_string();
        let route = req.route_template.clone().unwrap_or_else(|| path.clone());
        let request_id = req.headers.get("x-request-id").cloned().unwrap_or_default();
        let response = next.clone()(req)?;
        let status = response.status;
        let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
        let line = format!(
            "request_id={request_id} method={method} path={path} route={route} status={status} duration_ms={duration_ms}"
        );
        jet_log_emit("info", &line, &[]);
        Ok(response)
    })
}

fn jet_http_mux_as_handler(mux: JetHTTPMux) -> JetHTTPHandler {
    std::sync::Arc::new(move |req| {
        Ok(jet_http_mux_dispatch(&mux, req).unwrap_or_else(jet_http_srv_error_response))
    })
}

fn jet_http_srv_static_files_handler(
    prefix: String,
    root: String,
    options: JetHTTPStaticOptions,
) -> JetHTTPHandler {
    std::sync::Arc::new(move |req| {
        jet_http_srv_static_files(&req, &prefix, std::path::Path::new(&root), options).map_err(|_| {
            JetHTTPError::IO { operation: "read static file".to_string() }
        })
    })
}

/// D-HTTP-STATIC-FILES1=A: mount `root` under `prefix` on a mux. The catch-all
/// route serves GET; the mux answers HEAD from the same handler.
fn jet_http_srv_static_files_mount(
    mux: &JetHTTPMux,
    prefix: &String,
    root: &String,
    options: JetHTTPStaticOptions,
) {
    let trimmed = prefix.trim_end_matches('/');
    let pattern = format!("{trimmed}/*jet_static_path");
    jet_http_mux_add_handler(
        mux,
        "GET",
        &pattern,
        jet_http_srv_static_files_handler(trimmed.to_string(), root.clone(), options),
    );
}

fn jet_http_srv_static_files_mount_defaulted(
    mux: &JetHTTPMux,
    prefix: &String,
    root: &String,
    index: Option<bool>,
    dotfiles: Option<bool>,
    follow_links: Option<bool>,
) {
    jet_http_srv_static_files_mount(
        mux,
        prefix,
        root,
        JetHTTPStaticOptions {
            index: index.unwrap_or(true),
            dotfiles: dotfiles.unwrap_or(false),
            follow_links: follow_links.unwrap_or(false),
        },
    );
}

pub(crate) fn jet_app_http_mux_new() -> JetHTTPMux {
    jet_http_mux_new()
}

/// Serve one App page: the App resolves the navigation, runs the loader,
/// decodes typed inputs, and chooses the page or boundary plus its status.
pub(crate) fn jet_app_http_page_response<F>(mux: &JetHTTPMux, path: &str, handler: F)
where
    F: Fn(&JetHTTPRequest) -> (i64, String) + Send + Sync + 'static,
{
    jet_http_mux_add(mux, "GET", path, move |request| {
        let (status, body) = handler(&request);
        let mut response = jet_http_srv_response(status, &body);
        response
            .headers
            .append("content-type", "text/html; charset=utf-8")
            .expect("static content type is valid");
        response
    });
}

/// The request target as the App router sees it: path plus query string.
pub(crate) fn jet_app_http_request_url(request: &JetHTTPRequest) -> String {
    request.path.clone()
}

pub(crate) fn jet_app_http_mount<F>(mux: &JetHTTPMux, path: &str, handler: F)
where
    F: Fn(&String) + Send + Sync + 'static,
{
    jet_http_mux_add(mux, "POST", path, move |request| {
        handler(&request.path);
        jet_http_srv_response_owned(200, "ok".to_string())
    });
}

pub(crate) fn jet_app_http_assets(mux: &JetHTTPMux, root: &String) {
    jet_http_srv_static_files_mount(
        mux,
        &"/assets".to_string(),
        root,
        JetHTTPStaticOptions::safe(),
    );
}

pub(crate) fn jet_app_http_reload(mux: &JetHTTPMux) {
    jet_http_mux_add(mux, "GET", "/__jet/reload", move |_| {
        let watched = std::env::var("JET_DEV_FILE").ok();
        let fingerprint = |path: &str| {
            std::fs::metadata(path)
                .ok()
                .and_then(|metadata| {
                    metadata
                        .modified()
                        .ok()
                        .map(|modified| (metadata.len(), modified))
                })
        };
        let initial = watched.as_deref().and_then(fingerprint);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let changed = loop {
            if watched
                .as_deref()
                .is_some_and(|path| fingerprint(path) != initial)
            {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let status = if changed { 200 } else { 204 };
        let body = if changed { "data: reload\n\n" } else { "" };
        let mut response = jet_http_srv_response(status, &body.to_string());
        response
            .headers
            .append("content-type", "text/event-stream")
            .expect("static content type is valid");
        response
    });
}

pub(crate) fn jet_app_http_serve(mux: JetHTTPMux, port: u16, dev: bool) {
    use std::io::Write;
    let server = jet_http_server_bind(&format!("127.0.0.1:{port}"), mux, None, None)
        .unwrap_or_else(|error| {
            let message = format!("web app server failed: {error}");
            jet_runtime_stop("E3001", "", 0, &message)
        });
    println!(
        "serving http://{}{}",
        jet_http_server_local_addr(&server)
            .unwrap_or_else(|error| {
                let message = format!("web app address failed: {error}");
                jet_runtime_stop("E3001", "", 0, &message)
            }),
        if dev { " (live reload)" } else { "" }
    );
    let _ = std::io::stdout().flush();
    jet_http_server_serve(&server)
        .unwrap_or_else(|error| {
            let message = format!("web app server failed: {error}");
            jet_runtime_stop("E3001", "", 0, &message)
        });
}

fn jet_http_srv_json_text<S: JetHTTPStatusCode>(status: S, body: &String) -> JetHTTPResponse {
    let mut response = jet_http_srv_response(status, body);
    let _ = response
        .headers
        .set("content-type", "application/json; charset=utf-8");
    response
}

/// D-HTTP-JSON1=A: one JSON response. The content type is set for the caller.
fn jet_http_srv_json<S: JetHTTPStatusCode, T: __jet_Encode>(status: S, value: &T) -> JetHTTPResponse {
    jet_http_srv_json_text(status, &jet_enc_json_to_string(value))
}


// ── Moved from NetHTTP.rs (HTTP serve/router; needs HTTPMessage/HTTPRoute) ──

// D-HTTP-ROUTE-SYNTAX2=A: both HTTP front doors use the shared route grammar.
type RouteSegment = JetHTTPRouteSegment;

// A router is an ordinary owned value: a task that captures one takes its own
// copy, so both halves stay cloneable. Handlers are already shared `Arc`s.
#[derive(Clone)]
pub(crate) struct JetHTTPRoute {
    pub(crate) method: String,
    pub(crate) template: String,
    segments: Vec<RouteSegment>,
    handler: JetHTTPHandler,
    /// Checked TIR OpenAPI facts. Runtime dispatch never derives contract
    /// metadata from handler behavior.
    pub(crate) contract_json: String,
}

#[derive(Clone)]
pub struct JetHTTPRouter {
    pub(crate) routes: Vec<JetHTTPRoute>,
}

impl JetShow for JetHTTPRequest {
    fn jet_show(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}
impl JetShow for JetHTTPResponse {
    fn jet_show(&self) -> String {
        format!("HTTP {}", self.status)
    }
}
impl JetShow for JetHTTPRouter {
    fn jet_show(&self) -> String {
        format!("HTTPRouter({} routes)", self.routes.len())
    }
}

// ── HTTP/1.1 server (blocking, one thread per connection) ────────────────────
// note: `jet serve` uses one task per connection. This is excellent for internal
//       services and tools at hundreds of concurrent connections. For very high
//       connection counts, Jet is not the right tool yet — see docs/services.md.

fn jet_http_serve<F>(addr: &String, handler: F)
where
    F: Fn(JetHTTPRequest) -> JetHTTPResponse + Send + Sync + 'static,
{
    use std::io::Write;
    let listener = std::net::TcpListener::bind(addr.as_str()).unwrap_or_else(|e| {
        eprintln!("E2801: Bind on `{}` failed: {}", addr, e);
        std::process::exit(1);
    });
    let handler = std::sync::Arc::new(handler);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("E2801: Accept failed: {}", e);
                continue;
            }
        };
        let h = handler.clone();
        std::thread::spawn(move || {
            let raw = match jet_http_srv_read(&mut stream) {
                Ok(raw) => raw,
                Err(error) => {
                    let _ = stream.write_all(jet_http_srv_read_error_response(&error).as_bytes());
                    return;
                }
            };
            let req = match jet_http_srv_parse(&raw) {
                Ok(req) => req,
                Err(error) => {
                    let _ = stream.write_all(jet_http_srv_read_error_response(&error).as_bytes());
                    return;
                }
            };
            let resp = jet_scheduler_with_root_control(|| h(req));
            let _ = jet_http_srv_write_response(&mut stream, &resp, "HTTP/1.1", true);
        });
    }
}

fn jet_http_parse_request(raw: &str) -> JetHTTPRequest {
    let normalized;
    let bytes = if jet_http_header_end(raw.as_bytes()).is_some() {
        raw.as_bytes()
    } else {
        normalized = format!("{}\r\n\r\n", raw.replace('\n', "\r\n"));
        normalized.as_bytes()
    };
    jet_http_srv_parse(bytes).unwrap_or_else(|_| {
        JetHTTPRequest::server("GET", "/".to_string(), Vec::new(), JetHTTPHeaders::new())
    })
}

pub(crate) fn jet_http_parse_request_carrier(
    raw: &str,
) -> (String, String, Vec<(String, String)>, Vec<u8>) {
    let request = jet_http_parse_request(raw);
    let body = request
        .body
        .bytes(jet_http_default_body_limit() as usize)
        .unwrap_or_default();
    (
        request.method,
        request.path,
        request.headers.entries,
        body,
    )
}
fn jet_http_format_response(resp: &JetHTTPResponse) -> String {
    jet_http_srv_format(resp)
}

// D-ROUTE1=A: router runtime ──────────────────────────────────────────────────

fn jet_http_router_new() -> JetHTTPRouter {
    JetHTTPRouter { routes: Vec::new() }
}

fn jet_http_router_parse_pattern(pattern: &str) -> Result<Vec<RouteSegment>, String> {
    jet_http_route_parse(pattern).map(|pattern| pattern.segments)
}

fn jet_http_router_register(
    router: &mut JetHTTPRouter,
    method: String,
    pattern: String,
    handler: JetHTTPHandler,
    file: &str,
    line: u32,
    contract_json: String,
) {
    jet_http_require_route_contract(&method, &pattern, &contract_json, file, line, false);
    // E2804 (runtime): duplicate method+pattern fails at registration time in
    // Jet-owned runtime voice, not a raw Rust panic banner.
    let segs = match jet_http_router_parse_pattern(&pattern) {
        Ok(segs) => segs,
        Err(message) => jet_panic(file, line, &message),
    };
    let is_dup = router.routes.iter().any(|r| {
        r.method == method
            && r.segments.len() == segs.len()
            && r.segments
                .iter()
                .zip(segs.iter())
                .all(|(a, b)| match (a, b) {
                    (RouteSegment::Static(x), RouteSegment::Static(y)) => x == y,
                    (RouteSegment::Param(_), RouteSegment::Param(_)) => true,
                    (RouteSegment::CatchAll(_), RouteSegment::CatchAll(_)) => true,
                    _ => false,
                })
    });
    if is_dup {
        jet_panic(
            file,
            line,
            &format!("E2804: duplicate route `{} {}`", method, pattern),
        );
    }
    router.routes.push(JetHTTPRoute {
        method,
        template: pattern,
        segments: segs,
        handler,
        contract_json,
    });
}

fn jet_http_router_dispatch(
    router: &JetHTTPRouter,
    req: JetHTTPRequest,
) -> Result<JetHTTPResponse, JetHTTPError> {
    let path_segs = match jet_http_route_path(&req.path) {
        Ok(path) => path,
        Err(_) => return Ok(jet_http_srv_response_owned(400, "400 bad request".to_string())),
    };
    let mut candidates: Vec<(usize, std::collections::BTreeMap<String, String>)> = Vec::new();
    for (i, route) in router.routes.iter().enumerate() {
        let pattern = JetHTTPRoutePattern { segments: route.segments.clone() };
        if let Some(params) = jet_http_route_match(&pattern, &path_segs) {
            candidates.push((i, params));
        }
    }
    if candidates.is_empty() {
        return Ok(jet_http_srv_response_owned(404, "404 not found".to_string()));
    }
    let method_match = candidates
        .iter()
        .filter(|(i, _)| router.routes[*i].method == req.method)
        .max_by(|(left, _), (right, _)| {
            let left_pattern = JetHTTPRoutePattern { segments: router.routes[*left].segments.clone() };
            let right_pattern = JetHTTPRoutePattern { segments: router.routes[*right].segments.clone() };
            jet_http_route_selection_cmp(&left_pattern, *left, &right_pattern, *right)
        });
    let Some((route_idx, params)) = method_match else {
        return Ok(jet_http_srv_response_owned(405, "405 method not allowed".to_string()));
    };
    let route = &router.routes[*route_idx];
    let mut req2 = req;
    req2.params = params.clone();
    req2.route_template = Some(route.template.clone());
    (route.handler)(req2)
}

fn jet_http_serve_router(addr: &String, router: JetHTTPRouter) {
    let router = std::sync::Arc::new(router);
    jet_http_serve(addr, move |request| {
        jet_http_router_dispatch(&router, request)
            .unwrap_or_else(|_| jet_http_srv_response_owned(500, "500 Internal Server Error".to_string()))
    })
}

fn jet_http_request_param(req: &JetHTTPRequest, name: &String) -> Option<String> {
    req.params.get(name.as_str()).cloned()
}
