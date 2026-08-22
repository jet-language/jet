#[test]
fn generic_codable_injects_wire_param_bounds() {
    let out = compile_temp(
        "generic_serde.jet",
        r#"
use core.encoding.json as json

#Codable
struct Wrap<T> {
    value: T
}

#Codable
struct Tagged<K> {
    raw: Int
    #Skip marker: K?
}

fn run() {
    print("x")
}
"#,
    );
    let rs = &out.rust;
    // D-SERDE9: the wire-reaching param T carries `__jet_Encode`/`__jet_Decode`.
    assert!(
        rs.contains("impl<T: __jet_Encode") && rs.contains("__jet_Encode for __jet_Wrap<T>"),
        "Wrap's Encode impl must bound T: __jet_Encode\n{rs}"
    );
    assert!(
        rs.contains("impl<T: __jet_Decode") && rs.contains("__jet_Decode for __jet_Wrap<T>"),
        "Wrap's Decode impl must bound T: __jet_Decode\n{rs}"
    );
    // D-SERDE10: the phantom param K gets NO Encode/Decode bound (only Clone).
    // (D-MEM1 S6: struct renamed `Id<K>` -> `Tagged<K>` — `Id<T>` is now the
    // reserved `Pool<T>` handle type.)
    assert!(
        rs.contains("impl<K: Clone> __jet_Encode for __jet_Tagged<K>"),
        "Tagged's Encode impl must NOT bound K with __jet_Encode (phantom param)\n{rs}"
    );
    assert!(
        rs.contains("impl<K: Clone> __jet_Decode for __jet_Tagged<K>"),
        "Tagged's Decode impl must NOT bound K with __jet_Decode (phantom param)\n{rs}"
    );
    assert!(
        !rs.contains("K: __jet_Encode") && !rs.contains("K: __jet_Decode"),
        "phantom param K must never get a serde bound\n{rs}"
    );
}

/// c136: a generic `#[Codable]` value round-trips through json encode/decode, and
/// a phantom-param type serializes regardless of its phantom argument (D-SERDE10).
#[test]
fn generic_codable_round_trips() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping generic serde round-trip (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_gserde_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, _stderr) = build_and_run(
        &dir,
        "gserde",
        r#"
use core.encoding.json as json

#Codable
struct Wrap<T> {
    value: T
}

#Codable
struct Tagged<K> {
    raw: Int
    #Skip marker: K?
}

fn run() {
    wi :: Wrap<Int>{ value: 7 }
    print(json.to_string(wi))
    back :: json.decode<Wrap<Int>>("{{\"value\":42}}") ?? panic("bad")
    print(back.value)
    id :: Tagged<Wrap<Int>>{ raw: 9, marker: None }
    print(json.to_string(id))
    rid :: json.decode<Tagged<Wrap<Int>>>("{{\"raw\":3}}") ?? panic("bad id")
    print(rid.raw)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "generic serde program should run cleanly");
    assert_eq!(stdout, "{\"value\":7}\n42\n{\"raw\":9}\n3\n");
    let _ = fs::remove_dir_all(&dir);
}

// ── c152: full TOML adapter (D-ENC-DYN1=A+) ──────────────────────────────────
// Nested `[table]`s, arrays-of-tables, dotted keys, and typed scalars decode into
// nested `#[Codable]` structs, and the rich tree round-trips through `to_string`.
#[test]
fn toml_full_nested_decode_and_round_trip() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping toml_full_nested_decode_and_round_trip (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_toml_full_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Typed decode into nested structs + array-of-tables.
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "toml_typed",
        r#"
use core.encoding.toml as toml
#Codable
struct Server { host: String  port: Int }
#Codable
struct Config { title: String  server: Server  ports: [Int] }
fn run() {
    raw :: "title = \"jet\"\nports = [80, 443]\n\n[server]\nhost = \"db.local\"\nport = 5432\n"
    cfg :: toml.decode<Config>(raw) ?? panic("bad toml")
    print(cfg.title)
    print(cfg.server.host)
    print(cfg.server.port)
    print(cfg.ports.len())
    print(toml.to_string(cfg))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "toml typed decode failed: {stderr}");
    assert_eq!(
        stdout,
        "jet\ndb.local\n5432\n2\ntitle = \"jet\"\nports = [80, 443]\n\n[server]\nhost = \"db.local\"\nport = 5432\n"
    );

    // Dynamic parse → rich tree → round-trip identity for a nested document.
    let (code2, stdout2, stderr2) = build_and_run(
        &dir,
        "toml_dyn",
        r#"
use core.encoding.toml as toml
fn run() {
    raw :: "name = \"a\"\n\n[db]\nhost = \"h\"\nport = 1\n"
    d :: toml.parse(raw) ?? panic("bad")
    print(toml.to_string(d))
}
"#,
        &[],
        None,
    );
    assert_eq!(code2, 0, "toml dynamic parse failed: {stderr2}");
    assert_eq!(stdout2, "name = \"a\"\n\n[db]\nhost = \"h\"\nport = 1\n");
    let _ = fs::remove_dir_all(&dir);
}

// ── card #131 S1-bridge: hand-written `impl T.Encode` / `impl T.Decode` (D-SERDE2) ──
// A hand codec passes sema and MUST produce Rust rustc accepts (I2). The Jet-facing
// verbs `encode`/`decode` bridge internally to the Rust `__jet_Encode`/`__jet_Decode`
// traits' `jet_encode(&self) -> DataTree` / `jet_decode(&DataTree) -> Result<Self, …>`.
// The impl uses a custom wire key (`"email"`, not the field name `addr`) so the round
// trip can only succeed through the HAND methods, never a derive.
#[test]
fn hand_written_encode_decode_round_trips() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping hand_written_encode_decode_round_trips (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_hand_codec_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "hand_codec",
        r#"
use core.encoding.json as json

struct Email { addr: String }

impl Email.Encode {
    fn encode(self) DataTree {
        m :: [String:DataTree]{ "email": DataTree.Text(~self.addr) }
        return DataTree.Object(m)
    }
}

impl Email.Decode {
    fn decode(tree: DataTree) Email [FieldError]! {
        f := tree.field("email") ?? DataTree.Text("")
        s := f.text() ?? ""
        return .Ok(Email{addr: s})
    }
}

fn run() {
    e := Email{addr: "a@b.com"}
    s := json.to_string(e)
    print(s)
    back := json.decode<Email>(s) ?? panic("decode failed")
    print(back.addr)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "hand codec round trip failed: {stderr}");
    // Custom wire key proves the hand `encode` ran; `back.addr` proves hand `decode` ran.
    assert_eq!(stdout, "{\"email\":\"a@b.com\"}\na@b.com\n");
    let _ = fs::remove_dir_all(&dir);
}

/// D-SERDE2/D-SERDE16: the beginner `#Codable` request and the expert hand
/// codec both feed the same DataTree protocol, so their wire bytes and decoded
/// values agree for the same shape.
#[test]
fn derive_and_hand_written_codecs_share_one_engine() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping derive_and_hand_written_codecs_share_one_engine (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_codec_engine_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "codec_engine",
        r#"
use core.encoding.json as json

#Codable
struct DerivedEmail { address: String }

struct HandEmail { address: String }
impl HandEmail.Encode {
    fn encode(self) DataTree -> DataTree.Object(["address": DataTree.Text(~self.address)])
}
impl HandEmail.Decode {
    fn decode(tree: DataTree) HandEmail [FieldError]! {
        field :: tree.field("address") ?? DataTree.Text("")
        address :: field.text() ?? ""
        return Ok(HandEmail{ address: address })
    }
}

fn run() {
    derived :: DerivedEmail{ address: "ada@jet" }
    hand :: HandEmail{ address: "ada@jet" }
    derived_wire :: json.to_string(derived)
    hand_wire :: json.to_string(hand)
    derived_bytes :: derived_wire.bytes()
    hand_bytes :: hand_wire.bytes()
    assert(derived_bytes == hand_bytes)
    derived_back :: json.decode<DerivedEmail>(hand_wire) ?? panic("derived decode")
    hand_back :: json.decode<HandEmail>(derived_wire) ?? panic("hand decode")
    assert(derived_back.address == hand_back.address)
    print(derived_bytes == hand_bytes)
    print(derived_back.address == hand_back.address)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "derive/hand codec parity failed: {stderr}");
    assert_eq!(stdout, "true\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

/// D-SERDE9/D-SERDE16: a generic expert entry point calls the same Encode and
/// Decode contracts emitted for a generic `#Codable` type.
#[test]
fn serde_derive_and_generic_share_one_engine() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping serde_derive_and_generic_share_one_engine (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_serde_engine_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "serde_engine",
        r#"
use core.encoding.json as json

#Codable
struct Boxed<T> { value: T }

fn generic_encode<T: Encode>(value: T) String -> json.to_string(value)
fn generic_decode<T: Decode>(wire: String) T [FieldError]! -> json.decode<T>(wire)

fn run() {
    value :: Boxed<Int>{ value: 7 }
    derived_wire :: json.to_string(value)
    generic_wire :: generic_encode(value)
    derived_bytes :: derived_wire.bytes()
    generic_bytes :: generic_wire.bytes()
    assert(derived_bytes == generic_bytes)
    derived_back :: json.decode<Boxed<Int>>(generic_wire) ?? panic("derived decode")
    generic_back :: generic_decode<Boxed<Int>>(derived_wire) ?? panic("generic decode")
    assert(derived_back.value == generic_back.value)
    print(derived_bytes == generic_bytes)
    print(derived_back.value == generic_back.value)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "derive/generic serde parity failed: {stderr}");
    assert_eq!(stdout, "true\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

/// card #131: `DataTree.decode<T>()` dispatches primitive, container,
/// generated, and hand-written Decode implementations through one spelling.
#[test]
fn datatree_decode_dispatches_all_decode_impl_kinds() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_datatree_decode_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
#Codable
struct Point { x: Int }
struct Email { addr: String }
impl Email.Decode {
    fn decode(tree: DataTree) Email [FieldError]! {
        value := tree.field("address") ?? DataTree.Text("")
        return .Ok(Email{ addr: value.text() ?? "" })
    }
}

fn run() {
    i_tree := DataTree.Int(41)
    xs_tree := DataTree.Array([DataTree.Int(1), DataTree.Int(2)])
    p_tree := DataTree.Object(["x": DataTree.Int(7)])
    e_tree := DataTree.Object(["address": DataTree.Text("a@b")])
    i := i_tree.decode<Int>() ?? panic("primitive")
    xs := xs_tree.decode<[Int]>() ?? panic("list")
    p := p_tree.decode<Point>() ?? panic("derive")
    e := e_tree.decode<Email>() ?? panic("hand")
    print(i + xs[1] + p.x)
    print(e.addr)
}
"#;
    let out = compile_temp("datatree_decode.jet", src);
    assert!(out.rust.contains("<i64 as __jet_Decode>::jet_decode"));
    assert!(out.rust.contains("<__jet_Point as __jet_Decode>::jet_decode"));
    assert!(out.rust.contains("<__jet_Email as __jet_Decode>::jet_decode"));
    let (code, stdout, stderr) = build_and_run(&dir, "datatree_decode", src, &[], None);
    assert_eq!(code, 0, "DataTree.decode dispatch failed: {stderr}");
    assert_eq!(stdout, "50\na@b\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn generated_enum_codecs_reenter_jet_pipeline() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_enum_serde_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json
#Codable
enum Event {
    Idle
    Count(Int)
    Named(name: String, enabled: Bool)
}
fn run() {
    a := Event.Idle
    b := Event.Count(3)
    c := Event.Named{ name: "x", enabled: true }
    print(json.to_string(a))
    print(json.to_string(b))
    print(json.to_string(c))
    back := json.decode<Event>("{{\"Count\":7}}") ?? panic("decode")
    if back == .Count(n) { print(n) }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "enum_serde", src, &[], None);
    assert_eq!(code, 0, "generated enum codec failed: {stderr}");
    assert_eq!(stdout, "\"Idle\"\n{\"Count\":3}\n{\"Named\":{\"name\":\"x\",\"enabled\":true}}\n7\n");
    let _ = fs::remove_dir_all(&dir);
}

/// D-SERDE7: internal tags apply uniformly to unit, single-payload, and
/// named-payload variants. Exact JSON plus decode proves the AOT contract.
#[test]
fn generated_internal_tagged_enum_round_trips_every_variant_shape() {
    let dir = std::env::temp_dir().join(format!("jet_tagged_enum_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#[Codable, Discriminant("type")]
enum Event {
    Idle
    Count(Int)
    Named(name: String, enabled: Bool)
}

fn run() {
    unit := Event.Idle
    tuple := Event.Count(3)
    named := Event.Named{ name: "x", enabled: true }
    print(json.to_string(unit))
    print(json.to_string(tuple))
    print(json.to_string(named))
    a := json.decode<Event>("{{\"type\":\"Idle\"}}") ?? panic("unit")
    b := json.decode<Event>("{{\"type\":\"Count\",\"value\":7}}") ?? panic("tuple")
    c := json.decode<Event>("{{\"type\":\"Named\",\"name\":\"y\",\"enabled\":false}}") ?? panic("named")
    print(json.to_string(a))
    print(json.to_string(b))
    print(json.to_string(c))
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "tagged_enum", src, &[], None);
    assert_eq!(code, 0, "generated internally tagged enum failed: {stderr}");
    assert_eq!(
        stdout,
        "{\"type\":\"Idle\"}\n{\"type\":\"Count\",\"value\":3}\n{\"type\":\"Named\",\"name\":\"x\",\"enabled\":true}\n{\"type\":\"Idle\"}\n{\"type\":\"Count\",\"value\":7}\n{\"type\":\"Named\",\"name\":\"y\",\"enabled\":false}\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn builtin_codec_expansion_has_no_ast_transplant_or_rust_fallback() {
    let bundle_pipeline = include_str!("../../crates/jet-sema/src/Sema/Bundle/Pipeline.rs");
    let serde = include_str!("../../crates/jet-sema/src/Sema/Registration/Serde.rs");
    let items = include_str!("../../crates/jet-codegen/src/Codegen/Items.rs");
    assert!(bundle_pipeline.contains(
        "super::super::Registration::expand_builtin_serde_items(&mut module.items, &mut diags);"
    ));
    assert!(serde.contains("fn struct_codec_items"));
    assert!(serde.contains("fn enum_codec_items"));
    assert!(serde.contains("is_generated_serde: true"));
    assert!(!serde.contains("Lexer::lex"));
    assert!(!serde.contains("Parser::parse"));
    assert!(!serde.contains("parse_generated_fragment"));
    assert!(!items.contains("emit_struct_serde"));
    assert!(!items.contains("emit_enum_serde"));
}

/// Card #131: generated struct codecs preserve field-policy behavior while
/// running through ordinary Jet bodies: absent options stay off the wire and
/// computed fields encode through their getter without becoming decode slots.
#[test]
fn generated_struct_codecs_preserve_option_and_computed_fields() {
    let dir = std::env::temp_dir().join(format!("jet_struct_serde_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#Codable
struct Record {
    base: Int
    note: String?
    doubled: Int -> base * 2
}

fn run() {
    value := Record{ base: 4, note: None }
    print(json.to_string(value))
    back := json.decode<Record>("{{\"base\":5,\"doubled\":999}}") ?? panic("decode")
    print(back.base)
    print(back.doubled)
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_serde", src, &[], None);
    assert_eq!(code, 0, "generated struct codec failed: {stderr}");
    assert_eq!(stdout, "{\"base\":4,\"doubled\":8}\n5\n10\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn nested_pattern_subjects_clone_read_self_and_keep_take_self_by_value() {
    fn method_body<'a>(rust: &'a str, name: &str) -> &'a str {
        let tail = rust
            .split_once(&format!("fn {}", jet::AST::mangle(name)))
            .map(|(_, tail)| tail)
            .unwrap_or_else(|| panic!("missing generated method `{name}`"));
        let next_method = tail.find("\n    fn __jet_");
        let impl_end = tail.find("\n}\n");
        let end = match (next_method, impl_end) {
            (Some(a), Some(b)) => a.min(b),
            (Some(end), None) | (None, Some(end)) => end,
            (None, None) => tail.len(),
        };
        &tail[..end]
    }

    let dir = std::env::temp_dir().join(format!("jet_nested_pattern_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
struct Inner { note: String? }
struct Envelope {
    inner: Inner

    fn borrowed(self) String {
        if self.inner.note == Val(value) { return value }
        return "none"
    }

    fn owned(^self) String {
        if self.inner.note == Val(value) { return value }
        return "none"
    }
}
fn owned_local_nested_field_remains_reusable() {
    local := Envelope{ inner: Inner{ note: Val("local") } }
    if local.inner.note == Val(value) { print(value) }
    if local.inner.note == Val(value) { print(value) }
}
fn run() {
    borrowed := Envelope{ inner: Inner{ note: Val("borrowed") } }
    print(borrowed.borrowed())
    owned := Envelope{ inner: Inner{ note: Val("owned") } }
    print(owned.owned())
    owned_local_nested_field_remains_reusable()
}
"#;
    let out = compile_temp("nested_pattern_borrow_provenance.jet", src);
    let borrowed = method_body(&out.rust, "borrowed");
    let owned = method_body(&out.rust, "owned");
    assert!(borrowed.contains(".clone()"), "{borrowed}");
    assert!(!owned.contains(".clone()"), "{owned}");

    let (code, stdout, stderr) = build_and_run(&dir, "nested_pattern", src, &[], None);
    assert_eq!(code, 0, "nested borrowed/take-self proof failed: {stderr}");
    assert_eq!(stdout, "borrowed\nowned\nlocal\nlocal\n");
    let _ = fs::remove_dir_all(&dir);
}

/// Derived objects keep declaration order across renamed fields and optional
/// omission. Ordinary maps retain their independent key-ordering behavior.
#[test]
fn generated_struct_encode_preserves_order_with_rename_and_option() {
    let dir = std::env::temp_dir().join(format!("jet_struct_serde_order_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#Encode
struct Wire {
    first: String
    #Rename("wireSecond") second: String
    maybe: String?
    last: Int
}

fn run() {
    absent := Wire{ first: "a", second: "b", maybe: None, last: 4 }
    present := Wire{ first: "a", second: "b", maybe: Val("c"), last: 4 }
    arbitrary := [String:Int]{ "z": 1, "a": 2 }
    print(json.to_string(absent))
    print(json.to_string(present))
    print(json.to_string(arbitrary))
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_serde_order", src, &[], None);
    assert_eq!(code, 0, "ordered generated struct codec failed: {stderr}");
    assert_eq!(
        stdout,
        "{\"first\":\"a\",\"wireSecond\":\"b\",\"last\":4}\n{\"first\":\"a\",\"wireSecond\":\"b\",\"maybe\":\"c\",\"last\":4}\n{\"a\":2,\"z\":1}\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Card #131: flatten, rename, and decode defaults are behavior of generated
/// Jet codec bodies, not a hidden Rust-only derive path.
#[test]
fn generated_struct_codecs_preserve_flatten_rename_and_default() {
    let dir = std::env::temp_dir().join(format!("jet_struct_markers_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#Codable
struct Inner { x: Int  y: Bool }

#[Codable, RenameAll(camel)]
struct Outer {
    display_name: String
    #Flatten inner: Inner
    count: Int{4 + 5}
}

fn run() {
    value := Outer{ display_name: "n", inner: Inner{ x: 1, y: true }, count: 2 }
    print(json.to_string(value))
    back := json.decode<Outer>("{{\"displayName\":\"m\",\"x\":3,\"y\":false}}") ?? panic("decode")
    print(back.display_name)
    print(back.inner.x)
    print(back.count)
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_markers", src, &[], None);
    assert_eq!(code, 0, "generated marker codec failed: {stderr}");
    assert_eq!(stdout, "{\"count\":2,\"displayName\":\"n\",\"x\":1,\"y\":true}\nm\n3\n9\n");
    let _ = fs::remove_dir_all(&dir);
}

/// Card #131 / D-SERDE8: strict unknown-key rejection is emitted as ordinary
/// Jet control flow and carries the offending wire path plus E2412 reason.
#[test]
fn generated_struct_decode_denies_unknown_fields() {
    let dir = std::env::temp_dir().join(format!("jet_struct_deny_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#[Codable, DenyUnknownFields]
struct Strict { name: String }

fn run() {
    result := json.decode<Strict>("{{\"name\":\"x\",\"extra\":1}}")
    if result == .Err(errors) {
            loop error in errors {
            print(error.path)
            print(error.reason)
        }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_deny", src, &[], None);
    assert_eq!(code, 0, "generated strict codec failed: {stderr}");
    assert_eq!(stdout, "extra\nE2412: unknown field `extra`\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn generated_struct_decode_accumulates_nested_errors_and_validation() {
    let dir = std::env::temp_dir().join(format!("jet_struct_decode_errors_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#Codable
struct Inner { left: Int  right: Bool }

#Codable
struct Outer { inner: Inner  count: Int }

#Codable
struct Account {
    email: String
    age: Int

    validate {
        check(email.contains("@"), at: email, "email")
        check(age >= 18, at: age, "age")
    }
}

fn run() {
    malformed := json.decode<Outer>("{{\"inner\":{{\"left\":\"bad\",\"right\":\"bad\"}},\"count\":\"bad\"}}")
    if malformed == .Err(errors) {
        print(errors.len())
        loop error in errors { print(error.path) }
    }
    invalid := json.decode<Account>("{{\"email\":\"missing-at\",\"age\":12}}")
    if invalid == .Err(errors) {
        print(errors.len())
        loop error in errors { print(error.path) }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_decode_errors", src, &[], None);
    assert_eq!(code, 0, "generated decoder accumulation failed: {stderr}");
    assert_eq!(stdout, "3\ninner.left\ninner.right\ncount\n2\nemail\nage\n");
    let _ = fs::remove_dir_all(&dir);
}

/// Card #131: built-in Decode fragments live beside their source type, so a
/// consumer can dispatch through an imported type without entry-local aliases.
/// The nested List/Option/Map fields also prove D-SERDE16's public dispatch.
#[test]
fn generated_decode_dispatches_across_module_boundaries() {
    let dir = std::env::temp_dir().join(format!("jet_serde_module_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let lib = r#"
#Codable
pub struct Address { pub city: String }

#Codable
pub struct Order {
    pub shipping: Address
    pub quantities: [Int]
    pub coupon: String?
    pub labels: [String:Int]
}

"#;
    let main = r#"
use core.encoding.json as json
use orders

fn run() {
    order := json.decode<orders.Order>("{{\"shipping\":{{\"city\":\"Paris\"}},\"quantities\":[2,3],\"coupon\":null,\"labels\":{{\"fragile\":1}}}}") ?? panic("decode")
    print(json.to_string(order))
}
"#;
    let (code, stdout, stderr) = build_and_run_multi(
        &dir,
        "serde_module",
        "main.jet",
        &[("main.jet", main), ("orders.jet", lib)],
    );
    assert_eq!(code, 0, "cross-module generated decode failed: {stderr}");
    assert_eq!(stdout, "{\"shipping\":{\"city\":\"Paris\"},\"quantities\":[2,3],\"labels\":{\"fragile\":1}}\n");
    let _ = fs::remove_dir_all(&dir);
}

/// D-METADERIVE1 orphan law: expansion is legal when either derive provider
/// or target type is entry-local. Both directions must generate usable code.
#[test]
fn user_derive_orphan_rule_allows_either_local_side() {
    let dir = std::env::temp_dir().join(format!("jet_derive_orphan_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let lib = r#"
derive T.RemoteLabel {
    fn remote_label(self) String -> "remote:{T.@name}"
}

#LocalLabel
pub struct RemoteType { pub value: Int }

pub fn remote_type_label() String {
    value := RemoteType{ value: 2 }
    return value.local_label()
}
"#;
    let main = r#"
use labels

derive T.LocalLabel {
    fn local_label(self) String -> "local:{T.@name}"
}

#RemoteLabel
struct LocalType { value: Int }

fn run() {
    local := LocalType{ value: 1 }
    print(local.remote_label())
    print(labels.remote_type_label())
}
"#;
    let (code, stdout, stderr) = build_and_run_multi(
        &dir,
        "derive_orphan",
        "main.jet",
        &[("main.jet", main), ("labels.jet", lib)],
    );
    assert_eq!(code, 0, "local-orphan derive dispatch failed: {stderr}");
    assert_eq!(stdout, "remote:LocalType\nlocal:RemoteType\n");
    let _ = fs::remove_dir_all(&dir);
}

/// D-LAYOUT-FACTS1=B: the focused fact and full reflection projection share
/// one typed layout model, including typed field selection and explicit
/// provenance for the default, C, and columnar declarations.
#[test]
fn user_derive_layout_fact_matches_reflection_projection() {
    let dir = std::env::temp_dir().join(format!("jet_layout_facts_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
derive T.LayoutFacts {
    info :: T.@layout
    reflected :: T.reflect().layout
    selected :: info[.count]
    kind :: info.kind
    target :: info.target
    guarantee :: info.guarantee
    source :: info.source
    reflected_kind :: reflected.kind
    field_name :: selected.name
    size :: info.size
    reflected_size :: reflected.size
    selected_offset :: selected.offset
    reflected_offset :: reflected.fields[0].offset
    name :: T.reflect().name
    fn layout_facts(self) String -> "{@kind}:{@target}:{@guarantee}:{@source}:{@reflected_kind}:{@field_name}:{@size}:{@reflected_size}:{@selected_offset}:{@reflected_offset}"
}

#LayoutFacts
struct Packet {
    count: Int
    label: String
}

#[Layout(c), LayoutFacts]
struct CPacket {
    count: U32
    flag: U8
}

#[Layout(columnar), LayoutFacts]
struct ColumnPacket {
    count: Int
    label: String
}

fn run() {
    packet := Packet{ count: 2, label: "ok" }
    c_packet := CPacket{ count: 2, flag: 1 }
    column_packet := ColumnPacket{ count: 2, label: "ok" }
    print(packet.layout_facts())
    print(c_packet.layout_facts())
    print(column_packet.layout_facts())
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "layout_facts", src, &[], None);
    assert_eq!(code, 0, "layout facts derive failed: {stderr}");
    assert_eq!(
        stdout,
        "default:x86_64-unknown-linux-gnu:physical layout unspecified:struct declaration:default:count:null:null:null:null\n"
            .to_string()
            + "c:x86_64-unknown-linux-gnu:repr(C) declaration:struct declaration:c:count:8:8:0:0\n"
            + "columnar:x86_64-unknown-linux-gnu:columnar storage declaration:struct declaration:columnar:count:32:32:0:0\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Card #129 / R11: generated declarations are ordinary Jet items. They must
/// be registered before later generated code (here `#[Codable]`) is checked,
/// and `#[Default(expr)]` must retain its exact compile-time value.
#[test]
fn user_derive_generated_struct_reenters_registration_and_serde() {
    let dir = std::env::temp_dir().join(format!("jet_derive_reentry_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

derive T.ConfigSchema {
    #Codable
    struct GeneratedConfig {
        ports: [Int]{[80, 443]}
    }
}

#ConfigSchema
struct Schema<T> { witness: T }

fn run() {
    config := json.decode<GeneratedConfig>("{{}}") ?? panic("decode")
    print(config.ports)
}
"#;
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "user_derive_generated_struct",
        src,
        &[],
        None,
    );
    assert_eq!(code, 0, "generated struct did not re-enter registration: {stderr}");
    assert_eq!(stdout, "[80, 443]\n");
    let _ = fs::remove_dir_all(&dir);
}

/// Card #129 / D-METADERIVE1: an emitted inherent impl keeps the target's
/// generic identity through sema, TIR, AOT, and default `jet dev`.
#[test]
fn user_derive_generic_impl_runs_in_aot_and_default_dev() {
    let dir = std::env::temp_dir().join(format!("jet_derive_generic_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
derive T.TypeName {
    info :: T.reflect()
    param :: info.type_params[0].name
    fn get_value(self) @param -> ~self.value
    fn type_name(self) String -> T.@name
}

#TypeName
struct Box<T> { value: T }

fn run() {
    boxed := Box<Int>{ value: 7 }
    n := boxed.get_value()
    print(n)
    print(boxed.type_name())
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "user_derive_generic", src, &[], None);
    assert_eq!(code, 0, "generic user derive failed in AOT: {stderr}");
    assert_eq!(stdout, "7\nBox\n");

    let file = dir.join("user_derive_generic.jet");
    fs::write(&file, src).unwrap();
    match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "generic user derive failed in dev: {stderr}");
            assert_eq!(stdout, "7\nBox\n");
        }
        other => panic!("generic user derive did not run in default dev: {other:?}"),
    }
    match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, true) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(
                exit_code,
                0,
                "generic user derive failed in the TIR interpreter: {stderr}"
            );
            assert_eq!(stdout, "7\nBox\n");
        }
        other => panic!("generic user derive did not run in the TIR interpreter: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

/// R11 also means generated code gets ordinary semantic rejection. A derive
/// cannot smuggle a non-duplicable function value through explicit `copy`.
#[test]
fn user_derive_generated_non_clonable_copy_is_rejected_in_sema() {
    let src = r#"
derive T.CopyCallback {
    fn duplicate(self) fn(Int) Int -> ~self.callback
}

#CopyCallback
struct Handler { callback: fn(Int) Int }

fn run() { print(0) }
"#;
    let diags = jet::compile(src).expect_err("generated function copy must be rejected");
    assert!(
        diags.iter().any(|diag| diag.code == "E0211"),
        "expected generated code to re-enter cloneability checking: {diags:?}"
    );
}

/// D-STRUCT-ONCE1=A: the existing typed `@loop` reaches marker bodies, root
/// impl-item position, and test declarations. The generated names and test
/// slots must survive the ordinary sema/codegen path together.
#[test]
fn structure_once_loops_cover_marker_impl_and_test_items() {
    let dir = std::env::temp_dir().join(format!(
        "jet_structure_once_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("structure_once.jet");
    let source = r#"
marker AddFields(@sites: [.Type]) {
    type_name :: target.@name
    @loop field in target.@fields {
        method :: "field_{field.@name}"
        impl @type_name {
            fn @method(self) String -> field.@name
        }
    }
}

struct Case { n: Int }

#AddFields
struct Person { first: String  last: String }

@cases :: [Case]{ Case{ n: 1 }, Case{ n: 2 } }

@loop T in [Person] {
    impl T {
        fn generated(self) String -> "generated"
    }
}

@loop case in @cases {
    #Test("case {case.n}") {
        assert(case.n > 0)
    }
}

fn run() {
    person :: Person{ first: "a", last: "b" }
    print(person.field_first())
    print(person.field_last())
    print(person.generated())
}
"#;
    fs::write(&file, source).unwrap();
    let shown = file.to_string_lossy();
    let (harness, _) = jet::compile_tests_with_path(source, &shown).unwrap_or_else(|diags| {
        panic!(
            "D-STRUCT-ONCE1 fixture rejected:\n{}",
            jet::render_diagnostics(&shown, source, &diags)
        )
    });
    assert!(harness.contains("field_first"), "marker-body method missing");
    assert!(harness.contains("field_last"), "marker-body method missing");
    assert!(harness.contains("generated"), "closed type-list impl missing");
    assert!(harness.contains("case 1"), "first interpolated test missing");
    assert!(harness.contains("case 2"), "second interpolated test missing");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn structure_once_duplicate_generated_tests_reenter_test_registration() {
    let source = r#"
struct Case { n: Int }
@cases :: [Case]{ Case{ n: 1 }, Case{ n: 1 } }

@loop case in @cases {
    #Test("case {case.n}") {
        assert(case.n > 0)
    }
}

fn run() {}
"#;
    let diagnostics = jet::compile(source).expect_err("duplicate generated tests must fail");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0105"),
        "expected duplicate generated test diagnostic, got: {diagnostics:?}"
    );
}

#[test]
fn structure_once_example_runs_aot_with_generated_meaning() {
    if !common::have_rustc() {
        eprintln!("note: skipping structure_once_example_runs_aot_with_generated_meaning (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_structure_once_aot_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = include_str!("../../examples/features/reflection/derive_loop.jet");
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "structure_once_aot",
        source,
        &[],
        None,
    );
    assert_eq!(code, 0, "D-STRUCT-ONCE1 AOT example failed: {stderr}");
    assert_eq!(stdout, "x\ny\nx\ny\ngenerated\n");
    let _ = fs::remove_dir_all(&dir);
}

/// #495 / I2: a field read from a bare (`Read`) parameter is still rooted in
/// the borrowed parameter. The explicit `~` required by E0209 must produce
/// owned values for both shallow and nested fields, compile through rustc, and
/// run with the expected data.
#[test]
fn consuming_core_constructor_copies_borrowed_field_explicitly() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_borrowed_field_copy_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "core_borrowed_field_copy",
        r#"
use core.encoding.json as json

struct Address { text: String }
struct Email { addr: String, nested: Address, items: [Address] }

fn pick() Int {
    return 0
}

fn encoded(e: Email, i: Int) String {
    shallow := DataTree.Text(~e.addr)
    nested := DataTree.Text(~e.nested.text)
    indexed := DataTree.Text(~e.items[0].text)
    computed := DataTree.Text(~e.items[i + 1].text)
    called := DataTree.Text(~e.items[pick()].text)
    parenthesized := DataTree.Text(~e.items[-(-i)].text)
    conditional := DataTree.Text(~e.items[if i == 0 -> 0 else -> 1].text)
    return "{json.to_string(shallow)}|{json.to_string(nested)}|{json.to_string(indexed)}|{json.to_string(computed)}|{json.to_string(called)}|{json.to_string(parenthesized)}|{json.to_string(conditional)}"
}

fn slice_data(xs: [DataTree]) DataTree {
    return DataTree.Array(xs[0..1])
}

fn run() {
    e := Email{addr: "a@b.com", nested: Address{text: "inside"}, items: [Address{text: "zero"}, Address{text: "item"}]}
    sliced := slice_data([DataTree.Text("slice0"), DataTree.Text("slice1")])
    print("{encoded(e, 0)}|{json.to_string(sliced)}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "explicit field copy failed to compile/run: {stderr}");
    assert_eq!(
        stdout,
        "\"a@b.com\"|\"inside\"|\"zero\"|\"item\"|\"zero\"|\"zero\"|\"zero\"|[\"slice0\",\"slice1\"]\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ── c152: full YAML adapter (D-ENC-YAML1 = A) ────────────────────────────────
// Block mappings + sequences, flow collections, typed scalars, block scalars,
// comments, document markers, and anchors/aliases.
#[test]
fn yaml_full_nested_decode_and_features() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping yaml_full_nested_decode_and_features (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_yaml_full_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Typed decode of a nested document with a block sequence of mappings.
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "yaml_typed",
        r#"
use core.encoding.yaml as yaml
#Codable
struct Service { name: String  port: Int }
#Codable
struct Config { app: String  services: [Service] }
fn run() {
    raw :: "app: myapp\nservices:\n  - name: web\n    port: 80\n  - name: db\n    port: 5432\n"
    cfg :: yaml.decode<Config>(raw) ?? panic("bad yaml")
    print(cfg.app)
    print(cfg.services.len())
    print(cfg.services[0].name)
    print(cfg.services[1].port)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "yaml typed decode failed: {stderr}");
    assert_eq!(stdout, "myapp\n2\nweb\n5432\n");

    // Advanced features: flow collections, comments, `---`, anchors/aliases, block scalar.
    let (code2, stdout2, stderr2) = build_and_run(
        &dir,
        "yaml_adv",
        r#"
use core.encoding.yaml as yaml
fn run() {
    raw :: "---\n# a config\nflowlist: [1, 2, 3]\nbase: &b\n  host: local\n  port: 80\nuse: *b\nnote: |\n  one\n  two\n"
    d :: yaml.parse(raw) ?? panic("bad yaml")
    if d == .Object(top) {
        if top["flowlist"] == .Array(xs) {
            print(xs.len())
        }
        if top["use"] == .Object(u) {
            print(u.len())
        }
        if top["note"] == .Text(s) {
            print(s.contains("one"))
        }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code2, 0, "yaml advanced features failed: {stderr2}");
    assert_eq!(stdout2, "3\n2\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

// D-VALIDATE-DECODE1=B / D-MIGRATE4=A: one canonical typed decode contract.
// Published-schema migration is silent inside Result<T, [FieldError]>; the
// migration report is internal to the shared codec implementation.
#[test]
fn typed_decode_returns_field_errors_and_silently_migrates() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping typed decode contract test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_decode_contract_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "decode_contract",
        r#"
use core.encoding.json as json
use core.encoding.toml as toml
use core.encoding.csv as csv

#[PublishedSchema, Codable]
struct Config { port: Int  host: String }

migration Config {
    add host: String = "localhost"
}

fn run() [FieldError]! {
    old :: json.decode<Config>("{{\"port\":2}}")?
    print("json {old.port} {old.host}")
    toml_old :: toml.decode<Config>("port = 3\n")?
    print("toml {toml_old.port} {toml_old.host}")
    rows :: csv.decode<Config>("port\n4\n5\n")?
    print("csv {rows.len()} {rows[1].port} {rows[1].host}")
    if json.decode<Config>("{{\"port\":\"nope\",\"host\":\"h\"}}") == {
        .Ok(value) -> print("unexpected {value.port}")
        .Err(errors) -> print("error {errors.len()} {errors[0].path} {errors[0].reason}")
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "canonical decode program failed: {stderr}");
    assert_eq!(
        stdout,
        "json 2 localhost\ntoml 3 localhost\ncsv 2 5 localhost\nerror 1 port expected Int, found text \"nope\"\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn typed_decode_has_no_second_success_result_api() {
    let src = r#"
use core.encoding.json as json

#[PublishedSchema, Codable]
struct User { id: Int  name: String }

migration User {
    rename old_name => name
}

fn run() {
    user :: json.decode<User>("{{\"id\":1,\"old_name\":\"Ada\"}}") ?? panic("bad")
    print(user.name)
}
"#;
    let shown = "decode_contract_api.jet";
    let out = jet::compile_with_path(src, shown).unwrap_or_else(|diags| {
        panic!("front end rejected fixture:\n{}", jet::render_diagnostics(shown, src, &diags))
    });
    assert!(out.rust.contains("fn jet_decode("));
    assert!(!out.rust.contains("jet_decode_with_status"));
}

/// The production interpreter and AOT compiler both consume the same
/// migration semantics. This source is intentionally small enough to run as
/// a focused jet run proof when the full Rust harness is unavailable.
#[test]
fn typed_decode_tier_fixture_is_canonical() {
    let jet = jet_bin();
    if !common::have_rustc() || !jet.exists() {
        eprintln!("note: skipping typed decode tier fixture (need jet + rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_decode_tiers_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#[PublishedSchema, Codable]
struct Config { port: Int  host: String }

migration Config {
    add host: String = "localhost"
}

fn run() {
    value :: json.decode<Config>("{{\"port\":7}}") ?? panic("bad")
    print("{value.port} {value.host}")
}
"#;
    let (code, compiled, stderr) = build_and_run(&dir, "decode_tier_fixture", src, &[], None);
    assert_eq!(code, 0, "full decode fixture failed: {stderr}");
    let path = dir.join("decode_tier_fixture.jet");
    fs::write(&path, src).unwrap();
    let quick = Command::new(&jet).arg("run").arg(&path).current_dir(&dir).output().unwrap();
    assert!(quick.status.success(), "quick decode fixture failed: {}", String::from_utf8_lossy(&quick.stderr));
    assert_eq!(String::from_utf8_lossy(&quick.stdout), compiled);
    let _ = fs::remove_dir_all(&dir);
}
