# Core library (`core`)

The Jet Core library gives you files, terminal I/O, environment variables,
process control, math, time, random numbers, JSON, tasks, and channels —
enough to write real command-line tools. Every fallible call returns a
`T ? E` value; nothing in Core panics on its own.

<!-- Stable IDs bind these public Core declarations to reviewed capability depth. -->
<!-- CAPABILITY_CLAIMS:BEGIN -->
<!-- CAPABILITY_CLAIM: claim.core-foundation | Core foundations are reachable Jet software. -->
<!-- CAPABILITY_CLAIM: claim.core-concurrency | Tasks and events share one runtime. -->
<!-- CAPABILITY_CLAIM: claim.core-files-data | Files, paths, archives, compression, and DB APIs are production claims. -->
<!-- CAPABILITY_CLAIM: claim.core-encoding-text | Codecs and text follow published standards. -->
<!-- CAPABILITY_CLAIM: claim.core-network-http | Network and HTTP claims require live interoperability. -->
<!-- CAPABILITY_CLAIM: claim.core-security | Security APIs require fail-closed entropy. -->
<!-- CAPABILITY_CLAIM: claim.core-data-compute | Typed data claims require real documented semantics. -->
<!-- CAPABILITY_CLAIM: claim.core-ui-web | UI and web share one typed component model. -->
<!-- CAPABILITY_CLAIM: claim.game-product | Game claims require a playable runtime and editor. -->
<!-- CAPABILITY_CLAIM: claim.plugin-ffi | Plugins and FFI use one typed interop structure. -->
<!-- CAPABILITY_CLAIMS:END -->

**How it works today:** Core modules are built into the compiler. Use them by
name; the compiler type-checks your calls and generates only the helpers you
actually use (see [Using modules](#using-modules) and [Pay for what you call](#pay-for-what-you-call)).

**Canonical name:** `core` (owner, 2026-06-26). Every first-party library — the
built-in modules below and the ring packages — lives under the single `core.*`
namespace. There is no `jet.*` or `std.*` library namespace.

**Naming (S54):** types and error enums are PascalCase (`String`, `IOError`,
`JSON`); functions and module segments are snake_case (`read`, `core.files`).
See S66 for acronym capitalization.

---

## Quick start

```jet
use core.files as fs
use core.io as io
use core.env as env

fn run() {
    args :: io.args()
    if args.len() < 2 {
        io.eprint("usage: greet <name>")
        return
    }
    name :: args.get(1) ?? return
    greeting :: env.get("GREETING") ?? "hello"
    fs.write("/tmp/greet.txt", "{greeting}, {name}!") ?? return
    print(fs.read("/tmp/greet.txt") ?? return)
}
```

Build and run (extra words after the file become program arguments):

```bash
nix develop -c jet run tool.jet World
# or: nix develop -c jet build tool.jet && ./build/tool World
```

---

## Using modules

Core modules use `use` — no quotes, unlike file imports.

```jet
use core.files as fs                 // one submodule
use core.encoding.json as json       // a nested submodule
```

`use core.files` and `use core.encoding.json` each resolve to a
compiler-known module under the `core` root.

**Not allowed:**

```jet
import core.files as fs     // ordinary parse error under D-S14-PAUSE
use "core/files"           // quoted paths are for .jet files only
```

If you name a local file or folder `core`, `jet`, `http`, `regex`, `csv`, `toml`,
`crypto`, or `archive`, the compiler rejects it — those names are reserved for
first-party packages (**E1002**). An unknown core module is **E1001**;
selective imports (`use core.files.{read}`) are rejected — keep qualified access
through an alias. An unknown item in a known core module is **E1004**, with a
did-you-mean suggestion when possible.

Fallible core functions return `T ? E` and must be handled with `?`, `??`, or
a pattern test like any other Jet result. `core.files` has both whole-file
helpers (`read`/`write`/…) and streaming handles (`open`/`create`); paths are
plain `String`; binary APIs use `U8` and `[U8]`.

---

## Errors and results

Fallible Core functions return `T ? E`. Handle them like any other Jet
result — with `?`, `??`, or a pattern test:

```jet
use core.files as fs

fn run() {
    text :: fs.read("data.txt") ?? return   // stop on error
    upper :: text.to_upper()
    fs.write("out.txt", upper) ?? panic("couldn't save")  // bug if this fails
}
```

Each module has a small error type (`IOError`, `JSONError`, …). There is no
automatic conversion between error types in v1.

---

## Optional values (`T?`) — combinators (D-HOLE1)

`T?` is either `Val(x)` (present) or `None` (absent) — see S31/S35 for the
core pattern-test and `??` fallback forms. Composing two or more optionals
gets library combinators instead of a general "hole"/absent-propagating value
type (D-HOLE1 rejected that: it would duplicate `T?` and silently bypass
distinct-type arithmetic gating like `@Numeric`).

| Method | Type | What it does |
| --- | --- | --- |
| `.map(f)` | `(T?, fn(T) -> R) -> R?` | Applies `f` to the payload if present; `None` stays `None` |
| `.zip(other)` | `(T?, U?) -> (a: T, b: U)?` | Pairs two optionals: present only when **both** are present |
| `Option.lift2(f, a, b)` | `(fn(T, U) -> R, T?, U?) -> R?` | Applies a two-argument function to `a`/`b` only when both are present |

```jet
price: Float? :: lookup_price(id)
qty: Float? :: lookup_qty(id)

// zip: both present -> present pair; either None -> None
total1 :: price.zip(qty).map((pair) => pair.a * pair.b)

// lift2: same idea, no explicit pair
total2 :: Option.lift2((p, q) => p * q, price, qty)

// total1, total2: Float? — None unless both price and qty were present
```

See `examples/features/types/option_combinators.jet`.

---

## Collections and iterators (D-ITERTOOLS1=A)

Core collection spellings stay explicit: `[T]` for lists, `[K: V]` for the
default ordered map, and named types for specialized behavior.

| Type | Constructors | Main methods |
| --- | --- | --- |
| `[T]` | list literal `[a, b]` | `map`, `filter`, `each`, `find`, `any`, `all`, `sort_by`, `reduce`, `take`, `skip`, `step_by`, `dedup`, `chunks`, `windows`, `enumerate`, `zip`, `unzip`, `take_while`, `skip_while`, `flat_map`, `filter_map`, `scan`, `fold`, `sum`, `product`, `min`, `max`, `min_by`, `max_by`, `group_by`, `count_by`, `partition`, `flatten`, `intersperse` |
| `[K: V]` | map literal `["a": 1]` | `keys`, `values`, `has_key`, `get`, `add`, `add_new`, `remove`, `len`, `is_empty`, `clear` |
| `Set<T>` | `Set.new()`, `Set.from(xs)` | `add`, `remove`, `has`, `union`, `to_list`, `len`, `is_empty`, `clear` |
| `SortedSet<T>` | `SortedSet.new()`, `SortedSet.from(xs)` | `add`, `remove`, `has`, `first`, `last`, `union`, `to_list`, `len`, `is_empty`, `clear` |
| `Deque<T>` | `Deque.new()`, `Deque.from(xs)` | `push_front`, `push_back`, `pop_front`, `pop_back`, `peek_front`, `peek_back`, `to_list`, `len`, `is_empty`, `clear` |
| `PriorityQueue<T>` | `PriorityQueue.new()`, `PriorityQueue.from(xs)` | `push`, `pop`, `peek`, `to_sorted_list`, `len`, `is_empty`, `clear` |
| `Lru<K,V>` | `Lru.new(capacity)` | `add`, `add_new`, `get`, `remove`, `has_key`, `keys`, `capacity`, `len`, `is_empty`, `clear` |
| `Bag<T>` | `Bag.new()`, `Bag.from(xs)` | `add`, `remove`, `has`, `count`, `to_list`, `len`, `is_empty`, `clear` |
| `BitSet` | `BitSet.new()` | `add`, `remove`, `has`, `count`, `to_list`, `len`, `clear` |
| `ByteBuffer` | `ByteBuffer.new()`, `ByteBuffer.from(bytes)` | `write_u8`, `write_u16_le`, `write_u16_be`, `write_u32_le`, `write_u32_be`, `write_u64_le`, `write_u64_be`, `write_bytes`, `to_bytes`, `len`, `is_empty`, `clear` |

Example: `examples/features/collections/iter_tools_audit.jet` covers the
adapter and specialized-container surface.

---

## Pay for what you call

Using `core.files` costs nothing in the generated binary until you **call**
something from it. A program that uses every Core module but only calls
`print` stays hello-world sized. Only the helpers your program can reach are
compiled in.

---

## Modules

### `core.files` — files and folders

One module for both whole-file convenience helpers and streaming handles
(D-FILES-WRITE1). Paths are plain `String`s.

```jet
use core.files as fs

fn run() {
    path :: "/tmp/notes.txt"
    fs.write(path, "hello\n") ?? return
    fs.append_all(path, "world\n") ?? return
    print(fs.read(path) ?? return)        // "hello\nworld\n"
    print(fs.exists(path))                // true
    print(fs.is_dir("/tmp"))              // true
    entries :: fs.list_dir("/tmp") ?? return
    print(entries.len())
}
```

Whole-file helpers:

| Function | Returns | What it does |
|----------|---------|--------------|
| `read(path)` | `String ? IOError` | Read entire file as UTF-8 text |
| `read_bytes(path)` | `[U8] ? IOError` | Read entire file as bytes |
| `write(path, text)` | `() ? IOError` | Create or overwrite a text file |
| `append_all(path, text)` | `() ? IOError` | Append text to a file, one shot |
| `exists(path)` | `Bool` | Whether the path exists |
| `remove(path)` | `() ? IOError` | Delete a file |
| `remove_dir(path)` | `() ? IOError` | Delete an empty directory |
| `remove_all(path)` | `() ? IOError` | Delete a file or directory tree |
| `list_dir(path)` | `[DirEntry] ? IOError` | One entry per directory member, sorted by name (D-LSDIR1) |
| `create_dir(path)` | `() ? IOError` | Create a directory, including missing parents |
| `create_dir_all(path)` | `() ? IOError` | Create a directory tree |
| `is_dir(path)` | `Bool` | Whether the path is a directory |
| `copy(from, to)` | `() ? IOError` | Copy a file |
| `copy_dir(from, to)` | `() ? IOError` | Copy a directory tree |
| `rename(from, to)` | `() ? IOError` | Rename or move a file |
| `stat(path)` | `Stat ? IOError` | Metadata: size, times, permissions, kind |
| `canonicalize(path)` | `String ? IOError` | Existing path, absolute and symlink-resolved |
| `absolute(path)` | `String ? IOError` | Absolute path without requiring it to exist |
| `walk(path)` | `[WalkEntry] ? IOError` | Recursive entries below `path`, sorted per directory |
| `glob(pattern)` | `[String] ? IOError` | Recursive `*`/`?` path match |
| `symlink(from, to)` | `() ? IOError` | Create a symbolic link |
| `read_link(path)` | `String ? IOError` | Read a symbolic link target |
| `hard_link(from, to)` | `() ? IOError` | Create a hard link |
| `read_at(path, offset, len)` | `[U8] ? IOError` | Read bytes at an offset |
| `write_at(path, offset, bytes)` | `() ? IOError` | Write bytes at an offset |
| `fsync(path)` | `() ? IOError` | Flush a file to stable storage |
| `write_atomic(path, bytes)` | `() ? IOError` | Write via temp file then rename |
| `temp_dir(prefix)` | `TempDir ? IOError` | Create a temp directory; last handle drop removes it |
| `temp_file(prefix)` | `TempFile ? IOError` | Create a temp file; last handle drop removes it |
| `lock(path)` | `FileLock ? IOError` | Create an advisory lock file; last handle drop removes it |

**`IOError`** — `NotFound(path)`, `PermissionDenied(path)`, or `Other(message)`.

Streaming handles (E2-M7/D-IO2) — for bounded-memory reads/writes instead of
loading the whole file:

```jet
use core.files as files

fn count_lines(path: String) -> Int ? IOError {
    handle :: files.open(copy path)?
    n := 0
    loop line; handle.lines() {
        n = (n + 1)
    }
    return Ok(n)
}
```

| Function/method | Returns | What it does |
|------------------|---------|--------------|
| `open(path)` | `FileReader ? IOError` | Open a file for buffered line-by-line reading |
| `create(path)` | `FileWriter ? IOError` | Create/overwrite a file for buffered writing |
| `append(path)` | `FileWriter ? IOError` | Open a file for buffered appending |
| `reader.read_line()` | `String? ? IOError` | One line (no newline), `None` at EOF |
| `reader.lines()` | iterator of `String` | `loop line; handle.lines()` |
| `writer.write_line(text)` | `() ? IOError` | Write `text` plus a trailing newline |
| `writer.flush()` | `() ? IOError` | Force buffered bytes to disk |

Handles close automatically on every exit path (RAII), including early `?`
returns. `append_all` (whole-file) and `append` (streaming handle
constructor/method) are deliberately different names — D-FILES-APPEND1 — so
the same module can offer both without a name collision.

**`DirEntry`** (D-LSDIR1) has three readable fields:

| Field    | Type   | Meaning                             |
|----------|--------|--------------------------------------|
| `name`   | String | bare filename (no directory prefix)  |
| `path`   | String | full path (portable, OS-native sep)  |
| `is_dir` | Bool   | true when the entry is a directory   |

Use `entry.path` for a ready-to-use path (don't build `"{dir}/{entry}"` by
hand) and `entry.name` for filename checks (`entry.name.ends_with(".txt")`).
`Stat` fields are `size`, `modified_ms`, `created_ms`, `readonly`, `is_file`,
`is_dir`, `is_symlink`, and `kind` (`"file"`, `"dir"`, `"symlink"`, or
`"other"`). `WalkEntry` fields are `path`, `relative`, `is_dir`, and `depth`.
`TempDir`, `TempFile`, and `FileLock` expose `.path`; cleanup is RAII on the
last handle drop. `core.path` provides `path.join(dir, name) -> String` plus
`.parent()`, `.extension()`, and `.normalize()` for composing paths
independently of `DirEntry`. Examples: `examples/features/io/dir_entry.jet`
and `examples/features/io/files_depth.jet`.

### `core.url` and `core.mime` — typed web addresses and media types

`core.url` parses, normalizes, joins, and renders typed `Url` values. Hosts are
lowercased and IDNA labels are punycoded; paths remove dot segments; query
pairs preserve repeated keys. `core.http.client` accepts either `String` or
`Url` for URL arguments.

```jet
use core.url as url
use core.mime as mime

fn run() {
    base :: url.parse("https://Bücher.example/a/./b/../c?x=1") ?? return
    next :: base.join("../notify?user=ada lovelace&user=grace") ?? return
    print(next.to_string())

    html :: mime.parse("Text/HTML; charset=UTF-8") ?? return
    print(html.essence())
}
```

| Function / method | Returns | What it does |
|-------------------|---------|--------------|
| `url.parse(text)` | `Url ? String` | Parse absolute WHATWG-style URLs: http(s), file, data, and other schemes |
| `url.from_parts(scheme, host, path, query, fragment)` | `Url ? String` | Build a URL from decoded components; query is `[[String]]` key/value rows |
| `url.file(path)` / `url.data(mime, text)` | `Url` | Build `file://` and `data:` URLs |
| `url.query(pairs)` | `String` | Encode repeated query pairs from `[[String]]` |
| `url.percent_encode(text)` / `url.percent_decode(text)` | `String` / `String ? String` | Component percent encoding and decoding |
| `u.scheme()` / `.host()` / `.port()` / `.path()` / `.fragment()` | mixed | Typed component accessors |
| `u.path_segments()` / `.query_pairs()` / `.query()` | `[String]` / `[[String]]` / `String` | Decoded path/query views plus encoded query text |
| `u.normalize()` / `.join(relative)` | `Url` / `Url ? String` | Normalize or resolve a relative reference |
| `u.set_query(k, v)` / `.add_query(k, v)` | `Url` | Return a new URL with query pairs changed; repeated keys are preserved by `add_query` |

`core.mime` parses `type/subtype; param=value`, exposes typed accessors, and
ships a small extension table for common web/static-file types. Sniffing is not
implicit; callers choose an explicit MIME type or extension lookup.

| Function / method | Returns | What it does |
|-------------------|---------|--------------|
| `mime.parse(text)` | `Mime ? String` | Parse media type, subtype, and parameters |
| `mime.from_extension(ext)` / `mime.extension(type)` | `String?` / `String?` | Map common extensions and MIME essences |
| `m.media_type()` / `.subtype()` / `.essence()` | `String` | Type/subtype accessors |
| `m.param(name)` / `.params()` | `String?` / `[[String]]` | Parameter lookup and decoded key/value rows |

### `core.email` — bounded messages and SMTP submission

`core.email` separates a typed message from its transport. Address and header
construction rejects control characters before serialization. MIME output uses
CRLF, bounded Base64 lines, deterministic content-derived multipart boundaries,
and never emits Bcc recipients in message headers. Attachments and recipient,
header, and total-message counts are bounded before serialization or transport.

```jet
use core.email as email

fn run() {
    from :: email.address("Mara <mara@example.com>") ?? return
    to :: email.address("Ada <ada@example.net>") ?? return
    message :: email.message(from, [to], [], "Welcome", "Hello", "", []) ?? return
    bytes :: email.serialize(message) ?? return
    print(bytes.len())
}
```

The native surface ships `address`, `attachment`, `message`, `envelope`,
`serialize`, `smtp_from_env`, and `smtp`. `Message.with_envelope` replaces SMTP
routing without changing MIME headers. Bcc enters the default envelope but
never serialized headers.

`smtp_from_env()` reads `SMTP_HOST`, optional `SMTP_PORT` and `SMTP_SECURITY`
(`starttls` by default, or `tls`), paired `SMTP_USERNAME`/`SMTP_PASSWORD`,
optional `SMTP_CA_PEM`, and optional `SMTP_RECIPIENT_POLICY` (`require_all` by
default, or `deliver_accepted`). It constructs the same `SmtpConfig` accepted
by `smtp(config)`; missing or unsafe values are configuration errors.

Optional DKIM uses the same Mailer policy. Set `SMTP_DKIM_DOMAIN`,
`SMTP_DKIM_SELECTOR`, and `SMTP_DKIM_PRIVATE_KEY_BASE64` together;
`SMTP_DKIM_SIGNED_HEADERS` may replace the default comma-separated
`from,to,subject,mime-version,content-type` header set.
Partial, malformed, or non-32-byte key configuration fails before connecting.
The expert form sets `dkim:Val(DkimConfig)`; `dkim:None` sends unsigned mail.
One Mailer owns one signing identity and signs every message with fixed
`ed25519-sha256` relaxed/relaxed DKIM over the final MIME bytes. Use separate
Mailers for separate identities.

`Mailer.send(message)` is the only submission call. Port 587 requires verified
STARTTLS; port 465 requires TLS from connect. Custom CA PEM extends system roots
without disabling hostname verification. Password bytes leave `Secret` only in
the private authentication boundary and are zeroized with every temporary and
Mailer drop. Ambient task cancellation and `@Context` deadlines interrupt DNS,
connect, TLS, and SMTP wait checkpoints. Cancellation after DATA becomes
`DeliveryUnknown`; Jet never retries automatically. `SendReport` means relay
acceptance, not inbox delivery.

```jet
password :: crypto.Secret.from_text(env.get("SMTP_PASSWORD") ?? return)
config := email.SmtpConfig.{
    host: "smtp.example.com", port: 587, security: .StartTls,
    auth: .Password.{ username: "mailer", password: password },
    recipient_policy: .RequireAll, trust: .System,
    limits: email.Limits.safe(),
    dkim: None,
}
mailer := email.smtp(config) ?? return
report :: mailer.send(message) ?? return
```

To enable DKIM, store the 32-byte Ed25519 seed in `Secret`, construct
`DkimConfig.{ domain, selector, private_key, signed_headers }`, and place
`Val(dkim)` in the config. `from` must be signed. Publish
`v=DKIM1; k=ed25519; p=<base64 public key>` at
`<selector>._domainkey.<domain>`. Key rotation creates a new selector and
Mailer. SPF and DMARC are separate DNS policies; DKIM signing does not publish
or configure them.

### `core.http` — HTTP client and server

`core.http.client` is the request side; `core.http.server` is the serving side.
The client accepts `String` or typed `Url` values, uses HTTPS by default through
the hidden rustls bridge, and keeps the compiler itself dependency-free. The
server is std-only HTTP/1.1 unless the named TLS option is supplied.

```jet
use core.http.client as client
use core.http.server as server

fn run() {
    mux :: server.mux()
    mux.post("/api/:name/*path", (req: HttpSrvReq) =>
        server.response(200, req.body())
    )

    req :: client.request("POST", "http://127.0.0.1:8080/api/ada/profile")
        .form("tool", "jet")
        .cookie("session", "abc")
        .connect_timeout(1000)
        .read_timeout(1000)
    resp :: req.send() ?? return
    print(resp.status())
}
```

Client surface:

| Function / method | Returns | What it does |
|-------------------|---------|--------------|
| `client.get(url)` / `client.post(url, body)` | `HttpClientResp ? String` | One-shot request helpers |
| `client.request(method, url)` | `HttpClientReq` | Start a typed request builder |
| `req.header(name, value)` / `.body(text)` | `HttpClientReq` | Add headers or a string body |
| `req.form(name, value)` / `.multipart_text(name, value)` | `HttpClientReq` | Encode form or text multipart fields |
| `req.cookie(name, value)` / `.redirects(n)` | `HttpClientReq` | Set Cookie header or redirect limit |
| `req.timeout(ms)` / `.connect_timeout(ms)` / `.read_timeout(ms)` / `.total_timeout(ms)` | `HttpClientReq` | Set global/per-phase deadlines |
| `req.proxy(url)` | `HttpClientReq` | Use an explicit proxy; env proxies are honored by default |
| `req.send()` | `HttpClientResp ? String` | Execute the request |
| `resp.status()` / `.body()` / `.header(name)` / `.cookies()` | mixed | Inspect response status, text body, headers, and Set-Cookie values |

The compatibility text response path accepts at most 8 MiB of transfer-decoded
bytes and rejects non-UTF-8 data. The byte-native streaming `Body` API remains
open.

Server surface:

| Function / method | Returns | What it does |
|-------------------|---------|--------------|
| `server.mux()` | `HttpMux` | Create a function-first router |
| `mux.get/post/put/delete/patch(path, handler)` | nothing | Register `fn(HttpSrvReq) -> HttpSrvResp` handlers |
| `server.serve(addr, mux)` | `() ? String` | Serve HTTP/1.1 forever |
| `server.serve(addr, mux, tls: server.tls(cert, key))` | `() ? String` | Serve HTTPS with explicit TLS material |
| `server.serve_once(addr, mux)` / `server.serve_once_listener(listener, mux)` | `() ? String` | Testable one-request serving |
| `server.response(status, body)` / `resp.header(name, value)` | `HttpSrvResp` | Build a response |
| `server.sse(data)` | `HttpSrvResp` | Server-sent event response |
| `server.static_file(path, mime)` / `.static_file_range(req, path, mime)` | `HttpSrvResp ? String` | Static file response, with Range support |
| `server.access_log(req, status)` | `String` | Stable access-log line |
| `req.method()` / `.path()` / `.param(name)` / `.header(name)` / `.body()` / `.body_len()` / `.under_limit(max)` | mixed | Inspect request data and enforce body limits |

Card 301 audit state:

| Area | State |
|------|-------|
| HTTPS client / server TLS | Shipped: client default HTTPS, server `tls:` named option |
| Typed URL input | Shipped: client calls accept `Url` or `String` |
| Redirects, cookies, forms, multipart, proxy, phase timeouts | Shipped: request builder methods above |
| Router params and wildcard routes | Shipped: `:name` params plus final `*` wildcard (`param("wildcard")`) |
| SSE, static files, Range, access log, request body limits | Shipped: server helpers above |
| Bounded hostile request parsing | Partial: HTTP/1.1 incrementally frames and dechunks octets (including extensions) before validating the decoded compatibility text body, preserves pipelined boundaries, and caps the decoded body at 1 MiB plus 32 KiB of chunk metadata; it rejects malformed/truncated chunks, request trailers, non-HTTP header whitespace/control values, multiple/unsupported transfer codings, oversized headers/bodies, ambiguous Content-Length, Content-Length with Transfer-Encoding, folded headers, and malformed framing before dispatch. The compatibility request body is still buffered text. |
| Request method | Shipped: standard and extension methods preserve their case and must be one nonempty HTTP token; separators, controls, whitespace, and non-ASCII bytes fail with 400 and close before body permission or dispatch |
| Request target and Host authority | Partial: origin-form and HTTP(S) absolute-form share strict raw path/query validation and route through the same path; absolute authority must match Host after case, IPv6, percent-hex, and default-port normalization. HTTP/1.1 requires exactly one valid Host, while HTTP/1.0 may omit it; malformed escapes, illegal raw URI characters, mismatch, userinfo, fragments, unsupported target forms, and malformed or ambiguous authorities fail with 400 and close before dispatch. Asterisk-form and CONNECT authority-form remain open and are rejected. |
| Connection options | Shipped for plain HTTP/1.x: repeated fields and comma lists share one HTTP-token parser, extension options and empty list members remain compatible, `close` dominates, and malformed options fail with 400 before body permission or dispatch |
| Content-Length | Shipped for plain HTTP/1.x: repeated and comma-combined values share one decimal parser and must be nonempty ASCII digits with the same numeric value; leading zeros remain valid, while signs, empty members, overflow, and conflicts fail with 400 before body permission or dispatch |
| Persistent connections | Partial: the dependency-free plain HTTP/1.x server preserves pipelined request boundaries, responds sequentially in wire order, idles for at most 60 seconds, stops keep-alive reuse promptly during shutdown, and closes after 1,000 requests; TLS persistence and HTTP/2 remain open |
| HTTP/1 response framing | Partial: only HTTP/1.0 and HTTP/1.1 requests reach handlers; unsupported versions close with 505, and 1xx/204/304 responses publish neither body bytes nor Content-Length; streaming, trailers, and HTTP/2 remain open |
| `Expect: 100-continue` | Shipped for plain HTTP/1.1: one interim response follows successful framing/size validation, Content-Length oversize fails with 413 before upload, unsupported or repeated expectations fail with 417 before dispatch, and final pipelined responses remain ordered; the separate TLS serving path remains open |
| Bounded streaming bodies | Not shipped: request and response bodies are still buffered `String` values rather than D-HTTP-CORE2 streaming byte bodies with backpressure |
| Transparent Content-Encoding decoding | Not shipped: gzip and other content-coding support remains open under D-DEP-HTTP2=B; the compatibility text cap applies only after HTTP transfer framing is decoded |
| Graceful shutdown | Not shipped: `serve_once*` is a deterministic test entrypoint, not the D-HTTP-SERVER2 drain/cancel/report lifecycle |
| Pooling and HTTP/2 | Not shipped: the current request-scoped bridge does not implement D-HTTP-CLIENT2's shared `Client` pool or native HTTP/2 transport |
| WebSocket | Ratified as standalone `core.ws` (D-WS1=B); implementation and interoperability proof remain open |

Example: `examples/features/net/http_rest_service.jet`.

### `core.crypto` — safe envelopes and expert primitives

`core.crypto` is the safe-by-default cryptography surface. Beginner APIs hide
nonce handling and algorithm selection; raw algorithm choice lives under
`core.crypto.expert` and requires an audited `@Unsafe` region. RustCrypto crates
are linked only through the hidden bridge crate, not the compiler.

```jet
use core.crypto as crypto

fn run() {
    recipient :: crypto.X25519SecretKey.generate() ?? return
    box :: crypto.seal([recipient.public_key()], "hello".bytes(), []) ?? return
    plain :: crypto.open(&recipient, box, []) ?? return

    password :: crypto.Secret.from_text("correct horse battery staple")
    stored :: crypto.password_hash(password) ?? return
    print(crypto.password_verify(password, stored) ?? return)
}
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `sha256(text)` / `sha256_bytes(bytes)` | `String` | SHA-256 hex digest |
| `sha512_bytes(bytes)` | `String` | SHA-512 hex digest |
| `blake3_bytes(bytes)` | `String` | BLAKE3 hex digest |
| `random.bytes(n)` | `[U8]` (edition 2026) | One fail-closed OS CSPRNG request, capped at 1,048,576 bytes; edition 2026 reports E3001/exit 70 when the internal provider rejects the length or is unavailable. The ratified fallible `RandomError` surface waits for the next major edition. |
| `seal(recipients, bytes, aad)` / `open(&identity, box, aad)` | `Sealed ? CryptoError` / `[U8] ? CryptoError` | Canonical recipient-based JETV value envelope with internal key and nonce handling |
| `file_seal(recipients, source, destination)` / `file_open(&identity, source, destination)` | `() ? FileCryptoError` | Recipient-based JETC v2 files with bounded 1 MiB authenticated chunks and atomic no-overwrite publication |
| `expert.open_v1(key, envelope)` | `[U8] ? CryptoError` | Audited `@Unsafe`-only reader for canonical historical JETC v1 ChaCha20-Poly1305 or AES-256-GCM bytes; every failure is `OpenFailed` |
| `expert.migrate_v1(key, source, recipients, destination)` | `() ? FileCryptoError` | Audited `@Unsafe`-only migration from canonical historical JETC v1 to recipient JETC v2; preserves the source and reopen-verifies v2 before atomic publication |
| `sign(signing_key, bytes)` / `verify(verify_key, bytes, signature)` | `Signature ? CryptoError` / `Bool ? CryptoError` | Ed25519 signing and verification with nominal key and signature types |
| `x25519(secret_key, public_key)` | `SharedSecret ? CryptoError` | X25519 key agreement with nominal key and shared-secret types |
| `hkdf_sha256(ikm, salt, info, len)` | `Secret ? CryptoError` | HKDF-SHA256 expand without exposing derived secret bytes |
| `password_hash(password)` | `PasswordHash ? CryptoError` | Argon2id password hash with generated salt and safe defaults; accepts a nominal `Secret` |
| `password_verify(password, stored)` | `Bool ? CryptoError` | Verify a nominal `Secret` against a validated `PasswordHash` |
| `constant_time_equal(a, b)` | `Bool` | Constant-time comparison of nominal `Secret` values |

Card 302 audit state:

| Area | State |
|------|-------|
| AEAD envelope | Shipped: one recipient-based JETV `seal/open` path; historical symmetric JETC v1 has no writer or safe fallback and is readable only through `expert.open_v1` |
| Signatures | Shipped: Ed25519 sign/verify with RFC-vector golden |
| Password hashing | Shipped: Argon2id PHC hash/verify, random salt default, deterministic salted vector helper |
| KDF / key agreement | Shipped: HKDF-SHA256 and X25519 with RFC vectors |
| Hashes / comparison | Shipped: SHA-256, SHA-512, BLAKE3, constant-time equality |
| File envelope | JETC v2 recipient streaming plus exact expert JETC v1 open/migrate are shipped on Linux. Linux stages plaintext and output in unlinked `O_TMPFILE` inodes under a component-wise no-follow held parent, revalidates parent identity, links the still-open output fd to the final name once with `linkat(AT_EMPTY_PATH)`, and fsyncs the held directory; unsupported filesystems fail closed. Non-Linux filesystem backends, true maximum-size filesystem exercises, and remaining platform matrices stay open on #302 |
| Entropy | Shipped: one D-CRYPTO-RNG1 provider shared by `random.bytes`, envelope nonces, Ed25519 key generation, Argon2id salts, and file envelopes; Linux glibc uses `getrandom`, macOS uses `SecRandomCopyBytes`, Windows MSVC uses `BCryptGenRandom`, WASI preview 1 uses `random_get`; unsupported targets fail closed with no fallback. D-CRYPTO-WASI-ALLOC2: every interrupted WASI call's exact-count zeroed `Vec` is volatile-zeroized and dropped before a new ownership generation; allocator address reuse is allowed; no failed bytes escape; at most seventeen calls occur. Package key generation maps provider failure through a closed helper status to E1292, never raw provider/helper text |
| Secret display types | Shipped: `Secret` is a nominal runtime type used by secret-taking APIs; display, debug, print, serialization, reflection, comparison, hashing, and cloning are rejected |
| PQ hybrid agility | Tracked by #71, not duplicated here |

Examples: `examples/features/crypto/crypto_suite.jet`,
`examples/features/crypto/crypto_envelope.jet`, and
`examples/features/crypto/crypto_sign.jet`.

### `core.auth` — strict JWT and PASETO verification

`core.auth` exports two standalone verifiers:

```jet
verify_jwt(token, key:, audience:, issuer:, clock_skew:) -> Claims ? AuthError
verify_paseto(token, key:, audience:, issuer:, clock_skew:, footer:, implicit:) -> Claims ? AuthError
```

`issuer` and `clock_skew` are optional for both functions; `footer` and
`implicit` are optional for PASETO. JWT accepts only HS256 and keys of at least
32 bytes. PASETO accepts only `v4.public`, requires a 32-byte Ed25519 public
key, and verifies the PAE input including the supplied footer and implicit
assertion. Unknown algorithms, versions, and purposes fail closed.

Both formats require integer `exp` and matching `aud` claims. An optional
expected issuer must match `iss`. Expiry is compared in milliseconds, equality
is expired, subsecond skew is preserved, and arithmetic overflow is rejected.
Token JSON rejects duplicate object keys after escape decoding, including
duplicates in headers and claims. Base64url input must be unpadded and
canonical.

`Claims` exposes `subject: String?`, the validated `audience: String`,
`issuer: String?`, `expires_at: Int`, and `issued_at: Int?`. `AuthError` is an
inspectable enum with `MalformedToken`, `UnsupportedToken`, `InvalidSignature`,
`WeakKey`, `MissingClaim`, `WrongAudience`, `WrongIssuer`, `TokenExpired`, and
`DecodeError` variants. The implementation is compiler-embedded, reuses Jet's
JSON and crypto mechanisms, and adds no external dependency.

Example: `examples/features/crypto/auth_tokens.jet`.

### `core.watcher` — file/process/port change events

`core.watcher` owns watch-style APIs (D-WATCH-SCOPE1). It uses std-only polling
today: file watchers diff recursive metadata snapshots, process watchers check
process liveness, and port watchers attempt a TCP connect. Handles can be
polled directly or connected to `core.event` scopes with callbacks.

```jet
use core.event as event
use core.watcher as watcher

fn run() {
    scope :: event.scope()
    files :: watcher.files("src") ?? return
    files.on(scope, (ev) => { print("{ev.kind}: {ev.path}") })
    loop ev; files.poll() {
        print(ev.kind)
    }
}
```

| Function/method | Returns | What it does |
|------------------|---------|--------------|
| `watcher.files(path)` | `WatchHandle ? IOError` | Watch a file or directory tree |
| `watcher.process_pid(pid)` | `WatchHandle` | Watch a process id for exit |
| `watcher.port(host, port)` | `WatchHandle` | Watch for TCP readiness |
| `watcher.set()` | `WatchSet` | Create a multiplexer for handles |
| `handle.poll()` / `handle.events()` | `[WatchEvent]` | Drain newly observed events |
| `handle.on(scope, f)` / `.once(scope, f)` | `Subscription` | Run callback on future `poll()` events |
| `handle.cancel()` / `.is_active()` / `.summary()` | mixed | Stop/query a handle |
| `set.add(handle)` | nothing | Add a handle to a set |
| `set.poll()` / `set.events()` | `[WatchEvent]` | Poll all handles |

`WatchEvent` fields are `domain` (`"file"`, `"process"`, `"port"`), `kind`,
`path`, `detail`, `pid`, and `port`. Example:
`examples/features/io/watcher.jet`.

---

### `core.io` — terminal and arguments

```jet
use core.io as io

fn run() {
    args :: io.args()                    // [String]; index 0 is the program name
    name :: io.input("your name? ") ?? return  // reads one line, strips newline
    print("hi, {name}")
    io.eprint("(log) done")                 // like print, but to stderr
    out :: io.stdout()
    out.write("done") ?? return
    out.flush() ?? return
}
```

Pipe input for scripts:

```bash
printf "Ada\n" | nix develop -c jet run ask.jet
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `args()` | `[String]` | Command-line arguments |
| `input([prompt])` | `String ? IOError` | Read one line from stdin; optional prompt |
| `read_all_input()` | `String ? IOError` | Read all of stdin to end-of-file |
| `eprint(value)` | nothing | Print to stderr (any printable value) |
| `stdin()` | `StdinHandle` | Buffered stdin handle with `.read_line()` and `.lines()` |
| `stdout()` / `stderr()` | `Stdout` / `Stderr` | Stream handles |
| `stream.write(text)` | `() ? IOError` | Write without adding a newline |
| `stream.write_line(text)` | `() ? IOError` | Write text plus newline |
| `stream.write_bytes(bytes)` | `() ? IOError` | Write raw `[U8]` bytes |
| `stream.flush()` | `() ? IOError` | Force buffered bytes through the OS handle |
| `stream.is_tty()` | `Bool` | Whether that stream is attached to a terminal |
| `terminal_width()` / `terminal_height()` | `Int` | Terminal size from the OS where available, then `COLUMNS`/`LINES`, then `80x24` |
| `style(name, text)` | `String` | ANSI style only when stdout is a TTY and `NO_COLOR` is absent |
| `style_force(name, text)` | `String` | Expert override that always emits known ANSI styles |
| `progress(text)` | `() ? IOError` | TTY: carriage-return progress update; non-TTY: one plain line |

`print` stays in the core prelude (no `use` needed). Use `io.eprint` for stderr.
`core.term` still owns `live { ... }` and `term.read_key()` for direct raw-key
input; it is the shipped raw-mode/key-event bridge under D-TERM1.

`jet run file.jet -- arg1 arg2` forwards everything after `--` verbatim as
program arguments (`io.args()` sees them, argv[1..]); plain positional words
with no separator also work (`jet run greet.jet Ada`). An unknown `--`-flag
written before the `--` is **E2102**, which teaches the `--` form (D-CLI1).
`jet test` also accepts `--`; `jet build` does not (no running process).

---

### `core.args` — declarative CLI parsing (D-ARGS1)

Build a flag/option/positional spec once and parse `io.args()` against it,
instead of hand-walking `[String]`:

```jet
use core.args as args

fn run() {
    spec :: args.spec()
        .flag("verbose", "print extra detail")
        .option("output", "write result to FILE", "FILE")
        .positional("input", "file to read")
    parsed :: spec.parse(io.args()) ?? panic(spec.help())
    print(parsed.flag("verbose"))
    print(parsed.option("output") ?? "(default)")
}
```

`args.spec()` returns an `ArgsSpec` builder; each method consumes it and
returns a new one:

| Method | Signature | Registers |
|--------|-----------|-----------|
| `.flag(name, help)` | `(String, String) → ArgsSpec` | `--name` boolean flag |
| `.flag_short(name, short, help)` | `(String, String, String) → ArgsSpec` | `--name` plus `-n`; combined shorts like `-vv` work for flags |
| `.option(name, help, meta)` | `(String, String, String) → ArgsSpec` | `--name VALUE` / `--name=VALUE` string option |
| `.option_short(name, short, help, meta)` | `(String, String, String, String) → ArgsSpec` | value option plus `-n VALUE` / `-nVALUE` |
| `.option_int(name, help, meta)` | `(String, String, String) → ArgsSpec` | option whose value must parse as `Int` |
| `.option_float(name, help, meta)` | `(String, String, String) → ArgsSpec` | option whose value must parse as `Float` |
| `.option_choice(name, help, meta, choices)` | `(String, String, String, String) → ArgsSpec` | option restricted to comma-separated choices |
| `.option_default(name, help, meta, value)` | `(String, String, String, String) → ArgsSpec` | optional string option with default |
| `.option_env(name, help, meta, env)` | `(String, String, String, String) → ArgsSpec` | optional string option with environment fallback |
| `.required_option(name, help, meta)` | `(String, String, String) → ArgsSpec` | required string option |
| `.repeat(name, help, meta)` | `(String, String, String) → ArgsSpec` | repeatable string option |
| `.positional(name, help)` | `(String, String) → ArgsSpec` | required positional |
| `.subcommand(name, help, spec)` | `(String, String, ArgsSpec) → ArgsSpec` | subcommand with its own nested spec |
| `.version(text)` | `(String) → ArgsSpec` | enables `--version` |
| `.completion(shell)` | `(String) → String` | shell completion text for bash/zsh/fish-style generators |
| `.help()` | `() → String` | formatted help text with defaults, env fallbacks, choices, and subcommands |
| `.parse(argv)` | `([String]) → ParsedArgs ? String` | parses `argv` against the spec; unknown flags include suggestions |

`ParsedArgs` query methods:

| Method | Signature | Returns |
|--------|-----------|---------|
| `.flag(name)` | `(String) → Bool` | true if `--name` was passed |
| `.option(name)` | `(String) → String?` | value of `--name VALUE`, or `None` |
| `.option_int(name)` | `(String) → Int?` | parsed integer value |
| `.option_float(name)` | `(String) → Float?` | parsed float value |
| `.options(name)` | `(String) → [String]` | every value passed to a repeated option |
| `.positional(idx)` | `(Int) → String?` | the nth positional (0-based), or `None` |
| `.subcommand()` | `() → String?` | matched subcommand name |

`--help` and `--version` are recognized automatically; parse does not exit the
process, so tools can decide whether to print `spec.help()` or continue. `.parse`
returns `ParsedArgs ? String`, where the error string carries the parse message
(unknown flag with "did you mean", missing positional, bad typed value, …).
Wrong argument counts on builder/query methods are **E1301**–**E1304**.
Examples: `examples/features/io/args_spec.jet`,
`examples/features/io/args_audit.jet`, and typed entry-parameter CLIs under
`examples/features/cli/`.

---

### `core.reflect` — runtime reflection floor (D-ANY-JAI1)

`reflect.of(x)` inspects any value that `"{x}"` interpolation could show —
same requirement, `Display` (auto-derived or explicit):

```jet
use core.reflect as reflect

struct Point {
    x: Int
    y: Int

    impl Display {
        fn display(self) -> String {
            return "({self.x}, {self.y})"
        }
    }
}

fn run() {
    p :: Point.{ x: 3, y: 4 }
    v :: reflect.of(p)
    print(v.type_name())    // "Point"
    print(v.display())      // "(3, 4)" — exactly what "{p}" would print
    loop f; v.fields() {
        print("{f.name()} = {f.value()}")
    }
}
```

`reflect.of(x)` returns a `Value` handle:

| Method | Signature | Returns |
|--------|-----------|---------|
| `.type_name()` | `() → String` | the value's declared type name |
| `.display()` | `() → String` | the same string `"{x}"` interpolation shows |
| `.fields()` | `() → [Field]` | one entry per struct field; `[]` for anything else (primitives, enums, tuples, lists) |

Each `Field` carries a name and its rendered value:

| Method | Signature | Returns |
|--------|-----------|---------|
| `.name()` | `() → String` | the field's declared name |
| `.value()` | `() → String` | the field's rendered value |

A value that isn't `Display`-able (a closure, a `Shared<T>`) is **E0112** at
the `reflect.of(...)` call site — the fix is the same as for a failed `"{x}"`
interpolation: add `impl Display`, or reflect one of its fields instead.
Example: `examples/features/reflection/reflect-value.jet`.

---

### `core.env` — environment and working directory

```jet
use core.env as env

fn run() {
    home :: env.home_dir()               // String? — may be None
    mode :: env.get("MODE") ?? "dev"     // String? from the environment
    env.set("MODE", "prod")              // set in Jet's process environment
    removed :: env.unset("CI") ?? false
    names :: env.vars() ?? []              // sorted names; never bulk values
    here :: env.current_dir() ?? return  // current working directory
    print(home ?? "(no home)")
    print(mode)
    print(here)
}
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `get(name)` | `String?` | Environment variable, or None if unset |
| `set(name, value)` | nothing | Set an environment variable |
| `unset(name)` | `Bool ? EnvError` | Remove a variable; true when it existed |
| `vars()` | `[String] ? EnvError` | Sorted owned snapshot of variable names |
| `current_dir()` | `String ? IOError` | Current working directory |
| `home_dir()` | `String?` | User home directory, if known |

Jet captures the inherited environment without decoding it and owns one
process-global logical overlay. Mutations are visible to later Jet reads and
child launches, but do not mutate libc's environment or the Windows process
environment block; foreign APIs must receive changed values explicitly.
Every child gets one atomic overlay snapshot before its `ProcessSpec`
`env_clear`, `env`, and `env_remove` overrides are applied. Raw Unix bytes and
Windows UTF-16 values survive child inheritance. `vars()` returns names only
and fails with `EnvError.NonUnicode` if any current name or value cannot be
decoded losslessly; it never skips or replaces an entry.

`EnvError` has `InvalidName`, `InvalidValue`, and `NonUnicode`. Names must be
nonempty and contain neither NUL nor `=`; values cannot contain NUL. Current
editions retain the source-compatible `set -> Void` signature and report an
invalid call as E3001. A future major release and edition opt-in changes `set`
to `Void ? EnvError`.

---

### `core.os` — system facts and interrupt hook (D-OSFACTS1)

```jet
use core.os as os

fn run() {
    print(os.name())           // linux, macos, windows, …
    print(os.arch())           // x86_64, aarch64, …
    print(os.cpu_count())      // logical CPU count
    os.on_interrupt(() => {
        print("stopping")
    })
}
```

`core.env` owns environment variables and cwd/home. `core.os` owns facts about
this process and machine.

| Function | Returns | What it does |
|----------|---------|--------------|
| `name()` | `String` | OS name from the target platform |
| `family()` | `String` | OS family (`unix`, `windows`, `wasm`, …) |
| `arch()` | `String` | CPU architecture |
| `cpu_count()` | `Int` | Logical CPU count, at least 1 |
| `temp_dir()` | `String` | Platform temp directory |
| `executable()` | `String` | Current executable path, or empty if unavailable |
| `pid()` | `Int` | Current process id |
| `hostname()` | `String` | Hostname, falling back to `localhost` |
| `username()` | `String` | Current username, or empty if unavailable |
| `set_current_dir(path)` | `Void ? IOError` | Change process working directory |
| `on_interrupt(handler)` | `Void` | Register a process-lifetime handler for Ctrl-C / SIGINT on Unix and Windows |

Interrupt handlers are additive. Each Ctrl-C runs every registered handler in
registration order on Jet's interrupt dispatcher, never inside the operating
system callback. Registration is active before `on_interrupt` returns. The
`Void` return means registrations live until the process exits; there is no
unregister/drop handle. Calling `on_interrupt` on a target without process
interrupts fails explicitly instead of silently discarding the handler.

Example: `examples/features/io/os_facts.jet`.

---

### `core.process` — exit and subprocesses (D-PROCESS1)

`process.run(cmd)` accepts checked `Sh` typed text and directly executes its
argv. Literal words become argv items; every `{hole}` becomes exactly one item,
even when its value contains spaces, globs, or shell metacharacters. No shell
parses the command. `process.cmd(argv).run()` remains the explicit argv path;
both reach the same subprocess primitive. `Sh.raw(text)` is the sole audited
runtime-string escape and splits audited text only on whitespace; it still does
not invoke a shell.

```jet
use core.process as process
use core.time as time

fn run() {
    target :: "directory with spaces;*.tmp"
    copied :: process.run(sh"cp -- {target} backup") ?? return

    spec :: process.cmd(["cargo", "test"])
        .cwd("crates/app")
        .env_clear()
        .env("RUST_BACKTRACE", "1")
        .stdout(.Stream)
        .stderr(.Inherit)
        .timeout(Duration.seconds(30)?)

    child :: spec.spawn() ?? return
    loop line; child.stdout.lines() {
        print(line)
    }
    status :: child.wait() ?? return
    if !status.success { process.exit(status.code ?? 1) }

    pipe :: process.pipeline([
        process.cmd(["cat", "input.txt"]),
        process.cmd(["grep", "error"]),
    ]) ?? return
    print(pipe.output)

    process.exit(0)          // end the program with an exit code (never returns)
}
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `exit(code)` | never | Stop the program with the given exit code |
| `run(cmd)` | `ProcessResult ? IOError` | Execute checked `Sh` argv directly; explicit `[String]` argv remains accepted for compatibility |
| `cmd(argv)` | `ProcessSpec` | Build a subprocess spec from an argv array; no shell string |
| `pipeline(specs)` | `ProcessResult ? IOError` | Connect stdout to stdin across `[ProcessSpec]` stages, no shell |

`ProcessSpec` builder methods are value-returning: `cwd(path)`, `env(key,
value)`, `env_remove(key)`, `env_clear()`, `stdin(mode)`, `stdout(mode)`,
`stderr(mode)`, `timeout(duration)`, `output_limit(bytes)`, and `detached()`.
`mode` is one of the three stream-mode dot-literals: `.Stream` (pipe it —
drain live via `child.stdout.lines()`), `.Inherit` (pass through to the
parent's stream), or `.Capture` (pipe it — collect into `ProcessResult` at
`run()`/`wait()`). `stdin` defaults to closed (no `.stdin(...)` call — the
child gets no stdin at all, never the parent's terminal by accident).
`timeout` takes a `Duration` (e.g. `Duration.seconds(30)?`). A spec can
`run()` to collect a `ProcessResult` or `spawn()` to return a `ProcessChild`.

`ProcessChild` exposes `id()`, `wait()`, `kill()`, `terminate()`,
`interrupt()`, a `.stdin` writer (`child.stdin.write(text)`), and `.stdout`/
`.stderr` streaming readers consumed only via
`loop line; child.stdout.lines() { ... }` (same loop-source-only shape as
`FileReader.lines()`/`io.stdin().lines()` — storing the reader or the line
stream in a name is E2502).

**`ProcessResult`** — `code: Int`, `success: Bool`, `timed_out: Bool`,
`signal: Int?`, `output: String`, `errors: String`.

---

### `core.math` — numbers

```jet
use core.math as math

fn run() {
    print(math.sqrt(2.0))
    print(math.pow(2.0, 10.0))
    print(math.abs(-3))                     // works on Int and Float
    print(math.min(3, 7))                   // generic over Comparable types
    print(math.max(3.5, 7.2))
    print(math.floor(3.9))
    print(math.ceil(3.1))
    print(math.round(3.6))                  // returns Int
    print(math.clamp(15, 0, 10))            // 10
    print(math.pi)
    print(math.e)
}
```

| Item | Notes |
|------|-------|
| `sqrt`, `pow`, `floor`, `ceil` | `Float` in, `Float` out |
| `round` | `Float` in, `Int` out |
| `abs` | `Int` or `Float` |
| `min[T]`, `max[T]` | Two values of the same comparable type |
| `clamp(x, lo, hi)` | Keep `x` inside the range |
| `pi`, `e` | Float constants |
| `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2` | Trig family |
| `sinh`, `cosh`, `tanh` | Hyperbolic trig |
| `exp`, `ln`, `log2`, `log10`, `hypot` | libm exponent/log/vector length floor |
| `trunc`, `fract`, `sign` | Float decomposition/classification |
| `is_nan`, `is_inf`, `is_finite` | Float predicates |
| `to_bits`, `from_bits` | Float bit round-trip through `Int` |
| `degrees`, `radians`, `lerp` | Unit conversion and interpolation |
| `checked_add/sub/mul/pow` | Integer operations returning `Int?` on overflow |
| `saturating_add/sub/mul` | Integer operations clamping on overflow |
| `wrapping_add/sub/mul` | Integer operations wrapping on overflow |
| `int_pow`, `gcd`, `lcm` | Integer power and number theory helpers |

Example: `examples/features/math/math_audit.jet`.

---

### Linear algebra — `Vec2`/`Vec3`/`Vec4`, `Mat3`/`Mat4` (D-LINALG1)

Built-in value types — no import. Components are `Float` (F64); matrices are
column-major. Operators `+`/`-` are element-wise, `*` is element-wise on vectors
(Hadamard) / matrix-multiply on matrices, and `Mat * Vec` transforms a vector.

```jet
fn run() {
a: Vec3 :: Vec3(1.0, 2.0, 3.0)
b: Vec3 :: Vec3(4.0, 5.0, 6.0)
sum: Vec3 :: a + b
    print(a.dot(b))                 // 32.0
    print(a.cross(b).to_array())    // [-3.0, 6.0, -3.0]
    print(Vec3(0.0, 3.0, 4.0).length())   // 5.0

scale: Mat3 :: Mat3(2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0)
out: Vec3 :: scale * Vec3(1.0, 2.0, 3.0)
    print(out.to_array())           // [2.0, 4.0, 6.0]
}
```

| Item | Notes |
|------|-------|
| `Vec2`/`Vec3`/`Vec4(x, …)` | Positional construction from `Float` components |
| `Mat3`/`Mat4(m0, …)` | N*N components, column-major |
| `T.splat(x)` / `T.from_array(a)` | Fill all components / build from `[Float#N]` |
| `v.dot(w)` | Scalar dot product |
| `v.cross(w)` | Cross product (`Vec3` only) → `Vec3` |
| `v.length()` / `v.normalize()` | Euclidean length / unit vector |
| `m.matmul(n)` / `m.transpose()` | Matrix product / transpose |
| `m.transform(v)` | Same as `m * v` |
| `v.to_array()` | Round-trip out to `[Float#N]` (D-FIXARR1 bridge) |
| `+` `-` `*` | Element-wise (vectors); `*` = matmul (matrices) / transform (`Mat*Vec`) |

---

### SIMD lanes — `F32x4`, `F64x2` (D-SIMD1/D-SIMD2)

Built-in portable lane types — no import. `F32x4` holds four `F32` lanes, `F64x2`
two `F64`. Element-wise `+`/`-`/`*`/`/` run across every lane at once; `v[i]`
reads a lane; reductions fold the lanes. On the pinned stable toolchain these
lower to a safe scalar-array fallback (no intrinsics, no `std::simd` gate) — a
portable-SIMD backend can replace it later behind the same surface.

```jet
fn run() {
v: F32x4 :: F32x4(1.0, 2.0, 3.0, 4.0)
w: F32x4 :: F32x4(10.0, 20.0, 30.0, 40.0)
s: F32x4 :: v + w
    print(s.to_array())             // [11.0, 22.0, 33.0, 44.0]
    print(v[2])                     // 3.0
    print(v.sum())                  // 10.0
    print(v.reduce(@Max))           // 4.0
    print(F32x4.splat(7.0).to_array())   // [7.0, 7.0, 7.0, 7.0]
}
```

| Item | Notes |
|------|-------|
| `F32x4(a, b, c, d)` / `F64x2(a, b)` | Positional lane construction |
| `T.splat(x)` / `T.from_array(a)` | One scalar in every lane / build from `[F32#4]`·`[F64#2]` |
| `v[i]` | Read lane `i` (bounds-checked) |
| `+` `-` `*` `/` | Element-wise across all lanes |
| `v.sum()` `v.product()` `v.min()` `v.max()` | Named reductions → lane scalar |
| `v.reduce(@Add)` `@Mul` `@Min` `@Max` | General reduce by op marker |
| `v.to_array()` | Round-trip out to `[F32#4]` / `[F64#2]` |

---

### `core.random` — random numbers

```jet
use core.random as random

fn run() {
    random.seed(42)                         // make the sequence repeatable
    print(random.int(1, 6))                 // inclusive range (like dice)
    print(random.float())                   // 0.0 .. 1.0
    print(random.normal(0.0, 1.0))          // deterministic after seed()
    items :: [10, 20, 30]
    print(random.pick(items))               // one item, or None if list empty
    print(random.sample(items, 2))          // no replacement
    random.shuffle(&items)                  // shuffle in place
    print(items)
}
```

`core.random` is deterministic PRNG randomness for games, simulations, tests,
and sampling. It is not for secrets; use `core.crypto.random.bytes` for keys,
nonces, tokens, salts, and anything security-sensitive.

| Function | Returns | What it does |
|----------|---------|--------------|
| `seed(n)` | nothing | Reset the generator (deterministic after this) |
| `int(low, high)` | `Int` | Random integer, both ends inclusive |
| `float()` | `Float` | Random float from 0 up to (but not including) 1 |
| `float_range(low, high)` | `Float` | Random float in `[low, high)`; returns `low` when the range is empty |
| `bool(p)` | `Bool` | Draw `true` with probability `p`, clamped at 0 and 1 |
| `normal(mean, stddev)` | `Float` | Gaussian draw via Box-Muller; negative stddev is treated as 0 |
| `exponential(lambda)` | `Float` | Exponential draw; non-positive lambda returns 0 |
| `pick(xs)` | `T?` | Random element, or None if `xs` is empty |
| `weighted_pick(xs, weights)` | `T?` | Weighted element; None for length mismatch or no positive weights |
| `sample(xs, k)` | `[T]` | Up to `k` distinct elements without replacement |
| `shuffle(&xs)` | nothing | Randomly reorder a list in place |
| `rng(seed)` | `Rng` | A **deterministic** RNG capability seeded by `seed` (D-DET1) |
| `split(seed)` | `Rng` | Derive a deterministic child stream from the ambient stream plus `seed` |
| `bytes(n)` | `[U8]` | PRNG bytes for fixtures/simulation; not cryptographic |

The ambient calls above (`int`/`float`/…) read a process-global generator, so a
`fn … --[]->` cannot call them (E3403 — they break reproducibility). To use
randomness inside a `fn … --[]->`, take a seeded `Rng` **as a parameter** and draw
through it — the seed makes the stream reproducible on every machine:

```jet
fn roll(rng: &Rng) --[]-> Int {
    return rng.int(1, 6)            // inclusive; advances the stream (needs &Rng)
}
fn run() {
    r := random.rng(42)            // same seed → same draws everywhere
    print(roll(&r))
}
```

The injected `Rng` mirrors the full ambient `random.*` set (D-DET-CAPAPI):

| `Rng` method | Returns | What it does |
|--------------|---------|--------------|
| `int(lo, hi)` | `Int` | Draw an Int in `[lo, hi]` (inclusive); advances the stream |
| `float()` | `Float` | Draw a Float in `[0.0, 1.0)`; advances the stream |
| `float_range(lo, hi)` | `Float` | Draw a Float in `[lo, hi)`; advances the stream |
| `bool()` | `Bool` | Draw a coin; advances the stream |
| `bool(p)` | `Bool` | Draw `true` with probability `p`; advances the stream |
| `normal(mean, stddev)` | `Float` | Gaussian draw; advances the stream |
| `exponential(lambda)` | `Float` | Exponential draw; advances the stream |
| `bytes(n)` | `[U8]` | Deterministic PRNG bytes; advances the stream |
| `split()` | `Rng` | Derive a child stream and advance the parent |
| `pick(xs)` | `T?` | Uniform element of `[T]`, or None if empty; advances the stream |
| `weighted_pick(xs, weights)` | `T?` | Weighted element; advances the stream |
| `sample(xs, k)` | `[T]` | Up to `k` elements without replacement; advances the stream |
| `shuffle(&xs)` | nothing | Reorder a list in place (Fisher–Yates); advances the stream |

Every draw needs a `&Rng` receiver, and `shuffle` needs the list passed with
`&` because it edits in place.

### `core.solve` — finite solver state

`core.solve` gives constraint-style code an explicit state value instead of a
second execution model. The first slice accepts ordinary `Bool` constraints in
the order you add them. Failed constraints are counted; queries are
deterministic.

```jet
use core.solve as solve

fn run() {
    solver := solve.Solver.new(42)
    solver.require(2 + 2 == 4)
    solver.require("red" != "blue")

    print(solver.status())          // ok
    print(solver.failure_count())   // 0
}
```

There is no Prolog-style unification, hidden choice point, or language
backtracking. Finite search stays normal Jet loops and conditionals; the solver
object records the checks you choose to make visible.

---

### `core.game` — headless game substrate

`core.game` is the scene-first substrate. The current slice is deliberately
headless: no renderer, audio, editor, asset file I/O, or native backend is
required to type-check and run a deterministic game transcript.

```jet
use core.game as game

struct Position { x: Int }
struct Velocity { dx: Int }

fn run() {
    scene := game.Scene.new("arcade")
    scene.assets.image("assets/player.png") ?? panic("image")
    scene.input.bind("jump", "Space")
    scene.component<Position>()
    scene.component<Velocity>()
    scene.on_frame((frame) => {
        if frame.input.pressed("jump") {
            print("jump {frame.index}")
        }
    })
    replay :: game.Replay.record("runs/demo.jetreplay")
    print(game.run(scene, replay: replay))
}
```

| Surface | Returns | What it does |
|---------|---------|--------------|
| `game.Scene.new(name)` | `GameScene` | Create one scene identity with assets, input, components, and frame hooks |
| `scene.assets.image(path)` / `.sound(path)` | `GameImage ? String` / `GameSound ? String` | Register a typed scene asset handle; paths containing `missing` fail deterministically |
| `scene.input.bind(action, key)` | nothing | Bind an action name to a device key name |
| `scene.on_frame((frame) => { ... })` | nothing | Attach frame logic to the scene |
| `frame.input.pressed(action)` | `Bool` | Read the deterministic per-frame input snapshot |
| `scene.component<T>()` | nothing | Register a struct-marker component type on the scene |
| `scene.query<T...>()` | `[String]` | Query registered component markers as a deterministic scene view |
| `game.Replay.record(path)` | `GameReplay` | Name a `.jetreplay` game-input artifact for transcript recording; proof replays use `.jetproof-replay` |
| `game.Backend.headless()` | `GameBackend` | Explicit no-renderer/no-audio/no-editor backend value |
| `game.run(scene, replay: replay)` | `String` | Run three deterministic headless frames and return a transcript |

Renderer, audio, editor, and native asset backends are replaceable packages on
top of this surface. Gameplay code keeps the same scene/data/replay API when a
backend package is introduced.

---

### `core.perf` — fidelity signal

`core.perf.Perf` is one runtime-global quality/performance signal. Runtime does
not skip work, reschedule tasks, or change cleanup policy. App code reads the
signal and chooses behavior.

```jet
use core.perf as perf

fn run() -> Void ? {
    if perf.fidelity() < 0.5 {
        print("low quality mode")
    }
    perf.override_fidelity(0.25)?   // tests or explicit app policy
    perf.reset_fidelity()
}
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `fidelity()` | `Float` | Current value, from `0.0` lowest quality through `1.0` full quality |
| `default_fidelity()` | `Float` | The default value, `1.0` |
| `override_fidelity(v)` | `Void ? String` | Set the process-global value; rejects values outside `0.0..1.0` |
| `reset_fidelity()` | nothing | Restore `default_fidelity()` |

Platform battery, thermal, network, load, and carbon providers do not ship in
Epoch 3 (D-ADAPT-PROVIDER1=A). Automatic adaptive scheduling is declined
(D-ADAPTRT1=C).

---

### `core.text` — Unicode text algorithms

`String` stays small. `core.text` owns Unicode-heavy operations and tooling may
insert these calls from String contexts.

| Function | Returns | What it does |
|----------|---------|--------------|
| `nfc/nfd/nfkc/nfkd(text)` | `String` | Normalize text for comparison or storage |
| `casefold(text)` / `caseless_eq(a,b)` | `String` / `Bool` | Locale-free caseless matching |
| `lower/upper(text)` | `String` | Unicode case mapping |
| `graphemes/words/sentences(text)` | `[String]` | Segmentation helpers |
| `width(text)` | `Int` | Terminal display columns, including wide CJK/emoji floor |
| `is_alphabetic/is_numeric/is_whitespace/is_ascii(text)` | `Bool` | Unicode classification over the whole string |
| `scalar_count/byte_count/scalars(text)` | `Int` / `[String]` | UTF-8/scalar facts |
| `splitn/rsplitn(text, sep, n)` | `[String]` | Bounded split helpers |
| `trim/trim_start/trim_end(text)` | `String` | Unicode-whitespace trim |
| `pad_start/pad_end/center(text, width, fill)` | `String` | Display-width padding |
| `starts_any/ends_any(text, parts)` | `Bool` | Prefix/suffix combinators |
| `char_indices(text)` | `[String]` | `"byte:scalar"` debug view |

Locale collation and language-specific sorting are not in v1 core; those need
locale data, not a hidden ASCII fallback.

---

### `core.time` — clocks, calendars, and zones

```jet
use core.time as time

fn run() {
    started :: time.now()                // milliseconds since 1970-01-01 UTC
    time.sleep(100)                      // pause ~100 ms (blocking)
    sw :: time.start()                   // Stopwatch
    time.sleep(50)
    print(sw.elapsed_millis())           // at least 50
    print(time.now() - started)

    dt :: time.parse_rfc3339("2024-03-10T06:30:00Z") ?? return
    ny :: time.zone("America/New_York") ?? return
    print(dt.in_zone(ny).format("yyyy-MM-dd HH:mm:ss VV XXX"))
}
```

| Function / type | Returns | What it does |
|-----------------|---------|--------------|
| `now()` | `Int` | Current Unix time in milliseconds |
| `now_utc()` | `DateTime` | Current UTC wall-clock date-time |
| `from_unix_ms(ms)` | `DateTime` | Convert Unix milliseconds to UTC `DateTime` |
| `parse_rfc3339(text)` | `DateTime ? String` | Parse RFC 3339 / ISO 8601 offset text |
| `today()` | `LocalDate` | Current UTC date |
| `local_time(h, m, s)` / `parse_time(text)` | `LocalTime` / `LocalTime ? String` | Local wall-clock time |
| `instant()` | `Instant` | Monotonic clock sample for elapsed-time measurement |
| `zone(name)` / `utc()` | `Zone ? String` / `Zone` | IANA time zone from TZif zoneinfo, or UTC |
| `zoned(dt, zone)` | `ZonedDateTime` | View a UTC `DateTime` in a zone |
| `zoned_local(date, time, zone)` | `ZonedDateTime` | Resolve local civil time in a zone |
| `sleep(millis)` | nothing | Block for about `millis` milliseconds (runtime E3003 if an ambient `@Context(deadline: …)` budget expires first) |
| `time.start()` | `Stopwatch` | Start a stopwatch |
| `sw.elapsed_millis()` | `Int` | Milliseconds since `time.start()` |
| `clock(seed)` | `Clock` | A **deterministic** clock capability starting at `seed` ms (D-DET1) |
| `Duration.milliseconds/seconds/minutes/hours(n)` | `Duration ? RangeError` | Checked runtime elapsed-time span |
| `period(years, months, days)` / `period_days(n)` / `period_months(n)` / `period_years(n)` | `Period` | Calendar span for local-date arithmetic |

`DateTime` is an unambiguous UTC instant. `LocalDate` and `LocalTime` are civil
values without a zone. `ZonedDateTime` combines an instant with a `Zone` so
formatting and calendar arithmetic use the right offset. `Duration` is elapsed
time; `Period` is calendar time. Across DST, `z.add_duration(Duration.hours(24)?)`
adds 24 real hours, while `z.add_period(time.period_days(1))` keeps the same
local clock time on the next calendar day.

`time.zone(name)` reads IANA TZif data from `JET_TZDB_DIR` first, then `TZDIR`,
then the repo bundled fallback at `$JET_ROOT/corelib/tzdb`, then common system
zoneinfo directories. The Nix shell sets `TZDIR` to `pkgs.tzdata`; other
environments may set `JET_TZDB_DIR` to an updated tzdb without changing code.

Useful methods:

| Type | Methods |
|------|---------|
| `LocalDate` | `year()`, `month()`, `day()`, `add_days(n)`, `add_months(n)`, `add_period(p)`, `diff_days(other)`, `weekday()`, `iso_weekday()`, `day_of_year()`, `iso_week()`, `truncate(unit)`, `format(pattern)`, `to_string()` |
| `LocalTime` | `hour()`, `minute()`, `second()`, `to_string()` |
| `DateTime` | `date()`, `time()`, `hour()`, `minute()`, `second()`, `to_timestamp()`, `to_unix_ms()`, `plus_duration(d)`, `truncate(unit)`, `round(unit)`, `in_zone(zone)`, `format_rfc3339()`, `format(pattern)`, `to_string()` |
| `ZonedDateTime` | `date()`, `time()`, `offset_seconds()`, `to_datetime()`, `zone()`, `add_duration(d)`, `add_period(p)`, `format(pattern)`, `to_string()` |
| `Instant` | `elapsed_millis()` |
| `Duration` | `in(unit)` |
| `Zone` | `name()` |

Format patterns are literal text plus `yyyy`, `MM`, `dd`, `HH`, `mm`, `ss`,
`EEE`, `DDD`, `VV`, and `XXX`. Leap seconds are not represented as a distinct
instant: RFC 3339 parsing rejects `:60`; use upstream clock smear policy before
data reaches Jet.

**Test hook:** when the environment variable `LEX_TEST_EPOCH` is set to an
integer, `time.now()` returns that value instead of the real clock. Tests use
this to pin output; normal programs ignore it.

A `fn … --[]->` cannot call ambient `time.now()` (E3403 — the wall clock is not
reproducible). To use time inside a `fn … --[]->`, take a seeded `Clock` **as a
parameter** and read through it; the clock only moves when you `tick` it, so the
result is reproducible:

```jet
fn at(clock: Clock) --[]-> Int {
    return clock.now()             // current value in ms; pure read
}
fn run() {
    c :: time.clock(1000)          // a Clock starting at 1000 ms
    print(at(c))                   // 1000, on every machine
}
```

| `Clock` method | Returns | What it does |
|----------------|---------|--------------|
| `now()` | `Int` | The clock's current value in ms (read; no `&` needed) |
| `tick(ms)` | `Int` | Advance the clock by `ms` (relative) and return the new value (needs `&Clock`) |
| `advance(to_ms)` | `Int` | Set the clock to the **absolute** instant `to_ms` and return it (needs `&Clock`; D-DET-CAPAPI) |
| `wait(d)` | `Int` | Advance the clock by a `Duration` `d` and return the new value (needs `&Clock`; D-DET-CAPAPI) |

A runtime `Duration` is built with checked type-owned unit methods such as
`Duration.seconds(n)?`. Read a whole unit with `d.in(.Milliseconds)?`; the
result truncates toward zero and reports `RangeError` on overflow. Static unit
literals such as `5s` remain unchanged.

| `Duration` method | Returns | What it does |
|-------------------|---------|--------------|
| `in(unit)` | `Int ? RangeError` | Whole milliseconds, seconds, minutes, or hours; truncates toward zero |

**Expert escape — `assume_deterministic { … }`.** Inside a `fn … --[]->`, a block
written `assume_deterministic { … }` suspends the determinism check (E3401/E3403)
for its body — the "I know this is deterministic" hatch. It is a semantic
footgun: nothing verifies the claim, so use it only when you can guarantee
reproducibility yourself. See `examples/features/effects/determinism.jet`.

---

### `core.encoding` — unified serialization (json, csv, toml, yaml)

One library, every format a submodule (D-ENC1). Import the whole library and
reach each format by name, or import a single format directly:

```jet
use core.encoding                    // encoding.json.*, encoding.csv.*, …
use core.encoding.json as json       // or just one format
```

Every format speaks the same two verbs: `parse` (text → value) and `to_string`
(value → text, D-JSONVERB1). JSON adds `to_string_pretty` and `decode`.

```jet
use core.encoding

fn run() {
    raw :: "{\"name\":\"jet\",\"ok\":true,\"n\":1.5}"
    data :: encoding.json.parse(raw) ?? return
    print(encoding.json.to_string(data))           // compact one line
    print(encoding.json.to_string_pretty(data))    // indented

    if data == .Object(entries) {
        if entries.contains("name") {
            print(entries["name"])
        }
    }
}
```

**One dynamic value, four format adapters (D-SERDE13).** Every format's untyped
`parse` returns **`DataTree`** — variants `.Null` / `.Bool` / `.Int` / `.Float` /
`.Text` / `.Array` / `.Object`. `DataTree` is the only user-facing tree name;
the retired `Data` spelling is a teaching error, not an alias. Every adapter shares
one structure with one walker and one accessor set (`.field(name)`, `.at(i)`,
`.int()`, `.float()`, `.text()`, `.bool()`). Integral numbers decode to `.Int`,
fractional to `.Float`; objects keep field order.

| Function | Returns | What it does |
|----------|---------|--------------|
| `parse(text)` | `JSON ? JSONError` | Parse a JSON string |
| `decode(text)` | `JSON ? JSONError` | Lenient parse — coerces string→number/bool, logs each coercion (D-JSON3) |
| `to_string(j)` | `String` | Compact JSON text |
| `to_string_pretty(j)` | `String` | Indented JSON text |

**`JSONError`** — `line` and `message` pointing at the parse failure.

**`core.encoding.csv`** — `parse(text) -> [[String]] ? String` (rows of fields),
`to_string(rows) -> String`, plus bounded `reader` / `writer` handles over
RFC-4180 records. Quoted fields preserve commas, escaped quotes, and embedded
newlines; malformed quote closure is an error rather than a partial row.
**`core.encoding.toml`** / **`core.encoding.yaml`**
— `parse(text) -> TOML ? JSONError` / `YAML ? JSONError` (full adapters over
`DataTree`, not a flat map), `to_string(value)`.

**Ratified Epoch 3 breadth (D-ENCSTREAM1 and follow-ups).** The same `DataTree`
tree backs one whole-value and streaming adapter contract per format:
The exact signatures, defaults/ranges/accounting, tagged XML schemas, error
paths/projections, canonical byte rules, strict decoder matrices, lifecycle,
test vectors, and edition migrations are normative in
[`../spec/encoding-decisions.md`](../spec/encoding-decisions.md).

| Module | Surface | What it does |
|--------|---------|--------------|
| `core.encoding.json` | `canonical(data, limits)`, `reader`, `writer` | Edition-2027 RFC 8785 JCS; pull `DataEvent` streaming; shipped `events(DataTree)->String` remains separate until migration |
| `core.encoding.jsonl` | `parse(text)`, `to_string(rows)` | JSON Lines over `[DataTree]` |
| `core.encoding.csv` | `parse(text)`, `decode<T>`, `to_string(rows)`, `reader`, `writer` | Whole-value and bounded pull records over the same CSV quoting and validation law |
| `core.encoding.xml` | `parse`, `parse_bytes`, `to_string`, `to_bytes`, `canonical`, `reader`, `writer` | Exact tagged ordinary-`DataTree` tree/events with namespaces, token-local lexical evidence, safe entities/limits, and W3C C14N |
| `core.encoding.cbor` | `parse`, `decode<T>`, `to_bytes`, `to_bytes_canonical`, `reader`, `writer` | RFC 8949 typed/native bytes and Core deterministic profile |

Each adapter is a full serde equivalent, not a lossy subset:

- **JSON** — full RFC 8259: exponents and the strict number grammar, every
  escape including `\uXXXX` with surrogate-pair combining; rejects invalid
  escapes, lone surrogates, and raw control characters with a line + message.
- **JSONL** — one JSON value per non-empty line, returned as `[DataTree]`.
- **XML** — D-ENCXML1's closed `$xml`/`$xml_event` ordinary-`DataTree` algebra,
  expanded names, ordered namespaces/attributes/content, encoding/BOM and
  token-local lexical evidence; no lossy `{name, attrs, children, text}` alias.
- **CBOR** — typed `[U8]` maps to native byte strings. Untyped `DataTree` rejects
  byte strings and every value outside its closed algebra. Canonical maps use
  RFC 8949 section 4.2.1 complete encoded-key byte ordering. Whole-value normal
  mode accepts definite and indefinite byte strings, text strings, arrays, and
  maps; canonical validation rejects every indefinite-length item at its original
  byte offset. Indefinite chunks and containers share the same depth, item, live
  allocation, duplicate-key, UTF-8, path, and typed-decode checks as definite
  values.
- **Base encodings are not dynamic-tree/stream codecs.** `core.encoding.base64` exposes
  `encode`/`decode` and `encode_url`/`decode_url`; `core.encoding.base32`
  exposes `encode`/`decode`. They are scalar `[U8]`/`String` RFC 4648 helpers
  with edition-2027 strict defaults and named narrow allowances.
- **CSV** — header-mapped typed rows (`decode<T>` maps columns to fields by name).
- **TOML** — full TOML 1.0: `[table]` headers, `[[array-of-tables]]`, dotted keys,
  inline tables, strings (every escape + multi-line), integers in every base,
  floats incl. `inf`/`nan`, booleans, datetimes, arrays.
- **YAML** — full YAML 1.2 core (D-ENC-YAML1): block + flow maps/sequences,
  core-schema typed scalars, single/double-quoted + plain + block scalars
  (`|`/`>` with chomping), comments, `---`/`...` document markers, and
  anchors/aliases (`&a`/`*a`). Explicit/custom tags (`!!str`, `!T`) are deferred.

**Current implementation boundary:** JSONL, the existing lossy XML prototype,
base32/base64url, and an infallible key-sorting `json.canonical` exist. CBOR's
typed whole-value byte verbs, closed errors/options, native `[U8]`, original-wire
Core deterministic validation, live allocation limits, and normal-mode
indefinite values execute in the native runtime; pull handles also exist. Exact
lossless XML algebra/C14N, RFC 8785 serialization, strict edition migration,
full hostile standards corpora, complete stream lifecycle proof, error-allocation
oracles, and AOT/JIT/comptime parity remain open. Entries above state ratified API
law, not a broad-complete implementation claim.

Compiler/runtime codec implementations remain std-only under I6.

Jet has no general `Any` top type (D-DYNAMIC-TYPE1): use the precise shape for
the job — an enum for a closed set of variants, generics or traits for
abstraction, `T?` for absence, and `DataTree` for parsed dynamic input. Writing
`Any` in type position is **E0350**.

### `core.data` — typed tables, series, status, plots

D-DATA-SURFACE1 makes `core.data` the beginner facade for typed tables,
series, stats, CSV, and plots. The first slice is in-memory and deterministic:
`data.csv<T>(text)` decodes CSV into `[T]` using the same `@[Codable]` model as
`core.encoding.csv.decode<T>`. Selectors are typed lambdas, so a misspelled row
field is a Jet field error before codegen.

| Function | Returns | What it does |
|----------|---------|--------------|
| `csv<T>(text)` | `[T] ? DecodeError` | Header-mapped typed CSV rows |
| `table(rows)` / `rows(table)` | `Table<T>` / `[T]` | Wrap and unwrap the typed in-memory table model |
| `series(values)` / `values(series)` | `Series<T>` / `[T]` | Wrap and unwrap typed series values |
| `missing_count(series)` | `Int` | Count absent `T?` values in a typed series |
| `lazy(table)` / `collect(plan)` | `LazyFrame<T>` / `Table<T>` | Build a typed plan; execute it only when materialized |
| `lazy_filter(plan, row => ok)` / `lazy_sort_by(plan, row => key)` | `LazyFrame<T>` | Append deferred typed operations without visiting rows |
| `plan(frame)` | `[String]` | Deterministic plan-step names for audit/test output |
| `count(value)` | `Int` | Count rows/values in `[T]`, `Table<T>`, `Series<T>`, or `LazyFrame<T>` |
| `sum(values)` / `mean(values)` / `min(values)` / `max(values)` | `Float` | Numeric series stats over `[Float]` |
| `median(values)` / `quantile(values, q)` | `Float` | Sorted numeric quantiles |
| `variance(values)` / `stddev(values)` / `describe(values)` | `Float` / `Float` / `DataSummary` | Numeric distribution summary |
| `rolling_mean(values, width)` | `[Float]` | Prefix-safe rolling window mean |
| `group_count(rows, row => row.key)` | `[DataGroup]` | Count rows by a `String` key |
| `group_sum(rows, row => row.key, row => row.value)` | `[DataGroup]` | Sum a `Float` selector per key |
| `group_mean(rows, row => row.key, row => row.value)` | `[DataGroup]` | Mean a `Float` selector per key |
| `filter(rows, row => ok)` / `sort_by(rows, row => key)` | `[T]` | Typed in-memory row pipeline |
| `inner_join(left, right, l => key, r => key)` | `[DataJoin<L, R>]` | Stable matching row pairs with SQL join multiplicity |
| `left_join(left, right, l => key, r => key)` | `[DataJoin<L, R?>]` | Stable row pairs; unmatched left rows carry `None` |
| `pivot_sum(rows, row => row_key, row => col_key, row => value)` | `[DataGroup]` | Deterministic row/column sum cells as `row|col` keys |
| `status()` | `[DataStatus]` | Native/bridge replacement facts for data workflows |
| `bar_text(groups)` / `bar_svg(groups)` | `String` | Deterministic text/SVG bar output |

`Table<T>` and `LazyFrame<T>` keep typed rows; `Series<T>` keeps typed values.
Missing values are ordinary Jet optionals (`T?`) inside a series, not a second
sentinel type. `DataGroup` fields: `.key: String`, `.count: Int`, `.sum: Float`,
`.mean: Float`. `DataJoin<L, R>` fields are `.left: L` and `.right: R`; the
left-join form uses `R?`. `DataStatus` fields: `.step`, `.path`, `.replacement`.

```jet
use core.data as data

@[Codable]
struct Ticket {
    team: String
    minutes: Float
}

fn run() {
    rows :: data.csv<Ticket>("team,minutes\nCore,4.0\nCore,8.0\nTools,5.0") ?? panic("bad csv")
    groups :: data.group_mean(rows, t => t.team, t => t.minutes)
    print(data.bar_text(groups))
}
```

### `core.fmt` — human-readable formatting

D-HUMANFMT1 keeps formatting as library calls, not a second syntax inside
interpolation. The beginner path is the thing report and CLI authors need every
day: readable numbers, bytes, durations, ordinals, plural phrases, and padding.

| Function | Returns | What it does |
|----------|---------|--------------|
| `number(n)` | `String` | Thousands-grouped integer |
| `decimal(x, places)` | `String` | Fixed decimal with grouped whole part |
| `percent(x, places)` | `String` | `x * 100` with `%` |
| `bytes(n)` | `String` | SI byte units (`KB`, `MB`, `GB`, ...) |
| `duration(ms)` | `String` | Compact `d h m s` / `ms` duration |
| `ordinal(n)` | `String` | `1st`, `2nd`, `3rd`, `4th` |
| `plural(n, one, many)` | `String` | Count plus singular/plural noun |
| `pad_left` / `pad_right` / `pad_center` | `String` | Width padding by character count |

```jet
use core.fmt as fmt

fn run() {
    print("{fmt.bytes(1500000000)} in {fmt.duration(222000)}")
    print("{fmt.number(1204331)} rows")
}
```

### `core.log` — structured logs, spans, sinks

`core.log` records events as typed fields plus optional span context. Plain
`info`/`warn`/`error`/`debug` is still the beginner path; `*_fields` carries
typed `LogField` values for services and audit logs.

```jet
use core.log as log

fn run() {
    log.set_sink("jsonl", "service.log")
    span :: log.span("request")
    log.enter(span)
    log.info_fields("served", [log.field("route", "/"), log.int("status", 200)])
    log.close(span)
}
```

Core helpers include `field`, `int`, `float`, `bool`, `redact`, `counter`,
`span`, `enter`, `close`, `set_sink`, `sample_every`, `otlp_file`, `set_level`,
`set_trace_id`, and `setup`.

#### Typed (de)serialization — one derive, every format (D-SERDE1–8)

Mark a type `@[Codable]` and it crosses the wire in any format. `@[Codable]` is
both directions; the one-way markers are `@[Encode]` (write-only) and `@[Decode]`
(read-only). The derive is compiler-owned (like `derive Comparable`) — no macros,
no runtime reflection.

```jet
use core.encoding.csv as csv
use core.encoding.json as json

@[Codable]
struct Order {
    id: Int
    @[Rename("customer")] who: String      // wire key overrides the field name
    items: [String]
    note: String?                          // absent optional is omitted on the wire
}

fn run() {
    o :: Order.{ id: 7, who: "Ada", items: ["pen", "ink"], note: None }
    print(json.to_string(o))               // {"id":7,"customer":"Ada","items":["pen","ink"]}

    raw :: "{{\"id\":9,\"customer\":\"Bo\",\"items\":[\"ink\"],\"note\":\"rush\"}}"
    back :: json.decode<Order>(raw) ?? panic("bad order")   // typed decode
    print(back.who)                        // Bo
}
```

**Encode** — `to_string(v)` / `to_string_pretty(v)` accept any `@[Codable]`/`@[Encode]`
value (the dynamic `JSON` tree and the `[[String]]`/`[K: V]` forms still work too). Field
order is preserved.

**Typed decode** — `decode<T>(text)` (D-SERDE6) returns `T ? DecodeError` for
json/toml/yaml, and `[T] ? DecodeError` for csv (one struct per row, columns mapped
to fields by header name). The target type comes from the `<T>` turbofish or an
cfg: Config :: json.decode(text)`). Bare `json.decode(text)` with no
target stays the lenient dynamic `JSON` (above). `DecodeError` carries a field `path`
and a `reason`; compose it with `??`.

```jet
raw :: "item,qty\npen,3\nink,5"
sales :: csv.decode<Sale>(raw) ?? panic("bad csv")   // [Sale]
print(json.to_string(sales))   // [{"item":"pen","qty":3},{"item":"ink","qty":5}]
```

**Hand codecs and subtree dispatch** (D-SERDE2, D-SERDE13–16) use the same
protocol as built-in derives. Write `impl T.Encode` with `encode(self) ->
DataTree` and `impl T.Decode` with `decode(tree: DataTree) -> T ? DecodeError`.
Tree accessors add their field/index path and return `DecodeError`, so `?`
chains without manual mapping. `tree.decode<T>()` dispatches any subtree
through `T`'s ordinary `Decode` implementation, including primitives, user
types, lists, options, and string-keyed maps. A derived parent therefore
composes with a hand-written field codec; generated and hand-written paths are
one mechanism.

```jet
impl Email.Decode {
    fn decode(tree: DataTree) -> Email ? DecodeError {
        address := tree.text()?
        return Ok(Email.{ address })
    }
}

items := tree.field("items")?.decode<[LineItem]>()?
```

**Traced decode — was this migrated?** (D-MIGRATE3=A, D-MIGRATE4=A):
`decode_traced<T>(text)` sits beside `decode<T>` on every codec
(json/csv/toml/yaml share the decode machinery) and returns
`DecodeResult<T> ? DecodeError` — `{ value: T, migration: MigrationStatus }`.
`MigrationStatus` carries `.migrated: Bool`, `.from` (the source shape's
version label, `"v1"` = oldest), and `.steps` (one entry per migration step
applied, `"v1->v2"` style). `decode` itself is untouched — same call, same
cost, for anyone not asking (I8).

```jet
r    :: json.decode_traced<UserRecord>(raw)?
user :: r.value
if r.migration.migrated {
    log.info("record {user.id} arrived as schema {r.migration.from}")
}
```

Decoding a `@PublishedSchema` type with `migration { }` blocks (below) runs
the runtime chain: the current shape is tried first; on mismatch the data's
field-name set picks the historical shape it matches (newest match wins) and
the migration steps rewrite it forward — `rename` moves a key, `remove` drops
one, `add` fills the default, `change` runs the `via { … }` converter. Plain
`decode` applies the same chain silently; data matching no shape keeps the
ordinary decode error. `.migrated` is `false` and `.from`/`.steps` are empty
for a plain type and for fresh (current-shape) data, and types without
migration blocks pay nothing.

**Accumulated validation — `validate { }`** (D-VALIDATE1, card #506): a
struct declares its own validation rules in the body, beside its fields —
the struct stays the one schema (I8). Rules are `check(condition, at: field,
"message")` statements; `field` is a bare sibling-field reference
(D-FIELDPOL1). Every failing `check` accumulates — a rule set with three
violations reports all three, not just the first:

```jet
struct Signup {
    email: String
    password: String

    validate {
        check(email.len() > 0, at: email, "email required")
        check(password.len() >= 12, at: password, "needs at least 12 characters")
        check(password != email, at: password, "password can't be the email")
    }
}

errs :: Signup.validate(bad_signup) // Signup ? [FieldError]
```

`Type.validate(value)` runs the block standalone, returning `value ?
[FieldError]` — `FieldError` carries `.path`/`.reason`, the same shape as
`DecodeError`. Rule expressions are purity-checked (S60/E3401): a `check`'s
condition and message may reference only the struct's own fields and pure
calls, never Net/Db/Io. Card #506 slice 1 ships the block and
`Type.validate(value)`; `decode<T>()` auto-run and the `Validate.over(s)`
use-site escape (for rules needing outside context, like a database lookup)
are follow-on work — see docs/spec/syntax-decisions.md's D-VALIDATE1 entry.

**Field attributes** (D-SERDE5):

| Attribute | Effect |
|-----------|--------|
| `@[Rename("k")]` | use `k` as the wire key for this field |
| `@[Skip]` | never serialize; on decode use the field's default |
| `@[Default]` / `@[Default(8080)]` | when the key is absent, use the type's default (or the given literal) |
| `@[Flatten]` | inline a `@[Codable]` struct field's keys into the parent object |

**Container attributes:**

| Attribute | Effect |
|-----------|--------|
| `@[RenameAll(camel)]` | map every field's wire key — `camel`/`snake`/`pascal`/`kebab`/`screaming` (D-SERDE3) |
| `@[DenyUnknownFields]` | a wire key the struct doesn't declare is an error, not ignored (D-SERDE8) |
| `@[Tag("type")]` / `@[Untagged]` | enum wire representation (D-SERDE7); default is externally tagged |

**Enums** serialize externally tagged by default: a unit variant is its bare name
(`"Closed"`), a payload variant is `{"Variant": payload}`. `@[Tag("type")]` switches
to internal tagging (`{"type":"Click", …}`); a single unnamed payload uses the
canonical `value` key (`{"type":"Count","value":7}`). `@[Untagged]` emits the
payload alone.

Unknown wire keys are ignored by default (forward-compatible); opt into strict
checking with `@[DenyUnknownFields]`. Diagnostics: E2407 (`@[Rename]` non-string),
E2408 (`@[Flatten]` non-struct), E2409 (bad `@[RenameAll]` style), E2410 (missing
required field, runtime), E2411 (type isn't serializable — also fires at the use
site for a non-codable generic argument), E2412 (unknown field, runtime). E2413 is
retired (D-SERDE12).

Generic `@[Codable]` is first-class (D-SERDE9-12): the derive auto-injects
`T: Encode`/`T: Decode` bounds on exactly the type params that reach the wire —
the user never spells them. A phantom or `@[Skip]`-only param carries no serde
bound (only structural `Clone`), so `Id<Kind>` serializes for any `Kind`. A
non-codable type argument fails at the use site (E2411), not the definition.

The expert hand-impl path is live: `impl T.Encode { fn encode(self) -> DataTree
{ … } }` and `impl T.Decode { fn decode(tree: DataTree) -> T ? DecodeError {
… } }`. Generated and hand-written codecs use the same protocol dispatch.

---

### `core.tasks` — tasks and channels

Blocking tasks and typed channels are Jet's concurrency model. There is no
`async`/`await` and no mutex API; tasks communicate by sending owned values.

```jet
use core.tasks as tasks

fn sum_range(first: Int, last: Int) -> Int {
    total := 0
    loop n; first..last {
        total += n
    }
    return total
}

fn run() {
    a :: tasks.spawn(() => sum_range(1, 25))
    b :: tasks.spawn(() => sum_range(26, 50))
    c :: tasks.spawn(() => sum_range(51, 75))
    d :: tasks.spawn(() => sum_range(76, 100))
    print(a.join() + b.join() + c.join() + d.join())
}
```

Channels carry one type:

```jet
use core.tasks as tasks

fn run() {
(sender, ch) :: tasks.channel<Int>()
    task :: tasks.spawn(take(sender) () => {
        sender.send(42)
    })
    task.join()
    print(ch.receive() ?? panic("channel closed"))
}
```

`tasks.channel<T>()` returns the send/receive pair directly (D-TUPLE-DESTRUCT1) —
destructure it with `(tx, rx) := tasks.channel<T>()`. `tasks.channel<T>(capacity:
N)` creates a bounded channel; `send` parks when the queue already holds `N`
values and resumes when a receiver drains space. A second sender is `copy tx`
(D-CAP2's one copy verb — a cheap handle duplicate, not a method on the channel;
there's no combined channel value).

| Function / type | Returns | What it does |
|-----------------|---------|--------------|
| `tasks.spawn(lambda)` | `Task<T>` | Run a zero-parameter lambda on a new task |
| `task.join()` | `T` | Wait for the task and consume the task handle |
| `task.wait()` | `T` | Alias of `.join()` |
| `task.pause()` | nothing | Request paused state on the task control plane (D-COROUTINE1) |
| `task.resume()` | nothing | Clear paused state on the task control plane |
| `task.cancel()` | nothing | Request cancellation on the task control plane |
| `task.trace()` | `String` | Read control-plane state as `paused=...,cancel=...` |
| `tasks.channel<T>()` | `(Sender<T>, Receiver<T>)` | Create an unbounded linked send/receive pair |
| `tasks.channel<T>(capacity: N)` | `(Sender<T>, Receiver<T>)` | Create a bounded pair with real backpressure |
| `tasks.after(ms: N)` | `Receiver<Unit>` | One-shot timer channel |
| `tasks.after(ms: N, value: fallback)` | `Receiver<T>` | One-shot typed timer channel for timeout values |
| `tasks.interval(ms: N)` | `Receiver<Int>` | Interval timer channel sending `1`, `2`, ... |
| `copy sender` | `Sender<T>` | Create another send half (cheap handle duplicate, `copy` verb — not a method) |
| `sender.send(value)` | nothing | Move one value into the channel |
| `receiver.receive()` | `T ? Closed` | Block for a value, or return `Closed` when senders are gone |

Values crossing `spawn` or `send` must be sendable: no `View<T>` or string-view
windows, no trait values, and no closure values unless they are handed over
with `take`. A `Task` that goes out of scope without
`.join()` emits warning **L1101**.
With `@Context(deadline: <Int epoch_ms>)`, blocking waits (`task.join()` /
`task.wait()` / `ch.receive()` / `sender.send()` / `time.sleep`, TCP read/write,
and `ProcessChild.wait()`) observe the inherited budget and report runtime
**E3003** on exceed. Task cancellation wakes the same scheduler wait points.

`taskgroup` owns child tasks until scope exit. `g.all`, `g.race`, and `g.any`
join task lists on the scheduler; `race`/`any` cancel losers. `g.select()` races
receivers and timers: `.recv(rx)` waits for a channel value, `.after(ms: N)` is a
unit timer arm, and `.after(ms: N, value: fallback)` is a typed timeout arm that
can be mixed with same-`T` receive arms.

### `core.testing` — fixtures under `@Test`

D-TESTKIT1 keeps `@Test` as the only test syntax. `core.testing` is a helper
library for test data and deterministic fixtures.

```jet
use core.testing as testing

fn run() {
    print(testing.fixture("fixtures/input.txt"))
    print(testing.golden("expected/out.txt", "actual output"))
    print(testing.snap("case", "snapshot text"))
}
```

Helpers: `snap`, `golden`, `fixture`, `temp_dir`, `corpus`, `fake_clock`, and
`fake_rng`. Use `expect(value).snapshot()` inside `@Test`
blocks for assertion snapshots; `testing.snap` is for explicit named files.

Benchmark limits use a `@Bench` region plus a typed `Budget` declaration. The
shared budget evaluator owns samples, baselines, confidence, reports, and CI
outcomes; `core.testing` has no separate benchmark evaluator.

#### `jet test` — directory recursion, filters, shuffle, parallel runs

`jet test <dir>` walks every subdirectory (skipping `build/` and dotdirs),
running every `.jet` file found, in sorted path order. A directory that has a
`pkg.jet` manifest is still treated as a project root (its single entry file
runs), same as before.

Tests run in parallel by default — one thread per test, with its own
`testing.temp_dir` (the thread id is folded into the path) and its own
captured `print()` output, flushed right above that test's result line so a
test's own output always reads the same as it did running alone. `--serial`
opts out and runs one test at a time.

```
jet test <file|dir>              # parallel by default; walks subdirectories
jet test <file|dir> --serial     # one test at a time
jet test <file> --filter=foo     # only run tests whose name contains "foo"
jet test <file> --shuffle        # random (printed) order — order-dependence check
jet test <file> --shuffle=42     # reproduce a specific shuffled order
```

#### `jet fuzz` — fuzz a property test

`jet fuzz <file> [<test-name>]` fuzzes a parameterized `@Test fn` (the same
property-test form D-TEST1 gives `jet test` — see above) well past the
200-case property-test budget, with corpus persistence and automatic
minimization. The test name is optional when the file has exactly one
property test; with more than one, name which:

```
jet fuzz examples/features/tooling/fuzz_demo.jet reverse_twice_is_identity
```

- **Corpus persistence**: every failure's seed is saved under
  `.jet/fuzz/<file>/<test>/` (override with `--corpus=<dir>`) and replayed
  first on the next run — a fixed bug stays caught until it's actually fixed.
- **Minimization**: the first failing case is shrunk with the same greedy
  algorithm `jet test`'s property-test driver uses, so the report names a
  minimal counterexample, not the first (possibly huge) random input.
- **Deterministic seeded PRNG**: the same `JetRng` splitmix64 generator
  D-TEST1 already ships (std-only, I6). `--seed=<n>` pins the base seed (the
  default is a fixed constant, so even a bare `jet fuzz` run reproduces); a
  saved seed alone is a full, exact reproduction of that case.
- **Budget flags**: `--iterations=<n>` (default 1000) and/or `--time=<n>`
  (wall-clock seconds).

A clean run:

```
corpus: 0 case(s) replayed clean
reverse_twice_is_identity: 500 iteration(s), no failure found
```

A failure — minimized, saved, and printed as a runnable repro:

```
always_small: FAIL (after 9 iteration(s))
  condition failed
  minimized input: n = 50
  seed: 2476628477891077985
  saved: .jet/fuzz/prop_shrink/seed_2476628477891077985.txt
repro: JET_PROP_SEED=2476628477891077985 jet test tests/fixtures/prop_shrink.jet
```

### `core.regex` — linear-time regular expressions

`use core.regex as re`. Matching is **linear-time** — the engine is a
std-only Thompson/Pike NFA with no catastrophic backtracking, so patterns are
ReDoS-safe by construction. Backreferences and lookaround do not exist (the
safety property would be lost), and that is deliberate.

Pattern-string calls return a `Result`; the `Err` carries a one-line message
when the pattern itself is malformed (the only failure at the boundary).
`compile` returns a reusable `Regex`, so hot paths parse once. A `Match`
records capture text plus byte spans: `group(0)` is the whole match,
`group(n)` is the n-th group as `String?`, and named groups are read with
`name("group")`.

```jet
use core.regex as re

fn run() {
    text :: "order 42 shipped"
    print(re.is_match("\\d+", text) ?? panic("bad pattern"))   // true

    m :: re.match("(\\d+) shipped", text) ?? panic("bad pattern")
    if m == Val(mat) {
        print(mat.group(0) ?? "")   // 42 shipped
        print(mat.group(1) ?? "")   // 42
    }

    print(re.replace_all("\\d+", text, "#") ?? panic("bad pattern"))

    flags :: re.flags(true, true, false)              // case-insensitive, multiline, dotall
    rx :: re.compile_with("^(?<word>[a-z]+)", flags) ?? panic("bad pattern")
    hit :: rx.match("Ada\nlovelace")
    if hit == Val(mat) {
        print(mat.name("word") ?? "")                // Ada
        print(mat.start())                           // 0
    }
}
```

| Call | Returns | Does |
|------|---------|------|
| `re.flags(case_insensitive, multiline, dotall)` | `RegexFlags` | typed flag set |
| `re.compile(pat)` | `Regex ? String` | parse once with default flags |
| `re.compile_with(pat, flags)` | `Regex ? String` | parse once with typed flags |
| `re.is_match(pat, text)` | `Bool ? String` | whether `pat` occurs anywhere |
| `re.match(pat, text)` | `Match? ? String` | first match with capture groups, `None` if none |
| `re.find(pat, text)` | `String? ? String` | first matched substring, `None` if none |
| `re.find_all(pat, text)` | `[String] ? String` | every non-overlapping match, left to right |
| `re.matches(pat, text)` | `[Match] ? String` | every non-overlapping match with captures/spans |
| `re.replace(pat, text, repl)` | `String ? String` | replace the first match (`$1`, `${name}` allowed in `repl`) |
| `re.replace_all(pat, text, repl)` | `String ? String` | replace every match |
| `re.split(pat, text)` | `[String] ? String` | split `text` on every match |
| `re.split_limit(pat, text, n)` | `[String] ? String` | split at most `n - 1` times |
| `rx.is_match(text)` | `Bool` | reuse a compiled regex |
| `rx.match(text)` | `Match?` | first match with captures/spans |
| `rx.matches(text)` | `[Match]` | all matches with captures/spans |
| `rx.replace_all_with(text, fn(Match) -> String)` | `String` | replace every match with callback output |
| `mat.group(n)` | `String?` | capture group `n` of a `Match` |
| `mat.name(name)` | `String?` | capture group by name |
| `mat.start()` / `mat.end()` | `Int` | byte span of the whole match |
| `mat.group_start(n)` / `mat.group_end(n)` | `Int?` | byte span of a capture |

Note: `{N}` quantifiers must be written `{{N}}` in Jet source — single braces
are string interpolation (S8). Write `\\d{{4}}` for "four digits".

`core.regex` has no external dependency and does not create a hidden FFI bridge.

---

### `core.reactive` — signals, derived values, effects (D-REACT1)

`use core.reactive as reactive`. Reactivity is an **opt-in library**, not core
language semantics — ordinary bindings stay non-reactive. The library adds three
explicit reactive values:

- **signal** — a mutable reactive source. `reactive.signal(initial)` infers `T`
  from the initial value and returns a `Signal<T>`. Read with `.get()`, update
  with `.set(v)`.
- **derived** / **computed** — a value recomputed from the signals it reads.
  `reactive.derived(() => expr)` returns a `Derived<T>`; `reactive.computed` is
  the D-SIGNAL1 canonical alias (type name `Computed<T>`). `.get()` reflects the
  latest computation.
- **effect** — a side effect. `reactive.effect(() => { … })` runs the body now,
  and again whenever a signal it read changes. **`@Reactive { … }`** (D-REACTCORE1)
  is sugar for the same scope — the compiler lowers it to `jet_reactive_effect`.
  **`@Reactive fn`** wraps the whole function body the same way (unit return only).

Dependency tracking is **explicit-by-read**: any `.get()` evaluated inside a
derived or effect body subscribes that derived/effect to the signal. A `.set(v)`
re-runs every subscriber.

```jet
use core.reactive as reactive

fn run() {
    price :: reactive.signal(100)
    qty :: reactive.signal(2)
    total :: reactive.derived(() => (price.get() * qty.get()))
    print(total.get())                       // 200

    reactive.effect(() => print(total.get()))  // prints 200 now
    price.set(150)                             // effect re-runs → 300
    qty.set(3)                                 // effect re-runs → 450
    print(total.get())                         // 450
}
```

| Call | Returns | Does |
|------|---------|------|
| `reactive.signal(initial)` | `Signal<T>` | a mutable reactive source holding `T` |
| `reactive.derived(() => expr)` | `Derived<T>` / `Computed<T>` | a value recomputed from the signals it reads |
| `reactive.computed(() => expr)` | `Computed<T>` | canonical alias for `derived` (D-SIGNAL1) |
| `reactive.effect(() => { … })` | — | a side effect re-run when a read signal changes |
| `@Reactive { … }` | — | explicit reactive effect scope (lowers like `reactive.effect`) |
| `sig.get()` / `der.get()` | `T` | read the current value (and subscribe, inside a derived/effect) |
| `sig.set(v)` | — | write a new value and re-run subscribers |

`Signal`/`Derived` are cheap shared handles — copying one (e.g. capturing it in a
lambda) shares the same reactive cell, so a derived/effect reads the live signal
while outer code keeps `.set`ting it. The runtime uses only Rust std (no external crate);
the compiler-side dataflow graph for tooling/IDEs is a separate, future tooling
feature.

---

### `core.event` — typed events and hooks (D-EVENT1)

`use core.event as event`. Events are first-party typed Core values. The compiler
knows the family for checking and lowering, but this slice adds no event syntax.

```jet
use core.event as event

fn run() {
    scope :: event.scope()
    clicked :: event.new<Int>()

    sub :: clicked.on(scope, (n) => { print("clicked {n}") })
    clicked.once(scope, (n) => { print("once {n}") })

    print(clicked.emit(1).summary())
    sub.unsubscribe()
    scope.cancel()
}
```

| Call | Returns | Does |
|------|---------|------|
| `event.new<T>()` | `Event<T>` | create a typed many-subscriber event |
| `event.async_result<T, E>(policy, failures)` | `AsyncEvent<T, E> ? String` | create one scheduler-backed bounded event queue |
| `event.hook<T, R>(fallback)` | `Hook<T, R>` | create an ordered hook; last active handler result wins |
| `event.decision_hook<T, E>(policy)` | `DecisionHook<T, E>` | create an ordered transform/continue/cancel/fail fold |
| `event.scope()` | `EventScope` | create an owner for subscriptions |
| `ev.on(scope, handler)` / `ev.once(scope, handler)` | `Subscription` | subscribe a handler owned by `scope`; `once` auto-unsubscribes |
| `ev.on_priority(scope, priority, handler)` | `Subscription` | subscribe with higher priority before source order |
| `ev.emit(payload)` | `EventTrace` | synchronously dispatch and return delivered counts |
| `async_ev.emit_async(payload)` | `Task<DispatchReport<E>>` | enqueue and return the single terminal report |
| `async_ev.queued_count()` / `.running_count()` / `.blocked_count()` | `Int` | inspect truthful scheduler states |
| `async_ev.close()` | — | reject pending/new producers and drain accepted work |
| `hook.run(payload, fallback)` | `R` | run active hook handlers or return fallback |
| `decision.run(payload)` | `HookOutcome<T, E>` | return final transformed value, cancellation, or failure |
| `sub.unsubscribe()` / `sub.is_active()` | — / `Bool` | manage an explicit subscription |
| `scope.cancel()` / `scope.active_count()` | — / `Int` | cancel all owned subscriptions and count active ones |
| `trace.summary()` | `String` | compact delivery trace for logs/tests |

`Event<T>` is for "something happened" streams. `Hook<T, R>` is for ordered
intervention points before/during/after an operation. Default dispatch is sync,
priority-descending, then registration order. `EventScope` is the beginner-safe
lifetime owner; explicit `Subscription` handles give experts manual control.
`EventScope.cancel()` is terminal and idempotent: it removes all owned listeners,
and later registration through that scope returns an inactive subscription.
During synchronous dispatch, removals before a listener's turn take effect,
additions wait for a later or nested dispatch, reentrant emissions run
depth-first, and `once` deactivates before calling its handler.

`AsyncPolicy` requires a positive capacity and chooses `Block`, `DropNewest`,
or `DropOldest`; `FailurePolicy` is `StopFirst`, `Collect`, `Log`, or `Ignore`.
Cancellation, inherited deadlines, close, and owner teardown share one terminal
transition. With `JET_OBSERVE=1`, the debugger and Canvas `?pid=` live view read
the same bounded payload-free executed lifecycle sequence; without a live PID,
Canvas reports no runtime Event facts.

---

### `core.web` — browser events and storage

`core.web` is the web-target browser API beside `core.ui` rendering:

```jet
use core.web as web

@Target(Js)
fn init() {
    saved :: web.storage.local.get("tasks") ?? "[]"
    web.storage.local.set("tasks", saved)
    web.on("#new-task", "input", (ev) => {
        web.storage.local.set("draft", web.value("#new-task"))
    })
}
```

`web.on(selector, event, handler)` binds a DOM event listener. `web.value(selector)`
reads an input value or text content. `web.storage.local` and
`web.storage.session` provide `get`, `set`, `remove`, and `clear`; `get` returns
`String?` so ordinary `??` handles missing keys.

---

### `core.mem` — arenas and regions

Expert-tier explicit allocators, unlocked by `use core.mem` (no `@Unsafe`
needed — arenas are the *safe* fast-allocation primitive). An arena bump-allocates
many values into one buffer and frees them all at once.

```jet
use core.mem

fn run() {
    arena :: mem.Arena.new()             // or .new(capacity: 4096)
    x :: arena.alloc(42)                 // x is a *view* into the arena
    y :: arena.alloc("hi")
    print(x)
    print(y)
    arena.reset()                        // frees everything; buffer reused
    z :: arena.alloc(7)
    print(z)
}
```

Raw pointer and MMIO helpers also live in `core.mem`. `mem.address_of(x)` returns
an inert address as `Int`; `mem.Ptr<T>.from_addr(addr)`, `mem.volatile_read(p)`,
and `mem.volatile_write(p, value)` require an audited `@Unsafe("reason")` region.

`arena.alloc(value)` hands back a **view** into the arena's storage, not an owned
copy. A view is fast and zero-copy, but it lives only inside its **region** — the
scope of the `arena` binding — and only until the arena is reset or closed. The
checker enforces both:

- returning, storing, or giving away a view → **E0631** (it would outlive the arena);
- using a view after `reset()` or `close(^allocator)` → **E0632**.

Both are compile errors, so a dangling arena pointer can never run. Copy what you
need out (`~x`) before it leaves the region.

For the cases scope-inference is too coarse — a region spanning two allocators, or
narrower than the function — write an explicit **`region r { … }`** block:

```jet
use core.mem

fn run() {
    @Region(scratch) {
        a :: mem.Arena.new()
        b :: mem.Bump.new()
        first :: a.alloc(1)
        second :: b.alloc(2)
        print(first)
        print(second)
    }                                    // both arenas freed here
}
```

| Type / verb | What it does |
|-------------|--------------|
| `mem.Arena.new()` / `.new(capacity: N)` | A general grow-only arena |
| `mem.Bump.new(capacity: N)` | One contiguous monotonic buffer; exhaustion is deterministic |
| `mem.Pool.new(slots: N)` | Fixed slot count with retained size/alignment-class reuse |
| `mem.Fixed.new(size: N)` | Compiler-synthesized inline `[Byte#N]` backing; `N` is positive comptime |
| `mem.Fixed.over(&bytes)` | Exclusively borrow one mutable `[Byte#N]` buffer for the handle's scope |
| `arena.alloc(value)` | Store `value`, return a scope-bound view |
| `arena.reset()` | Drop everything, keep the buffer (reusable) |
| `close(^arena)` | Terminally release the allocator resource |
| `region r { … }` | An explicit region — views inside may not escape it |

`Fixed` never grows or falls back to the heap. Values and their alignment
padding grow from the start of its buffer; reverse-drop records reserve space
from the end. An allocation fails before either cursor moves if those regions
would collide. `reset()` is rejected while allocation views are live, then
drops values in reverse order and reuses the same bytes. Fixed constructors
must directly initialize a lexical binding; handles and views cannot be
returned, stored, captured, or sent across task/join boundaries.

---

## Text parsing

Turn text into values with destination-owned `Type.parse` and split it into lines.
Parsing is fallible, so handle its result with `?`/`??`.

```jet
fn run() {
    n :: Int.parse("42") ?? -1                 // 42
    bad :: Int.parse("oops") ?? -1             // -1 (parse failed → fallback)
    print(n + bad)

    loop line; "first\nsecond".lines() {   // ["first", "second"]
        print(line)
    }
}
```

| API | Returns | What it does |
|-----|---------|--------------|
| `Int.parse(text)` | `Int ? ParseError` | Parse text as an integer (leading/trailing space ignored) |
| `Float.parse(text)` | `Float ? ParseError` | Parse text as a float |
| `String.lines()` | `[String]` | Split into lines (`\n` and `\r\n`; no trailing empty line) |

`.lines()` and `Int.parse(s)` / `Float.parse(s)` are fully
evaluated at comptime — `Ok(v)` / `Err(e)` construct `Result` values, and
`?` / `??` propagate or unwrap them in pure comptime expressions
(`examples/features/comptime/comptime_parse.jet`).

### `Cursor` — consuming text scanner (D-SHIFT1)

`Cursor.over(s)` wraps a string with a position; each read consumes a prefix
and advances. `take_pattern` reuses the `if x == "…{hole:Type}…"` pattern
grammar (D-PARSESTR1) in consume mode: it matches a *prefix* of the remaining
text and returns the typed holes. A miss is an ordinary error value.

```jet
fn run() {
    c :: Cursor.over("  inc-4411 sev 3: disk full\n")
    c.skip_ws()
    m :: c.take_pattern("inc-{id:Int} sev {sev:Int}: ") ?? panic("bad line")
    reason :: c.take_until("\n") ?? panic("no newline")
    print("{m.id} {m.sev} {reason}")        // 4411 3 disk full
}
```

| API | Returns | What it does |
|-----|---------|--------------|
| `Cursor.over(s)` | `Cursor` | Wrap a `String` in a consuming scanner |
| `c.take_pattern("…{h:T}…")` | `(holes…) ? String` | Match + consume a prefix; literal pattern only |
| `c.take_until(delim)` | `String ? String` | Text up to (not including) `delim`; error if absent |
| `c.skip_ws()` | — | Skip leading whitespace |

`examples/features/parsing/text-cursor.jet` is the golden example.

---

## Binary data (`U8`)

The `U8` type holds one byte (0–255). Literals outside that range are a compile
error (**E1003**).

```jet
fn run() {
    b: U8 :: 255
    print(Int.from_u8(b))                   // 255 as Int
    n :: U8.from_int(42) ?? return          // checked conversion
    bytes :: "hi".bytes()                  // [U8]
    text :: String.from_bytes(bytes) ?? return
    print(text)
}
```

| API | Returns | What it does |
|-----|---------|--------------|
| `String.bytes()` | `[U8]` | UTF-8 bytes of a string |
| `String.from_bytes(bs)` | `String ? UTF8Error` | Decode UTF-8 bytes |
| `U8.from_int(n)` | `U8 ? String` | Checked Int → U8 |
| `Int.from_u8(b)` | `Int` | U8 → Int |

Use `fs.read_bytes` / `fs.write` when you need raw file bytes.

### `Reader` — consuming byte scanner (D-SHIFT1)

`Reader.over(bytes)` wraps a `[U8]` buffer with a position; every read
advances and is fallible — a bounds miss is an ordinary error value, never a
panic or silent truncation. This is the "shift" kernel of linear wire-format
parsing, without a dedicated operator.

```jet
fn run() {
    packet: [U8] :: [0x2a, 0x00, 0x00, 0x00, 0x03, 0x00]
    r :: Reader.over(packet)
    magic :: r.read_u32_le() ?? panic("short")   // 42
    count :: r.read_u16_le() ?? panic("short")   // 3
    print("{magic} {count} {r.remaining()} {r.is_at_end()}")
}
```

`take_pattern` reuses the `b"…{hole:U<width>}…"` binary-pattern grammar
(D-BINPAT1) in consume mode — the byte-mode sibling of `Cursor.take_pattern`
above: it matches a *prefix* of the remaining bytes and returns the typed
holes, advancing the reader past them so more reads can follow. A miss is an
ordinary error value.

```jet
fn run() {
    header: [U8] :: [0x45, 0x00, 0x00, 0x28]
    r :: Reader.over(header)
    h :: r.take_pattern(b"{version:U4}{ihl:U4}{tos:U8}{len:U16be}") ?? panic("bad header")
    print("{h.version} {h.ihl} {h.tos} {h.len}")   // 4 5 0 40
}
```

| API | Returns | What it does |
|-----|---------|--------------|
| `Reader.over(bs)` | `Reader` | Wrap a `[U8]` in a consuming scanner |
| `r.read_u8()` | `U8 ? String` | One byte |
| `r.read_u16_le()` / `_be()` | `U16 ? String` | Two bytes, little/big-endian |
| `r.read_u32_le()` / `_be()` | `U32 ? String` | Four bytes |
| `r.read_u64_le()` / `_be()` | `U64 ? String` | Eight bytes |
| `r.take(n)` | `[U8] ? String` | Next `n` bytes (`n: Int`; sized ints widen with `Int.from_u*(n)`) |
| `r.take_pattern(b"…{h:U<w>}…")` | `(holes…) ? String` | Match + consume a prefix; literal pattern only |
| `r.remaining()` | `Int` | Bytes left |
| `r.is_at_end()` | `Bool` | Position at buffer end |

`examples/features/parsing/binary-reader.jet` is the golden example.

---

## Numeric surface (D-NUMOPS1)

`Int` and `Float` are the beginner defaults (64-bit: `Int` = `I64`, `Float` =
`F64`). The explicit-width menu — `I8 I16 I32 I64 U8 U16 U32 U64 F32 F64` — is
available for expert and FFI/binary work; `I64`/`F64` interchange with
`Int`/`Float` freely, every other width is its own distinct type. A bare
integer literal adopts the width of the slot it lands in (a binding/parameter/
return annotation, or sized arithmetic) and is range-checked at compile time —
a literal that doesn't fit is **E1003**. Widths never mix implicitly:
arithmetic, comparison, and assignment require the same width on both sides
(**E0109**/**E0112**/**E0108**), with no silent narrowing or widening. The
sized types erase to their Rust equivalents (`u8`…`i64`, `f32`) at codegen, so
they cross the C ABI by value (S59). Width conversions are always named
methods (below), never implicit.

Plain integer arithmetic (`+` `-` `*` `/`) **traps on overflow** at every width —
a result outside the type's range stops the program with a Jet panic instead of
silently wrapping. Opt a single op out at the use site:

```jet
fn run() {
hi: U8 :: 200
lo: U8 :: 100
    print(wrapping(hi + lo))            // 44   — wraps around (C behaviour)
    print(saturating(hi + lo))          // 255  — clamps to the type's range
    print(checked(hi + lo) ?? 0)        // 0    — checked(…) -> T?, None on overflow
}
```

| Form | Returns | What it does |
|------|---------|--------------|
| `expr` (`a + b`, …) | `T` | Traps on overflow (safe default) |
| `wrapping(a + b)` | `T` | Wraps around the type's range |
| `saturating(a + b)` | `T` | Clamps to `MIN`/`MAX` |
| `checked(a + b)` | `T?` | `None` on overflow |

Each wrapper takes exactly one integer `+`/`-`/`*`/`/`; anything else is **E1005**.

**Bounds and float constants** — per-type `MIN`/`MAX`, plus float specials:

| Member | On | Value |
|--------|----|-------|
| `U8.MAX` / `I32.MIN` / … | any integer type | the type's range ends |
| `Float.INFINITY` / `.NEG_INFINITY` | floats | ±∞ |
| `Float.NAN` | floats | not-a-number |
| `Float.EPSILON` | floats | smallest representable step |

**Predicates and bit queries:**

| Method | On | Returns |
|--------|----|---------|
| `x.is_nan()` / `.is_infinite()` / `.is_finite()` | floats | `Bool` |
| `n.count_ones()` / `.count_zeros()` | integers | `Int` |
| `n.leading_zeros()` / `.trailing_zeros()` | integers | `Int` |

**Bit operators** — `&` `|` `^` keep the operand width (both sides the same
type); `<<` `>>` take any integer shift-count and keep the left side's type. A
shift count past the type's width traps (no leaked Rust panic).

**Width conversions** are destination-owned named methods — no implicit narrowing or widening:

| Method | Returns | Direction |
|--------|---------|-----------|
| `Int.from_u8(n)` / `U32.from_u8(n)` / … (widening) | `T` | infallible |
| `U8.from_int(n)` / `I16.from_int(n)` / … (narrowing) | `T ? String` | fallible (`?`/`??`) |
| `F32.from_float(n)` | `F32 ? String` | fallible (finite F32 range) |
| `Float.from_i32(n)` | `Float` | infallible |

---

## Common mistakes (and what Jet suggests)

| You wrote | Jet wants |
|-----------|-----------|
| `println(...)` | `print(...)` |
| `eprintln(...)` | `io.eprint(...)` |
| `open("file")` / `File.open` | `fs.read(...)` / `fs.write(...)` |
| `getenv("X")` / `os.environ` | `env.get("X")` |
| `import core.files` | `use core.files` |
| `x :: …` / `x := …` | Immutable / mutable binding |

---

## `core.net` — sockets and DNS

`core.net` is the low-level socket layer. Calls look blocking at the Jet
surface. On Unix, TCP, UDP, and Unix-socket operations park through the shared
scheduler readiness backend and observe task cancellation and available
`@Context` deadlines. Windows IOCP lifecycle and platform proof remains #527.
Beginner calls accept strings; expert calls accept typed
`IpAddr` / `SocketAddr` values.

| Function | Returns | Notes |
|----------|---------|-------|
| `ip_addr(text)` | `IpAddr ? NetError` | Parse IPv4/IPv6 |
| `ip_to_string(ip)` / `ip_is_ipv4(ip)` | `String` / `Bool` | Inspect typed IP values |
| `socket_addr(host, port)` | `SocketAddr ? NetError` | Resolve/parse host and port |
| `socket_addr_parse(text)` | `SocketAddr ? NetError` | Parse `host:port` |
| `socket_host(addr)` / `socket_port(addr)` / `socket_to_string(addr)` | `String` / `Int` / `String` | Inspect typed socket addresses |
| `tcp_listen(addr)` / `tcp_connect(addr)` | `TcpListener ? NetError` / `TcpStream ? NetError` | String entrypoints |
| `tcp_listen_addr(addr)` / `tcp_connect_addr(addr)` | `TcpListener ? NetError` / `TcpStream ? NetError` | Typed entrypoints |
| `tcp_connect_timeout(addr, ms)` | `TcpStream ? NetError` | Typed dial with timeout |
| `tcp_connect_happy(host, port, ms)` | `TcpStream ? NetError` | Dual-stack dial with staggered IPv6/IPv4 racing under one cancellation/deadline budget |
| `listener.accept(deadline: Duration)?` | `TcpStream ? NetError` | Accept with an optional per-call deadline |
| `stream.read(limit, deadline: Duration)?` / `stream.write(bytes, deadline: Duration)?` / `stream.write_all(bytes, deadline: Duration)?` | `[U8] ? NetError` / `Int ? NetError` / `() ? NetError` | Canonical byte operations with optional per-call deadlines |
| `stream.read_text(limit, deadline: Duration)?` / `stream.write_text(text, deadline: Duration)?` | `String ? NetError` / `() ? NetError` | Checked UTF-8 projections with optional per-call deadlines |
| `stream.shutdown(.Read/.Write/.Both)` / `stream.close()` | `() ? NetError` | Explicit half-close; close is idempotent and later I/O is `.Closed` |
| `stream.ready(.Read/.Write/.ReadWrite, deadline: Duration)` | `NetReady ? NetError` | Same-handle readiness; earliest ambient or explicit deadline wins |
| `tcp_local_socket_addr(stream)` / `tcp_peer_socket_addr(stream)` | `SocketAddr ? NetError` | Typed stream addresses |
| `listener_local_socket_addr(listener)` | `SocketAddr ? NetError` | Typed listener address |
| `set_timeout(stream, ms)` | `() ? NetError` | Set read/write timeouts |
| `set_read_timeout(stream, ms)` / `set_write_timeout(stream, ms)` | `() ? NetError` | Directional timeouts |
| `udp_bind(addr)` / `udp_bind_addr(addr)` | `UdpSocket ? NetError` | Datagram sockets |
| `udp_local_addr(socket)` | `SocketAddr ? NetError` | Typed local address |
| `udp_set_timeout(socket, ms)` | `() ? NetError` | Persistent read/write deadline budget; earliest ambient deadline wins |
| `socket.ready(.Read/.Write/.ReadWrite, deadline: Duration)` / `socket.close()` | `NetReady ? NetError` / `() ? NetError` | Same UDP handle readiness and idempotent lifecycle |
| `udp_send_bytes_to(socket, bytes, addr)` | `Int ? NetError` | Send one arbitrary-byte datagram |
| `udp_receive(socket, limit)` | `UdpPacket ? NetError` | Full datagram receive with bounded returned payload |
| `socket.send_to(bytes, addr, deadline: Duration)` / `socket.receive(limit, deadline: Duration)` | `Int ? NetError` / `UdpPacket ? NetError` | Datagram-preserving per-call deadline overrides |
| `udp_packet_bytes/address/original_len/truncated(packet)` | `[U8]` / `SocketAddr` / `Int` / `Bool` | Packet data, source, wire length, and truncation fact |
| `unix_listen(path)` / `unix_connect(path)` | `UnixListener ? NetError` / `UnixStream ? NetError` | Unix-domain sockets where supported |
| `unix_accept(listener)` | `UnixStream ? NetError` | Accept one Unix stream; scheduler-aware cancellation and deadlines |
| `listener.accept(deadline: Duration)` | `UnixStream ? NetError` | Same-listener per-call deadline override |
| `unix_read_bytes(stream, limit)` / `unix_write_all_bytes(stream, bytes)` | `[U8] ? NetError` / `() ? NetError` | Unix byte stream operations; same deadline/close law as TCP |
| `unix_shutdown(stream, how)` / `unix_close(stream)` | `() ? NetError` | Explicit shutdown and idempotent close |
| `stream.set_timeout(Duration)` / `stream.read(limit, deadline: Duration)` / `stream.write_all(bytes, deadline: Duration)` / `stream.ready(interest, deadline: Duration)` / `stream.close()` | matching stream results | Same-handle Unix persistent/per-call deadlines, readiness, and lifecycle |
| `dns_a(name, ms)` / `dns_aaaa(name, ms)` | `[IpAddr] ? NetError` | System resolver config, timeout in ms |
| `dns_txt(name, ms)` | `[String] ? NetError` | TXT records |
| `dns_srv(name, ms)` | `[DnsSrv] ? NetError` | SRV records |
| `dns_*_at(server, name, ms)` | same as matching lookup | Expert override for a specific DNS server |
| `dns_srv_target(srv)` / `dns_srv_port(srv)` | `String` / `Int` | Inspect SRV records |
`NetError` has stable variants for input, permission, address, connection,
closed, timeout, cancellation, unsupported, DNS, TLS, protocol, and other OS
failures. `error_operation/address/name/message/os_code` expose portable control
and audit data. Raw OS text is never control-flow law.

Ordinary A/AAAA lookups use the platform resolver and preserve host files,
search policy, VPNs, and enterprise DNS. TXT/SRV use configured host name
servers; explicit `_at` calls query only their named server. The wire resolver
uses unpredictable IDs, validates sender/header/question/bounds/compression,
follows bounded CNAME chains across answer/additional records, and retries a
truncated UDP answer over bounded TCP. It never falls back to a public resolver.

---

## `core.tls`

`core.tls` upgrades a connected `core.net` TCP stream to a client TLS stream.
The built module exposes only this byte/text stream surface:

| Function | Returns | Notes |
|----------|---------|-------|
| `ClientConfig.default().with_alpn(protocols)` | `TlsClientConfig ? IOError` | Validate and offer ALPN protocols before any stream is consumed, without disabling verification |
| `RootCertificates.from_pem(bytes)` | `RootCertificates ? IOError` | Validate a custom PEM root bundle before any network use |
| `ClientIdentity.from_pem(cert_chain: bytes, private_key: bytes)` | `ClientIdentity ? IOError` | Validate one PEM identity chain and matching PKCS#8, PKCS#1, or SEC1 private key; key bytes are secret and wiped on drop |
| `config.with_trust(policy)` | `TlsClientConfig ? IOError` | Select `.System`, `.SystemPlus(roots)`, or `.CustomOnly(roots)` on a new immutable config |
| `config.with_client_identity(identity)` | `TlsClientConfig ? IOError` | Add a validated mTLS client identity on a new immutable config |
| `config.with_version_bounds(min: version, max: version)` | `TlsClientConfig ? IOError` | Select inclusive `.Tls12` / `.Tls13` bounds; reversed bounds fail before network use |
| `client(stream, server_name)` | `TlsStream ? NetError` | Consume the `TcpStream`; verify the server name with system roots; preserve its deadline budgets |
| `client(stream, server_name: name, config: config, deadline: duration)` | `TlsStream ? NetError` | Use the explicit client configuration and earliest handshake deadline on the same consumed stream |
| `read(stream, limit)` / `read_text(stream)` | `[U8] ? IOError` / `String ? IOError` | Scheduler-aware byte or checked-text read; empty bytes mean clean EOF |
| `write(stream, bytes)` / `write_all(stream, bytes)` | `Int ? IOError` / `() ? IOError` | Scheduler-aware partial or complete byte write |
| `write_text(stream, text)` | `() ? IOError` | Write the complete text payload |
| `close(stream)` | `() ? IOError` | Send close-notify; repeated close is harmless |
| `stream.read(limit, deadline: Duration)` / `stream.write_all(bytes, deadline: Duration)` / `stream.ready(interest, deadline: Duration)` / `stream.close()` | matching stream results | Same TLS handle, explicit per-call deadlines, readiness, and close-notify lifecycle |
| `stream.peer_identity()` | `TlsPeerIdentity` | Retained verified name plus immutable exact-DER wire-order chain; leaf exposes DER, certificate/SPKI SHA-256, DNS SANs, validity milliseconds, subject, and issuer |
| `stream.close_write(deadline: Duration)` | `() ? IOError` | Flush close-notify and close only writes; repeated calls are harmless, reads continue, and later writes return `.Closed` |

TLS handshake, read, write, and close-notify use the consumed socket's shared
readiness path. Handshake failures use `core.net.NetError`; stream byte,
readiness, and lifecycle failures use the shared `IOError` tree, including
`.Cancelled`, `.TimedOut`, `.Closed`, and `.Protocol`.
An empty TLS read is a verified close-notify. Raw transport EOF without
close-notify is `.Protocol(IOContext.{ operation: .Read, cause: ... })`
truncation.

---

## core.db

`core.db` opens SQLite connections through `db.open(path)` or
`db.open_memory()`. Queries use one path: SQL text plus `[DbValue]` parameters.
Checked `Sql` literals feed that path through `db.params(sql)`, so holes become
bound parameters, not string interpolation. The runtime uses SQLite's prepared
statement cache under that same path; there is no separate unsafe raw-query or
prepare-only API.

| API | Returns | Notes |
|-----|---------|-------|
| `conn.execute(sql, params)` | `Int ? DbError` | Affected row count |
| `conn.query(sql, params)` | `[Row] ? DbError` | `Row` is `Map<String, DbValue>` |
| `conn.query_one(sql, params)` | `Row? ? DbError` | First row, if any |
| `conn.begin()` / `commit()` / `rollback()` / `close()` | `Bool` | Explicit transaction control |
| `db.row_int(row, key)` / `row_float` / `row_text` / `row_bool` | `T ? String` | Typed column read with missing/type errors |
| `db.transaction(conn, label, statements)` | `Int ? DbError` | Runs statements in one transaction, rollback on first error |
| `db.migrate(conn, name, statements)` | `Int ? DbError` | Records migration checksum in `__jet_migrations`; rerun returns `0`, changed checksum errors |

`DbValue` variants are `Null`, `Int`, `Float`, `Text`, and `Bool`.

---

## Compression and archives

D-CORE-COMPRESS1=A assigns each operation one public home:

| Module | Job | API |
|--------|-----|-----|
| `core.compress.gzip` | gzip byte streams | `compress([U8]) -> [U8]`, `decompress([U8]) -> [U8] ? String` |
| `core.compress.zstd` | zstd byte streams | `compress([U8]) -> [U8]`, `decompress([U8]) -> [U8] ? String` |
| `core.archive` | zip/tar containers | `zip_compress`, `zip_decompress`, `tar_add`, `tar_get`, `tar_names_json` |

`core.archive` has no standalone gzip helpers. Compose formats explicitly for
containers such as `tar.gz`: build tar bytes with `core.archive`, then compress
those bytes with `core.compress.gzip`.

---

## Built Core Modules

D-STDLIBLEDGER1 keeps this reference to built modules only. It is not a
have/have-not ledger of missing domains.

D-OPTGC1 selects automatic scoped `@Policy(gc)` as the sole source path. The
collector is compiler-private: user code keeps ordinary bare values and opts in
at package, module, function, or block scope. `jet gc report` identifies the
exact automatic promotion sites to migrate back to ownership.

`core.io`, `core.env`, `core.os`, `core.process`, `core.math`, `core.random`,
`core.time`, `core.tasks`, `core.testing`, `core.mem`, `core.mem.alloc`,
`core.solve`, `core.data`, `core.files`, `core.path`, `core.url`, `core.mime`,
`core.watcher`, `core.net`, `core.scope`, `core.args`, `core.term`,
`core.reflect`, `core.encoding`, `core.encoding.json`, `core.encoding.jsonl`,
`core.encoding.csv`, `core.encoding.toml`, `core.encoding.yaml`,
`core.encoding.xml`, `core.encoding.cbor`, `core.encoding.hex`,
`core.encoding.base64`, `core.encoding.base32`, `core.text.unicode`,
`core.binary`, `core.text`, `core.fmt`, `core.uuid`, `core.log`,
`core.crypto`, `core.crypto.random`, `core.crypto.expert`, `core.http`,
`core.regex`, `core.archive`, `core.raylib`, `core.game`,
`core.compress.gzip`, `core.compress.zstd`, `core.db`, `core.plugin`,
`core.reactive`, `core.event`, `core.science.measurement`,
`core.reactive.loadable`, `core.perf`, `core.ui`, `core.web`,
`core.web.storage`, `core.web.storage.local`, `core.web.storage.session`,
`core.sketch.hll`, `core.sketch.tdigest`, `core.sketch.reservoir`,
`core.sketch.cms`, `core.time.date`, `core.time.datetime`,
`core.time.expiring`, `core.http.client`,
`core.http.server`, `core.web.devserver`, `core.vault`.

---

## Writing Core in Jet (future)

Today, Core lives in the compiler as typed signatures plus Rust prelude templates
(`Source/Prelude/Std.rs`). The **API** is Jet; the **implementation** is Rust until
the package system fully stabilizes.

---

## Examples in this repo

| Example | Shows |
|---------|-------|
| `examples/features/io/files.jet` | Read, transform, write with errors |
| `examples/features/serde/json.jet` | Parse, inspect, mutate, re-render JSON |
| `examples/features/io/cli.jet` | Args, environment, exit codes |
| `examples/features/io/cli_args.jet` | `core.args` — flag/option/positional spec + parse |
| `examples/features/io/db_checked_sql.jet` | `core.db` — checked SQL params, typed row reads, transactions, migrations |
| `examples/features/io/dir_entry.jet` | `fs.list_dir` → `[DirEntry]` |
| `examples/features/serde/serde_derive.jet` | `@[Codable]` encode + typed `decode<T>` with `@[Rename]` |
| `examples/features/serde/csv_typed.jet` | `csv.decode<Row>` → struct → JSON (the typed CSV pipeline) |
| `examples/features/serde/json_typed.jet` | Nested struct + list + optional round-trip with `@[RenameAll(camel)]` |
| `examples/features/serde/decode_traced.jet` | `decode_traced<T>` → `DecodeResult<T>`/`MigrationStatus`, incl. a real v1→v2 migration at decode time |
| `examples/features/reflection/reflect-value.jet` | `reflect.of(x)` — `.type_name()`/`.display()`/`.fields()` |
| `examples/features/syntax/maturity_tags.jet` | `@Meta(maturity: .Experimental / .Tested / .Hardened)` doc-only API metadata (D-MARK-META1=B) |

Run the full battery: `nix develop -c cargo test --test golden` and `nix develop -c cargo test --test corelib`.

See also: [Maturity tags](maturity-tags.md).
