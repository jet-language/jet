mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("jet-bind-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn generated(
    format: &str,
    path: &str,
    input: &str,
    root: Option<&str>,
    command: &str,
) -> String {
    jet::CBind::generate_data(format, path, input, root, command)
        .unwrap()
        .source
}

fn assert_success(output: std::process::Output) {
    assert!(
        output.status.success(),
        "status={:?}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn data_bind_generates_all_formats() {
    let json = generated(
        "json",
        "fixtures/repo.json",
        r#"{"repo-name":"jet","stars":4,"active":true,"owner":{"name":"Ada"},"tags":["lang","safe"],"note":null}"#,
        Some("Repo"),
        "jet bind json fixtures/repo.json --type Repo",
    );
    assert!(json.contains("#Codable"));
    assert!(json.contains("struct Repo"));
    assert!(json.contains(r#"#Rename("repo-name") repo_name: String"#));
    assert!(json.contains("owner: RepoOwner"));
    assert!(json.contains("tags: [String]"));
    assert!(json.contains("note: DataTree?"));

    let csv = generated(
        "csv",
        "fixtures/people.csv",
        "name,age,active,notes\n\"Ada, Lovelace\",36,true,\"said \"\"hi\"\"\"\nLin,,false,\"line 1\nline 2\"\n",
        Some("Person"),
        "jet bind csv fixtures/people.csv --type Person",
    );
    assert!(csv.contains("struct Person"));
    assert!(csv.contains("name: String"));
    assert!(csv.contains("age: Int?"));
    assert!(csv.contains("active: Bool"));
    assert!(csv.contains("notes: String"));

    let sql = generated(
        "sql",
        "schema.sql",
        "CREATE TABLE users (id INTEGER PRIMARY KEY, price DECIMAL(10,2), active BOOLEAN NOT NULL, created TIMESTAMP, data BLOB);\nCREATE TABLE audit (event TEXT NOT NULL);",
        Some("Root"),
        "jet bind sql schema.sql --type Root",
    );
    assert!(sql.contains("struct Users"));
    assert!(sql.contains("id: Int"));
    assert!(sql.contains("price: Decimal?"));
    assert!(sql.contains("active: Bool"));
    assert!(sql.contains("created: DateTime?"));
    assert!(sql.contains("data: Bytes?"));
    assert!(sql.contains("struct Audit"));
    assert!(!sql.contains("struct Root"));

    let xml = generated(
        "xml",
        "catalog.xml",
        "<catalog><!-- comment --><book id=\"1\"><title><![CDATA<A]]></title></book><book id=\"2\"><title>B</title></book></catalog>",
        Some("Catalog"),
        "jet bind xml catalog.xml --type Catalog",
    );
    assert!(xml.contains("struct Catalog"));
    assert!(xml.contains("book: [CatalogBook]"));
    assert!(xml.contains(r#"#Rename("@id") id: String"#));
    assert!(xml.contains("title: String"));

    let proto = generated(
        "proto",
        "repo.proto",
        "syntax = \"proto3\";\nmessage Repo { string name = 1; repeated Tag tags = 2; optional int64 stars = 3; }\nmessage Tag { string value = 1; }\n",
        Some("Repo"),
        "jet bind proto repo.proto --type Repo",
    );
    assert!(proto.contains("struct Repo"));
    assert!(proto.contains("tags: [Tag]"));
    assert!(proto.contains("stars: Int?"));
    assert!(proto.contains("// proto field number: 2"));
    assert!(proto.contains("#Codable"));
}

#[test]
fn data_bind_rejects_malformed_inputs() {
    assert!(jet::CBind::generate_data("json", "bad.json", "[]", None, "").is_err());
    assert!(jet::CBind::generate_data("json", "bad.json", "{\"x\":", None, "").is_err());
    assert!(jet::CBind::generate_data("csv", "bad.csv", "a,a\n1,2\n", None, "").is_err());
    assert!(jet::CBind::generate_data("csv", "bad.csv", "a,b\n1\n", None, "").is_err());
    assert!(jet::CBind::generate_data("csv", "bad.csv", "a,b\n\"unterminated,2\n", None, "").is_err());
    assert!(jet::CBind::generate_data("sql", "bad.sql", "CREATE TABLE x (id UNKNOWN);", None, "").is_err());
    assert!(jet::CBind::generate_data("sql", "bad.sql", "CREATE TABLE x (id INT", None, "").is_err());
    assert!(jet::CBind::generate_data("xml", "bad.xml", "<a><b></a>", None, "").is_err());
    assert!(jet::CBind::generate_data("xml", "bad.xml", "<!-- unclosed", None, "").is_err());
    assert!(jet::CBind::generate_data("xml", "bad.xml", "<a><![CDATA[unclosed</a>", None, "").is_err());
    assert!(jet::CBind::generate_data(
        "proto",
        "bad.proto",
        "message A { string x = 1; string x = 2; }",
        None,
        "",
    ).is_err());
    assert!(jet::CBind::generate_data(
        "proto",
        "bad.proto",
        "message A { string x = 1;",
        None,
        "",
    ).is_err());
}

#[test]
fn data_bind_provenance_is_hashed_and_stable() {
    let input = "{\"name\":\"jet\",\"count\":2}";
    let first = generated(
        "json",
        "fixtures/stable.json",
        input,
        Some("Stable"),
        "jet bind json fixtures/stable.json --type Stable",
    );
    let second = generated(
        "json",
        "fixtures/stable.json",
        input,
        Some("Stable"),
        "jet bind json fixtures/stable.json --type Stable",
    );
    assert_eq!(first, second);
    let hash = jet::SHA256::sha256_hex(input.as_bytes());
    assert!(first.contains("input: fixtures/stable.json"));
    assert!(first.contains(&format!("sha256: {hash}")));
    assert!(first.contains("generated by: jet bind json fixtures/stable.json --type Stable"));
    assert!(first.contains("format: json"));
    assert!(first.contains("inference: JSON object fields become #Codable fields"));

    let changed = generated(
        "json",
        "fixtures/stable.json",
        "{\"name\":\"jet\",\"count\":3}",
        Some("Stable"),
        "jet bind json fixtures/stable.json --type Stable",
    );
    assert_ne!(first, changed);
}

#[test]
fn data_bind_cli_writes_visible_output_and_regeneration_diff() {
    let dir = scratch("cli");
    let input = dir.join("records.json");
    fs::write(&input, "{\"name\":\"first\",\"count\":1}").unwrap();
    let input_text = input.to_str().unwrap();

    let first = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["bind", "json", input_text, "--type", "Record"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_success(first);
    let default_output = dir.join("bindings/records.jet");
    assert!(default_output.is_file());
    let before = fs::read_to_string(&default_output).unwrap();
    assert!(before.contains("#Codable"));
    assert!(before.contains("struct Record"));
    assert!(before.contains(input_text));

    fs::write(&input, "{\"name\":\"second\",\"count\":2,\"active\":true}").unwrap();
    let stale = fs::read_to_string(&default_output).unwrap();
    assert_eq!(before, stale);

    let regenerated = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["bind", "json", input_text, "--type", "Record"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_success(regenerated);
    let regenerated_output = fs::read_to_string(&default_output).unwrap();
    assert_ne!(before, regenerated_output);
    assert!(regenerated_output.contains("active: Bool"));

    let explicit = dir.join("generated/record.jet");
    let explicit_text = explicit.to_str().unwrap();
    let second = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "bind",
            "json",
            input_text,
            "--type",
            "Record",
            "-o",
            explicit_text,
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_success(second);
    let after = fs::read_to_string(&explicit).unwrap();
    assert_ne!(before, after);
    assert!(after.contains("active: Bool"));
    assert!(after.contains(explicit_text));
    assert!(Path::new(explicit_text).is_file());

    let _ = fs::remove_dir_all(dir);
}
