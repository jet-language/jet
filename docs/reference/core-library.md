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

### `core.lang` — language declarations

`core.lang` publishes the compiler vocabulary used by typed marker arguments.
These are ordinary generated enums. The marker registry is their source, so
diagnostics, `jet explain`, hover, completion, documentation, and reflection
show the same declaration.

The generated enum types are `ABI`, `Capability`, `FfiLanguage`, `InlineMode`,
`IntType`, `Layout`, `Maturity`, `NamingCase`, `ObligationMode`,
`PolicySetting`, `State`, `TaintKind`, `Target`, and `Track`.

```jet
use core.lang as lang

#Inline(lang.InlineMode.Always)
fn parse_fast(text: String) => Int = text.parse() ?? 0
```

An expected marker argument also accepts a dot literal without an import:
`#Inline(.Always)`.

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
distinct-type arithmetic gating like `#Numeric`).

| Method | Type | What it does |
| --- | --- | --- |
| `.map(f)` | `(T?, fn(T) => R) => R?` | Applies `f` to the payload if present; `None` stays `None` |
| `.zip(other)` | `(T?, U?) => (a: T, b: U)?` | Pairs two optionals: present only when **both** are present |
| `Option.lift2(f, a, b)` | `(fn(T, U) => R, T?, U?) => R?` | Applies a two-argument function to `a`/`b` only when both are present |

```jet
price: Float? :: lookup_price(id)
qty: Float? :: lookup_qty(id)

// zip: both present produces a pair; either None produces None
total1 :: price.zip(qty).map((pair) => pair.a * pair.b)

// lift2: same idea, no explicit pair
total2 :: Option.lift2((p, q) => p * q, price, qty)

// total1, total2: Float? — None unless both price and qty were present
```

See `examples/features/types/option_combinators.jet`.

---

## Collections and iterators (D-ITERTOOLS1=A)

Core collection spellings stay explicit: `[T]` for lists, `[K: V]` for the
default ordered map, and named types for specialized behavior. Adapter methods
return a lazy `Iter<T>` view; call `to_list()`, `collect()`, or a reducer
(`sum`, `fold`, …) to materialize. String `.split` returns `Iter<String>` on
the same model.

Under D-COMPREHENSION1, a finite `loop ... -> value` executes immediately and
returns `[T]`. Build maps with ordinary map operations, build sets with
`Set.from(...)`, and use the existing iterator adapters when work must stay
lazy. An expected type never changes the collector or evaluation time.

| Type | Constructors | Main methods |
| --- | --- | --- |
| `[T]` | list literal `[a, b]` | `map`, `filter`, `each`, `find`, `any`, `all`, `sort_by`, `reduce`, `take`, `skip`, `step_by`, `dedup`, `dedup_by`, `chunks`, `windows`, `chunk_while`, `indexed`, `indexes`, `zip`, `zip_short`, `zip_pad`, `unzip`, `take_while`, `skip_while`, `flat_map`, `filter_map`, `scan`, `fold`, `sum`, `product`, `min`, `max`, `min_by`, `max_by`, `min_max`, `min_max_by`, `group_by`, `count_by`, `count`, `extend`, `concat`, `partition`, `flatten`, `intersperse`, `repeat`, `cycle`, `drop_last`, `shuffle`, `is_sorted`, `is_sorted_by`, `last_index_of`, `average`, `compare`, `split`, `to_set`, `join`, `to_list`/`collect`, `lazy`, `starts_with`, `ends_with`, `slice`, `copy`, `equal`, `binary_search`, `binary_search_by`, `union`, `intersection`, `difference`, `random`, `replace` |
| `[K: V]` | map literal `["a": 1]`, `Map.new()`, `Map.from_keys(keys, default)` | `keys`/`values` (lazy `Iter` views), `has_key`, `get`, `add`/`replace`, `add_new`, `remove`/`pop`, `pop_first`, `contains_value`, `merge`, `copy`, `equal`, `first`, `to_list`, `any`, `all`, `map`, `filter`, `flat_map`, `fold`, `min`, `max`, `intersection`, `slice`, `len`, `is_empty`, `clear` |
| `Set<T>` | `Set.new()`, `Set.from(xs)` | `add`, `remove`, `has`, `union`, `intersection`, `difference`, `symmetric_difference`, `is_subset`, `is_superset`, `is_disjoint`, `copy`, `to_set`, `equal`, `capacity`, `first`, `values`, `all`, `filter`, `each`, `max`, `min`, `fold`, `map`, `flat_map`, `replace`, `take`, `to_list`, `len`, `is_empty`, `clear` |
| `SortedSet<T>` | `SortedSet.new()`, `SortedSet.from(xs)` | `add`, `remove`, `has`, `first`, `last`, `union`, `intersection`, `difference`, `symmetric_difference`, `is_subset`, `is_superset`, `is_disjoint`, `to_list`, `len`, `is_empty`, `clear` |
| `Deque<T>` | `Deque.new()`, `Deque.init(xs)` | `push_front`, `push_back`, `pop_front`, `pop_back`, `peek_front`, `peek_back`, `capacity`, `contains`, `get`, `delete`, `to_list`, `join`, `reverse`, `split`, `len`, `is_empty`, `clear` |
| `PriorityQueue<T>` | `PriorityQueue.new()`, `PriorityQueue.from(xs)` | `push`, `pop`, `peek`, `to_sorted_list`, `len`, `is_empty`, `clear` |
| `Cache<K,V>` | `Cache.new(capacity)` | `add`, `add_new`, `get`, `remove`, `has_key`, `keys`, `capacity`, `len`, `is_empty`, `clear` |
| `Bag<T>` | `Bag.new()`, `Bag.from(xs)` | `add`, `remove`, `has`, `count`, `to_list`, `len`, `is_empty`, `clear` |
| `BitSet` | `BitSet.new()` | `add`, `remove`, `has`, `count`, `to_list`, `len`, `clear` |
| `ByteBuffer` | `ByteBuffer.new()`, `ByteBuffer.with_capacity(n)`, `ByteBuffer.from(bytes)` | write: `write_u8`/`write_byte`, `write_u16_le`/`be`, `write_u32_le`/`be`, `write_u64_le`/`be`, `write_bytes`/`write`, `write_to`; cursor: `position`, `eof`, `seek`, `rewind`, `read`, `read_byte`/`next`, `read_bytes`, `read_string`, `get`, `first`; string-like: `contains`, `starts_with`, `ends_with`, `trim`/`trim_start`/`trim_end`, `to_lower`/`to_upper`/`to_title`/`title`, `replace`, `split`, `join`, `lines`, `index_of`/`last_index_of`, `is_ascii`, `to_string`/`string`, `parse`; lifecycle: `flush`, `close`, `shutdown`, `copy`/`clone`, `copy_to`, `equal`, `compare`, `capacity`, `get_buffer`/`buffer`, `to_bytes`, `len`, `is_empty`, `clear` |

`Set`'s closure and sequence adapters (`all`, `each`, `filter`, `fold`, `map`,
`flat_map`, `min`, `max`) route through the same `to_list()` / `Iter`
machinery every other container's adapters already use (I8: one mechanism,
not a parallel set-native lazy surface); `map`/`flat_map` return a plain list
or iter, since a Set's uniqueness does not carry through an arbitrary
mapping — pipe the result through `.to_set()` if you want it deduplicated
back into a Set. `values` is the lazy alias of `to_list`. `replace`/`take`
are the native Rust `HashSet` swap-in / remove-and-return methods. `first`
is shipped with arbitrary hash-order semantics.

`Set` declines `sort`, `shuffle`, `indexof`, and `indexed` pending ballot
`D-SET-DECLINE1` (card #1584): a hash Set has no position, so each name
needs `to_list()` first, same as `first`'s note above. `Set` also declines
`flatten`: Jet requires every Set element to implement Hash and Eq (E0506),
so no `Set<T>` can ever hold a nested List or Set for `flatten` to unpack.
`copyto` is declined on `Set` and `SortedSet`; use `to_list()` then list/iter
methods for all of the above.

Example: `examples/features/collections/iter_adapters.jet` covers adapters
including the #1479 surface (`repeat`, `cycle`, `drop_last`, `shuffle`,
`is_sorted`/`is_sorted_by`, `dedup_by`, `last_index_of`, `average`, `compare`,
`split`, `chunk_while`, `to_set`). `cycle` is infinite — call `.take(n)` (or
another finite adapter) before `to_list`. `shuffle` uses a fixed demo seed so
examples stay deterministic; use `Rng` when you need a real random shuffle.
Synonyms in the Core surface ledger map competitor spellings such as `fill`→
`repeat`, `cmp`→`compare`, `next`→`first`, `size_hint`→`len`, `compact`→
`filter`, `tostring`→`join`, and `clip`/`iterator`→`to_list`.
Also: `examples/features/collections/iter_tools_audit.jet` covers the
adapter and specialized-container surface. Lazy protocol:
`examples/features/collections/lazy_iter.jet`.
#1477 List/Map remainder: `examples/features/collections/list_surface.jet` and
`examples/features/collections/map_surface.jet`.

The zip family is available as a free call or a method and accepts any number
of list or iterator inputs. `zip` requires equal lengths, `zip_short` stops at
the shortest input, and `zip_pad` reaches the longest input. Omitted padding is
`None`; `fill: value` supplies one value for every missing column; and
`fills: (a: value, b: value, ...)` supplies a value per named column. Free-call
labels become row fields; methods use `a`, `b`, `c`, and so on.

```jet
left :: [1, 2, 3]
right :: [10, 20]

loop row, left.zip_pad(right, fill: 0) {
    print(row.b)
}
```

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

fn count_lines(path: String) => Int ? IOError {
    handle :: files.open(~path)?
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
last handle drop. `core.path` provides `path.join(dir, name) => String` plus
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
| `u.username()` / `.password()` / `.userinfo()` / `.authority()` | `String` | Credential and authority accessors (empty when absent) |
| `u.default_port()` | `Int?` | Well-known port for the scheme (`http`/`ws`→80, `https`/`wss`→443, …) |
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
default, or `deliver_accepted`). It constructs the same `SMTPConfig` accepted
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
Mailer drop. Ambient task cancellation and `#Context` deadlines interrupt DNS,
connect, TLS, and SMTP wait checkpoints. Cancellation after DATA becomes
`DeliveryUnknown`; Jet never retries automatically. `SendReport` means relay
acceptance, not inbox delivery.

```jet
password :: crypto.Secret.from_text(env.get("SMTP_PASSWORD") ?? return)
config := email.SMTPConfig.{
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
    mux.post("/api/:name/*path", (req: HTTPSrvReq) =>
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
| `client.get(url)` / `client.post(url, body)` | `HTTPClientResp ? String` | One-shot request helpers |
| `client.request(method, url)` | `HTTPClientReq` | Start a typed request builder; malformed or unsupported URLs fail with a stable Jet error before transport |
| `req.header(name, value)` / `.body(text|Body)` | `HTTPClientReq` | Add headers or a string/`Body` upload; `Body.reader` streams in 64 KiB wire chunks without materializing through `Body.bytes(1GiB)` first |
| `req.form(name, value)` / `.multipart_text(name, value)` | `HTTPClientReq` | Encode form or text multipart fields; multipart names percent-encode quotes and line breaks, and bounded RFC-valid boundary selection avoids every supplied name and value |
| `req.cookie(name, value)` / `.redirects(n)` | `HTTPClientReq` | Set Cookie header or a redirect limit from 0 through 4,294,967,295; unset follows at most 10, and out-of-range limits fail before transport |
| `req.timeout(ms)` / `.connect_timeout(ms)` / `.read_timeout(ms)` / `.total_timeout(ms)` / `.dns_timeout(ms)` / `.tls_timeout(ms)` / `.write_timeout(ms)` / `.first_byte_timeout(ms)` | `HTTPClientReq` | Set nonnegative global/per-phase deadlines; request overrides beat `Client.timeouts`; negative milliseconds fail before transport; an ambient `#Context(deadline: …)` remaining budget is converted to an absolute Instant at send entry and upper-bounds the request total |
| `req.proxy(url)` | `HTTPClientReq` | Use an explicit proxy; malformed URLs, refused tunnels, and rejected proxy authentication return stable Jet errors; env proxies are honored by default |
| `Client.new().proxy(policy)` | `HTTPClient` | Typed client proxy policy: `.FromEnvironment` (default), `.None` (ignore env), or `.Url(proxy)` |
| `Client.new().tls(config)` | `HTTPClient` | Apply a `core.tls.ClientConfig`; `CustomOnly` trust, mTLS client identity, and inclusive `.Tls12`/`.Tls13` version bounds are live-proven on HTTPS send (`http_client_law`) |
| `Client.new().cookies(.Memory)` | `HTTPClient` | Opt into one clone-shared, bounded RFC6265bis memory jar; shortcuts stay stateless |
| `Client.new().redirects(.Follow.{ max:, same_origin_credentials: })` | `HTTPClient` | Typed redirect policy (D-HTTP-CLIENT2); default unset is Follow(max:10, same_origin_credentials:true). Cross-origin always strips Authorization / Proxy-Authorization / Cookie; `same_origin_credentials: false` also strips them on same-origin hops |
| `Client.new().allow_http_downgrade(true)` | `HTTPClient` | Opt in to following HTTPS→HTTP redirects; denied by default (D-HTTP-CLIENT2) |
| `Client.new().retries(.Safe/.Idempotent/.None)` | `HTTPClient` | Stale pooled-connection retry before request bytes only (D-HTTP-CLIENT2): default unset is Safe (GET/HEAD/OPTIONS/TRACE); `.Idempotent` opts in PUT/DELETE; `.None` disables; max one attempt; IO-only (never Timeout/status); POST/PATCH never auto-retry |
| `req.send()` / `client.send(req)` | `HTTPClientResp ? String` | Execute the request; connection, pre-response I/O, and malformed response framing failures return stable Jet errors |
| `resp.status()` / `.body()` / `.header(name)` / `.cookies()` | mixed | Inspect response status, text body, headers, and Set-Cookie values |

The compatibility text response path accepts at most 8 MiB of transfer-decoded
bytes and rejects non-UTF-8 data. Client uploads and response downloads share the
byte-native streaming `Body` API; unknown-length uploads use HTTP/1.1 chunked
transfer encoding.

Server surface:

| Function / method | Returns | What it does |
|-------------------|---------|--------------|
| `server.mux()` | `HTTPMux` | Create a function-first router |
| `mux.get/post/put/delete/patch(path, handler)` | nothing | Register `fn(HTTPSrvReq) => HTTPSrvResp` handlers |
| `server.bind(addr, mux)` / `server.bind(addr, mux, tls: server.tls(cert, key))` | `HTTPServer ? String` | Bind plaintext or HTTPS; pair with `serve`/`shutdown` |
| `server.serve(addr, mux)` | `() ? String` | Serve HTTP/1.1 forever |
| `server.serve(addr, mux, tls: server.tls(cert, key))` | `() ? String` | Serve HTTPS with explicit TLS material |
| `server.serve_once(addr, mux)` / `server.serve_once_listener(listener, mux)` | `() ? String` | Testable one-request serving |
| `server.response(status, body)` / `resp.header(name, value)` | `HTTPSrvResp` | Build a response |
| `resp.trailers(^headers)` | `HTTPSrvResp ? HTTPError` | Consume and attach validated ordered response trailers; HTTP/1.0 and body-forbidden responses fail before publishing |
| `server.sse(data)` | `HTTPSrvResp` | Server-sent event response |
| `server.static_file(path, mime)` / `.static_file_range(req, path, mime)` | `HTTPSrvResp ? String` | Static file response, with Range support |
| `server.json(status, value)` | `HTTPSrvResp` | One JSON response from a `#Codable` value; sets `Content-Type: application/json; charset=utf-8` |
| `req.json<T>()` | `T ? HTTPError` | Read the request body as JSON and decode it; the ratified body cap frames the read |
| `resp.json<T>(limit)` | `T ? HTTPError` | Read a client response body as JSON and decode it; without `limit` the shared body cap applies |
| `server.static_files(mux, prefix, root)` | nothing | Mount a directory under a prefix; add `index`, `dotfiles`, `follow_links` for expert policy |
| `server.cors_policy(origins)` | `HTTPCorsPolicy ? HTTPError` | Build a CORS policy; add `methods`, `headers`, `credentials`, `max_age` for the full form |
| `server.cors(mux, policy)` | nothing | Install the policy as middleware on `mux` |
| `server.access_log(req, status)` | `String` | Stable access-log line |
| `server.request_id(mux)` | nothing | Install D-HTTP-SERVER2 built-in `request_id` middleware on `mux` |
| `req.method()` / `.path()` / `.param(name)` / `.header(name)` / `.body()` / `.body_len()` / `.under_limit(max)` | mixed | Inspect request data and enforce body limits |
| `req.trailers()` | `HTTPHeaders ? HTTPError` | Read ordered request trailers after Body reaches EOF; returns empty headers when none were sent |

Card 301 audit state:

| Area | State |
|------|-------|
| HTTPS client / server TLS | Shipped: client default HTTPS, server `tls:` named option |
| Typed URL input | Shipped: client calls accept `Url` or `String` |
| Redirects, cookies, forms, multipart, proxy, phase timeouts | Shipped: request builder methods above including per-request dns/tls/write/first_byte overrides; Client exposes typed `.cookies(.Memory)`, `.proxy(HTTPProxy)`, `.tls(TLSClientConfig)`, `.redirects(.Follow.{ max:, same_origin_credentials: })`, `.allow_http_downgrade(Bool)`, and `.retries(.Safe/.Idempotent/.None)` (default Safe); ambient `#Context(deadline:)` upper-bounds send totals; live RFC6265bis cookie scope/bounds, CustomOnly HTTPS trust, mTLS client identity, inclusive TLS 1.2/1.3 version bounds, HTTPS→HTTP deny/opt-in, same-origin credential strip on Follow, request `.first_byte_timeout` overriding Client phase budgets, and stale-pool write-before-bytes IO retry (reuse-proven) / POST no-retry / `.retries(.None)` / `.retries(.Idempotent)` opt-in proven |
| Router params and wildcard routes | Shipped: `:name` params plus final `*` wildcard (`param("wildcard")`) |
| SSE, static files, Range, access log, request body limits | Shipped: server helpers above |
| Typed JSON both ways | Shipped (D-HTTP-JSON1=A): `resp.json<T>(limit)` on the client, `req.json<T>()` on the server, and `server.json(status, value)` for answers. All three ride the `#Codable` path. The two readers return one Result; `server.json` returns an `HTTPResponse` with `Content-Type: application/json; charset=utf-8` set. The raw text and byte paths stay unchanged for experts. |
| Middleware engine | Partial: the one Handler wrapper chain is declaration-order outermost-first, supports request/response mutation and short-circuiting, and runs routed responses plus automatic 400/404/405 and redacted 500 recovery responses through the same boundary. Each composed layer is total: handler errors/panics and middleware factory/runtime errors/panics become bounded responses that reached outer wrappers can observe. An outer short-circuit does not run inner middleware. The boundary is shared by plaintext pipelines and TLS-backed HTTP/1.1 or HTTP/2. The real Jet AOT example is executable; default `jet run` (JIT) and AOT share the Prelude HTTP serve path (I9). |
| Built-in `request_id` middleware | Shipped on the approved surface as an ordinary declaration-ordered Handler wrapper: `server.request_id(mux)` preserves one 1–128-byte visible-ASCII inbound `x-request-id` or assigns a fresh bounded ID, exposes it to access events, and correlates router and recovery responses that reach its layer unless the response already chose one. Plaintext and TLS-backed HTTP/1.1 or HTTP/2 share the same wrapper. |
| Built-in `recover` middleware | Partial: the shared dispatch boundary contains handler and middleware factory/runtime failures and maps them to a redacted internal response on every shipped transport. A separately installable `recover` wrapper and incident publication policy have no approved public spelling yet. |
| Built-in `timeout` / `body_limit` middleware | Open: server transport deadlines and the default 1 MiB framing cap are shipped, but they are not Handler wrappers. Timeout needs task cancellation rather than a detached worker, and route-specific limit/policy spellings remain owner-gated. |
| Built-in CORS middleware | Shipped (D-HTTP-CORS1=A): `server.cors_policy(origins, methods, headers, credentials, max_age)` builds one explicit policy and `server.cors(mux, policy)` installs it. `origins` takes a plain `[String]` list or the `.Any` case. `.Any` with credentials is refused when the policy is built, with copy that says what to change. Without an installed policy the server sends no CORS header at all. The wrapper answers a preflight (an `OPTIONS` request that names `access-control-request-method`) and stamps `access-control-allow-origin` plus `vary: origin` on answers to origins in the policy. Every other origin gets its answer with no CORS header. |
| Built-in compression middleware | Open: response compression policy and public spelling are not implemented in this tranche; native request gzip decoding is a separate transport feature described below. |
| Built-in access-log middleware | Partial: `server.access_log(req, status)` remains a query-free compatibility string, while the internal typed event includes request ID, route template, status, bytes, duration, peer, protocol, and TLS without authorization, cookies, body, or query. The ordinary Handler wrapper cannot truthfully know final streamed wire bytes; core.log emission and the writer-completion hook remain open, and no new public wrapper spelling is claimed. |
| Built-in `static_files` handler | Shipped (D-HTTP-STATIC-FILES1=A): `server.static_files(mux, prefix, root)` mounts a directory. The defaults normalize the path, refuse escapes, hide dot-files, refuse symbolic links, and serve `index.html` for a directory request or 404. A resolved path always has to stay under the root, so no link can leave the mounted folder. Content types come from the shipped MIME table, and conditional requests plus one byte range reuse the per-file machinery. The trailing `index`, `dotfiles`, and `follow_links` arguments are the expert opt-in; `follow_links` allows a link that stays inside the root and still refuses one that leaves it. The per-file `static_file` / `static_file_range` helpers stay for single named files. |
| Bounded hostile request parsing | Partial: HTTP/1.1 incrementally frames and dechunks octets (including extensions), preserves pipelined boundaries, and caps the decoded body at 1 MiB plus 32 KiB of chunk metadata and trailers. The canonical transport request retains declared trailers in order after the body drains; undeclared, forbidden, malformed, folded, excessive, or non-UTF-8 trailers fail closed. The parser also rejects malformed/truncated chunks, non-HTTP header whitespace/control values, multiple/unsupported transfer codings, oversized headers/bodies, ambiguous Content-Length, Content-Length with Transfer-Encoding, folded headers, and malformed framing. Plaintext handlers receive the shared byte-native Body without an eager compatibility buffer; gzip decoding and the TLS bridge still buffer a bounded request before dispatch. |
| Request method | Shipped: standard and extension methods preserve their case and route case-sensitively; only exact uppercase `GET`, `HEAD`, and `OPTIONS` receive those standard server semantics, and `Allow` preserves registered extension-method case. Methods must be one nonempty HTTP token; separators, controls, whitespace, and non-ASCII bytes fail with 400 and close before body permission or dispatch. |
| Request target and Host authority | Partial: origin-form and HTTP(S) absolute-form share strict raw path/query validation and route through the same path; exact `OPTIONS *` uses automatic server-wide `Allow` through the ordinary middleware chain without route-handler dispatch. Absolute authority must match Host after case, IPv6, percent-hex, and default-port normalization. CONNECT uses authority-form only: the request target is a Host-matching authority (exact port identity, no scheme default), handlers see the normalized authority in `path`, and routing matches `/{authority}` patterns such as `/:authority`. HTTP/1.1 requires exactly one valid Host, while HTTP/1.0 may omit it; malformed escapes, illegal raw URI characters, mismatch, userinfo, fragments, unsupported target forms, non-OPTIONS asterisk-form, non-CONNECT authority-form, CONNECT origin/absolute/asterisk forms, and malformed or ambiguous authorities fail with 400 and close before dispatch. HTTP/2 CONNECT omits `:scheme` and `:path`, requires `:authority`, shares the same Host-matching normalization and `/{authority}` routing, and rejects CONNECT with `:path`/`:scheme`, missing or malformed `:authority`, or Host mismatch before dispatch. |
| Connection options | Shipped for plain HTTP/1.x: repeated fields and comma lists share one HTTP-token parser, extension options and empty list members remain compatible, `close` dominates, and malformed options fail with 400 before body permission or dispatch |
| Content-Length | Shipped for plain HTTP/1.x: repeated and comma-combined values share one decimal parser and must be nonempty ASCII digits with the same numeric value; leading zeros remain valid, while signs, empty members, overflow, and conflicts fail with 400 before body permission or dispatch |
| Persistent connections | Shipped for the server: plain HTTP/1.x preserves pipelined request boundaries, responds sequentially in wire order, idles for at most 60 seconds, stops keep-alive reuse promptly during shutdown, and closes after 1,000 requests. Native HTTP/2 serves concurrent streams with flow control and GOAWAY drain over cleartext preface or the existing rustls connection selected by ALPN `h2`; HTTP/1.1 remains the TLS fallback. |
| HTTP message trailers | Shipped (D-HTTP-TRAILER-API1=A): `req.trailers()` succeeds only after Body EOF and returns ordered repeated fields or empty headers; `resp.trailers(^headers)` is the sole response builder and consumes its Headers argument. Forbidden fields fail validation. HTTP/1.1, including TLS, emits an upfront generated `Trailer` declaration with chunked framing; HTTP/1.0 fails before publishing; native HTTP/2 over cleartext or ALPN-selected TLS accepts and emits trailing HEADERS with END_STREAM. |
| HTTP/1 response framing | Partial: only HTTP/1.0 and HTTP/1.1 requests reach handlers; unsupported versions close with 505. `server.response` preserves status integers 100–599 and converts every out-of-range value to a generic 500 before headers or body reach the wire. HEAD preserves a known selected representation's Content-Length and metadata without sending body bytes; unknown-length HEAD responses publish no false framing. Statuses 1xx/204/304 publish neither body bytes nor Content-Length; 205 publishes canonical `Reset Content`, `Content-Length: 0`, and no body bytes regardless of handler or HEAD metadata. Unknown-length HTTP/1.0 responses close-delimit and cannot reuse the connection. The shared transport emits HTTP/1.1 response trailers only with an upfront generated `Trailer` declaration and chunked framing, preserves repeats, and rejects forbidden fields or HTTP/1.0 use before publishing headers. Native HTTP/2 response framing is shipped. |
| `Expect: 100-continue` | Shipped for plain HTTP/1.1: one interim response follows successful framing/size validation, Content-Length oversize fails with 413 before upload, unsupported or repeated expectations fail with 417 before dispatch, and final pipelined responses remain ordered; TLS keep-alive serving shares the named tls: option but still buffers each request before dispatch |
| Bounded streaming bodies | Partial: client uploads stream in 64 KiB chunks from `Body` (including `Body.reader`) without `Body.bytes(1GiB)` materialization before connect, and response downloads stream through the same model. Plaintext HTTP/1 server request bodies are pulled from the socket only as the handler consumes the shared Body; unknown-length responses pull at most 64 KiB per write and use chunked framing, socket backpressure, write-idle deadlines, and source cancellation. HTTP/2 request/response bodies stream with flow-control backpressure and cancellation over cleartext or TLS. Gzip request decoding and TLS-backed HTTP/1 requests remain bounded-buffered; the public compatibility `server.response` surface still has no streaming-body builder. |
| Transparent Content-Encoding decoding | Partial across both facets: the native client advertises and decodes gzip by default, hides decoded Content-Encoding/Length while retaining `raw_content_encoding()`, supports `.raw_encoding()`, and fails closed on unsupported encodings (exact `http_client_law`). Plaintext and TLS HTTP/1.x server requests decode native gzip after transfer framing, remove the stale Content-Encoding/Length view, reject unsupported coding with 415 and malformed gzip with 400, and cap encoded plus decoded bodies at the server limit with 413. HTTP/2 request decoding and response compression middleware remain open. |
| Graceful shutdown | Shipped for HTTP/1.x and HTTP/2 over cleartext or TLS: `Server.bind`/`serve`/`shutdown(grace)` stop accepts, send HTTP/2 GOAWAY with the accepted last-stream id, drain active work until grace, cancel stragglers, refuse new streams/requests (including TLS keep-alive reuse), and return bounded report counts |
| Pooling and HTTP/2 | Shipped across both facets: shared native `Client` pools HTTP/1.1 keepalive after drained bodies and multiplexed HTTP/2 sessions (ALPN plus explicit h2c); concurrent streams open without holding the connection mutex across HEADERS waits, with exact `http_client_law` interop coverage for reuse, hostile HPACK, and TLS ALPN. Native HTTP/2 server serving runs over cleartext preface and rustls ALPN with flow control and graceful GOAWAY drain. |
| WebSocket | Shipped as standalone `core.ws` (D-WS1=B): `ws.connect(url)` and `ws.upgrade(req)` share one RFC6455 codec with 1 MiB message bounds, masked client frames, ping/pong, close, and ambient-deadline cancellation. Live client↔server echo, hostile handshake rejection, and oversized-frame refusal are covered by `tests/ws_law.rs`. `wss://` TLS upgrade remains open behind the existing TLS bridge. |

### `core.ws` — WebSocket client and server

`core.ws` is the standalone WebSocket home (D-WS1=B). It imports HTTP request
types for server upgrade and does not hide WebSocket APIs under `core.http`.

| Function / method | Type | Notes |
|-------------------|------|-------|
| `ws.connect(url)` | `WsConn ? WsError` | Cleartext `ws://` dial and RFC6455 handshake |
| `ws.upgrade(req)` | `WsConn ? WsError` | Server upgrade from an HTTP request during mux dispatch |
| `conn.send_text(text)` / `.send_bytes(bytes)` | `() ? WsError` | Data frames; client frames are masked |
| `conn.recv()` | `WsMessage ? WsError` | Text, binary, or close; respects ambient deadlines |
| `conn.close(code, reason)` | `() ? WsError` | Sends a close frame and shuts down the socket |

Example: `examples/features/net/ws_echo.jet`.

Examples: `examples/features/net/http_rest_service.jet` and
`examples/features/net/http_server_trailers.jet`.

### `core.browser` — WebDriver BiDi automation (D-BROWSER-AUTO1=A)

`core.browser` is the portable browser automation home. It speaks versioned
WebDriver BiDi over `core.ws`, with Jetpack-locked browser binaries and an
explicit capability-checked CDP expert path. There is no Node or Playwright
runtime dependency.

| Surface | Notes |
|---------|-------|
| `browser.profile` / `browser.timeout` / `browser.connect_profile` | Pin the BiDi command contract and connect |
| `browser.locked(engine)` | Read a Jetpack `[[browser]]` pin (`jetpack browser lock`) |
| `session.context` / `context.page` / `context.tab` / frames | Isolated user contexts; explicit close |
| Semantic locators + waits | `get_by_role` / `text` / `label` / `placeholder` / `test_id` / `css`; `wait` / `wait_gone`; click/hover/fill/press |
| Events + network | `subscribe` / `next_event`; redacted request facts; intercept continue/fail/fulfill |
| Artifacts | cookies, local/session storage, `set_files`, downloads folder, screenshot, PDF |
| `session.protocol("cdp"\|"bidi")` | Expert raw commands; CDP only after `goog:cdp` capability |
| `privacy` / `receipt` / `trace` | Isolated profiles on; shared denied; redacted audit facts only |

Acceptance matrix and agent cookbook:
`examples/features/net/browser_matrix.jet`,
`examples/features/net/browser_agent.jet`. Focused proof:
`tests/browser_bidi.rs`, `tests/browser_lock.rs`.

### `core.crypto` — safe envelopes and expert primitives

`core.crypto` is the safe-by-default cryptography surface. Beginner APIs hide
nonce handling and algorithm selection; raw algorithm choice lives under
`core.crypto.expert` and requires an audited `#Unsafe` region. RustCrypto crates
are linked only through the hidden bridge crate, not the compiler.

```jet
use core.crypto as crypto

fn run() {
    recipient :: crypto.X25519SecretKey.new_random() ?? return
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
| `expert.open_v1(key, envelope)` | `[U8] ? CryptoError` | Audited `#Unsafe`-only reader for canonical historical JETC v1 ChaCha20-Poly1305 or AES-256-GCM bytes; every failure is `OpenFailed` |
| `expert.migrate_v1(key, source, recipients, destination)` | `() ? FileCryptoError` | Audited `#Unsafe`-only migration from canonical historical JETC v1 to recipient JETC v2; preserves the source and reopen-verifies v2 before atomic publication |
| `sign(signing_key, bytes)` / `verify(verify_key, bytes, signature)` | `Signature ? CryptoError` / `Bool ? CryptoError` | Ed25519 signing and verification with nominal key and signature types |
| `x25519(secret_key, public_key)` | `SharedSecret ? CryptoError` | X25519 key agreement with nominal key and shared-secret types |
| `hkdf_sha256(ikm, salt, info, len)` | `Secret ? CryptoError` | HKDF-SHA256 expand with a 0–8160-byte output bound, without exposing derived secret bytes |
| `password_hash(password)` | `PasswordHash ? CryptoError` | Argon2id password hash with generated salt and safe defaults; accepts a nominal `Secret` |
| `password_verify(password, stored)` | `Bool ? CryptoError` | Verify a nominal `Secret` against a validated `PasswordHash` |
| `expert.argon2id(password, salt, memory_kib, iterations, lanes, output_len)` | `Secret ? CryptoError` | Audited deterministic Argon2id with the ratified hard bounds; compiler-known violations are E2702 |
| `constant_time_equal(a, b)` | `Bool` | Constant-time comparison of nominal `Secret` values |
| `constant_time_equal_bytes(a, b)` | `Bool` | Constant-time comparison of two byte lists |

Card 302 audit state:

| Area | State |
|------|-------|
| AEAD envelope | Shipped: one recipient-based JETV `seal/open` path; historical symmetric JETC v1 has no writer or safe fallback and is readable only through `expert.open_v1` |
| Signatures | Shipped: Ed25519 sign/verify with RFC-vector golden |
| Password hashing | Shipped: Argon2id PHC hash/verify, random salt default, deterministic salted vector helper |
| KDF / key agreement | Shipped: HKDF-SHA256 and X25519 with RFC vectors |
| Hashes / comparison | Shipped: SHA-256, SHA-512, BLAKE3, constant-time equality |
| File envelope | JETC v2 recipient streaming plus exact expert JETC v1 open/migrate are shipped on Linux. Linux stages plaintext and output in unlinked `O_TMPFILE` inodes under a component-wise no-follow held parent, revalidates parent identity, links the still-open output fd to the final name once with `linkat(AT_EMPTY_PATH)`, and fsyncs the held directory. Exact-maximum sparse input enters bounded streaming; larger input is rejected before staging; hostile framing, short I/O, cancellation, tamper, and publication races leave no output. Other targets currently fail closed and make no JETC filesystem-runtime claim |
| Entropy | Shipped: one D-CRYPTO-RNG1 provider shared by `random.bytes`, envelope nonces, Ed25519 key generation, Argon2id salts, and file envelopes. Linux glibc `getrandom` has live runtime proof. Source adapters use macOS `SecRandomCopyBytes`, Windows MSVC `BCryptGenRandom`, and WASI preview 1 `random_get`; their runtime execution remains on #526. Unsupported targets fail closed with no fallback. D-CRYPTO-WASI-ALLOC2: every interrupted WASI call's exact-count zeroed `Vec` is volatile-zeroized and dropped before a new ownership generation; allocator address reuse is allowed; no failed bytes escape; at most seventeen calls occur. Package key generation maps provider failure through a closed helper status to E1292, never raw provider/helper text |
| Reference and tier proof | Expert XChaCha20-Poly1305 matches the CFRG vector and AES-256-GCM matches NIST CAVS. Argon2id matches RFC 9106 section 5.3 through Jet's canonical worker, with a public expert known answer and Argon2i mutation control; Ed25519, X25519, and HKDF retain their RFC vectors. Generated bridge dependencies use exact versions, disabled defaults, and explicit features. The typed crypto golden is byte-identical in AOT and default dev. Resident JIT names its unsupported result-status boundary, and default dev takes the transparent AOT fallback rather than changing behavior |
| Secret display types | Shipped: `Secret` is a nominal runtime type used by secret-taking APIs; display, debug, print, serialization, reflection, comparison, hashing, and cloning are rejected |
| PQ hybrid agility | Tracked by #71, not duplicated here |

Examples: `examples/features/crypto/crypto_suite.jet`,
`examples/features/crypto/crypto_envelope.jet`, and
`examples/features/crypto/crypto_sign.jet`.

### `core.vault` — repository secrets and typed key generations

`core.vault` keeps the existing `get(name) => String?` API and adds persistent
typed `SigningKey` and `X25519SecretKey` generations. Every call below requires
the `Secret` effect. `KeyRef<T>` is safe to clone, compare, hash, display, and
persist; it contains public identity metadata, never key bytes.

```jet
use core.crypto as crypto
use core.vault as vault

fn provision() =[Secret]=> () ? vault.VaultError {
    plan :: vault.prepare_generate<crypto.SigningKey>("release")?
    write :: vault.authorize_write(&plan, reason: "create release signer")?
    key_ref :: vault.commit_generate<crypto.SigningKey>(take(write), take(plan))?
    print(key_ref) // repo:release@v1
}
```

| API | Result |
|-----|--------|
| `current<T>(name)` | active `KeyRef<T>?`; absent or no active generation is `None` |
| `versions<T>(name)` | all refs, newest first |
| `load<T>(&ref)` / `status<T>(&ref)` | exact key or `KeyStatus`; revoked loads fail |
| `prepare_generate<T>` / `prepare_store<T>` / `prepare_rotate<T>` | move-only five-minute `MutationPlan<T>` |
| `prepare_retire<T>` / `prepare_revoke<T>` | plan bound to an exact ref and reason |
| `authorize_write<T>(&plan, reason:)` | one-use `VaultWrite<T>` after native approval |
| `commit_generate/store/rotate/retire/revoke<T>(take(write), take(plan))` | atomic compare-and-swap mutation |

The store uses the existing `.jet/secrets-recipients` age recipient set and
canonical `JVLT` v2 bytes. Historical String-only stores migrate on the first
authorized typed mutation. Interactive authorization uses the native preview;
a headless process needs the exact reviewed grant
`jet trust grant vault.write:<repository_uuid>`. Source, workspace settings,
environment variables, DAP, and stdin cannot approve a write. Linux uses
`openat2`, `pidfd`, anonymous `O_TMPFILE` staging, inode-bound locks,
`renameat2(RENAME_EXCHANGE)`, file/directory fsync, and authenticated
next-open backup recovery; unsupported providers fail closed. `VaultError`
redacts paths, identities, recipients, backend text, and key/store bytes.

Portable backup uses `WrappedVaultKey` (`JVKW` v1). Recipient export accepts
1–16 distinct X25519 public keys; passphrase export accepts a bounded `Secret`.
Both authenticate the source identity and concrete key type. Parsing checks
only public framing; unlock, tamper, type, and embedded-public-key failures all
return redacted `KeyWrapError.OpenFailed`.

| Backup API | Result |
|------------|--------|
| `export_to_recipients<T>(&ref, recipients)` | recipient-mode `WrappedVaultKey` |
| `export_to_passphrase<T>(&ref, &passphrase)` | passphrase-mode `WrappedVaultKey` |
| `prepare_import_wrapped<T>(name, wrapped, KeyUnlock.Recipient/Passphrase(...))` | bound `WrappedImportPlan<T>` |
| `authorize_wrapped_import<T>(&plan, reason)` | exact-preview `VaultWrite<T>` |
| `commit_import_wrapped<T>(take(write), take(plan))` | idempotent existing ref or atomic new generation |

Same-repository exact-origin imports return the existing Active/Retired ref;
revoked origins stay revoked. Cross-repository or renamed imports create the
next local generation with a new identity and imported-origin audit metadata.
Revocation is local bearer-copy state: already exported envelopes cannot be
remotely erased. Expert raw imports are prepared and committed only through
`core.vault.expert` inside `#Unsafe`; raw export remains the existing
`core.crypto.expert` operation.

`ExpiringSecret<T>` is the one secret-lifetime wrapper. `T` is closed to
`crypto.Secret`, `crypto.SigningKey`, and `crypto.X25519SecretKey`; construction
moves the credential into the wrapper. The wrapper retains a private observer
of the injected clock, so ordinary `~clock` copies cannot change its expiry.
Access is closure-only:

Generic cache expiry uses
`ExpiringValue.new(value, ttl, clock)`. Fresh deterministic values use
type-owned `.new`; entropy-drawing key constructors use `.new_random`.

```jet
clock := Clock.new(0)
ttl := Duration.minutes(5) ?? return
key := crypto.SigningKey.new_random() ?? return
secret := vault.ExpiringSecret.new(^key, ttl, clock)
result := secret.with((borrowed) => borrowed.public_key())
```

`.with` returns `Result<R, Expired>`. Its parameter is a compiler-owned,
non-escaping read loan: it cannot be moved, copied, stored, returned, or
captured. Expiry and wrapper drop destroy the owned credential through its
audited zeroizing `Drop`. A wrapper backed by `Clock.system()` observes time
and therefore carries the `Time` effect.

Examples: `examples/features/crypto/vault_keys.jet` and
`examples/features/crypto/vault_key_wrap.jet`, plus
`examples/features/memory/expiring_secret.jet`.

### `core.auth` — token verification and session batteries

`core.auth` exports standalone JWT/PASETO verifiers plus D-AUTH1 session
batteries. `app.auth` reuses the same Prelude symbols (one mechanism):

```jet
verify_jwt(token, key:, audience:, issuer:, clock_skew:) => Claims ? AuthError
verify_paseto(token, key:, audience:, issuer:, clock_skew:, footer:, implicit:) => Claims ? AuthError

register_user(user_id, password_hash) => () ? String
password_login(user_id, password_hash, now_ms, ttl_ms) => Session ? String
session_validate(session_id, now_ms) => Session ? String
magic_link_issue(user_id, now_ms, ttl_ms) => String ? String
magic_link_consume(token, now_ms, ttl_ms) => Session ? String
oauth_begin(provider) => String ? String
oauth_finish(state, subject, now_ms, ttl_ms) => Session ? String
```

`issuer` and `clock_skew` are optional for both verifiers; `footer` and
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
`DecodeError` variants. Sessions use httponly/secure/samesite cookie defaults.
The implementation is compiler-embedded, reuses Jet's JSON and crypto
mechanisms, and adds no external dependency.

Examples: `examples/features/crypto/auth_tokens.jet`,
`examples/features/crypto/auth_sessions.jet`.

### `core.sync` — CRDT values and row policy

`core.sync` ships D-SYNC1 CRDT value types and D-DBPOLICY1 row policies:

```jet
text_new / text_set / text_edit / text_merge / text_show / text_metadata
counter_new / counter_inc / counter_merge / counter_value
map_new / map_set / map_get / map_merge / map_show
list_new / list_push / list_merge / list_show
policy_new(table, expression) => RowPolicy ? String
policy_allows(policy, user, row_owner) => Bool
```

Merges are deterministic and keep every replica's edits. `SyncText` is a
sequence CRDT: `text_edit(doc, replica, at, delete_count, insert)` writes at a
character position, and two replicas that edit while apart reach one document.
A replica name must own one line of edits; editing two copies of a document
under one name is not a concurrent edit and does not merge. `text_metadata`
reports the highest counter each replica has written, which orders edits but
does not decide causality. Beginner
row policies use `owner == user`; expert policies may use `true`. `app.sync(doc,
over: session)` publishes the typed CRDT representation through a bounded
session registry and returns a monotonic delivery receipt. Database row-policy
enforcement uses the explicit `DBScope` selected by `D-DBPOLICY-BIND1`.

Example: `examples/features/tooling/sync_crdt.jet`.

### `core.watcher` — file/process/port change events

`core.watcher` owns watch-style APIs (D-WATCH-SCOPE1). It uses std-only polling
today: file watchers diff recursive metadata snapshots, process watchers check
process liveness, and port watchers attempt a TCP connect. Handles can be
polled directly or connected to `core.event` scopes with callbacks. Cancelling
or dropping the scope detaches its callbacks; cancelling a handle stops future
polls. Watchers own no background thread or shell process.

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
| `readline()` | `String ? IOError` | Same as `input()` with no prompt (peer free-function spelling) |
| `read_until(delim)` | `String ? IOError` | Read stdin bytes until `delim` (excluded); empty delim errors |
| `take(n)` | `[U8] ? IOError` | Read up to `n` raw bytes from stdin |
| `buffered()` | `StdinHandle` | Same buffered stdin handle as `stdin()` (Jet buffers by default) |
| `confirm(prompt)` | `Bool` | Ask yes or no; show `[y/N]` and use no for a bare Enter |
| `choose(prompt, items)` | `String ? IOError` | Number the strings and re-prompt until the user makes a valid choice |
| `input_secret(prompt)` | `String ? IOError` | Read one line without echo; return an error when stdin is not a terminal |
| `read_all_input()` | `String ? IOError` | Read all of stdin to end-of-file |
| `print(value…)` / `println(value…)` | nothing | Print each value on its own line (`println` is the peer spelling of `print`) |
| `sprint(text)` | `String` | Identity format-to-string for a `String` (use `"{x}"` for other values) |
| `repr(text)` | `String` | Debug representation of a `String` |
| `binread(path)` | `[U8] ? IOError` | Read a file as raw bytes |
| `binwrite(path, bytes)` | `() ? IOError` | Atomically write raw bytes to a file |
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
| `progress(source[, description[, format]])` | `Iter<T>` | Wrap a `List<T>` or `Iter<T>`; report percent, count, elapsed time, remaining estimate, and rate as items are pulled. Format fields are `{description}`, `{percent}`, `{count}`, `{total}`, `{elapsed}`, `{remaining}`, and `{rate}`. |

`print` stays in the core prelude (no `use` needed). `io.print` / `io.println`
are the qualified twins for `#NoPrelude` files — same newline-per-value
behavior. Use `io.eprint` for stderr. Use `input` or `readline` for public
text and scripts. Use `input_secret` for passwords and tokens. It never falls
back to an echoed read when stdin is redirected. `sprint` / `repr` take a
`String`; format other values with interpolation first. `buffered()` is an
alias of `stdin()` — Jet already buffers stdin. `core.term` still owns
`live { ... }` and `term.read_key()` for direct raw-key input; it is the
shipped raw-mode/key-event bridge under D-TERM1.

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
| `.parse_or_exit(argv)` | `([String]) → ParsedArgs` | prints help and exits 0 for `--help`; prints usage errors and exits 2 |

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

`--help` and `--version` are recognized automatically. Use `.parse` for tests,
embedders, or custom error handling because it does not exit the process. It
returns `ParsedArgs ? String`, where the error string contains the parse message.
Use `.parse_or_exit` for a command-line entry point. It prints help and exits 0
for `--help`, or prints a usage error and exits 2 for invalid arguments.
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
        fn display(self) => String {
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
editions retain the source-compatible `set => ()` signature and report an
invalid call as E3001. A future major release and edition opt-in changes `set`
to `() ? EnvError`.

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
| `pid()` / `getpid()` | `Int` | Current process id |
| `hostname()` | `String` | Hostname, falling back to `localhost` |
| `username()` | `String` | Current username, or empty if unavailable |
| `release()` | `String` | Kernel / OS release string |
| `version()` | `String` | Human-readable OS version string |
| `getppid()` | `Int` | Parent process id (0 when unavailable) |
| `getuid()` / `geteuid()` | `Int` | Real / effective user id |
| `getgid()` / `getegid()` | `Int` | Real / effective group id |
| `getgroups()` | `List[Int]` | Supplementary group ids |
| `getpgid(pid)` / `getsid(pid)` | `Int ? IOError` | Process group / session id |
| `getpgrp()` | `Int` | Calling process group id |
| `expand(template)` | `String` | Expand `$VAR` / `${VAR}` from the environment |
| `uptime()` | `Float` | Seconds since boot when known, else `0.0` |
| `loadavg()` | `List[Float]` | 1/5/15-minute load averages |
| `times()` | `List[Float]` | Process CPU times (user, system, children, elapsed) |
| `exitcode(status)` | `Int` | Exit code extracted from a wait status |
| `success(status)` | `Bool` | Whether a wait status is a normal zero exit |
| `sync()` | `()` | Flush filesystem buffers (POSIX no-op elsewhere) |
| `umask(mask)` | `Int` | Set and return the previous file-creation mask |
| `getpriority(who)` | `Int ? IOError` | Nice value for process `who` (`0` = self) |
| `setpriority(who, prio)` | `() ? IOError` | Set nice value for process `who` |
| `utime(path, atime, mtime)` | `() ? IOError` | Set access / modification times |
| `stop(code)` | never returns | Exit this process with `code` |
| `atexit(handler)` | `()` | Register a process-exit callback |
| `set_current_dir(path)` | `() ? IOError` | Change process working directory |
| `on_interrupt(handler)` | `()` | Register a process-lifetime handler for Ctrl-C / SIGINT on Unix and Windows |

POSIX process/session control (requires `#Unsafe("…")` and an OS gate):

| Function | Returns | What it does |
|----------|---------|--------------|
| `fork()` | `Int ? IOError` | Fork; `0` in the child, child pid in the parent |
| `setuid` / `setgid` / `setpgid` / `setpgrp` / `setsid` / `initgroups` | fallible | Credential / session control |
| `kill(pid, sig)` | `() ? IOError` | Send a signal |
| `wait` / `waitpid` | `Int ? IOError` | Wait status |
| `pipe()` | `List[Int] ? IOError` | `[read_fd, write_fd]` |
| `close_fd(fd)` | `()` | Close a raw pipe/fifo descriptor |
| `mkfifo(path, mode)` | `() ? IOError` | Create a named pipe |

Interrupt handlers are additive. Each Ctrl-C runs every registered handler in
registration order on Jet's interrupt dispatcher, never inside the operating
system callback. Registration is active before `on_interrupt` returns. The
`()` return means registrations live until the process exits; there is no
unregister/drop handle. Calling `on_interrupt` on a target without process
interrupts fails explicitly instead of silently discarding the handler.

Examples: `examples/features/io/os_facts.jet`,
`examples/features/io/os_process_control.jet`.

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
    copied :: process.run(Sh.{"cp -- {target} backup"}) ?? return

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
`stderr(mode)`, `timeout(duration)`, `output_limit(bytes)`, `detached()`, and
`terminal()` or `terminal(policy)`.
`mode` is one of the three stream-mode dot-literals: `.Stream` (pipe it —
drain live via `child.stdout.lines()`), `.Inherit` (pass through to the
parent's stream), or `.Capture` (pipe it — collect into `ProcessResult` at
`run()`/`wait()`). `stdin` defaults to closed (no `.stdin(...)` call — the
child gets no stdin at all, never the parent's terminal by accident).
`timeout` takes a `Duration` (e.g. `Duration.seconds(30)?`). A spec can
`run()` to collect a `ProcessResult`, `run_checked()` to reject a failed exit,
or `spawn()` to return a `ProcessChild`.

Use `run()` when you need the full result and will inspect `success` yourself.
It returns `ProcessResult` for a nonzero exit with `success` set to `false`.
Use `run_checked()` when a nonzero exit must take the error path. Its `IOError`
includes the exit code, the signal when present, and at most 4096 bytes of
captured stderr.

**Terminal sessions (D-PROCESS-SESSION1=A, D-PROCESS-SESSION2=D).** Argv
execution with no terminal is the default and stays the safe path. Interactive
programs — a debugger, a REPL, a shell — often need a real terminal, and they
print different output without one. `terminal()` is the beginner opt-in.
`terminal(policy)` adds explicit initial size and mode. Both forms stay on the
same `ProcessSpec`, so cwd, environment, streams, timeout, and the child
lifecycle keep one model:

```jet
child :: process.cmd(["lldb", app]).terminal().spawn()?

policy :: TerminalPolicy.{
    size: TerminalSize.{ cols: 120, rows: 40 },
    mode: .Raw
}
plan :: process.cmd(["python", "-i"]).terminal(policy)
if plan.capabilities().has(TerminalFact.resize) {
    child :: plan.spawn()?
    session :: child.terminal ?? return
    session.resize(TerminalSize.{ cols: 160, rows: 50 })?
}
```

`TerminalMode` is `.Raw` or `.Cooked`. The no-argument form uses an `80x24`
`.Cooked` policy. `capabilities()` returns a `Set[String]`. Use the checked
keys `TerminalFact.terminal`, `TerminalFact.resize`, and `TerminalFact.raw`
for stable facts. String keys remain open for preview facts without adding a
second report type; a close literal typo suggests the nearest stable key.
`ProcessChild.terminal` holds a terminal session only for a terminal-backed
child, so its type is `TerminalSession?`. After unwrapping it, `resize(size)`
returns `Unit ? IOError`.

A terminal session needs a native PTY or ConPTY. On Unix, `run()` and
`spawn()` create a real PTY, attach the child to a controlling session, and
expose its one combined byte stream through `stdout`; `stderr` is empty because
a PTY has no second output stream. The child session uses the requested size
and mode, and `TerminalSession.resize` changes the PTY window size. The stable
capability keys are `terminal`, `resize`, and `raw`. Unsupported targets fail
closed with an `IOError` instead of silently falling back to pipes.

`pipeline()` keeps ordinary pipe edges. A terminal-backed spec cannot be a
pipeline stage; use `spawn()` for the interactive child. PTY/ConPTY transport,
transcripts, binary streams, process-tree control, and resource limits remain
separate backend slices of the same process mechanism.

`ProcessChild` exposes `id()`, `wait()`, `kill()`, `terminate()`,
`interrupt()`, `.terminal`, a `.stdin` writer (`child.stdin.write(text)`), and
`.stdout`/`.stderr` streaming readers consumed only via
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
| `acosh`, `asinh`, `atanh`, `cbrt`, `exp2`, `exp_m1`, `ln_1p`, `log`, `signum`, `fma` | Extended float family (D-CORESURFACE1) |
| `is_even`, `is_odd`, `isqrt`, `factorial`, `binomial`, `digits`, `leading_ones`, `trailing_ones` | Whole-number helpers |
| `checked_abs`, `checked_neg`, `checked_div`, `checked_rem` | More checked integer ops |
| `fraction` | Exact ratio of two whole numbers (`Fraction?`) |
| `is_normal`, `is_subnormal`, `is_canonical`, `is_signed`, `is_zero`, `is_integer`, `sign_bit` | Float classification |
| `next_up`, `next_down`, `next_after`, `ldexp`, `scaleb`, `logb`, `ilogb`, `significand`, `ulp`, `radix`, `zero` | Float scale and neighbors |
| `copy`, `inv`, `cot`, `cmp`, `erf`, `erfc`, `gamma`, `lgamma` | Misc float helpers |
| `sin_cos`, `modf`, `frexp`, `div_mod`, `div_rem` | Named-tuple pairs |

Example: `examples/features/math/math_audit.jet`, `examples/features/math/more_math.jet`,
`examples/features/math/fraction.jet`.

---

### Unit families and physical quantities

`#UnitFamily` makes named unit types. Printing a unit value shows its magnitude
and declared symbol. Physical arithmetic also shows a normalized derived unit.
Jet loads the seven SI dimensions and standard SI, accepted non-SI, customary,
and electronics units from ordinary `Prelude/Units.jet` source.

```jet
#UnitFamily(Token, dimension, base: token) { token }
#UnitFamily(TokenRate, dimension: Token / Time, base: token_per_second) {
    token_per_second
}

fn run() {
    distance :: 12meter
    speed :: distance / 3second
    rate :: 30token / 2second
    print(distance) // 12 meter
    print(speed)    // 4 meter/second
    print(rate)     // 15 token/second
}
```

Use `dimension` to mint one package-owned base axis. Use
`dimension: Mass * Length / Time / Time` to give a structural dimension a
name. Import one declaration when two packages must share a custom axis.

Most unit scales are exact ratios. Degree uses the exact symbolic definition
`pi / 180`. `mmHg` retains its NIST SP 811 convention. Dalton retains the
pinned BIPM/CODATA central value, standard uncertainty, and source. A measured
crossing requires an explicit rounded conversion and is never labeled exact.

Bare interpolation uses the symbol form. `{value#Unit(name)}` uses the
generated unit type name. `{value#Unit(bare)}` omits the unit. A hand-written
`Display` implementation replaces the default for its concrete unit type.
`.raw()` still returns the unchanged numeric value.

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
    print(v.reduce(.Max))           // 4.0
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
| `v.reduce(.Add)` `.Mul` `.Min` `.Max` `.Avg` | General reduce by `ReduceOp` value |
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
`fn … =[]=>` cannot call them (E3403 — they break reproducibility). To use
randomness inside a `fn … =[]=>`, take a seeded `Rng` **as a parameter** and draw
through it — the seed makes the stream reproducible on every machine:

```jet
fn roll(rng: &Rng) =[]=> Int {
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
| `scene.query<T...>()` | `[String]` | Return entity rows of component data for the registered types |
| `game.Replay.record(path)` | `GameReplay` | Name a `.jetreplay` game-input artifact for transcript recording; proof replays use `.jetproof-replay` |
| `game.Backend.headless()` | `GameBackend` | Explicit no-renderer/no-audio/no-editor backend with a 3-frame budget |
| `backend.should_continue()` | `Bool` | Whether `game.run` should execute another frame |
| `backend.present()` | nothing | End-of-frame present (headless ticks the budget) |
| `game.run(scene, replay: replay)` | `String` | Loop on `should_continue` / `present` and return a transcript |

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

fn run() => () ? {
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
| `override_fidelity(v)` | `() ? String` | Set the process-global value; rejects values outside `0.0..1.0` |
| `reset_fidelity()` | nothing | Restore `default_fidelity()` |

Platform battery, thermal, network, load, and carbon providers do not ship in
Epoch 3 (D-ADAPT-PROVIDER1=A). Automatic adaptive scheduling is declined
(D-ADAPTRT1=C).

---

### `String` convenience surface (Epoch 3, #1409)

These methods are ambient `String` operations. Unicode classification, title
casing, trimming, and padding call the same pinned `core.text` algorithms as
the qualified module; there is one semantic implementation across AOT,
comptime, and default `jet run`.

| Method | Returns | Meaning |
|--------|---------|---------|
| `.trim_start()` / `.trim_end()` | `String` | Remove Unicode `White_Space` at one edge |
| `.pad_start(width, fill)` / `.pad_end(width, fill)` | `String` | Pad to terminal display width using the first grapheme in `fill` |
| `.index_of(needle)` | `Int?` | Unicode-scalar index of the first substring |
| `.count(needle)` | `Int` | Non-overlapping substring count; empty needles count as zero |
| `.is_alphabetic()` / `.is_numeric()` / `.is_whitespace()` | `Bool` | True only when non-empty and every scalar has the pinned property |
| `.is_ascii()` | `Bool` | True when every byte is ASCII |
| `.to_title()` | `String` | Word-start Unicode titlecase mapping; remaining letters are lowercase |
| `.split_once(separator)` | `(before: String, after: String)?` | Split at the first separator |
| `.last_index_of(needle)` | `Int?` | Unicode-scalar index of the last substring |
| `.is_lower()` / `.is_upper()` | `Bool` | True when there is at least one cased scalar and every cased scalar has that case |
| `.capitalize()` | `String` | Titlecase the first scalar; lowercase the rest |
| `.swapcase()` | `String` | Swap cased scalars via the pinned upper/lower maps |
| `.remove_prefix(p)` / `.remove_suffix(s)` | `String` | Strip an exact prefix/suffix, or return self unchanged |
| `.compare(other)` | `Int` | Lexicographic `-1` / `0` / `1` |
| `.equal(other)` | `Bool` | Same as `==` for text |
| `.copy()` | `String` | Owned clone (value strings already copy on assign) |
| `.reverse()` | `String` | Reverse Unicode scalar order |
| `.normalize()` | `String` | NFC (same as `core.text.nfc`) |
| `.rsplit(sep)` | `Iter<String>` | Split from the right; part order is left-to-right |

Competitor accounting is explicit: Python `partition`/`count`, Rust
`find`/`split_once`/`is_ascii`, Go `Cut`/`Count`, Swift `split`/`firstIndex`,
Kotlin `indexOf`/`count`, and JavaScript `indexOf`/`split` map to the rows
above or the existing `before`/`after`/`split` methods. Locale collation and
locale-sensitive casing remain out of scope under `D-TEXTUNICODE1=A`; regex
replacement remains owned by `D-REGEXENGINE1=A`. These explicit v1 decisions
are not silently added to the ambient String surface.

String declines (#1476, #1580): mutation verbs (`clear`/`push`/`pop`/`remove`/
`write`/`copyto`) stay off immutable text — rebuild with `+` / `replace` /
`slice`. Sequence adapters (`all`/`map`/`fold`/`skip`/`chunk`/…) and indexers
(`get`/`first`/`last`/`codepointat`) live on `.chars()` / `.bytes()` then
List/Iter (I8). `parse`/`tofloat` stay on destination types (`Int.parse` /
`Float.parse`, E0311). `match`/`matches` stay on `core.regex`. `concat` stays
on `+` / interpolation, the same join Jet already ships. Buffer-only names
(`capacity`/`intern`/`isvalid`/`isprint`/`chop`/`replacerange`/`indexofany`/
`lastindexofany`/`rpartition`) are declined; use the shipped surface or
`core.text` helpers instead. Card #1580's 34-row batch (`clear`/`get`/`push`/
`matches`/`parse`/`pop`/`remove`/`replacerange`/`isprint`/`map`/`write`/`all`/
`skip`/`droplast`/`indexed`/`first`/`flatmap`/`each`/`last`/`max`/`min`/`fold`/
`chunk`/`codepointat`/`indexofany`/`intern`/`lastindexofany`/`scan`/`tofloat`/
`concat`/`match`/`chop`/`rpartition`/`isvalid`) restates this same reasoning
as ballot `D-STR-DECLINE1`, pending owner ratification; card #1581 applies the
ratified outcome to the ledger.

---

### `core.text` — Unicode text algorithms

`core.text` owns the Unicode algorithms used by both its qualified calls and the
ambient String convenience methods above. Results are pinned to Unicode 16.0.0;
they do not inherit the host Rust, OS, locale, or terminal Unicode version.

| Function | Returns | What it does |
|----------|---------|--------------|
| `nfc/nfd/nfkc/nfkd(text)` | `String` | Normalize text for comparison or storage |
| `casefold(text)` / `caseless_eq(a,b)` | `String` / `Bool` | Locale-free caseless matching |
| `lower/upper(text)` | `String` | Full locale-free Unicode case mapping, including contextual final sigma |
| `graphemes/words/sentences(text)` | `[String]` | Segmentation helpers |
| `display_width(text)` | `Int` | Portable terminal columns: Ambiguous narrow, controls zero |
| `display_width(text, policy: TextWidth)` | `Int ? TextError` | Same algorithm with Ambiguous narrow/wide and controls zero/reject policy |
| `is_alphabetic/is_numeric/is_whitespace/is_ascii(text)` | `Bool` | Unicode classification over the whole string |
| `scalar_count/byte_count/scalars(text)` | `Int` / `[String]` | UTF-8/scalar facts |
| `splitn/rsplitn(text, sep, n)` | `[String]` | Bounded split helpers |
| `trim/trim_start/trim_end(text)` | `String` | Unicode-whitespace trim |
| `pad_start/pad_end/center(text, width, fill)` | `String` | Display-width padding |
| `starts_any/ends_any(text, parts)` | `Bool` | Prefix/suffix combinators |
| `char_indices(text)` | `[String]` | `"byte:scalar"` debug view |

The units are intentionally separate: `byte_count` counts UTF-8 bytes,
`scalar_count`/`scalars` count Unicode scalar values, `graphemes` returns UAX
#29 extended grapheme clusters, and `display_width` counts terminal columns.
`String.len()` is not documentation for any of those four units.

`TextWidth.{ ambiguous: .Narrow | .Wide, controls: .Zero | .Reject }` changes
only disputed terminal choices. Both forms segment extended grapheme clusters
first. Wide/Fullwidth and emoji-presentation clusters use two columns; flags,
keycaps, and valid emoji ZWJ sequences are charged once; combining and
default-ignorable-only clusters use zero. Defaults never inspect locale or
environment (D-TEXTWIDTH1=B).

| Unicode 16 audit lane | Shipped proof |
|-----------------------|---------------|
| Data ownership | Official UCD inputs, Unicode license, and SHA-256 manifest are checked in under `tests/data/unicode`; `gen-unicode-tables.mjs --check` proves byte-identical std-only regeneration. Generated tables are embedded; programs perform no file or network lookup. |
| Normalization and casing | Full `NormalizationTest.txt` and default/full `CaseFolding.txt`, plus every UnicodeData/SpecialCasing scalar mapping, run against the comptime engine; one end-to-end fixture compares the same hostile values with AOT. |
| Segmentation | Full Unicode 16 GraphemeBreakTest, WordBreakTest, and SentenceBreakTest corpora cover emoji ZWJ, RI, Hangul, combining marks, Hebrew punctuation, and abbreviations. |
| Shared consumers | AOT, comptime/interpreter, diagnostics, public classification/trim/padding, and regex Unicode classes use the pinned tables. Unsupported comptime regex syntax returns E0956 rather than silently using a host fallback. |
| Complexity | Normalization uses stable linear CCC counting rather than insertion sort; large descending-combining and segmentation inputs are regression-tested. |

Locale collation, locale-sensitive casing, and language-specific sorting are
not v1 core. They require explicit i18n locale data; Jet does not substitute
ASCII or the host locale.

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
| `time(h, m, s)` / `local_time(h, m, s)` / `parse_time(text)` | `LocalTime` / `LocalTime ? String` | Local wall-clock time |
| `datetime(y, m, d, h, mi, s)` | `DateTime` | UTC date-time from civil components |
| `days_in_month(y, m)` / `is_leap_year(y)` | `Int` / `Bool` | Calendar facts |
| `nanoseconds`/`microseconds`/`milliseconds`/`seconds`/`minutes`/`hours`(n) | `Duration ? RangeError` | Checked elapsed-time spans (nanosecond storage, D-TIMERES1=A) |
| `instant()` | `Instant` | Monotonic clock sample for elapsed-time measurement |
| `zone(name)` / `utc()` | `Zone ? String` / `Zone` | IANA time zone from TZif zoneinfo, or UTC |
| `zoned(dt, zone)` | `ZonedDateTime` | View a UTC `DateTime` in a zone |
| `zoned_local(date, time, zone)` | `ZonedDateTime` | Resolve local civil time in a zone |
| `sleep(millis)` | nothing | Block for about `millis` milliseconds (runtime E3003 if an ambient `#Context(deadline: …)` budget expires first) |
| `time.start()` | `Stopwatch` | Start a stopwatch |
| `sw.elapsed_millis()` | `Int` | Milliseconds since `time.start()` |
| `clock(seed)` | `Clock` | A **deterministic** clock capability starting at `seed` ms (D-DET1) |
| `Clock.system()` | `Clock` | An explicit monotonic production clock capability; carries the `Time` effect |
| `Duration.nanoseconds/microseconds/milliseconds/seconds/minutes/hours(n)` | `Duration ? RangeError` | Checked runtime elapsed-time span (D-TIMERES1=A: nanosecond count) |
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
| `LocalDate` | `year()`, `month()`, `day()`, `add_days(n)`, `add_months(n)`, `add_period(p)`, `diff_days(other)`, `weekday()`, `iso_weekday()`, `day_of_year()`, `iso_week()`, `quarter_of_year()`, `days_in_month()`, `is_leap_year()`, `truncate(unit)`, `replace(y, m, d)`, `format(pattern)`, `to_string()` |
| `LocalTime` | `hour()`, `minute()`, `second()`, `to_string()` |
| `DateTime` | `date()`, `time()`, `hour()`, `minute()`, `second()`, `millisecond()`, `microsecond()`, `nanosecond()`, `to_timestamp()`, `to_unix_ms()`, `plus_duration(d)`, `difference(other)`, `truncate(unit)`, `round(unit)`, `floor(unit)`, `ceil(unit)`, `replace(y, m, d, h, min, s)`, `in_zone(zone)`, `format_rfc3339()`, `format(pattern)`, `to_string()` |
| `ZonedDateTime` | `date()`, `time()`, `offset_seconds()`, `is_dst()`, `to_datetime()`, `zone()`, `add_duration(d)`, `add_period(p)`, `format(pattern)`, `to_string()` |
| `Instant` | `elapsed_millis()`, `elapsed()` |
| `Duration` | `in(unit)`, `is_zero()`, `total_seconds()`, `difference(other)` |
| `Zone` | `name()` |

Format patterns are literal text plus `yyyy`, `MM`, `dd`, `HH`, `mm`, `ss`,
`EEE`, `DDD`, `VV`, and `XXX`. Leap seconds are not represented as a distinct
instant: RFC 3339 parsing rejects `:60`; use upstream clock smear policy before
data reaches Jet.

**Test hook:** when the environment variable `LEX_TEST_EPOCH` is set to an
integer, `time.now()` returns that value instead of the real clock. Tests use
this to pin output; normal programs ignore it.

A `fn … =[]=>` cannot call ambient `time.now()` or construct `Clock.system()`
(E3403 — the system clock is not reproducible). `Clock.system()` is the explicit
production-clock constructor; `time.clock(seed)` remains the manual clock for
deterministic tests. Copying either clock creates an independent timeline at the
same observed instant.

To use time inside a `fn … =[]=>`, take a seeded `Clock` **as a parameter** and
read through it; the clock only moves when you `tick` it, so the result is
reproducible:

```jet
fn at(clock: Clock) =[]=> Int {
    return clock.now()             // current value in ms; pure read
}
fn run() {
    c :: Clock.new(1000)          // a Clock starting at 1000 ms
    print(at(c))                   // 1000, on every machine
}
```

| `Clock` method | Returns | What it does |
|----------------|---------|--------------|
| `now()` | `Int` | The clock's current value in ms (read; no `&` needed) |
| `tick(ms)` | `Int` | Advance the clock by `ms` (relative) and return the new value (needs `&Clock`) |
| `advance(to_ms)` | `Int` | Set the clock to the **absolute** instant `to_ms` and return it (needs `&Clock`; D-DET-CAPAPI) |
| `wait(d)` | `Int` | Advance the clock by a `Duration` `d` and return the new value (needs `&Clock`; D-DET-CAPAPI) |
| `Clock.system()` | `Clock` | Explicit monotonic production clock. It carries the `Time` effect and cannot enter pure evaluation |

Copying a clock with `~clock` forks an independent timeline. A copied manual
clock starts at the same value; a copied system clock keeps advancing from the
same observed instant. Backward mutation never rewinds a system clock.

A runtime `Duration` is an i64 nanosecond count (D-TIMERES1=A), built with
checked type-owned unit methods such as `Duration.seconds(n)?` or
`Duration.nanoseconds(n)?`. Read a whole unit with `d.in(.Nanoseconds)?` /
`d.in(.Milliseconds)?`; the result truncates toward zero and reports
`RangeError` on overflow. Static unit literals such as `5s` remain unchanged.

| `Duration` method | Returns | What it does |
|-------------------|---------|--------------|
| `in(unit)` | `Int ? RangeError` | Whole nanoseconds, microseconds, milliseconds, seconds, minutes, or hours; truncates toward zero |
| `is_zero()` | `Bool` | Whether the span is exactly zero |
| `total_seconds()` | `Int` | Whole seconds in the span (truncates toward zero) |
| `difference(other)` | `Duration` | This span minus `other` (saturating) |

**Expert escape — `assume_deterministic { … }`.** Inside a `fn … =[]=>`, a block
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

**`core.encoding.csv`** — `parse(text) => [[String]] ? String` (rows of fields),
`to_string(rows) => String`, plus bounded `reader` / `writer` handles over
RFC-4180 records. Quoted fields preserve commas, escaped quotes, and embedded
newlines; malformed quote closure is an error rather than a partial row.
**`core.encoding.toml`** / **`core.encoding.yaml`**
— `parse(text) => TOML ? JSONError` / `YAML ? JSONError` (full adapters over
`DataTree`, not a flat map), `to_string(value)`.

**Ratified Epoch 3 breadth (D-ENCSTREAM1 and follow-ups).** The same `DataTree`
tree backs one whole-value and streaming adapter contract per format:
The exact signatures, defaults/ranges/accounting, tagged XML schemas, error
paths/projections, canonical byte rules, strict decoder matrices, lifecycle,
test vectors, and edition migrations are normative in
[`../spec/encoding-decisions.md`](../spec/encoding-decisions.md).

| Module | Surface | What it does |
|--------|---------|--------------|
| `core.encoding.json` | `canonical` (2026 prototype / 2027 JCS+limits), `reader`, `writer` | Edition-split whole-value canonical; pull `DataEvent` streaming; shipped `events(DataTree) => String` remains separate until migration |
| `core.encoding.jsonl` | `parse(text)`, `to_string(rows)` | JSON Lines over `[DataTree]` |
| `core.encoding.csv` | `parse(text)`, `decode<T>`, `to_string(rows)`, `reader`, `writer` | Whole-value and bounded pull records over the same CSV quoting and validation law |
| `core.encoding.xml` | `parse`, `parse_bytes`, `decode<T>`, `decode_bytes<T>`, `root`, `expanded_name`, `attribute`, `content`, `to_string`, `to_bytes`, `canonical`, `reader`, `writer` | Exact tagged ordinary-`DataTree` tree/events with namespaces, token-local lexical evidence, safe entities/limits, W3C C14N, and D-ENCXML-PROJECTION1=A typed helpers |
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

**Current implementation boundary:** JSONL, the lossless tagged XML engine and
pull handles, base32/base64url, and edition-split `json.canonical`
(edition 2026: infallible prototype bytes; edition 2027: fallible RFC 8785 JCS)
exist. XML whole and stream parsing enforce the exact XML 1.0 Fifth Edition
`Char` production for literal scalars and numeric references, with identical
typed errors across every byte split. XML attribute and namespace values apply
XML 1.0 line-end and whitespace normalization, including explicit general-entity
replacement text, while numeric references remain literal and lexical tokens
remain exact for preserving writers in comptime, AOT, and dev. D-ENCXML-PROJECTION1=A
ships `xml.decode`/`decode_bytes` plus `root`/`expanded_name`/`attribute`/`content`
over that tree. CBOR's
typed whole-value byte verbs, closed errors/options, native `[U8]`, original-wire
Core deterministic validation, live allocation limits, and normal-mode
indefinite values execute in the native runtime; pull handles also exist. Exact
XML 1.0/Namespaces and inclusive/exclusive C14N corpus closure, XML byte-identity
whole-value verbs, RFC 8785 serialization, strict edition
migration, full hostile standards corpora, complete stream lifecycle proof, and
error-allocation oracles remain open. Entries above state ratified API law, not a
broad-complete implementation claim.

Compiler/runtime codec implementations remain std-only under I6.

Jet has no general `Any` top type (D-DYNAMIC-TYPE1): use the precise shape for
the job — an enum for a closed set of variants, generics or traits for
abstraction, `T?` for absence, and `DataTree` for parsed dynamic input. Writing
`Any` in type position is **E0350**.

### `core.data` — typed tables, series, status, plots

D-DATA-SURFACE1 makes `core.data` the beginner facade for typed tables,
series, stats, CSV/JSON ingest, and plots. D-DATAFLOW1=A adds bounded typed
pull streams (`csv_reader`/`json_reader`), `DataLimits`, and `DataError` for
the current edition: filters and scalar reducers stay streaming, while group,
sort, join, pivot, and collect enforce named ceilings. Invalid analytics
(empty mean/variance, bad quantile, non-finite input, overflow) return `DataError`
instead of silent zeros or clamps. Numeric reducers use population variance
(divide by `n`), Neumaier summation, and collapse signed zero to `+0.0` on
output. Pivot cells use distinct `DataPivotCell` row/column keys.

`data.csv<T>(text)` decodes CSV into `[T]` using the same `#Codable` model as
`core.encoding.csv.decode<T>`. `data.json<T>(text)` decodes a JSON array of objects
into `[T]` via the same Decode path as `core.encoding.json.decode<[T]>`. Selectors are typed
lambdas, so a misspelled row field is a Jet field error before codegen.

| Function | Returns | What it does |
|----------|---------|--------------|
| `csv<T>(text)` | `[T] ? [FieldError]` | Header-mapped typed CSV rows |
| `json<T>(text)` | `[T] ? [FieldError]` | Typed rows from a JSON array of objects |
| `csv_reader<T>(file, limits)` / `json_reader<T>(file, limits)` | `DataStream<T> ? DataError` | Bounded pull over `core.encoding` readers |
| `DataLimits.safe()` | `DataLimits` | Default group/sort/join/output ceilings + `EncodingLimits.safe()` |
| `table(rows)` / `rows(table)` | `Table<T>` / `[T]` | Wrap and unwrap the typed in-memory table model |
| `series(values)` / `values(series)` | `Series<T>` / `[T]` | Wrap and unwrap typed series values |
| `schema(table_or_series)` | `[DataColumn]` | Column names and Jet type names for the row/value model |
| `missing_count(series)` | `Int` | Count absent `T?` values in a typed series |
| `lazy(table)` / `collect(plan)` | `LazyFrame<T>` / `Table<T> ? DataError` | Build a typed plan; execute it only when materialized |
| `lazy_filter(plan, row => ok)` / `lazy_sort_by(plan, row => key)` | `LazyFrame<T>` | Append deferred typed operations without visiting rows |
| `plan(frame)` | `[String]` | Deterministic plan-step names for audit/test output |
| `count(value)` | `Int` | Count rows/values in `[T]`, `Table<T>`, `Series<T>`, or `LazyFrame<T>` |
| `sum(values)` / `mean(values)` / `min(values)` / `max(values)` | `Float ? DataError` | Numeric series stats over `[Float]` (empty mean/min/max are `Empty`) |
| `median(values)` / `quantile(values, q)` | `Float ? DataError` | Sorted numeric quantiles; `q` must be finite in `0.0..=1.0` |
| `variance(values)` / `stddev(values)` / `describe(values)` | `Float ? DataError` / `DataSummary ? DataError` | Population variance/stddev (Welford, divide by `n`); empty is `Empty` |
| `rolling_mean(values, width)` | `[Float] ? DataError` | Rolling window mean; width must be positive |
| `group_count(rows, row => row.key)` | `[DataGroup] ? DataError` | Count rows by a `String` key |
| `group_sum(rows, row => row.key, row => row.value)` | `[DataGroup] ? DataError` | Sum a `Float` selector per key |
| `group_mean(rows\|stream, row => row.key, row => row.value)` | `[DataGroup] ? DataError` | Mean a `Float` selector per key; streams reuse pull limits |
| `filter(rows, row => ok)` / `sort_by(rows, row => key)` | `[T]` / `[T] ? DataError` | Typed in-memory row pipeline |
| `inner_join(left, right, l => key, r => key)` | `[DataJoin<L, R>] ? DataError` | Stable matching row pairs with SQL join multiplicity |
| `left_join(left, right, l => key, r => key)` | `[DataJoin<L, R?>] ? DataError` | Stable row pairs; unmatched left rows carry `None` |
| `pivot_sum(rows, row => row_key, row => col_key, row => value)` | `[DataPivotCell] ? DataError` | Distinct row/column sum cells |
| `status()` | `[DataStatus]` | Native and bridge facts: path, copy, ownership, trust, fallback, replacement |
| `require_bridge(provider)` | `() ? DataError` | Fail closed for unavailable `py` / `r` / `gpu` bridges; never fabricates results |
| `bar_text(groups)` / `bar_svg(groups)` | `String ? DataError` | Deterministic text/SVG bar output; reject negative/non-finite geometry |
| `line_text(groups, options)` / `line_svg(groups, options)` | `String ? DataError` | Deterministic line output with x labels, title, axis labels, markers, optional reference line, style, color, and legend |

`DataStream<T>.next()` returns `T? ? DataError`: clean EOF is stable `None`,
terminal errors latch, and complete rows already returned stay valid. Edition
2026 keeps the prior non-fallible signatures frozen.

Flagship proof for this slice is `examples/features/tooling/data_analysis.jet`
(CSV ingest → filter → sort → join → group → stats → plot → status). The
hostile corpus is `examples/features/tooling/data_hostile.jet`: empty and
missing series, duplicate-key joins, delimiter-like pivot keys, stable sort
ties, non-finite numerics, signed-zero collapse, population variance
(including singleton `0.0`), invalid quantiles and windows, SVG-escaped plot
labels, and tightened `DataLimits` failures. Both ship golden output under
`examples/features/expected/tooling/` and AOT coverage in
`tests/data_hostile.rs`. Strict resident-JIT parity (no AOT fallback) is
covered by `tests/dev.rs`
(`data_pipelines_and_parsing_match_interpreter_jit_and_aot`).

`Table<T>` and `LazyFrame<T>` keep typed rows; `Series<T>` keeps typed values.
`data.schema` returns `[DataColumn]` with `.name` and `.type_name` for each
column of a table/lazy row type, or a single `value` column for a series element
type (including when that element is itself a struct). Empty tables and series
still report the static element model — schema is type-driven, not sample-driven.
Missing values are ordinary Jet optionals (`T?`) inside a series, not a second
sentinel type. `DataGroup` fields: `.key: String`, `.count: Int`, `.sum: Float`,
`.mean: Float`. `DataLineOptions` fields are `.title`, `.x_label`, `.y_label`,
`.markers`, `.reference: Float?`, `.style` (`solid`, `dashed`, or `dotted`),
`.color`, and `.legend`. `DataJoin<L, R>` fields are `.left: L` and `.right: R`; the
left-join form uses `R?`. `DataStatus` fields: `.step`, `.path`, `.copy`,
`.ownership`, `.trust`, `.fallback`, `.replacement`. Bridge rows are separate
`py.*`, `r.*`, and `gpu.*` entries (D-DATA-BRIDGE1); unavailable bridges keep
`path=unavailable` by default and `data.require_bridge` returns
`DataErrorKind.Bridge` with those facts in `.reason` — never a silent
fallback. R becomes `available` only when Rscript is on PATH and the expert
opt-in `JET_DATA_R_BRIDGE=1` is set; Python and GPU stay unavailable until
their binders ship.

```jet
use core.data as data

#Codable
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
Two checked selectors cover language-owned values: `{value#Fixed(n)}` formats
a `Float`, and `{value#Unit(name)}` or `{value#Unit(bare)}` selects a unit style.

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
`set_trace_id`, `setup`, `critical`, `fatal`, `disable`, `flush`, and
`enabled(level)`.

`critical` is a severity above `error`. `fatal` emits then exits the process
with status 1. `disable` suppresses further emission until process end.
`flush` forces the active sink to flush. `enabled(level)` reports whether the
named severity would emit under the active `set_level` threshold.

#### Typed (de)serialization — one derive, every format (D-SERDE1–8)

Mark a type `#Codable` and it crosses the wire in any format. `#Codable` is
both directions; the one-way markers are `#Encode` (write-only) and `#Decode`
(read-only). The derive is compiler-owned (like `derive Comparable`) — no macros,
no runtime reflection.

```jet
use core.encoding.csv as csv
use core.encoding.json as json

#Codable
struct Order {
    id: Int
    #Rename("customer") who: String      // wire key overrides the field name
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

**Encode** — `to_string(v)` / `to_string_pretty(v)` accept any `#Codable`/`#Encode`
value (the dynamic `JSON` tree and the `[[String]]`/`[K: V]` forms still work too). Field
order is preserved.

**Typed decode** — `decode<T>(text)` (D-GENERIC-CALL1; D-SERDE6 owns the codec
model) returns `T ? [FieldError]` for
json/toml/yaml, and `[T] ? [FieldError]` for csv (one struct per row, columns mapped
to fields by header name). The target type comes from the `<T>` turbofish or an
cfg: Config :: json.decode(text)`). Bare `json.decode(text)` with no
target stays the lenient dynamic `JSON` (above). Decode failures carry an
accumulated `[FieldError]` list; each item has a `path` and a `reason`.
Compose it with `??`.

```jet
raw :: "item,qty\npen,3\nink,5"
sales :: csv.decode<Sale>(raw) ?? panic("bad csv")   // [Sale]
print(json.to_string(sales))   // [{"item":"pen","qty":3},{"item":"ink","qty":5}]
```

**Hand codecs and subtree dispatch** (D-SERDE2, D-SERDE13–16) use the same
protocol as built-in derives. Write `impl T.Encode` with `encode(self) =>
DataTree` and `impl T.Decode` with `decode(tree: DataTree) => T ? [FieldError]`.
`.field` and `.at` add their field/index path; scalar accessors leave the path
empty and a containing decoder frames them with `FieldError.under`. All return
`[FieldError]`, so `?` chains without manual mapping. `tree.decode<T>()` dispatches any subtree
through `T`'s ordinary `Decode` implementation, including primitives, user
types, lists, options, and string-keyed maps. A derived parent therefore
composes with a hand-written field codec; generated and hand-written paths are
one mechanism.

```jet
impl Email.Decode {
    fn decode(tree: DataTree) => Email ? [FieldError] {
        address := FieldError.under("address", tree.text())?
        return Ok(Email.{ address })
    }
}

items := tree.field("items")?.decode<[LineItem]>()?
```

**Traced decode — was this migrated?** (D-MIGRATE3=A, D-MIGRATE4=A):
`decode_traced<T>(text)` sits beside `decode<T>` on every codec
(json/csv/toml/yaml share the decode machinery) and returns
`DecodeResult<T> ? [FieldError]` — `{ value: T, migration: MigrationStatus }`.
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

Decoding a `#PublishedSchema` type with `migration { }` blocks (below) runs
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
typed decode failures. Rule expressions are purity-checked (S60/E3401): a `check`'s
condition and message may reference only the struct's own fields and pure
calls, never Net/DB/IO. Derived decoders invoke this validator after shape
decoding; hand codecs opt in explicitly. The `Validate.over(s)` use-site
escape (for rules needing outside context, like a database lookup) remains
follow-on work — see docs/spec/syntax-decisions.md's D-VALIDATE1 entry.

**Field attributes** (D-SERDE5):

| Attribute | Effect |
|-----------|--------|
| `#Rename("k")` | use `k` as the wire key for this field |
| `#Skip` | never serialize; on decode use the field's default |
| `#Default` / `#Default(8080)` | when the key is absent, use the type's default (or the given literal) |
| `#Flatten` | inline a `#Codable` struct field's keys into the parent object |

**Container attributes:**

| Attribute | Effect |
|-----------|--------|
| `#RenameAll(camel)` | map every field's wire key — `camel`/`snake`/`pascal`/`kebab`/`screaming` (D-SERDE3) |
| `#DenyUnknownFields` | a wire key the struct doesn't declare is an error, not ignored (D-SERDE8) |
| `#Discriminant("type")` / `#Untagged` | enum wire representation (D-SERDE7); default is externally tagged |

**Enums** serialize externally tagged by default: a unit variant is its bare name
(`"Closed"`), a payload variant is `{"Variant": payload}`. `#Discriminant("type")` switches
to internal tagging (`{"type":"Click", …}`); a single unnamed payload uses the
canonical `value` key (`{"type":"Count","value":7}`). `#Untagged` emits the
payload alone.

Unknown wire keys are ignored by default (forward-compatible); opt into strict
checking with `#DenyUnknownFields`. Diagnostics: E2407 (`#Rename` non-string),
E2408 (`#Flatten` non-struct), E2409 (bad `#RenameAll` style), E2410 (missing
required field, runtime), E2411 (type isn't serializable — also fires at the use
site for a non-codable generic argument), E2412 (unknown field, runtime). E2413 is
retired (D-SERDE12).

Generic `#Codable` is first-class (D-SERDE9-12): the derive auto-injects
`T: Encode`/`T: Decode` bounds on exactly the type params that reach the wire —
the user never spells them. A phantom or `#Skip`-only param carries no serde
bound (only structural `Clone`), so `Id<Kind>` serializes for any `Kind`. A
non-codable type argument fails at the use site (E2411), not the definition.

The expert hand-impl path is live: `impl T.Encode { fn encode(self) => DataTree
{ … } }` and `impl T.Decode { fn decode(tree: DataTree) => T ? [FieldError] {
… } }`. Generated and hand-written codecs use the same protocol dispatch.

---

### `core.tasks` — tasks and channels

Blocking tasks and typed channels are Jet's concurrency model. There is no
`async`/`await` and no mutex API; tasks communicate by sending owned values.

```jet
use core.tasks as tasks

fn sum_range(first: Int, last: Int) => Int {
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
    task :: tasks.spawn(() => {
        sender.send(42)
    })
    task.join()
    print(ch.receive() ?? panic("channel closed"))
}
```

`tasks.channel<T>()` returns the send/receive pair directly (D-TUPLE-DESTRUCT1) —
destructure it with `(tx, rx) := tasks.channel<T>()`. `tasks.channel<T>(capacity:
N)` creates a bounded channel; `send` parks when the queue already holds `N`
values and resumes when a receiver drains space. A second sender is `~tx`
(D-SHAPE-COPY1's copy sigil makes a cheap handle duplicate;
there's no combined channel value).

| Function / type | Returns | What it does |
|-----------------|---------|--------------|
| `tasks.spawn(lambda)` | `Task<T>` | Run a zero-parameter lambda on a new task |
| `tasks.join_all(handles)` | `[T]` | Consume `[Task<T>]`, wait in list order, and return results in that order |
| `tasks.wait_any(handles)` | `T` | Consume `[Task<T>]` and return the first finished result |
| `tasks.yield_now()` | nothing | Cooperative yield at a scheduler wait point (`yield` is the stream keyword) |
| `tasks.current_task()` | `String` | Control-plane trace of the running task (`paused=...,cancel=...`) |
| `task.join()` | `T` | Wait for the task and consume the task handle |
| `task.wait()` | `T` | Alias of `.join()` |
| `task.pause()` | nothing | Request paused state on the task control plane (D-COROUTINE1) |
| `task.resume()` | nothing | Clear paused state on the task control plane |
| `task.cancel()` | nothing | Request cancellation on the task control plane |
| `task.trace()` | `String` | Read control-plane state as `paused=...,cancel=...` |
| `task.exception()` | `String` | `"cancelled"` after cancel; otherwise `""` |
| `tasks.channel<T>()` | `(Sender<T>, Receiver<T>)` | Create an unbounded linked send/receive pair |
| `tasks.channel<T>(capacity: N)` | `(Sender<T>, Receiver<T>)` | Create a bounded pair with real backpressure |
| `tasks.after(ms: N)` | `Receiver<Unit>` | One-shot timer channel |
| `tasks.after(ms: N, value: fallback)` | `Receiver<T>` | One-shot typed timer channel for timeout values |
| `tasks.interval(ms: N)` | `Receiver<Int>` | Interval timer channel sending `1`, `2`, ... |
| `~sender` | `Sender<T>` | Create another send half with the copy sigil |
| `sender.send(value)` | nothing | Move one value into the channel |
| `sender.close()` | nothing | Close the send half explicitly |
| `receiver.receive()` | `T ? Closed` | Block for a value, or return `Closed` when senders are gone |
| `receiver.close()` | nothing | Close the receive half explicitly |

Values crossing `spawn` or `send` must be sendable: no `View<T>` or string-view
windows, no trait values, and no closure values with non-sendable captures.
Copyable captures copy automatically; owned non-copyable captures move. A
`Task` that goes out of scope without
`.join()` emits warning **L1101**.
With `#Context(deadline: <Int epoch_ms>)`, blocking waits (`task.join()` /
`task.wait()` / `ch.receive()` / `sender.send()` / `time.sleep`, TCP read/write,
and `ProcessChild.wait()`) observe the inherited budget and report runtime
**E3003** on exceed. Task cancellation wakes the same scheduler wait points.

Use `tasks.join_all([first, second])` when code already owns free task handles
and needs every result in handle-list order. The list and each handle are
consumed. `taskgroup` remains the structured default: it owns child tasks until
scope exit. Inside one, use `g.all`, `g.race`, and `g.any`; `race`/`any` cancel
losers. `g.select()` races receivers and timers: `.recv(rx)` waits for a channel
value, `.after(ms: N)` is a unit timer arm, and `.after(ms: N, value: fallback)`
is a typed timeout arm that can be mixed with same-`T` receive arms.

### `core.testing` — fixtures under `#Test`

D-TESTKIT1 keeps `#Test` as the only test syntax. `core.testing` is a helper
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
`fake_rng`. Use `expect(value).snapshot()` inside `#Test`
blocks for assertion snapshots; `testing.snap` is for explicit named files.

Benchmark limits use a `#Bench` region plus a typed `Budget` declaration. The
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

`jet fuzz <file> [<test-name>]` fuzzes a parameterized `#Test fn` (the same
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
| `re.escape(text)` | `String` | escape metacharacters for a literal match |
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
| `rx.pattern()` / `rx.source()` | `String` | raw pattern text |
| `rx.flags()` / `rx.options()` | `String` | active flag letters (`i`/`m`/`s`) |
| `rx.names()` | `[String]` | named capture group names |
| `rx.count(text)` | `Int` | number of non-overlapping matches |
| `rx.replace_all_with(text, fn(Match) => String)` | `String` | replace every match with callback output |
| `mat.group(n)` | `String?` | capture group `n` of a `Match` |
| `mat.name(name)` | `String?` | capture group by name |
| `mat.named_captures()` | `[[String]]` | named groups as `[name, value]` pairs |
| `mat.start()` / `mat.end()` | `Int` | byte span of the whole match |
| `mat.group_start(n)` / `mat.group_end(n)` | `Int?` | byte span of a capture |

Note: `{N}` quantifiers must be written `{{N}}` in Jet source — single braces
are string interpolation (S8). Write `\\d{{4}}` for "four digits".

`core.regex` has no external dependency and does not create a hidden FFI bridge.

---

### `core.ui` — one typed tree across rendering backends

`core.ui` keeps component meaning in one `UiNode` tree. The beginner
constructors are `ui.text`, `ui.button`, and `ui.box`; null, TUI, browser DOM,
and Linux GTK consume that same tree for measurement, paint, event routing,
focus order, and accessible names.

```jet
use core.ui as ui

fn run() {
    tree :: ui.box([
        ui.text("Flight deck"),
        ui.button("Boost fuel"),
    ])
    backend :: ui.tui_backend()
    ui.mount(backend, tree)
}
```

| Call | Returns | Does |
|------|---------|------|
| `ui.text(text)` | `UiNode` | static text with a label role and accessible name |
| `ui.button(label)` | `UiNode` | keyboard-focusable button node |
| `ui.button(label, on_click: handler)` | `UiNode` | same button with a portable click handler (D-WEB-CLICK-PORT1=D); GTK, DOM, and TUI bind by node identity (D-UI-NODE-ID1=C / D-UI-EVT-DISP1=E). Unsupported backends may register the handler without ever firing it. |
| `ui.box(children)` | `UiNode` | vertical container for one typed child list |
| `ui.node(label, width, height)` | `UiNode` | low-level custom/decorative node |
| `ui.node_role(label, width, height, role)` | `UiNode` | low-level node with an explicit role |
| `ui.node_color(label, width, height, color)` | `UiNode` | styled text node with a `#RRGGBB` fill and accessible name |
| `ui.null_backend()` / `ui.tui_backend()` | backend | in-memory/DOM-selected or terminal renderer |
| `ui.gtk_backend()` | `GtkBackend` | Linux GTK4 renderer; needs a real display unless `JET_UI_HEADLESS=1` |
| `ui.mount(backend, tree[, constraint])` | — | one-call measure → layout → paint (D-UI-MOUNT1=A); default viewport is backend-sized |
| `backend.measure/layout/paint(...)` | mixed | expert stages behind the mount pipeline |
| `backend.on_event(ui.key_event("Tab"))` | `EventResult` | advance the backend's interactive focus order |
| `ui.reactive_render(() => { ... })` | — | repaint from signals read by the body |

Backend capability is explicit rather than silently emulated:

| Backend | Tree / layout / paint | Focus + accessible names | Native window | Hot reload / package |
|---------|-----------------------|--------------------------|---------------|----------------------|
| Browser DOM | yes | yes | browser-owned | `jet dev` / web build |
| TUI | yes | yes | terminal-owned | no / terminal binary |
| Linux GTK4 | yes | yes | yes | no / native binary |
| Null/in-memory | yes | deterministic test model | no | no |
| macOS, Windows, iOS, Android | unsupported | unsupported | unsupported | unsupported |

An unavailable GTK display reports `UI_UNSUPPORTED` instead of silently
pretending to render. `JET_UI_HEADLESS=1` is the explicit CI/test opt-in. The
other native/mobile rows are deliberately reported as unsupported until real
backends, accessibility-tree proof, and packaging exist.

**Portable click (D-WEB-CLICK-PORT1=D).** `ui.button(label, on_click: …)` stores
a handler slot on the node. At paint/mount, each backend binds that slot to a
stable identity: author `key` when set, otherwise the render path
(D-UI-NODE-ID1=C). A click looks up the slot in O(1) and runs it
(D-UI-EVT-DISP1=E). Only click/activate is portable; hover and other rich
events require an explicit capability module (D-UI-EVT-SET1=D). GTK, DOM, and
TUI share this mechanism — there is no second click API (I8).

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
  and again whenever a signal it read changes, and returns an `Effect`. Call
  `.unsubscribe()` to detach it idempotently and `.is_active()` to inspect its
  state. Dropping the final handle detaches it too. **`#Reactive { … }`** (D-REACTCORE1)
  creates the same effect with a runtime-owned lifetime.
  **`#Reactive fn`** wraps the whole function body the same way (unit return only).

Dependency tracking is **explicit-by-read**: any `.get()` evaluated inside a
derived or effect body subscribes that derived/effect to the signal. A `.set(v)`
re-runs every subscriber. Each re-run replaces the prior dependency set, so a
conditional effect stops listening to signals from branches it no longer reads.

```jet
use core.reactive as reactive

fn run() {
    price :: reactive.signal(100)
    qty :: reactive.signal(2)
    total :: reactive.derived(() => (price.get() * qty.get()))
    print(total.get())                       // 200

    subscription := reactive.effect(() => {    // prints 200 now
        print(total.get())
    })
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
| `reactive.effect(() => { … })` | `Effect` | a retained side effect with explicit lifecycle |
| `effect.unsubscribe()` | — | detach idempotently |
| `effect.is_active()` | `Bool` | whether the effect remains subscribed |
| `#Reactive { … }` | `Effect` (runtime-owned) | explicit reactive effect scope |
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

#Target(JS)
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

### `Cell<T>` — local interior mutability

`Cell<T>` stores private state that read-only code can update on one task.
It uses no `Arc` and no operating-system lock. Use `Shared<T>` when state must
cross a task, task group, channel, or parallel adapter.

```jet
cache := Cell.new(0)
cache.set(1)
old :: cache.replace(2)
current :: cache.get()
cache.edit(value => value += 1)
```

| Method | Returns | What it does |
|--------|---------|--------------|
| `Cell.new(value)` | `Cell<T>` | Create a local cell and infer `T` |
| `cell.get()` | `T` | Copy the current value; `T` must support Jet's copy law |
| `cell.set(value)` | nothing | Replace the value |
| `cell.replace(value)` | `T` | Replace the value and return the old value |
| `cell.get_or_set(init)` | `T` | Initialize an empty `Cell<T?>` once and copy the value |
| `cell.read(value => result)` | `R` | Run a closure under one read loan |
| `cell.edit(value => result)` | `R` | Run a closure under one edit loan |
| `cell.guard_read()` | `CellReadGuard<T>` | Keep a read loan across calls |
| `cell.guard_edit()` | `CellEditGuard<T>` | Keep an edit loan across calls |
| `guard.map(project)` | projected guard | Keep the same loan for one projection |
| `guard.split(first, second)` | two projected guards | Share the same loan across two disjoint field projections |

Many read guards can coexist. One edit guard excludes all other guards.
Mapped and split guards release the original loan only after the last derived
guard drops. Runtime conflicts stop with `Cell borrow conflict`. Use
`cell.read(value => result)` when `T` does not support copying.

A function can pass or return a guard directly. Named tuples can contain guards
recursively, which lets split guards cross a named helper boundary. A guard
cannot be stored in a user struct, enum, list, fixed list, map, `Option`,
`Result`, `Shared`, another `Cell`, a union, or a lambda. Keep it in a
local name or tuple and use `map` or `split` for projections.

---

### `core.mem` — arenas and regions

Expert-tier explicit allocators, unlocked by `use core.mem` (no `#Unsafe`
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
and `mem.volatile_write(p, value)` require an audited `#Unsafe("reason")` region.

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
    #Region(scratch) {
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

`take_pattern` reuses the `[U8].{"…{hole:U<width>}…"}` binary-pattern grammar
(D-BINPAT1) in consume mode — the byte-mode sibling of `Cursor.take_pattern`
above: it matches a *prefix* of the remaining bytes and returns the typed
holes, advancing the reader past them so more reads can follow. A miss is an
ordinary error value.

```jet
fn run() {
    header: [U8] :: [0x45, 0x00, 0x00, 0x28]
    r :: Reader.over(header)
    h :: r.take_pattern([U8].{"{version:U4}{ihl:U4}{tos:U8}{len:U16be}"}) ?? panic("bad header")
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
| `r.take(n)` | `[U8] ? String` | Next `n` bytes (`n`: `Int`, `U8`, `U16`, or `U32`; sized lengths widen internally, while `U64` stays explicit) |
| `r.take_pattern([U8].{"…{h:U<w>}…"})` | `(holes…) ? String` | Match + consume a prefix; literal pattern only |
| `r.remaining()` | `Int` | Bytes left |
| `r.is_at_end()` | `Bool` | Position at buffer end |

`examples/features/parsing/binary-reader.jet` is the golden example.

---

## Numeric surface (D-NUMOPS1)

`Int` and `Float` are the beginner defaults (64-bit: `Int` = `I64`, `Float` =
`F64`). The explicit-width menu — `I8 I16 I32 I64 U8 U16 U32 U64 F32 F64` — is
available for expert and FFI/binary work. `I64` and `F64` are the explicit
names for `Int` and `Float`. A bare whole-number literal adopts a fixed-width
peer that contains its value (D-INTLIT-WIDTH1=F). Without a sized peer it stays
`Int` (D-NUMLIT-PEER1=A). A destination-owned literal is range-checked at
compile time; a value that does not fit is **E1003**.

One numeric widening law applies to operators, arguments, returns, and
assignments. One value can widen to the other type when that type contains
every source value. Jet does not search for a third type and never narrows
implicitly. `F32` widens to `Float`. Small integer types widen to a float when
the crossing is always exact: `I8 I16 I32 U8 U16 U32` to `Float`, and
`I8 I16 U8 U16` to `F32`. Other integer-to-float crossings check exactness at
runtime and trap before rounding. `approx(value)` accepts possible precision
loss for one crossing. Incomparable operator types are **E0109**; invalid
destination types are **E0112** or **E0108**. The sized types erase to their
Rust equivalents (`u8`…`i64`, `f32`) at codegen, so they cross the C ABI by
value (S59). Explicit narrowing uses destination-owned named methods.

Plain integer arithmetic (`+` `-` `*` `/`) **traps on overflow** at every width —
a result outside the type's range stops the program with a Jet panic instead of
silently wrapping. Opt a single op out at the use site:

```jet
fn run() {
hi: U8 :: 200
lo: U8 :: 100
    print(wrapping(hi + lo))            // 44   — wraps around (C behaviour)
    print(saturating(hi + lo))          // 255  — clamps to the type's range
    print(checked(hi + lo) ?? 0)        // 0    — checked(…) => T?, None on overflow
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

**Width conversions** use one safe widening law and destination-owned named
methods for explicit narrowing. Widening is implicit when the destination
contains every source value. Integer-to-float crossings that cannot be proved
exact are checked at runtime; `approx(value)` accepts possible precision loss
for one crossing. Narrowing is never implicit.

| Method | Returns | Direction |
|--------|---------|-----------|
| `U8.from_int(n)` / `I16.from_int(n)` / … (narrowing) | `T ? String` | fallible (`?`/`??`) |
| `F32.from_float(n)` | `F32 ? String` | fallible (finite F32 range) |

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
surface and share one Prelude path for AOT, default `jet run` (Cranelift), and
interpreter ambient (I9). Example: `examples/features/net/socket_echo.jet`
(TCP/UDP/Unix listen+echo). Linux is the Epoch 3 proof platform; macOS/Windows
native socket execution remains Epoch 9. AOT emission pulls Process helpers
whenever FS runtime is needed so subprocess-backed net fixtures link cleanly.
On Unix, TCP, UDP, and Unix-socket operations park through the shared
scheduler readiness backend and observe task cancellation and available
`#Context` deadlines. Windows IOCP lifecycle and platform proof remains #527.
Beginner calls accept strings; expert calls accept typed
`IPAddr` / `SocketAddr` values.

| Function | Returns | Notes |
|----------|---------|-------|
| `ip_addr(text)` | `IPAddr ? NetError` | Parse IPv4/IPv6 |
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
| `nodelay(stream)` / `set_nodelay(stream, enabled)` | `Bool ? NetError` / `() ? NetError` | TCP_NODELAY get/set |
| `ttl(stream)` / `set_ttl(stream, hops)` | `Int ? NetError` / `() ? NetError` | IP TTL get/set |
| `socket_type(stream)` | `String` | Always `"stream"` for TCP |
| `sendfile(stream, path)` | `Int ? NetError` | Copy file bytes onto the stream (observable sendfile; not sendfile(2)) |
| `dns_ptr(name, ms)` | `[String] ? NetError` | PTR reverse lookup |
| `getservbyname(name)` / `getservbyport(port)` | `Int ? NetError` / `String ? NetError` | Embedded well-known service table |
| `udp_bind(addr)` / `udp_bind_addr(addr)` | `UdpSocket ? NetError` | Datagram sockets |
| `udp_local_addr(socket)` | `SocketAddr ? NetError` | Typed local address |
| `udp_set_timeout(socket, ms)` | `() ? NetError` | Persistent read/write deadline budget; earliest ambient deadline wins |
| `socket.ready(.Read/.Write/.ReadWrite, deadline: Duration)` / `socket.close()` | `NetReady ? NetError` / `() ? NetError` | Same UDP handle readiness and idempotent lifecycle |
| `udp_send_bytes_to(socket, bytes, addr)` | `Int ? NetError` | Send one arbitrary-byte datagram |
| `udp_receive(socket, limit)` | `UDPPacket ? NetError` | Full datagram receive with bounded returned payload |
| `socket.send_to(bytes, addr, deadline: Duration)` / `socket.receive(limit, deadline: Duration)` | `Int ? NetError` / `UDPPacket ? NetError` | Datagram-preserving per-call deadline overrides |
| `udp_packet_bytes/address/original_len/truncated(packet)` | `[U8]` / `SocketAddr` / `Int` / `Bool` | Packet data, source, wire length, and truncation fact |
| `unix_listen(path)` / `unix_connect(path)` | `UnixListener ? NetError` / `UnixStream ? NetError` | Unix-domain sockets where supported |
| `unix_accept(listener)` | `UnixStream ? NetError` | Accept one Unix stream; scheduler-aware cancellation and deadlines |
| `listener.accept(deadline: Duration)` | `UnixStream ? NetError` | Same-listener per-call deadline override |
| `unix_read_bytes(stream, limit)` / `unix_write_all_bytes(stream, bytes)` | `[U8] ? NetError` / `() ? NetError` | Unix byte stream operations; same deadline/close law as TCP |
| `unix_shutdown(stream, how)` / `unix_close(stream)` | `() ? NetError` | Explicit shutdown and idempotent close |
| `stream.set_timeout(Duration)` / `stream.read(limit, deadline: Duration)` / `stream.write_all(bytes, deadline: Duration)` / `stream.ready(interest, deadline: Duration)` / `stream.close()` | matching stream results | Same-handle Unix persistent/per-call deadlines, readiness, and lifecycle |
| `dns_a(name, ms)` / `dns_aaaa(name, ms)` | `[IPAddr] ? NetError` | System resolver config, timeout in ms |
| `dns_txt(name, ms)` | `[String] ? NetError` | TXT records |
| `dns_ptr(name, ms)` | `[String] ? NetError` | PTR reverse lookup |
| `dns_srv(name, ms)` | `[DNSSrv] ? NetError` | SRV records |
| `dns_*_at(server, name, ms)` | same as matching lookup | Expert override for a specific DNS server |
| `dns_srv_target(srv)` / `dns_srv_port(srv)` | `String` / `Int` | Inspect SRV records |

`NetError` has stable variants for input, permission, address, connection,
closed, timeout, cancellation, unsupported, DNS, TLS, protocol, and other OS
failures. `error_operation/address/name/message/os_code` expose portable control
and audit data. Raw OS text is never control-flow law. Linux is the Epoch 3
proof platform for TCP/UDP/Unix/DNS/TLS/happy-eyeballs (`tcp_connect_happy`);
native macOS/Windows hostile-matrix execution is deferred to Epoch 9 with the
same Prelude symbols (I9). Examples: `examples/features/net/socket_echo.jet`,
`examples/features/net/dns_lookup.jet`.

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
| `ClientConfig.default().with_alpn(protocols)` | `TLSClientConfig ? IOError` | Validate and offer ALPN protocols before any stream is consumed, without disabling verification |
| `RootCertificates.from_pem(bytes)` | `RootCertificates ? IOError` | Validate a custom PEM root bundle before any network use |
| `ClientIdentity.from_pem(cert_chain: bytes, private_key: bytes)` | `ClientIdentity ? IOError` | Validate one PEM identity chain and matching PKCS#8, PKCS#1, or SEC1 private key; key bytes are secret and wiped on drop |
| `config.with_trust(policy)` | `TLSClientConfig ? IOError` | Select `.System`, `.SystemPlus(roots)`, or `.CustomOnly(roots)` on a new immutable config |
| `config.with_client_identity(identity)` | `TLSClientConfig ? IOError` | Add a validated mTLS client identity on a new immutable config |
| `config.with_version_bounds(min: version, max: version)` | `TLSClientConfig ? IOError` | Select inclusive `.Tls12` / `.Tls13` bounds; reversed bounds fail before network use |
| `client(stream, server_name)` | `TLSStream ? NetError` | Consume the `TcpStream`; verify the server name with system roots; preserve its deadline budgets |
| `client(stream, server_name: name, config: config, deadline: duration)` | `TLSStream ? NetError` | Use the explicit client configuration and earliest handshake deadline on the same consumed stream |
| `read(stream, limit)` / `read_text(stream)` | `[U8] ? IOError` / `String ? IOError` | Scheduler-aware byte or checked-text read; empty bytes mean clean EOF |
| `write(stream, bytes)` / `write_all(stream, bytes)` | `Int ? IOError` / `() ? IOError` | Scheduler-aware partial or complete byte write |
| `write_text(stream, text)` | `() ? IOError` | Write the complete text payload |
| `close(stream)` | `() ? IOError` | Send close-notify; repeated close is harmless |
| `stream.read(limit, deadline: Duration)` / `stream.write_all(bytes, deadline: Duration)` / `stream.ready(interest, deadline: Duration)` / `stream.close()` | matching stream results | Same TLS handle, explicit per-call deadlines, readiness, and close-notify lifecycle |
| `stream.peer_identity()` | `TLSPeerIdentity` | Retained verified name plus immutable exact-DER wire-order chain; leaf exposes DER, certificate/SPKI SHA-256, DNS SANs, validity milliseconds, subject, and issuer |
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
`db.open_memory()`. Queries use one path: SQL text plus `[DBValue]` parameters.
Checked `SQL` literals feed that path through `db.params(sql)`, so holes become
bound parameters, not string interpolation. The runtime uses SQLite's prepared
statement cache under that same path; there is no separate unsafe raw-query or
prepare-only API.

`SQL.{"…"}` is the checked query form: literal segments stay in the template and
each `{hole}` becomes one bound parameter. `HTML.{"…"}` escapes each hole before
inserting it, and `Sh.{"…"}` makes each hole one argv item without shell word
splitting. A runtime `String` cannot become one of these types; the explicit
`.raw(...)` constructors are the audited escape for already-reviewed text.

D-DBDRIVER1=A / D-DBPOLICY-BIND1=A: `Driver` is the backend-neutral trait for
that parameterized surface (`query` / `query_one` / `execute` / `begin` /
`commit` / `rollback`). `DBConnection` opens the connection and establishes a
typed `DBScope`; only the scope implements row operations. Call sites can take
`T: Driver` without naming SQLite, while the policy and user stay attached to
the capability. Cleanup stays on `Close` via `close(...)`.

| API | Returns | Notes |
|-----|---------|-------|
| `db.policy(table, expression)` | `RowPolicy ? String` | Closed policies: `true` or `owner == user` |
| `conn.with_policy(policy, user)` | `DBScope` | Binds the policy and identity; the raw connection has no row operations |
| `scoped.execute(sql, params)` | `Int ? DBError` | Affected row count, with the policy applied; schema/control SQL belongs to `db.migrate` or explicit transaction controls |
| `scoped.query(sql, params)` | `[Row] ? DBError` | `Row` is `Map<String, DBValue>`; returned rows are scoped |
| `scoped.query_one(sql, params)` | `Row? ? DBError` | First allowed row, if any |
| `scoped.live(sql, params)` | `LiveQuery ? DBError` | The same policy is applied to the live-query read |
| `scoped.begin()` / `commit()` / `rollback()` / `close()` | `Bool` | Explicit transaction control through the same scope |
| `db.row_int(row, key)` / `row_float` / `row_text` / `row_bool` | `T ? String` | Typed column read with missing/type errors |
| `db.transaction(scoped, label, statements)` | `Int ? DBError` | Runs scoped statements in one transaction, rollback on first error |
| `db.migrate(scoped, name, statements)` | `Int ? DBError` | Records migration checksum in `__jet_migrations`; rerun returns `0`, changed checksum errors |

`DBValue` variants are `Null`, `Int`, `Float`, `Text`, and `Bool`.

---

## `core.compute` — Tensor storage and backend receipts

D-COMPUTE1=D / D-COMPUTE-TYPE1=D / D-COMPUTE-PLACE1=D: `core.compute` owns one
ranked Tensor operation family. Tensor views retain the owner allocation and
strides; `vec` and `matrix` are rank-1 / rank-2 aliases over that substrate.
This slice registers one CPU capability. Its receipt records backend, version,
profile, cache, and closed capabilities. `Auto` selects that CPU capability,
records the choice, and never fabricates an accelerator or changes precision.
Experts can pin CPU explicitly.

```jet
use core.compute as compute

fn run() {
    t :: compute.zeros([2, 3]) ?? panic("zeros")
    print(compute.rank(t))
    u :: compute.ones([2, 3]) ?? panic("ones")
    s :: compute.add(t, u) ?? panic("add")
    print(compute.to_list(s))
}
```

| API | Result |
|-----|--------|
| `zeros` / `ones` / `full` / `from_list` | `Tensor ? ComputeError` |
| `vec` / `matrix` | rank-1 / rank-2 `Tensor` aliases |
| `add` / `mul` / `sub` / `div` / `maximum` / `minimum` | elementwise (broadcasting) |
| `matmul` / `reshape` / `broadcast_to` / `transpose` | shape ops |
| `negate` / `abs` / `exp` / `log` / `sqrt` | unary ufuncs |
| `sum_axis` | reduce one axis |
| `eye` / `det` / `inv` / `solve` / `fft` | dense linalg + DFT |
| `to_sparse` / `sparse_mv` / `sparse_nnz` | CSR sparse view over dense |
| `value_and_grad_mul` / `jvp_*` / `vjp_*` / `grad_*` | reverse default + composable JVP/VJP |
| `mse_loss` / `sgd_step` / `serialize` / `deserialize` | ML step + tensor bytes |
| `matmul_f32_tile` / `profile_show` | CPU-SIMD profile vs oracle |
| `stream_new` / `transfer` / `kernel_bounds_ok` | stream, transfer, and checked bounds |
| `get` / `set` | indexed access (`set` takes `&Tensor`) |
| `shape` / `rank` / `numel` / `to_list` | inspection |
| `device` / `placement` / `on_device` / `device_cpu` / `device_auto` | placement receipts |

Semantics live only in `crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs`.
AOT emit, JIT deopt, and interpreter ambient call those same `jet_compute_*`
symbols (I9). No accelerator provider is registered in this slice. Requests
that need one return typed `ComputeError::Unsupported`; no engine substitutes
an accelerator.

Tensor serialization is the canonical wire `shape=axis,...;data=value,...`.
The serializer uses shortest round-tripping finite f64 text. The decoder
rejects duplicate or unknown fields, non-canonical axes or values, non-finite
data, and storage-length mismatches before constructing a Tensor.

`D-COMPUTE-KERNEL1=D` / `D-COMPUTE-KERNEL-SURFACE1=B`: `#Kernel(.parallel) fn`
is the explicit safe-kernel declaration. Sema accepts the marker only after proving the
current conservative kernel subset: read-only parameters, no reachable
effects or opaque calls, straight-line control flow, and checked Core compute
operations. The resulting bounds/alias/capture/race/barrier/control proof is
attached to TIR and carried unchanged by AOT, default `jet run`, and the
interpreter. Unsupported indexed writes, loops, captures, and provider calls
are rejected; they do not silently fall back to an unproved kernel.

`D-COMPUTE-AUTODIFF1=D`: reverse-mode VJP is the scalar-loss default;
`jvp_*` and `vjp_*` are composable explicit transforms. Tangent/cotangent
shapes are checked, broadcast gradients reduce to the input shape, and
`value_and_grad_mul` rejects non-scalar outputs. Gradient receipts inherit the
primal placement and profile.

`D-COMPUTE-BACKEND1=D`: the registered `cpu-oracle` publishes a stable backend,
version, profile, cache identity, and closed capability list in placement
receipts. General operations report their actual `F64Strict+Reproducible`
profile; the tiled path reports real `F32Strict+Reproducible` arithmetic and
ordered reduction. The ratified production profile and provider capabilities
remain gated, and unsupported requests fail before launch.

`D-COMPUTE-RAWBOUNDARY1=A` (ratified 2026-08-03): raw kernel boundaries use a
provider-issued opaque contract. The CPU module exposes no public contract
constructor because a reason and arity cannot prove address spaces, read/write
sets, effects, races, or barriers. A provider-issued typed `#Unsafe` boundary
proof is required before raw code can be launched. The first such provider
belongs to the accelerator-provider work outside Epoch 3.

Backend facts for Core modules (ownership/effects/failure/platform) live in
[core-backend-facts.md](core-backend-facts.md).

---

## `core.services` — service trees and mailboxes

D-SERVICE1=D / D-SERVICE-DELIVERY1=D / D-SERVICE-STATE1=D /
D-SERVICE-WORKFLOW1=D / D-SERVICE-IDENTITY1=D / D-SERVICE-UPGRADE1=D: typed
service trees over the existing task/channel model. Workers own bounded
mailboxes; delivery defaults to at-most-once with `Full` under capacity;
`send_durable` requires DurableAtLeastOnce plus an idempotency key; restart
policy defaults to OneForOne (also OneForAll / RestForOne); state adapters are
Empty / Snapshot / EventLog; workflows, directory identity, and generation
handoff/rollback are first-class.

`send_durable` remains the bounded in-memory tree path and returns
`Unit ? ServiceError`. Durable delivery uses the explicit `ServiceRuntime`
authority selected by `D-SERVICE-AUTHORITY1`. Its append-only store commits
before returning a typed receipt, and reopening the same store reconstructs
idempotency, retention, retry, and dead-letter state.

```jet
use core.services as services
use core.time as time

fn run() {
    runtime := services.runtime("orders.log", retention: time.hours(24))
    receipt := runtime.send(order_endpoint, order, key: order.id)?
    if receipt == {
        .Accepted(id) -> audit(id)
        .Duplicate(_) -> continue
        .Retained(_, until) -> schedule_retry(until)
        .DeadLettered(_) -> report("dead letter")
        .Rejected(reason) | .Unavailable(reason) | .Partitioned(reason)
        | .Revoked(reason) | .Stale(reason) | .Expired(reason) -> report(reason)
    }
}
```

`ServiceRuntime` is the only durable authority. `send`, `retry`,
`dead_letter`, `retain`, and `commit` use the same Prelude implementation in
AOT and ambient execution; `commit(id)` durably acknowledges a delivered
receipt and removes its pending copy. An uncommitted receipt can be recovered
by `retry(id)` after a process restart. The ordinary tree remains the bounded
local delivery path.

Snapshot and EventLog state also require an explicit injected authority. The
adapter receives a typed store capability plus its schema and version. The
store is checked before the adapter starts and has the same meaning in AOT and
ambient execution; no process-global path is consulted.

```jet
store := services.state_store("orders.state")?
services.set_state_event_log(&tree, store, "orders", 1)?
```

Generation handoff writes a rollback copy of durable state before switching
endpoints and returns a typed `ServiceUpgradeReceipt` through
`services.upgrade_receipt(tree)`. The receipt binds the old and new generation,
migration class, rollback availability, and pinned shards. Rollback verifies
that receipt, restores the durable copy atomically, and refuses forward-only
state. A stateless handoff records that no state rollback was needed.

```jet
use core.services as services

fn run() {
    tree :: services.tree("app")
    echo :: services.worker(&tree, "echo", 8) ?? panic("worker")
    services.start(&tree) ?? panic("start")
    services.send(&tree, echo, "hi") ?? panic("send")
}
```

---

## Compression and archives

D-CORE-COMPRESS1=A assigns each operation one public home:

| Module | Job | API |
|--------|-----|-----|
| `core.compress.gzip` | gzip byte streams | `compress([U8]) => [U8]`, `decompress([U8]) => [U8] ? String` |
| `core.compress.zstd` | zstd byte streams | `compress([U8]) => [U8]`, `decompress([U8]) => [U8] ? String` |
| `core.archive` | zip/tar containers | `zip_compress`, `zip_decompress`, `tar_add`, `tar_get`, `tar_names_json` |

`core.archive` has no standalone gzip helpers. Compose formats explicitly for
containers such as `tar.gz`: build tar bytes with `core.archive`, then compress
those bytes with `core.compress.gzip`.

---

## Built Core Modules

D-STDLIBLEDGER1 keeps this reference to built modules only. It is not a
have/have-not ledger of missing domains.

D-OPTGC1 selects automatic scoped `#Policy(gc)` as the sole source path. The
collector is compiler-private: user code keeps ordinary bare values and opts in
at package, module, function, or block scope. `jet gc report` identifies the
exact automatic promotion sites to migrate back to ownership.

`core.compiler`, `core.io`, `core.env`, `core.os`, `core.process`, `core.math`, `core.random`,
`core.time`, `core.tasks`, `core.testing`, `core.mem`, `core.mem.alloc`,
`core.solve`, `core.data`, `core.compute`, `core.files`, `core.path`, `core.url`, `core.mime`,
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
`core.web.storage`, `core.web.storage.local`, `core.web.storage.session`, `app`,
`core.sketch.hll`, `core.sketch.tdigest`, `core.sketch.reservoir`,
`core.sketch.cms`, `core.time.date`, `core.time.datetime`,
`core.time.expiring`, `core.http.client`,
`core.http.server`, `core.web.devserver`, `core.vault`.

---

## Writing Core in Jet

The ratified target boundary is a minimal audited intrinsic/ABI kernel plus
ordinary Jet Core packages. `core.archive` crosses that boundary through its
real `archive.jet` source module: the normal frontend checks and emits the
reachable package, and only the package's internal byte-format calls use the
audited Rust ABI kernel. AOT, JIT/dev, interpreter, and applicable web checks
share that source-owned TIR path.

---

## Examples in this repo

| Example | Shows |
|---------|-------|
| `examples/features/tooling/compute_tensor.jet` | `core.compute` Tensor / Vec / Matrix CPU oracle |
| `examples/features/tooling/compute_ndarray.jet` | broadcast, fused elementwise ufuncs, transpose, and axis reduction |
| `examples/features/tooling/compute_device.jet` | placement, stream, transfer receipts |
| `examples/features/tooling/compute_kernel.jet` | safe bounds + raw `#Unsafe` kernel contract |
| `examples/features/tooling/compute_simd.jet` | f32 tiled matmul CPU-SIMD profile |
| `examples/features/tooling/app_live.jet` | live queries + `#Transact` invalidate |
| `docs/reference/framework-transplant-closeout.md` | framework transplant shipped-law ledger |
| `docs/reference/language-shape-conformance.md` | #560 cross-surface conformance ledger |
| `examples/features/io/files.jet` | Read, transform, write with errors |
| `examples/features/serde/json.jet` | Parse, inspect, mutate, re-render JSON |
| `examples/features/io/cli.jet` | Args, environment, exit codes |
| `examples/features/io/cli_args.jet` | `core.args` — flag/option/positional spec + parse |
| `examples/features/io/db_checked_sql.jet` | `core.db` — checked SQL params, typed row reads, transactions, migrations |
| `examples/features/io/dir_entry.jet` | `fs.list_dir` → `[DirEntry]` |
| `examples/features/serde/serde_derive.jet` | `#Codable` encode + typed `decode<T>` with `#Rename` |
| `examples/features/serde/csv_typed.jet` | `csv.decode<Row>` → struct → JSON (the typed CSV pipeline) |
| `examples/features/serde/json_typed.jet` | Nested struct + list + optional round-trip with `#RenameAll(camel)` |
| `examples/features/serde/decode_traced.jet` | `decode_traced<T>` → `DecodeResult<T>`/`MigrationStatus`, incl. a real v1→v2 migration at decode time |
| `examples/features/reflection/reflect-value.jet` | `reflect.of(x)` — `.type_name()`/`.display()`/`.fields()` |
| `examples/features/syntax/maturity_tags.jet` | `#Meta(maturity: .Experimental / .Tested / .Hardened)` doc-only API metadata (D-MARK-META1=B) |

Run the full battery: `nix develop -c cargo test --test golden` and `nix develop -c cargo test --test corelib`.

See also: [Maturity tags](maturity-tags.md).
