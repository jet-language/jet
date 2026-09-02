use std::fs;
use std::io::{Read, Write};

use jet::Interpreter::dev_iteration;
use jet_foundation::JitBackend::RunOutcome;

mod common;

struct Output {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

fn on_large_stack(work: impl FnOnce() + Send) {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn_scoped(scope, work)
            .unwrap()
            .join()
            .unwrap();
    });
}

fn skip_if_cranelift_host_unsupported() -> bool {
    if jet_jit::cranelift_host_supported() {
        false
    } else if std::env::var("JET_REQUIRE_CRANELIFT_HOST").as_deref() == Ok("1") {
        panic!("Cranelift host required but unavailable");
    } else {
        true
    }
}

fn run(source: &str, name: &str) -> Output {
    run_with_mode(source, name, false)
}

fn run_with_mode(source: &str, name: &str, use_interpreter: bool) -> Output {
    let dir = common::unique_tmp(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"http_i9\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [FS, IO, Mem.Alloc, Net] } }\n",
    )
    .unwrap();
    let file = dir.join("main.jet");
    fs::write(&file, source).unwrap();
    jet_jit::reset_jit_trace_for_test();
    let outcome = dev_iteration(file.to_str().unwrap(), false, use_interpreter);
    let output = match outcome {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Output {
            stdout,
            stderr,
            exit_code,
        },
        RunOutcome::Problems(diags) => panic!("{name} failed: {diags:#?}"),
    };
    let _ = fs::remove_dir_all(dir);
    output
}
fn read_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let mut request = Vec::new();
    let mut chunk = [0u8; 512];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
    }
    request
}
const HTTP_RESPONSE_BODY_LIMIT: usize = 64 * 1024 * 1024;
const GZIP_RESPONSE_BODY_LIMIT: usize = 8 * 1024 * 1024;

fn write_repeated_http_body(stream: &mut std::net::TcpStream, mut remaining: usize) {
    const BLOCK: [u8; 64 * 1024] = [b'x'; 64 * 1024];
    while remaining > 0 {
        let count = remaining.min(BLOCK.len());
        if stream.write_all(&BLOCK[..count]).is_err() {
            return;
        }
        remaining -= count;
    }
}

fn write_chunked_http_body(stream: &mut std::net::TcpStream, mut remaining: usize) {
    const BLOCK: [u8; 64 * 1024] = [b'x'; 64 * 1024];
    while remaining > 0 {
        let count = remaining.min(BLOCK.len());
        if write!(stream, "{count:x}\r\n").is_err()
            || stream.write_all(&BLOCK[..count]).is_err()
            || stream.write_all(b"\r\n").is_err()
        {
            return;
        }
        remaining -= count;
    }
    let _ = stream.write_all(b"0\r\n\r\n");
}

struct GzipBits {
    bytes: Vec<u8>,
    bit: u8,
}

impl GzipBits {
    fn write(&mut self, value: u32, bits: u8) {
        for offset in 0..bits {
            if self.bit == 0 {
                self.bytes.push(0);
            }
            if value & (1 << offset) != 0 {
                let last = self.bytes.len() - 1;
                self.bytes[last] |= 1 << self.bit;
            }
            self.bit = (self.bit + 1) % 8;
        }
    }
}

fn gzip_reverse_bits(mut code: u32, bits: u8) -> u32 {
    let mut reversed = 0;
    for _ in 0..bits {
        reversed = (reversed << 1) | (code & 1);
        code >>= 1;
    }
    reversed
}

fn gzip_fixed_code(symbol: usize) -> (u32, u8) {
    let (code, bits) = match symbol {
        0..=143 => (0x30 + symbol as u32, 8),
        144..=255 => (0x190 + (symbol - 144) as u32, 9),
        256..=279 => ((symbol - 256) as u32, 7),
        280..=287 => (0xc0 + (symbol - 280) as u32, 8),
        _ => unreachable!("fixed DEFLATE symbol"),
    };
    (gzip_reverse_bits(code, bits), bits)
}

fn gzip_length_code(length: usize) -> (usize, u32, u8) {
    const BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83,
        99, 115, 131, 163, 195, 227, 258,
    ];
    const EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5,
        5, 0,
    ];
    for (index, (&base, &extra)) in BASE.iter().zip(EXTRA.iter()).enumerate() {
        let max = base + ((1usize << extra) - 1);
        if length <= max {
            return (257 + index, (length - base) as u32, extra);
        }
    }
    unreachable!("DEFLATE match length")
}

fn gzip_crc32_repeated(byte: u8, length: usize) -> u32 {
    let mut table = [0u32; 256];
    for (index, slot) in table.iter_mut().enumerate() {
        let mut crc = index as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
        *slot = crc;
    }

    let mut crc = !0u32;
    for _ in 0..length {
        crc = (crc >> 8) ^ table[((crc as u8) ^ byte) as usize];
    }
    !crc
}

/// Build a valid, compact gzip body that expands past the HTTP decoder budget.
fn gzip_fixed_repeat_bomb(output_len: usize, byte: u8) -> Vec<u8> {
    assert!(output_len > 0);
    let mut frame = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
    let mut bits = GzipBits {
        bytes: Vec::new(),
        bit: 0,
    };
    bits.write(1, 1);
    bits.write(1, 2);

    let (literal, literal_bits) = gzip_fixed_code(byte as usize);
    bits.write(literal, literal_bits);
    let mut remaining = output_len - 1;
    while remaining >= 3 {
        let length = remaining.min(258);
        let (symbol, extra, extra_bits) = gzip_length_code(length);
        let (code, code_bits) = gzip_fixed_code(symbol);
        bits.write(code, code_bits);
        bits.write(extra, extra_bits);
        bits.write(0, 5);
        remaining -= length;
    }
    for _ in 0..remaining {
        bits.write(literal, literal_bits);
    }
    let (end, end_bits) = gzip_fixed_code(256);
    bits.write(end, end_bits);
    frame.extend_from_slice(&bits.bytes);
    frame.extend_from_slice(&gzip_crc32_repeated(byte, output_len).to_le_bytes());
    frame.extend_from_slice(&(output_len as u32).to_le_bytes());
    frame
}

fn serve_hostile_http_response(
    stream: &mut std::net::TcpStream,
    request: &[u8],
    gzip_body: &[u8],
) {
    if request
        .windows(b"/close-get".len())
        .any(|window| window == b"/close-get")
        || request
            .windows(b"/close-request".len())
            .any(|window| window == b"/close-request")
    {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
        write_repeated_http_body(stream, HTTP_RESPONSE_BODY_LIMIT + 1);
    } else if request
        .windows(b"/chunked-get".len())
        .any(|window| window == b"/chunked-get")
        || request
            .windows(b"/chunked-request".len())
            .any(|window| window == b"/chunked-request")
    {
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
        );
        write_chunked_http_body(stream, HTTP_RESPONSE_BODY_LIMIT + 1);
    } else if request
        .windows(b"/gzip-get".len())
        .any(|window| window == b"/gzip-get")
        || request
            .windows(b"/gzip-request".len())
            .any(|window| window == b"/gzip-request")
    {
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            gzip_body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(gzip_body);
    } else {
        panic!("unknown hostile HTTP request: {:?}", String::from_utf8_lossy(request));
    }
    let _ = stream.flush();
}



const FORCED_DEOPT: &str = r#"
use core.http.client as http_client
use core.http.server as http_server
use core.text as text

#Codable
struct Reading {
    city: String
    degrees: Int
}

fn any_origins() HTTPCorsOrigins -> {
    return .Any
}

fn run() {
    print(text.casefold("Straße"))
    mux :: http_server.mux()
    mux.get("/health", () -> Ok(http_server.response(204, "")))
    policy :: http_server.cors_policy(["https://app.example"]) ?? panic("cors")
    http_server.cors(mux, policy)
    http_server.static_files(mux, "/assets", "/tmp")
    response :: http_server.json(201, Reading{city: "Reno", degrees: 32})
    response_text :: http_server.response(200, "hello")
    decoded :: response.json<Reading>() ?? panic("response")
    request :: http_client.request("POST", "http://example.test/")
        .json(Reading{city: "Reno", degrees: 31})
    request_decoded :: request.json<Reading>() ?? panic("request")
    request_content_type :: request.header("content-type") ?? "missing"
    print("{decoded.city}|{request_decoded.degrees}|{request_content_type}|{response_text.text() ?? "invalid body"}")
    malformed :: http_client.request("POST", "http://example.test/").body("{{")
    if malformed.json<Reading>() == {
        .Ok(_) -> print("accepted")
        .Err(error) -> print(error)
        else -> print("unexpected")
    }
    if http_server.cors_policy(any_origins(), [], [], true) == {
        .Ok(_) -> print("accepted")
        .Err(error) -> print(error)
        else -> print("unexpected")
    }
}
"#;

const ROUTE_HANDLERS: &str = r#"
use core.http.client as http
use core.http.server as server
use core.net as net

fn route_error(req: HTTPRequest) HTTPResponse !HTTPError -> {
    if req.path() == "/error" -> return Err(.InvalidFraming)
    return Err(.InvalidFraming)
}

fn run() !(HTTPError | NetError | TaskFailure) {
    listener :: net.tcp_listen("127.0.0.1:0")
    address :: listener.local_addr()
    state :: "captured"
    mux :: server.mux()
    mux.get("/zero", () -> Ok(server.response(200, "zero")))
    mux.get("/items/:id", (req: HTTPRequest) HTTPResponse !HTTPError -> {
        header :: req.header("x-state") ?? "missing"
        id :: req.param("id") ?? "missing"
        path :: req.path()
        return Ok(server.response(200, "{path}|{id}|{header}|{state}"))
    })
    mux.get("/error", route_error)
    t :: task {
        server.serve_once_listener(listener, mux)
        server.serve_once_listener(listener, mux)
        server.serve_once_listener(listener, mux)
    }
    zero :: http.request("GET", "http://{address}/zero").send() ?? panic("zero")
    print(zero.text() ?? "zero body")
    detail :: http.request("GET", "http://{address}/items/42")
        .header("x-state", "captured-header")
        .send() ?? panic("detail")
    print(detail.text() ?? "detail body")
    failed :: http.request("GET", "http://{address}/error").send() ?? panic("error")
    print(failed.status())
    t.join()
}
"#;

const HOSTILE_URLS: &str = r#"
use core.http.client as http_client
use core.net.ws as ws

fn run() {
    if http_client.get("__HTTP_URL__/path\nInjected: yes") == {
        .Ok(_) -> print("http url accepted")
        .Err(error) -> {
            if error == .InvalidUrl -> print("http url rejected")
            else -> print("http url wrong")
        }
        else -> print("http url unexpected")
    }
    method :: http_client.request("GET\nInjected", "__HTTP_URL__/method")
    if method.send() == {
        .Ok(_) -> print("method accepted")
        .Err(error) -> {
            if error == .InvalidHeader -> print("method rejected")
            else -> print("method wrong")
        }
        else -> print("method unexpected")
    }
    header_name :: http_client.request("GET", "__HTTP_URL__/header-name")
        .header("x-bad\nname", "value")
    if header_name.send() == {
        .Ok(_) -> print("header name accepted")
        .Err(error) -> {
            if error == .InvalidHeader -> print("header name rejected")
            else -> print("header name wrong")
        }
        else -> print("header name unexpected")
    }
    header_value :: http_client.request("GET", "__HTTP_URL__/header-value")
        .header("x-safe", "bad\nvalue")
    if header_value.send() == {
        .Ok(_) -> print("header value accepted")
        .Err(error) -> {
            if error == .InvalidHeader -> print("header value rejected")
            else -> print("header value wrong")
        }
        else -> print("header value unexpected")
    }
    content_length :: http_client.request("POST", "__HTTP_URL__/content-length")
        .header("Content-Length", "0")
        .body("body")
    if content_length.send() == {
        .Ok(_) -> print("content length accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("content length rejected")
            else -> print("content length wrong")
        }
        else -> print("content length unexpected")
    }
    transfer_encoding :: http_client.request("POST", "__HTTP_URL__/transfer-encoding")
        .header("Transfer-Encoding", "chunked")
        .body("body")
    if transfer_encoding.send() == {
        .Ok(_) -> print("transfer encoding accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("transfer encoding rejected")
            else -> print("transfer encoding wrong")
        }
        else -> print("transfer encoding unexpected")
    }
    duplicate_host :: http_client.request("GET", "__HTTP_URL__/duplicate-host")
        .header("Host", "one")
        .header("hOsT", "two")
    if duplicate_host.send() == {
        .Ok(_) -> print("duplicate host accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("duplicate host rejected")
            else -> print("duplicate host wrong")
        }
        else -> print("duplicate host unexpected")
    }
    connection_host :: http_client.request("GET", "__HTTP_URL__/connection-host")
        .header("cOnNeCtIoN", "hOsT")
    if connection_host.send() == {
        .Ok(_) -> print("connection host accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("connection host rejected")
            else -> print("connection host wrong")
        }
        else -> print("connection host unexpected")
    }
    connection_length :: http_client.request("POST", "__HTTP_URL__/connection-length")
        .header("Connection", "Content-Length")
        .body("body")
    if connection_length.send() == {
        .Ok(_) -> print("connection length accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("connection length rejected")
            else -> print("connection length wrong")
        }
        else -> print("connection length unexpected")
    }
    connection_transfer :: http_client.request("POST", "__HTTP_URL__/connection-transfer")
        .header("Connection", "tRaNsFeR-EnCoDiNg")
        .body("body")
    if connection_transfer.send() == {
        .Ok(_) -> print("connection transfer accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("connection transfer rejected")
            else -> print("connection transfer wrong")
        }
        else -> print("connection transfer unexpected")
    }
    proxy_connection :: http_client.request("GET", "__HTTP_URL__/proxy-connection")
        .header("pRoXy-CoNnEcTiOn", "keep-alive")
    if proxy_connection.send() == {
        .Ok(_) -> print("proxy connection accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("proxy connection rejected")
            else -> print("proxy connection wrong")
        }
        else -> print("proxy connection unexpected")
    }
    keep_alive :: http_client.request("GET", "__HTTP_URL__/keep-alive")
        .header("Keep-Alive", "timeout=5")
    if keep_alive.send() == {
        .Ok(_) -> print("keep-alive accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("keep-alive rejected")
            else -> print("keep-alive wrong")
        }
        else -> print("keep-alive unexpected")
    }
    te :: http_client.request("GET", "__HTTP_URL__/te")
        .header("TE", "trailers")
    if te.send() == {
        .Ok(_) -> print("te accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("te rejected")
            else -> print("te wrong")
        }
        else -> print("te unexpected")
    }
    trailer :: http_client.request("GET", "__HTTP_URL__/trailer")
        .header("Trailer", "x-trailer")
    if trailer.send() == {
        .Ok(_) -> print("trailer accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("trailer rejected")
            else -> print("trailer wrong")
        }
        else -> print("trailer unexpected")
    }
    upgrade :: http_client.request("GET", "__HTTP_URL__/upgrade")
        .header("Upgrade", "websocket")
    if upgrade.send() == {
        .Ok(_) -> print("upgrade accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("upgrade rejected")
            else -> print("upgrade wrong")
        }
        else -> print("upgrade unexpected")
    }
    proxy_authenticate :: http_client.request("GET", "__HTTP_URL__/proxy-authenticate")
        .header("Proxy-Authenticate", "Basic")
    if proxy_authenticate.send() == {
        .Ok(_) -> print("proxy authenticate accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("proxy authenticate rejected")
            else -> print("proxy authenticate wrong")
        }
        else -> print("proxy authenticate unexpected")
    }
    proxy_authorization :: http_client.request("GET", "__HTTP_URL__/proxy-authorization")
        .header("pRoXy-AuThOrIzAtIoN", "Basic secret")
    if proxy_authorization.send() == {
        .Ok(_) -> print("proxy authorization accepted")
        .Err(error) -> {
            if error == .InvalidFraming -> print("proxy authorization rejected")
            else -> print("proxy authorization wrong")
        }
        else -> print("proxy authorization unexpected")
    }
    if ws.connect("__WS_URL__/path\nInjected: yes") == {
        .Ok(_) -> print("ws accepted")
        .Err(error) -> {
            if error == .InvalidUrl -> print("ws rejected")
            else -> print("ws wrong error")
        }
        else -> print("ws unexpected")
    }
}
"#;
const HTTP_TEXT_ERROR_BODIES: &str = r#"
use core.http.client as http

fn response_text_default(response: HTTPResponse) String !Never -> {
    if response.text() == {
        .Ok(text) -> return "ok:{text.len()}"
        .Err(error) -> {
            if error == {
                .BodyTooLarge(limit) -> return "err:BodyTooLarge:{limit}"
                .UnsupportedEncoding -> return "err:UnsupportedEncoding"
                .IO(operation) -> return "err:IO:{operation}"
                .BodyConsumed -> return "err:BodyConsumed"
                else -> return "err:other"
            }
        }
        else -> return "err:unexpected"
    }
}

fn response_text_explicit(response: HTTPResponse) String !Never -> {
    if response.text(5) == {
        .Ok(text) -> return "ok:{text}"
        .Err(error) -> {
            if error == {
                .BodyTooLarge(limit) -> return "err:BodyTooLarge:{limit}"
                .BodyConsumed -> return "err:BodyConsumed"
                else -> return "err:other"
            }
        }
        else -> return "err:unexpected"
    }
}

fn response_text_consumed(response: HTTPResponse) String !Never -> {
    if response.text() == {
        .Ok(_) -> {
            if response.text() == {
                .Ok(_) -> return "ok:reused"
                .Err(error) -> {
                    if error == {
                        .BodyConsumed -> return "err:BodyConsumed"
                        else -> return "err:other"
                    }
                }
                else -> return "err:unexpected"
            }
        }
        .Err(_) -> return "err:first-read"
        else -> return "err:first-read"
    }
}

fn response_text_stream(response: HTTPResponse) String !Never -> {
    if response.text() == {
        .Ok(text) -> return "ok:{text}"
        .Err(error) -> {
            if error == {
                .BodyConsumed -> return "err:BodyConsumed"
                else -> return "err:other"
            }
        }
        else -> return "err:unexpected"
    }
}

fn run() {
    if http.get("__URL__/default") == {
        .Ok(response) -> print("default={response_text_default(response)}")
        .Err(_) -> print("default=request-error")
        else -> print("default=unexpected")
    }
    if http.get("__URL__/explicit") == {
        .Ok(response) -> print("explicit={response_text_explicit(response)}")
        .Err(_) -> print("explicit=request-error")
        else -> print("explicit=unexpected")
    }
    if http.get("__URL__/consumed") == {
        .Ok(response) -> print("consumed={response_text_consumed(response)}")
        .Err(_) -> print("consumed=request-error")
        else -> print("consumed=unexpected")
    }
    if http.get("__URL__/stream") == {
        .Ok(response) -> print("stream={response_text_stream(response)}")
        .Err(_) -> print("stream=request-error")
        else -> print("stream=unexpected")
    }
    if http.get("__URL__/over") == {
        .Ok(over) -> print("over={response_text_default(over)}")
        .Err(_) -> print("over=request-error")
        else -> print("over=unexpected")
    }
    if http.get("__URL__/binary") == {
        .Ok(binary) -> print("binary={response_text_default(binary)}")
        .Err(_) -> print("binary=request-error")
        else -> print("binary=unexpected")
    }
    if http.get("__URL__/truncated") == {
        .Ok(truncated) -> print("truncated={response_text_default(truncated)}")
        .Err(_) -> print("truncated=request-error")
        else -> print("truncated=unexpected")
    }
}
"#;
const HTTP_RESPONSE_SINKS: &str = r#"
use core.http.client as http

fn consume(response: HTTPResponse) String !Never -> {
    loop chunk in response.body().chunks(65536) {
        if chunk == {
            .Ok(_) -> {}
            .Err(error) -> {
                if error == .InvalidFraming -> return "rejected:InvalidFraming"
                else -> return "wrong-error"
            }
            else -> return "unexpected"
        }
    }
    return "accepted"
}

fn run() {
    if http.get("__URL__/close-get") == {
        .Ok(response) -> print("close-get={consume(response)}")
        .Err(_) -> print("close-get=request-error")
        else -> print("close-get=unexpected")
    }
    close_request :: http.request("GET", "__URL__/close-request").read_timeout(30000)
    if close_request.send() == {
        .Ok(response) -> print("close-request={consume(response)}")
        .Err(_) -> print("close-request=request-error")
        else -> print("close-request=unexpected")
    }
    if http.get("__URL__/chunked-get") == {
        .Ok(response) -> print("chunked-get={consume(response)}")
        .Err(_) -> print("chunked-get=request-error")
        else -> print("chunked-get=unexpected")
    }
    chunked_request :: http.request("GET", "__URL__/chunked-request").read_timeout(30000)
    if chunked_request.send() == {
        .Ok(response) -> print("chunked-request={consume(response)}")
        .Err(_) -> print("chunked-request=request-error")
        else -> print("chunked-request=unexpected")
    }
    if http.get("__URL__/gzip-get") == {
        .Ok(response) -> print("gzip-get={consume(response)}")
        .Err(_) -> print("gzip-get=request-error")
        else -> print("gzip-get=unexpected")
    }
    gzip_request :: http.request("GET", "__URL__/gzip-request").read_timeout(30000)
    if gzip_request.send() == {
        .Ok(response) -> print("gzip-request={consume(response)}")
        .Err(_) -> print("gzip-request=request-error")
        else -> print("gzip-request=unexpected")
    }
}
"#;
const HTTP_REQUEST_TEXT: &str = r#"
use core.http.server as server
use core.net as net

fn request_text_default(req: HTTPRequest) String !Never -> {
    if req.text() == {
        .Ok(text) -> return "default={text}"
        .Err(_) -> return "default=wrong-error"
        else -> return "default=unexpected"
    }
}

fn request_text_explicit(req: HTTPRequest) String !Never -> {
    if req.text(5) == {
        .Ok(text) -> return "explicit={text}"
        .Err(error) -> {
            if error == {
                .BodyTooLarge(limit) -> return "explicit=BodyTooLarge:{limit}"
                else -> return "explicit=wrong-error"
            }
        }
        else -> return "explicit=unexpected"
    }
}

fn request_text_over(req: HTTPRequest) String !Never -> {
    if req.text(4) == {
        .Ok(_) -> return "over=accepted"
        .Err(error) -> {
            if error == {
                .BodyTooLarge(limit) -> return "over=BodyTooLarge:{limit}"
                else -> return "over=wrong-error"
            }
        }
        else -> return "over=unexpected"
    }
}

fn request_text_consumed(req: HTTPRequest) String !Never -> {
    if req.text() == {
        .Ok(_) -> {
            if req.text() == {
                .Ok(_) -> return "consumed=reused"
                .Err(error) -> {
                    if error == {
                        .BodyConsumed -> return "consumed=BodyConsumed"
                        else -> return "consumed=wrong-error"
                    }
                }
                else -> return "consumed=unexpected"
            }
        }
        .Err(_) -> return "consumed=first-read-error"
        else -> return "consumed=first-read-error"
    }
}

fn request_text_binary(req: HTTPRequest) String !Never -> {
    if req.text() == {
        .Ok(_) -> return "binary=accepted"
        .Err(error) -> {
            if error == {
                .UnsupportedEncoding -> return "binary=UnsupportedEncoding"
                else -> return "binary=wrong-error"
            }
        }
        else -> return "binary=unexpected"
    }
}

fn request_text_classify(req: HTTPRequest) String !Never -> {
    path :: req.path()
    if path == "/default" -> return request_text_default(req)
    if path == "/explicit" -> return request_text_explicit(req)
    if path == "/over" -> return request_text_over(req)
    if path == "/consumed" -> return request_text_consumed(req)
    if path == "/binary" -> return request_text_binary(req)
    return "unknown"
}

fn request_text_handler(req: HTTPRequest) HTTPResponse !HTTPError -> {
    return Ok(server.response(200, request_text_classify(req)))
}

fn run() !(HTTPError | NetError | TaskFailure) {
    listener :: net.tcp_listen("127.0.0.1:__PORT__") ?? panic("listen")
    mux :: server.mux()
    mux.post("/default", request_text_handler)
    mux.post("/explicit", request_text_handler)
    mux.post("/over", request_text_handler)
    mux.post("/consumed", request_text_handler)
    mux.post("/binary", request_text_handler)
    server_task :: task {
        server.serve_once_listener(listener, mux) ?? panic("serve")
        server.serve_once_listener(listener, mux) ?? panic("serve")
        server.serve_once_listener(listener, mux) ?? panic("serve")
        server.serve_once_listener(listener, mux) ?? panic("serve")
        server.serve_once_listener(listener, mux) ?? panic("serve")
    }
    server_task.join() ?? panic("join")
}
"#;



#[test]
fn http_text_projections_preserve_default_limits_and_body_lifecycle_on_all_supported_tiers() {
    let has_cranelift = jet_jit::cranelift_host_supported();
    let has_rustc = common::have_rustc();
    let tier_count = 1 + usize::from(has_cranelift) + usize::from(has_rustc);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let server = std::thread::spawn(move || {
        for _ in 0..(tier_count * 7) {
            let (mut stream, _) = listener.accept().expect("client connection");
            let request = read_http_request(&mut stream);
            if request
                .windows(b"/over".len())
                .any(|window| window == b"/over")
            {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n",
                    )
                    .unwrap();
            } else if request
                .windows(b"/binary".len())
                .any(|window| window == b"/binary")
            {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\n",
                    )
                    .unwrap();
                stream.write_all(&[0, 0xff, 1]).unwrap();
            } else if request
                .windows(b"/stream".len())
                .any(|window| window == b"/stream")
            {
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhello")
                    .unwrap();
            } else if request
                .windows(b"/truncated".len())
                .any(|window| window == b"/truncated")
            {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nno",
                    )
                    .unwrap();
            } else {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                    )
                    .unwrap();
            }
            stream.flush().unwrap();
        }
    });
    let source = HTTP_TEXT_ERROR_BODIES.replace("__URL__", &format!("http://{address}"));
    let expected = "default=ok:5\n\
explicit=ok:hello\n\
consumed=err:BodyConsumed\n\
stream=ok:hello\n\
over=err:BodyTooLarge:1048576\n\
binary=err:UnsupportedEncoding\n\
truncated=err:IO:transport\n";
    on_large_stack(|| {
        if has_cranelift {
            let output = run_with_mode(&source, "http_i9_text_contract_jit", false);
            assert_eq!(output.stdout, expected);
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
        }
        let output = run_with_mode(&source, "http_i9_text_contract_interpreter", true);
        assert_eq!(output.stdout, expected);
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 0);
        if has_rustc {
            let (exit_code, stdout, stderr) =
                common::build_and_run("http_i9_text_contract", "aot", &source);
            assert_eq!(stdout, expected);
            assert_eq!(stderr, "");
            assert_eq!(exit_code, 0);
        }
    });
    server.join().expect("server");
}

#[test]
fn http_web_defaults_forced_deopt_uses_prelude_ambient() {
    on_large_stack(|| {
        let output = run(FORCED_DEOPT, "http_i9_forced_deopt");
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 0);
        assert!(output
            .stdout
            .starts_with("strasse\nReno|31|application/json|hello\nInvalidFraming\nPolicy { reason: "));
        assert!(output
            .stdout
            .contains("CORS credentials need named origins."));
        assert!(jet_jit::deopt_invoked_for_test());
        assert!(!jet_jit::fallback_invoked_for_test());
    });
}

#[test]
fn http_route_handlers_preserve_arity_context_and_errors_on_both_dev_tiers() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    on_large_stack(|| {
        for (use_interpreter, name) in [
            (false, "http_i9_routes_jit"),
            (true, "http_i9_routes_interpreter"),
        ] {
            let output = run_with_mode(ROUTE_HANDLERS, name, use_interpreter);
            assert_eq!(output.stdout, "zero\n/items/42|42|captured-header|captured\n400\n");
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
        }
    });
}

#[test]
fn hostile_http_and_websocket_urls_are_rejected_on_both_dev_tiers() {
    let modes = if jet_jit::cranelift_host_supported() {
        vec![
            (false, "http_i9_hostile_jit"),
            (true, "http_i9_hostile_interpreter"),
        ]
    } else if std::env::var("JET_REQUIRE_CRANELIFT_HOST").as_deref() == Ok("1") {
        panic!("Cranelift host required but unavailable");
    } else {
        vec![(true, "http_i9_hostile_interpreter")]
    };
    on_large_stack(|| {
        for (use_interpreter, name) in modes {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let observer = std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
                loop {
                    match listener.accept() {
                        Ok(_) => return true,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() >= deadline {
                                return false;
                            }
                            std::thread::yield_now();
                        }
                        Err(_) => return false,
                    }
                }
            });
            let source = HOSTILE_URLS
                .replace("__HTTP_URL__", &format!("http://{address}"))
                .replace("__WS_URL__", &format!("ws://{address}"));
            let output = run_with_mode(&source, name, use_interpreter);
            assert_eq!(
                output.stdout,
                "http url rejected\nmethod rejected\nheader name rejected\nheader value rejected\ncontent length rejected\ntransfer encoding rejected\nduplicate host rejected\nconnection host rejected\nconnection length rejected\nconnection transfer rejected\nproxy connection rejected\nkeep-alive rejected\nte rejected\ntrailer rejected\nupgrade rejected\nproxy authenticate rejected\nproxy authorization rejected\nws rejected\n"
            );
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
            assert!(
                !observer.join().unwrap(),
                "{name} reached the socket for a hostile request"
            );
        }
    });
}

#[test]
fn hostile_http_response_lengths_are_rejected_on_both_dev_tiers() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let server = std::thread::spawn(move || {
        const RESPONSE_BYTES: usize = 64 * 1024 * 1024 + 1;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {RESPONSE_BYTES}\r\nConnection: close\r\n\r\n"
        );
        for _ in 0..4 {
            let (mut stream, _) = listener.accept().expect("client connection");
            let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
            let _ = std::io::Write::flush(&mut stream);
        }
    });
    let url = format!("http://{address}/oversized");
    let source = r#"
use core.http.client as http

fn run() {
    if http.get("__URL__") == {
        .Ok(_) -> print("simple accepted")
        .Err(_) -> print("simple rejected")
        else -> print("simple unexpected")
    }
    request :: http.request("GET", "__URL__")
    if request.send() == {
        .Ok(_) -> print("request accepted")
        .Err(_) -> print("request rejected")
        else -> print("request unexpected")
    }
}
"#
    .replace("__URL__", &url);

    on_large_stack(|| {
        for (use_interpreter, name) in [
            (false, "http_i9_oversized_response_jit"),
            (true, "http_i9_oversized_response_interpreter"),
        ] {
            let output = run_with_mode(&source, name, use_interpreter);
            assert_eq!(output.stdout, "simple rejected\nrequest rejected\n");
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
        }
    });
    server.join().expect("server");
}

// The sink witnesses below intentionally use cleartext HTTP/1.1. `http.get`
// and `HTTPRequest.send` take the default `h2c = false` path, so this
// source-level fixture cannot negotiate H2. The direct H2C bridge harness in
// `tests/http_client_law.rs` covers that separate, protocol-configured path.
#[test]
fn hostile_http_response_sinks_reject_streamed_overruns_on_both_dev_tiers() {
    let modes = if jet_jit::cranelift_host_supported() {
        vec![
            (false, "http_i9_response_sinks_jit"),
            (true, "http_i9_response_sinks_interpreter"),
        ]
    } else if std::env::var("JET_REQUIRE_CRANELIFT_HOST").as_deref() == Ok("1") {
        panic!("Cranelift host required but unavailable");
    } else {
        vec![(true, "http_i9_response_sinks_interpreter")]
    };

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let gzip_body = gzip_fixed_repeat_bomb(GZIP_RESPONSE_BODY_LIMIT + 1, b'x');
    assert!(
        gzip_body.len() < 1024 * 1024,
        "gzip expansion witness must stay compact"
    );
    let connection_count = modes.len() * 6;
    let server = std::thread::spawn(move || {
        for _ in 0..connection_count {
            let (mut stream, _) = listener.accept().expect("client connection");
            stream
                .set_write_timeout(Some(std::time::Duration::from_secs(10)))
                .expect("write timeout");
            let request = read_http_request(&mut stream);
            serve_hostile_http_response(&mut stream, &request, &gzip_body);
        }
    });

    let source = HTTP_RESPONSE_SINKS.replace("__URL__", &format!("http://{address}"));
    let expected = "close-get=rejected:InvalidFraming\n\
close-request=rejected:InvalidFraming\n\
chunked-get=rejected:InvalidFraming\n\
chunked-request=rejected:InvalidFraming\n\
gzip-get=rejected:InvalidFraming\n\
gzip-request=rejected:InvalidFraming\n";

    on_large_stack(|| {
        for (use_interpreter, name) in modes {
            let started = std::time::Instant::now();
            let output = run_with_mode(&source, name, use_interpreter);
            assert!(
                started.elapsed() < std::time::Duration::from_secs(45),
                "{name} response sink did not terminate promptly"
            );
            assert_eq!(output.stdout, expected);
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
        }
    });
    server.join().expect("server");
}

#[test]
fn hostile_chunked_body_deadline_ends_request_on_both_dev_tiers() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }

    on_large_stack(|| {
        for (use_interpreter, name) in [
            (false, "http_i9_chunked_deadline_jit"),
            (true, "http_i9_chunked_deadline_interpreter"),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port");
            let port = listener.local_addr().expect("port address").port();
            drop(listener);
            let source = format!(
                r#"
use core.http.server as server
use core.net as net

fn run() !(HTTPError | NetError | TaskFailure) {{
    listener :: net.tcp_listen("127.0.0.1:{port}") ?? panic("listen")
    mux :: server.mux()
    mux.post("/", (req: HTTPRequest) HTTPResponse !HTTPError -> {{
        body :: req.body().text(1024) ?? "rejected"
        return Ok(server.response(200, body))
    }})
    server_task :: task {{
        server.serve_once_listener(listener, mux) ?? panic("serve")
    }}
    server_task.join() ?? panic("serve")
}}
"#
            );
            let tier_name = name.to_string();
            let runner = std::thread::spawn(move || run_with_mode(&source, &tier_name, use_interpreter));
            let started = std::time::Instant::now();
            let mut client = loop {
                match std::net::TcpStream::connect(("127.0.0.1", port)) {
                    Ok(stream) => break stream,
                    Err(error) if started.elapsed() < std::time::Duration::from_secs(30) => {
                        let _ = error;
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(error) => {
                        let _ = runner.join();
                        panic!("chunked server did not start: {error}");
                    }
                }
            };
            client
                .write_all(
                    b"POST / HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\nTrailer: X-Test\r\nConnection: close\r\n\r\n1\r\nx\r\n0\r\n",
                )
                .expect("write chunked prefix");
            client.flush().expect("flush chunked prefix");
            client
                .set_read_timeout(Some(std::time::Duration::from_secs(35)))
                .expect("set response timeout");
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let writer_stop = stop.clone();
            let mut trickle = client.try_clone().expect("clone client");
            let writer = std::thread::spawn(move || {
                let trailer = b"X-Test: value\r\n";
                for (index, byte) in trailer.iter().copied().enumerate() {
                    if writer_stop.load(std::sync::atomic::Ordering::Acquire)
                        || trickle.write_all(&[byte]).is_err()
                    {
                        return;
                    }
                    let _ = trickle.flush();
                    if index + 1 < trailer.len() {
                        for _ in 0..80 {
                            if writer_stop.load(std::sync::atomic::Ordering::Acquire) {
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                    }
                }
            });
            let mut response = Vec::new();
            let read_result = std::io::Read::read_to_end(&mut client, &mut response);
            stop.store(true, std::sync::atomic::Ordering::Release);
            drop(client);
            writer.join().expect("trickle writer");
            let output = runner.join().expect("chunked server");
            assert!(
                read_result.is_ok() && response.starts_with(b"HTTP/1.1 "),
                "chunked request was not ended by the body deadline after {:?}: read={read_result:?}, response={response:?}, stderr={}",
                started.elapsed(),
                output.stderr
            );
            assert!(started.elapsed() < std::time::Duration::from_secs(35));
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
        }
    });
}
#[test]
fn http_request_text_projections_preserve_body_contract_on_all_supported_tiers() {
    let has_cranelift = jet_jit::cranelift_host_supported();
    let has_rustc = common::have_rustc();
    let mut modes = vec![(true, false, "http_i9_request_text_interpreter")];
    if has_cranelift {
        modes.insert(0, (false, false, "http_i9_request_text_jit"));
    }
    if has_rustc {
        modes.push((false, true, "http_i9_request_text_aot"));
    }
    let cases: [(&str, &[u8], &str); 5] = [
        ("/default", b"hello", "default=hello"),
        ("/explicit", b"hello", "explicit=hello"),
        ("/over", b"hello", "over=BodyTooLarge:4"),
        ("/consumed", b"hello", "consumed=BodyConsumed"),
        ("/binary", &[0, 0xff, 1], "binary=UnsupportedEncoding"),
    ];
    on_large_stack(|| {
        for (use_interpreter, use_aot, name) in modes {
            let reservation = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let port = reservation.local_addr().expect("port address").port();
            drop(reservation);
            let source = HTTP_REQUEST_TEXT.replace("__PORT__", &port.to_string());
            let tier_name = name.to_string();
            let runner = std::thread::spawn(move || {
                if use_aot {
                    let (exit_code, stdout, stderr) =
                        common::build_and_run("http_i9_request_text", "aot", &source);
                    Output {
                        stdout,
                        stderr,
                        exit_code,
                    }
                } else {
                    run_with_mode(&source, &tier_name, use_interpreter)
                }
            });
            let started = std::time::Instant::now();
            for &(path, body, expected) in &cases {
                let mut client = loop {
                    match std::net::TcpStream::connect(("127.0.0.1", port)) {
                        Ok(stream) => break stream,
                        Err(error) if started.elapsed() < std::time::Duration::from_secs(30) => {
                            let _ = error;
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        Err(error) => {
                            let _ = runner.join();
                            panic!("request-text server did not start: {error}");
                        }
                    }
                };
                client
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .expect("set response timeout");
                let header = format!(
                    "POST {path} HTTP/1.1\r\nHost: local\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                client.write_all(header.as_bytes()).expect("write request header");
                client.write_all(body).expect("write request body");
                client.flush().expect("flush request");
                let mut response = Vec::new();
                let read_result = client.read_to_end(&mut response);
                if let Err(error) = read_result {
                    assert_eq!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset,
                        "request-text response read failed for {path}: {error}, response={response:?}"
                    );
                }
                let response = String::from_utf8(response).expect("UTF-8 HTTP response");
                assert!(
                    response.starts_with("HTTP/1.1 200"),
                    "request-text handler returned unexpected response: {response:?}"
                );
                let (_, response_body) = response
                    .split_once("\r\n\r\n")
                    .expect("request-text response headers");
                assert_eq!(response_body, expected, "request-text case {path}");
            }
            let output = runner.join().expect("request-text server");
            assert_eq!(output.stdout, "");
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
        }
    });
}
