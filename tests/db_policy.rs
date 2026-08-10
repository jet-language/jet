//! Focused proof that one explicit DB scope enforces row policy on every row path.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, run_default_multi};

const SOURCE: &str = r#"
use core.db as db

fn run() {
    conn := db.open_memory()
    policy :: db.policy("tasks", "owner == user") ?? panic("policy")
    scoped := conn.with_policy(policy, "alice")
    _created :: db.migrate(scoped, "tasks-v1", ["CREATE TABLE tasks (owner TEXT, title TEXT)"]) ?? panic("create")
    _schema :: db.migrate(scoped, "other-v1", ["CREATE TABLE other (owner TEXT, title TEXT)"]) ?? panic("schema")
    schema_bypass :: scoped.execute("DROP TABLE other", [])
    if schema_bypass == {
        .Ok(_) -> { print("schema:accepted") }
        .Err(_) -> { print("schema:rejected") }
    }
    _one :: scoped.execute(
        "INSERT INTO tasks (owner, title) VALUES (?, ?)",
        [DBValue.Text("bob"), DBValue.Text("one")]
    ) ?? panic("insert one")
    _two :: scoped.execute(
        "INSERT INTO tasks (title, owner) VALUES (?, ?)",
        [DBValue.Text("two"), DBValue.Text("bob")]
    ) ?? panic("insert two")
    rows :: scoped.query(
        "SELECT title FROM tasks WHERE title = ? OR title = ? ORDER BY title",
        [DBValue.Text("one"), DBValue.Text("two")]
    ) ?? panic("query")
    print("rows:{rows.len()}")
    bypass :: scoped.query("SELECT title FROM other", [])
    if bypass == {
        .Ok(_) -> { print("bypass:accepted") }
        .Err(_) -> { print("bypass:rejected") }
    }
    bob := conn.with_policy(policy, "bob")
    _bob_insert :: bob.execute(
        "INSERT INTO tasks (owner, title) VALUES (?, ?)",
        [DBValue.Text("alice"), DBValue.Text("three")]
    ) ?? panic("bob insert")
    bob_rows :: bob.query("SELECT title FROM tasks", []) ?? panic("bob query")
    alice_rows :: scoped.query("SELECT title FROM tasks", []) ?? panic("alice query")
    print("cross:{bob_rows.len()}:{alice_rows.len()}")
    live :: scoped.live("SELECT title FROM tasks", []) ?? panic("live")
    print("live:ok")
    commented := scoped.query("SELECT title FROM tasks -- hide policy", [])
    blocked := scoped.query("SELECT title FROM tasks /* hide policy */", [])
    joined := scoped.query("SELECT tasks.title FROM tasks JOIN other ON other.owner = tasks.owner", [])
    nested := scoped.query("SELECT title FROM tasks WHERE owner IN (SELECT owner FROM other)", [])
    upserted := scoped.execute(
        "INSERT INTO tasks (owner, title) VALUES (?, ?) ON CONFLICT(owner) DO UPDATE SET owner = ?",
        [DBValue.Text("alice"), DBValue.Text("four"), DBValue.Text("bob")]
    )
    replaced := scoped.execute(
        "INSERT OR REPLACE INTO tasks (owner, title) VALUES (?, ?)",
        [DBValue.Text("bob"), DBValue.Text("five")]
    )
    if commented == {
        .Err(_) -> { print("comment:rejected") }
        .Ok(_) -> { print("comment:accepted") }
    }
    if blocked == {
        .Err(_) -> { print("block:rejected") }
        .Ok(_) -> { print("block:accepted") }
    }
    if joined == {
        .Err(_) -> { print("join:rejected") }
        .Ok(_) -> { print("join:accepted") }
    }
    if nested == {
        .Err(_) -> { print("subquery:rejected") }
        .Ok(_) -> { print("subquery:accepted") }
    }
    if upserted == {
        .Err(_) -> { print("upsert:rejected") }
        .Ok(_) -> { print("upsert:accepted") }
    }
    if replaced == {
        .Err(_) -> { print("replace:rejected") }
        .Ok(_) -> { print("replace:accepted") }
    }
    _closed :: scoped.close()
}
"#;

#[test]
fn db_scope_enforces_policy_on_query_insert_and_live_aot() {
    let (code, stdout) = build_and_run("db_policy_scope", SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, "schema:rejected\nrows:2\nbypass:rejected\ncross:1:2\nlive:ok\ncomment:rejected\nblock:rejected\njoin:rejected\nsubquery:rejected\nupsert:rejected\nreplace:rejected\n");
}

#[test]
fn db_scope_enforces_policy_on_query_insert_and_live_default() {
    let (code, stdout, stderr) = run_default_multi(
        "db_policy_scope_jit",
        "main.jet",
        &[("main.jet", SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, "schema:rejected\nrows:2\nbypass:rejected\ncross:1:2\nlive:ok\ncomment:rejected\nblock:rejected\njoin:rejected\nsubquery:rejected\nupsert:rejected\nreplace:rejected\n");
}
