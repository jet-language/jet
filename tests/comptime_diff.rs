//! M9.5 differential battery (permanent CI). For each expression, the same
//! code is evaluated twice — once as `@comptime_value :: e;` (the sema
//! tree-walking interpreter) and once as a runtime `r :: e` (generated
//! Rust). The program prints both; the two lines MUST be byte-identical.
//!
//! Divergence is a P0 miscompile-class bug (S26 rule 6: comptime implements
//! runtime semantics exactly — i64 Int, IEEE f64 Float with S21 display,
//! char-counted Strings (S41), BTreeMap ordering (S38)).

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

mod common;
use common::{have_rustc, panic_message, test_worker_count};

/// Expressions whose comptime and runtime evaluation must agree. Each is
/// inlined verbatim on both sides, so it must be a self-contained
/// expression with an inferable type.
const CASES: &[&str] = &[
    // Int arithmetic + operator semantics
    "2 + 3 * 4",
    "100 / 7",
    "100 % 7",
    "7 / 2",
    "(0 - 17) % 5",
    "(0 - 17) / 5",
    "1 << 10",
    "255 & 15",
    // D-SHAPE-PIPE1=C reserves single `|` for pattern alternatives; Jet has
    // no bitwise-or expression. `|=` remains covered by tests/numops.rs.
    "6 ^ 3",
    "1000000 * 1000000",
    // Float rounding + S21 "always a decimal" display
    "3.0 / 2.0",
    "10.0 / 4.0",
    "1.0 / 3.0",
    "5.0",
    "2.0 * 2.0",
    "0.1 + 0.2",
    // Bool / comparison
    "3 < 5 && 2 == 2",
    "10 >= 10 || false",
    // String + Char ops (char-counted, S41)
    "\"Hello\".to_upper()",
    "\"WORLD\".to_lower()",
    "\"héllo\".len()",
    "\"  trim me  \".trim()",
    "\"  trim me  \".trim_start()",
    "\"  trim me  \".trim_end()",
    "\"jet\".pad_start(5, \".\")",
    "\"jet\".pad_end(5, \".\")",
    "\"hello jet\".index_of(\"jet\")",
    "\"banana\".count(\"an\")",
    "\"Hello\".is_alphabetic()",
    "\"123\".is_numeric()",
    "\" \t\".is_whitespace()",
    "\"jet lang\".is_ascii()",
    "\"hELLO jet\".to_title()",
    "(\"left:right\".split_once(\":\") ?? panic(\"split\")).before",
    "\"ab\".repeat(3)",
    "\"a,b,c\".split(\",\").to_list()",
    "\"hello world\".replace(\"o\", \"0\")",
    // List values, ordering, and methods
    "[1, 2, 3]",
    "[3, 1, 2]",
    "[10, 20, 30][1]",
    "[\"x\", \"y\", \"z\"]",
    "loop value, [1, 2, 3] -> value * 2",
    // Map ordering via derived lists (BTreeMap is sorted by key). `.keys()`/
    // `.values()` return a lazy one-pass `Iter` (no `Display`, E0915) —
    // `.to_list()` materializes it for the differential's string print.
    "[\"b\": 2, \"a\": 1, \"c\": 3].keys().to_list()",
    "[\"b\": 2, \"a\": 1, \"c\": 3].values().to_list()",
    "[2: \"two\", 1: \"one\"].keys().to_list()",
    "Rank.from([1, 2, 3]).intersection(Rank.from([2, 3, 4])).to_list()",
    "Rank.from([1, 2, 3]).difference(Rank.from([2, 3, 4])).to_list()",
    "Rank.from([1, 2, 3]).symmetric_difference(Rank.from([2, 3, 4])).to_list()",
    "Rank.from([1, 2]).is_subset(Rank.from([1, 2, 3]))",
    "Rank.from([1, 2, 3]).is_superset(Rank.from([1, 2]))",
    "Rank.from([1, 2]).is_disjoint(Rank.from([3, 4]))",
    "Set.from([1, 2, 3]).intersection(Set.from([2, 3, 4])).len()",
    "Set.from([1, 2, 3]).difference(Set.from([2, 3, 4])).len()",
    "Set.from([1, 2, 3]).symmetric_difference(Set.from([2, 3, 4])).len()",
    "Set.from([1, 2]).is_subset(Set.from([1, 2, 3]))",
    "Set.from([1, 2, 3]).is_superset(Set.from([1, 2]))",
    "Set.from([1, 2]).is_disjoint(Set.from([3, 4]))",
    // D-BIGINT1 (card #392): arbitrary-precision arithmetic — no overflow,
    // no auto-promotion. comptime must match AOT's limb-based `JetBigInt`
    // byte-for-byte (R12 parity).
    // parity: guard tests/comptime_diff.rs::comptime_bigint_matches_runtime
    "(BigInt(9223372036854775807) + BigInt(1)).to_string()",
    "(BigInt(\"999999999999999999999999999999\") + BigInt(\"999999999999999999999999999999\")).to_string()",
    "(BigInt(100) - BigInt(1)).to_string()",
    "(BigInt(7) * BigInt(6)).to_string()",
    "BigInt(5).sub(BigInt(3)).to_string()",
    "BigInt(3).neg().to_string()",
];

const F32_VALUE_FLOW: &str = r#"
fn pass_f32(value: F32) => F32 {
    return value
}

fn apply_f32(transform: fn(F32) => F32, value: F32) => F32 {
    return transform(value)
}

fn f32_value_flow() => String {
    literal :: F32.{ 16777217.0 }
    one :: F32.{ 1.0 }
    two :: F32.{ 2.0 }
    three :: F32.{ 3.0 }
    threshold :: F32.{ 16777215.0 }
    immutable :: F32.{ literal + one }
    mutable := F32.{ literal }
    mutable += one
    mutable -= two
    mutable *= three
    mutable /= two
    transform :: (value: F32) => value + one
    same :: pass_f32(literal)
    wide :: Float.from_f32(literal)
    narrowed :: F32.from_float(wide) ?? one
    nested :: [["values": [literal, apply_f32(transform, literal), narrowed]]]
    option_left :: [[[literal].get(0)]]
    option_right :: [[[same].get(0)]]
    result_left :: [[F32.from_float(wide)]]
    result_right :: [[F32.from_float(Float.from_f32(same))]]
    negative :: F32.{ -literal }
    difference :: F32.{ literal - one }
    product :: F32.{ literal * two }
    quotient :: F32.{ literal / two }
    return "{literal}|{immutable}|{mutable}|{negative}|{difference}|{product}|{quotient}|{literal == same}|{literal > threshold}|{nested[0]["values"]}|{option_left == option_right}|{result_left == result_right}"
}

@expected :: f32_value_flow()

fn run() {
    actual :: f32_value_flow()
    print("{@expected}")
    print("{actual}")
}
"#;

/// card #392: the `use core.X as alias; alias.method(...)` module-call form
/// needs its own program per case (an inline expression alone can't `use`),
/// so it gets a dedicated differential loop rather than reusing `CASES`.
const MODULE_CASES: &[&str] = &[
    // `("core.string", ...)` was a dead dispatch key (no import resolves to
    // it — `core.text` is the only ratified spelling), so every
    // `text.<method>(...)` call hit E0956. Fixed via `TextLite` (ported
    // verbatim from AOT's `jet_text_*` prelude fns).
    "use core.text as text\n@comptime_value :: text.trim(\" hi \")\n\nfn run() {\n    r :: text.trim(\" hi \")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.text as text\n@comptime_value :: text.upper(\"abc\")\n\nfn run() {\n    r :: text.upper(\"abc\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.text as text\n@comptime_value :: text.words(\"hello world's foo\")[0]\n\nfn run() {\n    r :: text.words(\"hello world's foo\")[0]\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.text as text\n@comptime_value :: text.pad_start(\"7\", 3, \"0\")\n\nfn run() {\n    r :: text.pad_start(\"7\", 3, \"0\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    // core.math: previously only sqrt/floor/ceil/round/abs/pow/min/max/clamp/
    // log2/log10 were dispatched; the rest (trig, checked/saturating/
    // wrapping, gcd/lcm) fell to E0956.
    "use core.math as math\n@comptime_value :: math.sin(0.0)\n\nfn run() {\n    r :: math.sin(0.0)\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.math as math\n@comptime_value :: math.gcd(12, 18)\n\nfn run() {\n    r :: math.gcd(12, 18)\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.math as math\n@comptime_value :: math.saturating_add(9223372036854775807, 1)\n\nfn run() {\n    r :: math.saturating_add(9223372036854775807, 1)\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    // card #392 pass 3: `core.url` (D-URL1=A), ported verbatim from AOT's
    // `jet_url_*` (`UrlMime.rs` + `MathRandomTime.rs`, see `UrlLite.rs`).
    // Plain string-returning free functions use this differential. URL
    // structs retain their canonical typed metadata through the comptime
    // marshalled value path; tier-wide typed-head coverage lives in
    // `tests/tir_language_features.rs`.
    // parity: guard tests/repl.rs::repl_core_url_dispatch
    "use core.url as url\n@comptime_value :: url.percent_encode(\"a b/c?d#e\")\n\nfn run() {\n    r :: url.percent_encode(\"a b/c?d#e\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.url as url\n@comptime_value :: url.percent_decode(\"a%20b%2Fc\") ?? panic(\"bad\")\n\nfn run() {\n    r :: url.percent_decode(\"a%20b%2Fc\") ?? panic(\"bad\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.url as url\n@comptime_value :: url.percent_decode(\"bad%\") ?? \"fallback\"\n\nfn run() {\n    r :: url.percent_decode(\"bad%\") ?? \"fallback\"\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.url as url\n@comptime_value :: url.query([[\"a\", \"1\"], [\"b\", \"2 c\"]])\n\nfn run() {\n    r :: url.query([[\"a\", \"1\"], [\"b\", \"2 c\"]])\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    // card #392 pass 3: `core.data`'s fixed-signature stats surface, ported
    // verbatim from AOT's `jet_data_*` (`EncodingTraits.rs`, see
    // `DataLite.rs`). `describe`/`status`/`bar_text`/`bar_svg` return/take
    // builtin struct values (`DataSummary`/`DataStatus`/`DataGroup`), which
    // this crate's generic `CtValue::Struct` display can't print the same
    // way AOT's derived struct `Display` does (a pre-existing limit shared
    // by every builtin struct type, not specific to `core.data` — see
    // `UrlLite.rs`'s note) — covered instead by
    // `tests/repl.rs::repl_core_data_dispatch`.
    "use core.data as data\n@comptime_value :: data.sum([1.0, 2.0, 3.5]) ?? panic(\"data\")\n\nfn run() {\n    r :: data.sum([1.0, 2.0, 3.5]) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\n@comptime_value :: data.mean([1.0, 2.0, 3.0, 4.0]) ?? panic(\"data\")\n\nfn run() {\n    r :: data.mean([1.0, 2.0, 3.0, 4.0]) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\n@comptime_value :: data.median([5.0, 1.0, 3.0, 2.0]) ?? panic(\"data\")\n\nfn run() {\n    r :: data.median([5.0, 1.0, 3.0, 2.0]) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\n@comptime_value :: data.variance([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]) ?? panic(\"data\")\n\nfn run() {\n    r :: data.variance([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\n@comptime_value :: data.stddev([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]) ?? panic(\"data\")\n\nfn run() {\n    r :: data.stddev([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\n@comptime_value :: data.quantile([1.0, 2.0, 3.0, 4.0, 5.0], 0.25) ?? panic(\"data\")\n\nfn run() {\n    r :: data.quantile([1.0, 2.0, 3.0, 4.0, 5.0], 0.25) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\n@comptime_value :: data.rolling_mean([1.0, 2.0, 3.0, 4.0], 2) ?? panic(\"data\")\n\nfn run() {\n    r :: data.rolling_mean([1.0, 2.0, 3.0, 4.0], 2) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\n@comptime_value :: data.min([3.0, -1.0, 5.0]) ?? panic(\"data\")\n\nfn run() {\n    r :: data.min([3.0, -1.0, 5.0]) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\n@comptime_value :: data.max([3.0, -1.0, 5.0]) ?? panic(\"data\")\n\nfn run() {\n    r :: data.max([3.0, -1.0, 5.0]) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    // #1657: catastrophic cancellation. A naive left-to-right sum answers
    // 0.0 here and the compensated kernel answers 1.0, so a second
    // implementation on either tier fails these three cases.
    "use core.data as data\n@comptime_value :: data.sum([10000000000000000.0, 1.0, -10000000000000000.0]) ?? panic(\"data\")\n\nfn run() {\n    r :: data.sum([10000000000000000.0, 1.0, -10000000000000000.0]) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\n@comptime_value :: data.mean([10000000000000000.0, 1.0, -10000000000000000.0]) ?? panic(\"data\")\n\nfn run() {\n    r :: data.mean([10000000000000000.0, 1.0, -10000000000000000.0]) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\n@comptime_value :: data.variance([10000000000000000.0, 1.0, -10000000000000000.0]) ?? panic(\"data\")\n\nfn run() {\n    r :: data.variance([10000000000000000.0, 1.0, -10000000000000000.0]) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\n@comptime_value :: data.rolling_mean([10000000000000000.0, 1.0, -10000000000000000.0], 3) ?? panic(\"data\")\n\nfn run() {\n    r :: data.rolling_mean([10000000000000000.0, 1.0, -10000000000000000.0], 3) ?? panic(\"data\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    // card #392 pass 4: `core.encoding.{csv,toml,yaml,xml,cbor,jsonl}` +
    // `core.encoding.json.{canonical,events}`, ported verbatim from AOT's
    // `jet_ring_csv_*`/`toml`/`yaml` mods/`jet_std_xml_*`/`jet_cbor_*`/
    // `jet_std_jsonl_*`/`jet_std_json_render_canonical`/`jet_std_json_events`
    // (see `EncodingLite.rs`). Every case round-trips `parse`+`to_string` (or
    // `to_bytes`+`parse`) so both the parser and the renderer sides differ
    // against real generated Rust, not just one direction.
    "use core.encoding.csv as csv\n@comptime_value :: csv.to_string(csv.parse(\"a,\\\"b,c\\\",\\\"e\\\"\\\"f\\\"\\n\") ?? panic(\"bad\"))\n\nfn run() {\n    r :: csv.to_string(csv.parse(\"a,\\\"b,c\\\",\\\"e\\\"\\\"f\\\"\\n\") ?? panic(\"bad\"))\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.csv as csv\n@comptime_value :: csv.parse(\"a,b,c\\n1,2\\n\") ?? panic(\"bad\")\n\nfn run() {\n    r :: csv.parse(\"a,b,c\\n1,2\\n\") ?? panic(\"bad\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.csv as csv\n@comptime_value :: csv.to_string(csv.parse(\"name,note\\nAda,\\\"line1\\nline2\\\"\\nLin,\\\"said \\\"\\\"hi\\\"\\\"\\\"\\n\") ?? panic(\"bad\"))\n\nfn run() {\n    r :: csv.to_string(csv.parse(\"name,note\\nAda,\\\"line1\\nline2\\\"\\nLin,\\\"said \\\"\\\"hi\\\"\\\"\\\"\\n\") ?? panic(\"bad\"))\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.toml as toml\n@comptime_value :: toml.to_string(toml.parse(\"[a]\\nx = 1\\n\\n[[a.b]]\\ny = 2\\n\\n[[a.b]]\\ny = 3\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n\nfn run() {\n    r :: toml.to_string(toml.parse(\"[a]\\nx = 1\\n\\n[[a.b]]\\ny = 2\\n\\n[[a.b]]\\ny = 3\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.toml as toml\n@comptime_value :: toml.to_string(toml.parse(\"x = 1.5\\ny = [1, 2, 3]\\nz = {{ a = 1, b = 2 }}\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n\nfn run() {\n    r :: toml.to_string(toml.parse(\"x = 1.5\\ny = [1, 2, 3]\\nz = {{ a = 1, b = 2 }}\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.yaml as yaml\n@comptime_value :: yaml.to_string(yaml.parse(\"a: &x 1\\nb: *x\\nc:\\n  - 1\\n  - 2\\nd: |\\n  hello\\n  world\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n\nfn run() {\n    r :: yaml.to_string(yaml.parse(\"a: &x 1\\nb: *x\\nc:\\n  - 1\\n  - 2\\nd: |\\n  hello\\n  world\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.xml as xml\n@comptime_value :: xml.to_string(xml.parse(\"<r xmlns=\\\"urn:r\\\" xmlns:p=\\\"urn:p\\\" p:a=\\\"x&amp;y\\\">a&amp;<!--c--><![CDATA[<x>]]><?go now?><p:c/></r>\") ?? panic(\"bad\"))\n\nfn run() {\n    r :: xml.to_string(xml.parse(\"<r xmlns=\\\"urn:r\\\" xmlns:p=\\\"urn:p\\\" p:a=\\\"x&amp;y\\\">a&amp;<!--c--><![CDATA[<x>]]><?go now?><p:c/></r>\") ?? panic(\"bad\"))\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.json as json\nuse core.encoding.cbor as cbor\nuse core.encoding.hex as hex\n@comptime_value :: hex.encode(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3],\\\"c\\\":-7}}\") ?? panic(\"bad\")) ?? panic(\"bad\"))\n\nfn run() {\n    r :: hex.encode(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3],\\\"c\\\":-7}}\") ?? panic(\"bad\")) ?? panic(\"bad\"))\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.json as json\nuse core.encoding.cbor as cbor\n@comptime_value :: json.to_string(cbor.parse(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3]}}\") ?? panic(\"bad\")) ?? panic(\"bad\")) ?? panic(\"bad\"))\n\nfn run() {\n    r :: json.to_string(cbor.parse(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3]}}\") ?? panic(\"bad\")) ?? panic(\"bad\")) ?? panic(\"bad\"))\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.jsonl as jsonl\n@comptime_value :: jsonl.to_string(jsonl.parse(\"{{\\\"a\\\":1}}\\n{{\\\"b\\\":2}}\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n\nfn run() {\n    r :: jsonl.to_string(jsonl.parse(\"{{\\\"a\\\":1}}\\n{{\\\"b\\\":2}}\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    // D-JSONCANON1: edition 2027 `json.canonical` is fallible (`String ?
    // encoding.EncodingError`); `run()` is infallible, so the ratified
    // migration form is the panic fallback, not `?` propagation.
    "use core.encoding.json as json\n@comptime_value :: json.canonical(json.parse(\"{{\\\"b\\\":1,\\\"a\\\":2}}\") ?? panic(\"bad\")) ?? panic(\"value is not canonical JSON\")\n\nfn run() {\n    r :: json.canonical(json.parse(\"{{\\\"b\\\":1,\\\"a\\\":2}}\") ?? panic(\"bad\")) ?? panic(\"value is not canonical JSON\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.json as json\n@comptime_value :: json.events(json.parse(\"{{\\\"a\\\":[1,2]}}\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n\nfn run() {\n    r :: json.events(json.parse(\"{{\\\"a\\\":[1,2]}}\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
];

const LOADABLE_CASES: &[&str] = &[
    "use core.reactive.loadable as loadable\n@comptime_value :: loadable.idle()\n\nfn run() {\n    r :: loadable.idle()\n    print(\"{@comptime_value.is_idle()}\")\n    print(\"{r.is_idle()}\")\n}\n",
    "use core.reactive.loadable as loadable\n@comptime_value :: loadable.loading()\n\nfn run() {\n    r :: loadable.loading()\n    print(\"{@comptime_value.is_loading()}\")\n    print(\"{r.is_loading()}\")\n}\n",
    "use core.reactive.loadable as loadable\n@comptime_value :: loadable.loaded(7)\n\nfn run() {\n    r :: loadable.loaded(7)\n    print(\"{@comptime_value.loaded() ?? 0}\")\n    print(\"{r.loaded() ?? 0}\")\n}\n",
    "use core.reactive.loadable as loadable\n@comptime_value :: loadable.failed(\"offline\")\n\nfn run() {\n    r :: loadable.failed(\"offline\")\n    print(\"{@comptime_value.is_failed()}\")\n    print(\"{r.is_failed()}\")\n}\n",
    "use core.reactive.loadable as loadable\n@comptime_value :: loadable.idle().is_idle()\n\nfn run() {\n    r :: loadable.idle().is_idle()\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.reactive.loadable as loadable\n@comptime_value :: loadable.loading().is_loading()\n\nfn run() {\n    r :: loadable.loading().is_loading()\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.reactive.loadable as loadable\n@comptime_value :: loadable.loaded(7).is_loaded()\n\nfn run() {\n    r :: loadable.loaded(7).is_loaded()\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.reactive.loadable as loadable\n@comptime_value :: loadable.failed(\"offline\").is_failed()\n\nfn run() {\n    r :: loadable.failed(\"offline\").is_failed()\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.reactive.loadable as loadable\n@comptime_value :: loadable.loaded(7).loaded() ?? 0\n\nfn run() {\n    r :: loadable.loaded(7).loaded() ?? 0\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
    "use core.reactive.loadable as loadable\n@comptime_value :: loadable.loaded(7).or_else(0)\n\nfn run() {\n    r :: loadable.loaded(7).or_else(0)\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n",
];

#[test]
fn comptime_loadable_matches_runtime() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping Loadable differential battery");
        return;
    }
    for (i, src) in LOADABLE_CASES.iter().enumerate() {
        if let Err(payload) = std::panic::catch_unwind(|| {
            check_comptime_src(10_000 + i, "core.reactive.loadable", src)
        }) {
            panic!("Loadable case {i} failed: {}", panic_message(payload));
        }
    }
}

#[test]
fn loadable_incompatible_defaults_stop_before_rustc() {
    for (name, expression) in [
        ("idle", "loadable.idle().or_else(7)"),
        ("loading", "loadable.loading().or_else(7)"),
        ("failed", "loadable.failed(\"offline\").or_else(7)"),
        ("loaded", "loadable.loaded(7).or_else(\"bad\")"),
    ] {
        let src = format!(
            "use core.reactive.loadable as loadable\n\nfn run() {{\n    print({expression})\n}}\n"
        );
        let Err(diags) = jet::Driver::compile_generated_src(
            &src,
            "loadable_default.jet",
            jet::Sema::CompileMode::Run,
        ) else {
            panic!("{name} incompatible fallback reached codegen and would be an I2 rustc rejection");
        };
        assert!(
            diags.iter().any(|diag| diag.code == "E0112"),
            "{name} fallback missed the type mismatch: {diags:?}"
        );
    }
}

#[test]
fn comptime_matches_runtime() {
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping comptime differential battery");
        return;
    }
    let next = Arc::new(AtomicUsize::new(0));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let workers = test_worker_count(16).min(CASES.len().max(1));
    let mut handles = Vec::new();
    for _ in 0..workers {
        let next = Arc::clone(&next);
        let failures = Arc::clone(&failures);
        handles.push(std::thread::spawn(move || {
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= CASES.len() {
                    break;
                }
                if let Err(payload) = std::panic::catch_unwind(|| check_comptime_case(i, CASES[i]))
                {
                    failures.lock().unwrap().push(panic_message(payload));
                }
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    let failures = failures.lock().unwrap();
    if !failures.is_empty() {
        panic!("{}", failures.join("\n\n"));
    }
}

#[test]
fn yielding_loop_matches_comptime_and_runtime() {
    if !have_rustc() {
        return;
    }
    check_comptime_case(0, "loop value, [1, 2, 3] -> value * 2");
    check_comptime_case(
        1,
        "loop value, [1, 2, 3, 4] -> { if value > 2 { break }; value * 2 }",
    );
}

#[test]
fn comptime_f32_width_survives_value_flow_and_matches_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping F32 differential battery");
        return;
    }
    check_comptime_src(32_000, "F32 value-flow width", F32_VALUE_FLOW);
}

#[test]
fn gzip_golden_and_hostile_inputs_match_comptime_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping gzip comptime differential");
        return;
    }
    let src = r#"use core.compress.gzip as gzip

fn codec_probe() => String {
    bytes :: [U8].{ 72, 101, 108, 108, 111 }
    gz :: gzip.decompress(gzip.compress(bytes)) ?? [U8].{}
    golden :: gzip.decompress([31, 139, 8, 0, 0, 0, 0, 0, 2, 3, 203, 72, 205, 201, 201, 7, 0, 134, 166, 16, 54, 5, 0, 0, 0]) ?? [U8].{}
    bad_size :: gzip.decompress([31, 139, 8, 0, 0, 0, 0, 0, 2, 3, 203, 72, 205, 201, 201, 7, 0, 134, 166, 16, 54, 6, 0, 0, 0]) ?? [U8].{ 255 }
    h :: U8.{ 72 }
    lower_h :: U8.{ 104 }
    o :: U8.{ 111 }
    max :: U8.{ 255 }
    return "{gz.len() == 5}|{gz[0] == h}|{golden.len() == 5}|{golden[0] == lower_h}|{golden[4] == o}|{bad_size[0] == max}"
}

@expected :: codec_probe()

fn run() {
    actual :: codec_probe()
    print("{@expected}")
    print("{actual}")
}
"#;
    check_comptime_src(32_001, "gzip independent golden and ISIZE corruption", src);
}

#[test]
fn zstd_comptime_codec_round_trips_through_resident_and_aot_decoders() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping zstd comptime differential");
        return;
    }
    let src = r#"use core.compress.zstd as zstd

@bytes :: [U8].{ 72, 101, 108, 108, 111 }
@encoded :: zstd.compress(@bytes)
@expected :: zstd.decompress(@encoded) ?? [U8].{}

fn run() {
    restored :: zstd.decompress(@encoded) ?? [U8].{}
    print("{@expected}")
    print("{restored}")
}
"#;
    check_comptime_src(32_002, "zstd resident encoder accepted by AOT decoder", src);
}

#[test]
fn zstd_72_mib_advertised_window_matches_resident_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping zstd window differential");
        return;
    }
    let src = r#"use core.compress.zstd as zstd

@expected :: zstd.decompress([40, 181, 47, 253, 0, 129, 41, 0, 0, 104, 101, 108, 108, 111]) ?? [U8].{ 255 }

fn run() {
    actual :: zstd.decompress([40, 181, 47, 253, 0, 129, 41, 0, 0, 104, 101, 108, 108, 111]) ?? [U8].{ 255 }
    print("{@expected}")
    print("{actual}")
}
"#;
    check_comptime_src(32_003, "zstd 72 MiB advertised window", src);
}

#[test]
fn comptime_bigint_matches_runtime() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping BigInt differential battery");
        return;
    }
    let cases = CASES
        .iter()
        .enumerate()
        .filter(|(_, expr)| expr.contains("BigInt"))
        .collect::<Vec<_>>();
    assert_eq!(
        cases.iter().map(|(_, expr)| **expr).collect::<Vec<_>>(),
        [
            "(BigInt(9223372036854775807) + BigInt(1)).to_string()",
            "(BigInt(\"999999999999999999999999999999\") + BigInt(\"999999999999999999999999999999\")).to_string()",
            "(BigInt(100) - BigInt(1)).to_string()",
            "(BigInt(7) * BigInt(6)).to_string()",
            "BigInt(5).sub(BigInt(3)).to_string()",
            "BigInt(3).neg().to_string()",
        ],
        "BigInt differential cases must stay exact"
    );
    for (i, expr) in cases {
        check_comptime_case(i, expr);
    }
}

fn check_comptime_case(i: usize, expr: &str) {
    let src = format!(
        "@comptime_value :: {e}\n\nfn run() {{\n    r :: {e}\n    print(\"{{@comptime_value}}\")\n    print(\"{{r}}\")\n}}\n",
        e = expr
    );
    check_comptime_src(i, expr, &src);
}

#[test]
fn reusable_regex_matches_across_comptime_tir_and_runtime() {
    check_comptime_src(
        34_000,
        "typed Regex methods and canonical grammar",
        r#"
@ct_regex :: Regex.{"(?<word>\p{{Alphabetic}}+)_(\d{{2,4}})"}
@ct_match :: @ct_regex.match("xx Jet_2026 yy") ?? panic("missing comptime match")
@comptime_value :: "{@ct_match.group(2) ?? "none"}|{@ct_match.name("word") ?? "none"}|{@ct_match.start()}|{@ct_match.end()}|{@ct_match.group_start(1) ?? -1}|{@ct_match.group_end(1) ?? -1}|{@ct_regex.replace_all("Jet_2026 Rust_2025", "${{word}}:$2")}|{@ct_regex.replace_all_with("Jet_2026 Rust_2025", (m: Match) => m.name("word") ?? "none")}"

fn run() {
    rt_regex :: Regex.{"(?<word>\p{{Alphabetic}}+)_(\d{{2,4}})"}
    rt_match :: rt_regex.match("xx Jet_2026 yy") ?? panic("missing runtime match")
    runtime_value :: "{rt_match.group(2) ?? "none"}|{rt_match.name("word") ?? "none"}|{rt_match.start()}|{rt_match.end()}|{rt_match.group_start(1) ?? -1}|{rt_match.group_end(1) ?? -1}|{rt_regex.replace_all("Jet_2026 Rust_2025", "${{word}}:$2")}|{rt_regex.replace_all_with("Jet_2026 Rust_2025", (m: Match) => m.name("word") ?? "none")}"
    print("{@comptime_value}")
    print("{runtime_value}")
}
"#,
    );
}

/// D-BOUND-HEAD1=A: DateTime heads use the same complete-literal parser when
/// sema folds them and when the generated runtime constructs them.
#[test]
fn typed_datetime_head_matches_comptime_and_runtime() {
    check_comptime_src(
        34_002,
        "typed DateTime head",
        r#"
@stamp :: DateTime.{"2026-08-07T12:00:00Z"}
@expected :: @stamp.to_string()

fn run() {
    runtime :: DateTime.{"2026-08-07T12:00:00Z"}
    print("{@expected}")
    print("{runtime.to_string()}")
}
"#,
    );
}

/// A compile-time binding that advances a receiver has to leave the advance
/// behind for the next compile-time binding to see, or the folded answers drift
/// from what the same code does at run time: two sequential reads would both
/// report byte zero.
#[test]
fn sequential_comptime_reads_advance_the_shared_reader() {
    check_comptime_src(
        34_001,
        "sequential compile-time reader reads",
        r#"
fn run() {
    @ct :: Reader.over([U8].{7, 9, 11})
    @ct_a :: @ct.read_u8() ?? panic("ct a")
    @ct_b :: @ct.read_u8() ?? panic("ct b")
    @comptime_value :: "{@ct_a}|{@ct_b}|{@ct.remaining()}"

    rt :: Reader.over([U8].{7, 9, 11})
    rt_a :: rt.read_u8() ?? panic("rt a")
    rt_b :: rt.read_u8() ?? panic("rt b")
    runtime_value :: "{rt_a}|{rt_b}|{rt.remaining()}"

    print("{@comptime_value}")
    print("{runtime_value}")
}
"#,
    );
}

/// Shared by `check_comptime_case` (single-expression cases) and
/// `check_comptime_module_case` (card #392's `use core.X as a; a.f(...)`
/// cases, which need a full program — a bare `use` isn't an expression).
fn check_comptime_src(i: usize, label: &str, src: &str) {
    let (_, user_diags) = jet::Lexer::lex(src);
    assert!(user_diags.is_empty(), "invalid differential fixture: {user_diags:?}");
    let src = framed_comptime_src(src);
    // The comptime interpreter and sema both recurse over typed expressions.
    // Libtest workers have much smaller stacks than the compiler process, so
    // run this shared compile boundary with the same headroom as golden tests.
    let compile = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn_scoped(scope, || {
                jet::Driver::compile_generated_src(
                    &src,
                    "comptime_diff.jet",
                    jet::Sema::CompileMode::Run,
                )
            })
            .expect("spawn comptime differential compiler")
            .join()
            .expect("comptime differential compiler panicked")
    });
    let compiled = match compile {
        Ok(c) => c,
        Err(diags) => panic!(
            "case {} `{}` failed the front end:\n{}",
            i,
            label,
            jet::render_diagnostics("comptime_diff.jet", &src, &diags)
        ),
    };
    // D-BIGINT1 (card #392): a `BigInt` case pulls the Top-tier prelude
    // module, which shares a file with `jet_atomic_windows`'s vetted FFI
    // internals (I1 gate, `JET_VETTED_UNSAFE_BEGIN/END` markers) — strip it
    // before the I1 check, same as `golden.rs::strip_vetted_prelude_modules`.
    let user_code = common::strip_vetted_prelude_modules(&compiled.rust);
    assert!(
        !user_code.contains("unsafe"),
        "case `{}` generated unsafe outside the vetted prelude",
        label
    );

    let dir = std::env::temp_dir();
    // Case numbers are per-test, so two tests running side by side both reach
    // index 0 and race for the same file: one rustc deletes the object the
    // other is linking, and the failure reads like an I2 miscompile. Number
    // the artifacts per run instead.
    static ARTIFACT: AtomicUsize = AtomicUsize::new(0);
    let unique = ARTIFACT.fetch_add(1, Ordering::Relaxed);
    let rs = dir.join(format!("jet_ctdiff_{}_{}_{}.rs", std::process::id(), i, unique));
    let bin = dir.join(format!("jet_ctdiff_{}_{}_{}", std::process::id(), i, unique));
    fs::write(&rs, &compiled.rust).unwrap();
    let mut rustc = Command::new("rustc");
    rustc
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin);
    if let Some(link) = &compiled.ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc.arg("-L").arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let out = rustc.output().unwrap();
    assert!(
        out.status.success(),
        "I2 violated: rustc rejected generated code for `{}`:\n{}",
        label,
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "case `{}` panicked at runtime", label);
    assert_parity_output(label, &run.stdout);
}

fn framed_comptime_src(src: &str) -> String {
    let mut lines: Vec<String> = src.lines().map(str::to_owned).collect();
    let second = lines
        .iter()
        .rposition(|line| line.trim_start().starts_with("print("))
        .expect("comptime differential source needs a runtime print");
    let first = lines[..second]
        .iter()
        .rposition(|line| line.trim_start().starts_with("print("))
        .expect("comptime differential source needs a comptime print");
    let comptime = print_argument(&lines[first]).to_string();
    let runtime = print_argument(&lines[second]).to_string();
    let indent = lines[first][..lines[first].len() - lines[first].trim_start().len()].to_string();
    lines[first] = format!("{indent}__comptime_diff_frame({comptime}, {runtime})");
    lines[second].clear();
    let mut framed = format!(
        "use core.encoding.hex as __comptime_diff_hex\n{}",
        lines.join("\n")
    );
    framed.push_str(&format!(
        "\n\nfn __comptime_diff_frame(expected_text: String, actual_text: String) {{\n\
    print(\"{{__comptime_diff_hex.encode(expected_text.bytes())}}\")\n\
    print(\"{{__comptime_diff_hex.encode(actual_text.bytes())}}\")\n\
}}\n"
    ));
    framed
}

fn print_argument(line: &str) -> &str {
    line.trim()
        .strip_prefix("print(")
        .and_then(|line| line.strip_suffix(')'))
        .expect("comptime differential parity print must occupy one complete line")
}

fn assert_parity_output(label: &str, stdout: &[u8]) {
    let (comptime, runtime) = parse_parity_output(stdout)
        .unwrap_or_else(|reason| panic!("case `{label}` emitted an invalid parity frame: {reason}"));
    assert_eq!(
        comptime,
        runtime,
        "DIVERGENCE for `{}`: comptime gave {:?}, runtime gave {:?} — this is a P0 miscompile",
        label,
        String::from_utf8_lossy(&comptime),
        String::from_utf8_lossy(&runtime)
    );
}

fn parse_parity_output(stdout: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let framed = stdout
        .strip_suffix(b"\n")
        .ok_or_else(|| "frame has no final newline".to_string())?;
    let mut lines = framed.split(|byte| *byte == b'\n');
    let comptime = lines.next().unwrap();
    let runtime = lines
        .next()
        .ok_or_else(|| "frame has no runtime value".to_string())?;
    if lines.next().is_some() {
        return Err("frame has trailing lines".to_string());
    }
    Ok((
        decode_parity_hex(comptime, "comptime")?,
        decode_parity_hex(runtime, "runtime")?,
    ))
}

fn decode_parity_hex(encoded: &[u8], label: &str) -> Result<Vec<u8>, String> {
    if encoded.len() % 2 != 0 {
        return Err(format!("{label} hex has odd length"));
    }
    encoded
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0]).ok_or_else(|| format!("{label} hex is invalid"))?;
            let low = hex_nibble(pair[1]).ok_or_else(|| format!("{label} hex is invalid"))?;
            Ok(high << 4 | low)
        })
        .collect()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn test_parity_frame(comptime: &[u8], runtime: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    for value in [comptime, runtime] {
        for byte in value {
            frame.extend_from_slice(format!("{byte:02x}").as_bytes());
        }
        frame.push(b'\n');
    }
    frame
}

#[test]
fn parity_frame_preserves_newlines_and_delimiter_like_bytes() {
    let comptime = b"3\n__comptime_diff_frame(999)\n0\n";
    let runtime = b"\n3\n__comptime_diff_frame(999)\n0";
    let frame = test_parity_frame(comptime, runtime);
    let (decoded_comptime, decoded_runtime) = parse_parity_output(&frame).unwrap();
    assert_eq!(decoded_comptime, comptime);
    assert_eq!(decoded_runtime, runtime);
}

#[test]
fn parity_frame_rejects_unequal_same_length_multiline_values() {
    let frame = test_parity_frame(b"alpha\nbeta", b"alpha\nzeta");
    let failure = std::panic::catch_unwind(|| assert_parity_output("adversarial", &frame))
        .expect_err("unequal parity values must fail");
    let message = panic_message(failure);
    assert!(message.contains("DIVERGENCE for `adversarial`"), "{message}");
    assert!(message.contains("beta") && message.contains("zeta"), "{message}");
}

#[test]
fn comptime_module_calls_match_runtime() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping comptime module-call differential battery");
        return;
    }
    for (i, src) in MODULE_CASES.iter().enumerate() {
        check_comptime_src(1000 + i, src, src);
    }
}

#[test]
fn xml_rich_whole_value_matches_comptime_and_runtime() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping rich XML comptime differential");
        return;
    }
    let src = "use core.encoding.xml as xml\n@comptime_value :: xml.to_string(xml.parse(\"<r xmlns=\\\"urn:r\\\" xmlns:p=\\\"urn:p\\\" p:a=\\\"x&amp;y\\\">a&amp;<!--c--><![CDATA[<x>]]><?go now?><p:c/></r>\") ?? panic(\"bad\"))\n\nfn run() {\n    r :: xml.to_string(xml.parse(\"<r xmlns=\\\"urn:r\\\" xmlns:p=\\\"urn:p\\\" p:a=\\\"x&amp;y\\\">a&amp;<!--c--><![CDATA[<x>]]><?go now?><p:c/></r>\") ?? panic(\"bad\"))\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n";
    check_comptime_src(2004, "rich lossless XML whole-value round-trip", src);
}

#[test]
fn xml_parse_options_match_comptime_and_runtime() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping XML options comptime differential");
        return;
    }
    let src = "use core.encoding.xml as xml\n@comptime_value :: xml.to_string(xml.parse_with(\"<r><a/></r>\", xml.XMLParseOptions.safe()) ?? panic(\"bad\"))\n\nfn run() {\n    r :: xml.to_string(xml.parse_with(\"<r><a/></r>\", xml.XMLParseOptions.safe()) ?? panic(\"bad\"))\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n";
    check_comptime_src(2005, "typed XML options", src);
}

/// #1657 / I9: `core.data` statistics run one kernel on every tier, so an
/// undefined answer is the same `DataError` everywhere. Empty input is the
/// case a second implementation gets wrong: a naive copy returns 0.0 where the
/// kernel reports `Empty`.
#[test]
fn data_empty_input_error_matches_comptime_and_runtime() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping empty-input core.data differential");
        return;
    }
    for (index, method) in ["sum", "mean", "min", "max", "median", "variance", "stddev"]
        .into_iter()
        .enumerate()
    {
        let src = format!(
            r#"use core.data as data

fn show(result: Float ? DataError) => String {{
    if result == {{
        .Ok(value) -> return "ok {{value}}"
        .Err(e) -> return "{{e.operation}}|{{e.reason}}"
    }}
    return "unreachable"
}}

fn empty() => [Float] {{
    return []
}}

@expected_empty :: show(data.{method}(empty()))

fn run() {{
    actual_empty :: show(data.{method}(empty()))
    print("{{@expected_empty}}")
    print("{{actual_empty}}")
}}
"#
        );
        check_comptime_src(2100 + index, &format!("empty core.data {method}"), &src);
    }
}

#[test]
fn xml_hostile_error_matches_comptime_and_runtime() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping hostile XML comptime differential");
        return;
    }
    let src = r#"use core.encoding.xml as xml

fn show(result: DataTree ? XMLError) => String {
    if result == {
        .Ok(_) -> return "ok"
        .Err(e) -> {
            return "{e.byte_offset}|{e.line}|{e.column}|{e.path}|{e.reason}"
        }
    }
    return "unreachable"
}

@expected_mismatch :: show(xml.parse("<root>\n<a></root>"))

fn run() {
    actual_mismatch :: show(xml.parse("<root>\n<a></root>"))
    print("{@expected_mismatch}")
    print("{actual_mismatch}")
}
"#;
    check_comptime_src(2006, "typed hostile XML mismatch error", src);
}

#[test]
fn cbor_generic_whole_decode_matches_comptime_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping CBOR whole-value differential");
        return;
    }
    // D-ENC-CBOR-SURFACE1 / #296: current generic whole-value decode,
    // including normal-mode indefinite containers and preferred Float16,
    // is one R12 semantic path at comptime and AOT. This intentionally does
    // not exercise the retired untyped `decode(DataTree)` compatibility arm.
    let src = "use core.encoding.cbor as cbor\n@comptime_value :: cbor.decode<[Float]>([159, 249, 62, 0, 249, 64, 0, 255]) ?? panic(\"bad\")\n\nfn run() {\n    r :: cbor.decode<[Float]>([159, 249, 62, 0, 249, 64, 0, 255]) ?? panic(\"bad\")\n    print(\"{@comptime_value}\")\n    print(\"{r}\")\n}\n";
    check_comptime_src(2000, "generic CBOR indefinite Float16 decode", src);
}

#[test]
fn cbor_current_whole_encode_parse_matches_comptime_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping current CBOR whole-value differential");
        return;
    }
    let src = "use core.encoding.json as json\nuse core.encoding.cbor as cbor\nuse core.encoding.hex as hex\n@encoded :: hex.encode(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3],\\\"c\\\":-7}}\") ?? panic(\"bad json\")) ?? panic(\"bad cbor\"))\n@parsed :: json.to_string(cbor.parse(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3]}}\") ?? panic(\"bad json\")) ?? panic(\"bad cbor\")) ?? panic(\"bad parse\"))\n\nfn run() {\n    r :: hex.encode(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3],\\\"c\\\":-7}}\") ?? panic(\"bad json\")) ?? panic(\"bad cbor\"))\n    p :: json.to_string(cbor.parse(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3]}}\") ?? panic(\"bad json\")) ?? panic(\"bad cbor\")) ?? panic(\"bad parse\"))\n    print(\"{@encoded}|{@parsed}\")\n    print(\"{r}|{p}\")\n}\n";
    check_comptime_src(2001, "current CBOR to_bytes and parse", src);
}

#[test]
fn cbor_canonical_typed_corpus_matches_comptime_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping canonical CBOR comptime differential");
        return;
    }
    let src = r#"use core.encoding.json as json
use core.encoding.cbor as cbor
use core.encoding.hex as hex

#Codable
struct Packet { id: Int, payload: [U8] }

@expected_map :: hex.encode(cbor.to_bytes_canonical(json.parse("{{\"aa\":1,\"b\":2}}") ?? panic("json")) ?? panic("canonical"))
@expected_floats :: hex.encode(cbor.to_bytes_canonical([1.5, 100000.0, -0.0]) ?? panic("canonical"))
@expected_nan :: hex.encode(cbor.to_bytes_canonical(Float.NAN) ?? panic("canonical"))
@expected_typed :: hex.encode(cbor.to_bytes_canonical(Packet.{ id: 7, payload: [222, 173] }) ?? panic("canonical"))

fn run() {
    actual_map := hex.encode(cbor.to_bytes_canonical(json.parse("{{\"aa\":1,\"b\":2}}") ?? panic("json")) ?? panic("canonical"))
    actual_floats := hex.encode(cbor.to_bytes_canonical([1.5, 100000.0, -0.0]) ?? panic("canonical"))
    actual_nan := hex.encode(cbor.to_bytes_canonical(Float.NAN) ?? panic("canonical"))
    actual_typed := hex.encode(cbor.to_bytes_canonical(Packet.{ id: 7, payload: [222, 173] }) ?? panic("canonical"))
    if actual_map != "a261620262616101" { panic("canonical encoded-key order drift") }
    if actual_floats != "83f93e00fa47c35000f98000" { panic("preferred Float width drift") }
    if actual_nan != "f97e00" { panic("canonical NaN drift") }
    if actual_typed != "a262696407677061796c6f616442dead" { panic("typed byte-string drift") }
    print("{@expected_map}|{@expected_floats}|{@expected_nan}|{@expected_typed}")
    print("{actual_map}|{actual_floats}|{actual_nan}|{actual_typed}")
}
"#;
    check_comptime_src(2002, "canonical CBOR map/float/typed corpus", src);
}

#[test]
fn cbor_options_and_hostile_errors_match_comptime_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping CBOR option/error differential");
        return;
    }
    let src = r#"use core.encoding.cbor as cbor

fn safe() => cbor.CBOROptions {
    return cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 1073741824, require_canonical: false }
}

fn show(bytes: [U8]) => String {
    if cbor.parse(bytes, safe()) == {
        .Ok(_) -> return "ok"
        .Err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}

fn show_strict(bytes: [U8]) => String {
    if cbor.parse(bytes, cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 1073741824, require_canonical: true }) == {
        .Ok(_) -> return "ok"
        .Err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}
fn show_depth(bytes: [U8]) => String {
    if cbor.parse(bytes, cbor.CBOROptions.{ max_depth: 1, max_items: 1000000, max_bytes: 1073741824, require_canonical: false }) == {
        .Ok(_) -> return "ok"
        .Err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}
fn show_items(bytes: [U8]) => String {
    if cbor.parse(bytes, cbor.CBOROptions.{ max_depth: 256, max_items: 2, max_bytes: 1073741824, require_canonical: false }) == {
        .Ok(_) -> return "ok"
        .Err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}
fn show_bytes(bytes: [U8]) => String {
    if cbor.parse(bytes, cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 2, require_canonical: false }) == {
        .Ok(_) -> return "ok"
        .Err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}
fn show_alloc(bytes: [U8]) => String {
    if cbor.parse(bytes, cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 3, require_canonical: false }) == {
        .Ok(_) -> return "ok"
        .Err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}

fn show_ints(bytes: [U8]) => String {
    if cbor.decode<[Int]>(bytes, safe()) == {
        .Ok(_) -> return "ok"
        .Err(e) -> return "{e[0].path}|{e[0].reason}"
    }
    return "unreachable"
}

@expected_malformed :: show([255])
@expected_truncated :: show([129])
@expected_noncanonical :: show_strict([24, 1])
@expected_unsupported :: show([192, 1])
@expected_mismatch :: show_ints([129, 97, 120])
@expected_depth :: show_depth([129, 129, 1])
@expected_items :: show_items([130, 1, 2])
@expected_bytes :: show_bytes([130, 1, 2])
@expected_alloc :: show_alloc([130, 1, 2])

fn run() {
    malformed_wire := [U8].{ 255 }
    truncated_wire := [U8].{ 129 }
    noncanonical_wire := [U8].{ 24, 1 }
    unsupported_wire := [U8].{ 192, 1 }
    mismatch_wire := [U8].{ 129, 97, 120 }
    depth_wire := [U8].{ 129, 129, 1 }
    items_wire := [U8].{ 130, 1, 2 }
    actual_malformed := show(malformed_wire)
    actual_truncated := show(truncated_wire)
    actual_noncanonical := show_strict(noncanonical_wire)
    actual_unsupported := show(unsupported_wire)
    actual_mismatch := show_ints(mismatch_wire)
    actual_depth := show_depth(depth_wire)
    actual_items := show_items(items_wire)
    actual_bytes := show_bytes(items_wire)
    actual_alloc := show_alloc(items_wire)
    print("{@expected_malformed}~{@expected_truncated}~{@expected_noncanonical}~{@expected_unsupported}~{@expected_mismatch}~{@expected_depth}~{@expected_items}~{@expected_bytes}~{@expected_alloc}")
    print("{actual_malformed}~{actual_truncated}~{actual_noncanonical}~{actual_unsupported}~{actual_mismatch}~{actual_depth}~{actual_items}~{actual_bytes}~{actual_alloc}")
}
"#;
    check_comptime_src(2003, "CBOR options and hostile error projection", src);
}
#[test]
fn local_comptime_is_literal_data() {
    let stdout = compile_and_run(
        r#"
fn build() => [Int] {
    xs := [Int].{}
    loop i, 1..5, 2 {
        if i == 3 { next }
        xs.push(i * 10)
    }
    loop cursor, 0..<3 {
        if cursor == 1 { next }
        xs.push(cursor)
    }
    return xs
}

fn run() {
    @xs :: build()
    runtime :: build()
    print("{@xs}")
    print("{runtime}")
}
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["[10, 50, 0, 2]", "[10, 50, 0, 2]"]
    );
}

#[test]
fn struct_and_enum_comptime_values_round_trip() {
    let stdout = compile_and_run(
        r#"
struct Pair {
    left: Int
    right: String
}

enum Light {
    Red
    Green
}

@pair_value :: Pair.{left: 7, right: "seven"}
@light_value :: Light.Green

fn run() {
    p :: Pair.{left: 7, right: "seven"}
    l :: Light.Green
    print("{@pair_value.left}")
    print("{p.left}")
    print("{@pair_value.right}")
    print("{p.right}")
    print("{@light_value == Light.Green}")
    print("{l == Light.Green}")
}
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["7", "7", "seven", "seven", "true", "true"]
    );
}

#[test]
fn if_expr_comptime_matches_runtime() {
    let stdout = compile_and_run(
        r#"
@true_value :: if 3 > 2 -> 10 else -> 20
@false_value :: if 1 > 2 -> 10 else -> 20

fn run() {
    c :: if 3 > 2 -> 10 else -> 20
    d :: if 1 > 2 -> 10 else -> 20
    print("{@true_value}")
    print("{c}")
    print("{@false_value}")
    print("{d}")
}
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["10", "10", "20", "20"]
    );
}


#[test]
fn ordinary_bindings_fold_when_possible_and_silently_fall_back() {
    let src = r#"
fn show(n: Int) {
    folded :: 40 + 2
    runtime :: n + 1
    print(folded)
    print(runtime)
}

fn run() { show(4) }
"#;
    let compiled = jet::compile(src).expect("implicit folding must not reject runtime fallback");
    assert!(
        compiled.rust.contains("42"),
        "the foldable binding was not baked into generated code:\n{}",
        compiled.rust
    );
    assert!(
        compiled.rust.contains("__jet_n") && compiled.rust.contains("+ 1"),
        "the unsupported binding did not silently remain runtime code:\n{}",
        compiled.rust
    );
    if have_rustc() {
        assert_eq!(compile_and_run(src), "42\n5\n");
    }
}

fn compile_and_run(src: &str) -> String {
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping comptime run fixture");
        return String::new();
    }
    let compiled = match jet::compile(src) {
        Ok(c) => c,
        Err(diags) => panic!(
            "fixture failed the front end:\n{}",
            jet::render_diagnostics("comptime_fixture.jet", src, &diags)
        ),
    };
    let dir = std::env::temp_dir();
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let rs = dir.join(format!("jet_ct_fixture_{}.rs", id));
    let bin = dir.join(format!("jet_ct_fixture_{}", id));
    fs::write(&rs, &compiled.rust).unwrap();
    let out = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "fixture panicked at runtime");
    String::from_utf8(run.stdout).unwrap()
}
