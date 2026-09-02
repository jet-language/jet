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
    audit :: db.policy_audit(scoped)
    print("audit:{audit}")
    _created :: db.migrate(scoped, "tasks-v1", [SQL{"CREATE TABLE tasks (owner TEXT, title TEXT)"}]) ?? panic("create")
    _schema :: db.migrate(scoped, "other-v1", [SQL{"CREATE TABLE other (owner TEXT, title TEXT)"}]) ?? panic("schema")
    scoped.execute(SQL{"DROP TABLE other"}) ? _ok -> {
        print("schema:accepted")
    } ! _error -> {
        print("schema:rejected")
    }
    bob_owner :: "bob"
    one_title :: "one"
    _one :: scoped.execute(
        SQL{"INSERT INTO tasks (owner, title) VALUES ({bob_owner}, {one_title})"}
    ) ?? panic("insert one")
    two_title :: "two"
    _two :: scoped.execute(
        SQL{"INSERT INTO tasks (title, owner) VALUES ({two_title}, {bob_owner})"}
    ) ?? panic("insert two")
    rows :: scoped.query(
        SQL{"SELECT title FROM tasks WHERE title = {one_title} OR title = {two_title} ORDER BY title"}
    ) ?? panic("query")
    print("rows:{rows.len()}")
    limit_n :: 1
    offset_n :: 0
    limited :: scoped.query(
        SQL{"SELECT title FROM tasks WHERE title = {one_title} ORDER BY title LIMIT {limit_n} OFFSET {offset_n}"}
    ) ?? panic("limit")
    print("limit:{limited.len()}")
    _one_row :: scoped.query_one(SQL{"SELECT title FROM tasks ORDER BY title"}) ?? panic("query one")
    print("query-one:ok")
    _tx :: db.transaction(scoped, "policy-tx", [SQL{"UPDATE tasks SET title = 'one'"}]) ?? panic("transaction")
    print("transaction:ok")
    scoped.query(SQL{"SELECT title FROM other"}) ? _rows -> {
        print("bypass:accepted")
    } ! _error -> {
        print("bypass:rejected")
    }
    bob := conn.with_policy(policy, "bob")
    alice_owner :: "alice"
    three_title :: "three"
    _bob_insert :: bob.execute(
        SQL{"INSERT INTO tasks (owner, title) VALUES ({alice_owner}, {three_title})"}
    ) ?? panic("bob insert")
    bob_rows :: bob.query(SQL{"SELECT title FROM tasks"}) ?? panic("bob query")
    alice_rows :: scoped.query(SQL{"SELECT title FROM tasks"}) ?? panic("alice query")
    print("cross:{bob_rows.len()}:{alice_rows.len()}")
    blank_user := conn.with_policy(policy, "   ")
    blank_owner :: "   "
    blank_title :: "blank"
    blank_user.execute(
        SQL{"INSERT INTO tasks (owner, title) VALUES ({blank_owner}, {blank_title})"}
    ) ? _ok -> {
        print("blank-user:accepted")
    } ! _error -> {
        print("blank-user:rejected")
    }
    _live :: scoped.live(SQL{"SELECT title FROM tasks"}) ?? panic("live")
    print("live:ok")
    scoped.query(SQL{"SELECT title FROM tasks -- hide policy"}) ? _rows -> {
        print("comment:accepted")
    } ! _error -> {
        print("comment:rejected")
    }
    scoped.query(SQL{"SELECT title FROM tasks /* hide policy */"}) ? _rows -> {
        print("block:accepted")
    } ! _error -> {
        print("block:rejected")
    }
    scoped.query(SQL{"SELECT tasks.title FROM tasks JOIN other ON other.owner = tasks.owner"}) ? _rows -> {
        print("join:accepted")
    } ! _error -> {
        print("join:rejected")
    }
    scoped.query(SQL{"SELECT title FROM tasks WHERE owner IN (SELECT owner FROM other)"}) ? _rows -> {
        print("subquery:accepted")
    } ! _error -> {
        print("subquery:rejected")
    }
    four_title :: "four"
    scoped.execute(
        SQL{"INSERT INTO tasks (owner, title) VALUES ({alice_owner}, {four_title}) ON CONFLICT(owner) DO UPDATE SET owner = {bob_owner}"}
    ) ? _ok -> {
        print("upsert:accepted")
    } ! _error -> {
        print("upsert:rejected")
    }
    five_title :: "five"
    scoped.execute(
        SQL{"INSERT OR REPLACE INTO tasks (owner, title) VALUES ({bob_owner}, {five_title})"}
    ) ? _ok -> {
        print("replace:accepted")
    } ! _error -> {
        print("replace:rejected")
    }
    _closed :: scoped.close()
}
"#;

#[test]
fn db_scope_enforces_policy_on_query_insert_and_live_aot() {
    let (code, stdout) = build_and_run("db_policy_scope", SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, "audit:DBPolicy(table=tasks, user=alice, expr=owner == user, predicate=owner = ?)\nschema:rejected\nrows:2\nlimit:1\nquery-one:ok\ntransaction:ok\nbypass:rejected\ncross:1:2\nblank-user:rejected\nlive:ok\ncomment:rejected\nblock:rejected\njoin:rejected\nsubquery:rejected\nupsert:rejected\nreplace:rejected\n");
}

#[test]
fn db_scope_enforces_policy_on_query_insert_and_live_default() {
    let (code, stdout, stderr) =
        run_default_multi("db_policy_scope_jit", "main.jet", &[("main.jet", SOURCE)]);
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, "audit:DBPolicy(table=tasks, user=alice, expr=owner == user, predicate=owner = ?)\nschema:rejected\nrows:2\nlimit:1\nquery-one:ok\ntransaction:ok\nbypass:rejected\ncross:1:2\nblank-user:rejected\nlive:ok\ncomment:rejected\nblock:rejected\njoin:rejected\nsubquery:rejected\nupsert:rejected\nreplace:rejected\n");
}

/// D-DBPOLICY1=A + I9: the closed policy language is one fact
/// (`Prelude/CoreLib/JetStd/RowPolicy.rs`), so a rejected policy is rejected on
/// every tier and an accepted one normalizes identically. A leading-digit table
/// and a padded table used to divide AOT from the JIT/ambient hosts, which each
/// re-derived the rule.
const INVALID_POLICY_SOURCE: &str = r#"
use core.db as db

fn run() {
    conn := db.open_memory()
    if db.policy("9tasks", "true") == {
        .Ok(_) -> {
            print("digit-table:accepted")
        }
        .Err(_) -> {
            print("digit-table:rejected")
        }
    }
    if db.policy("two words", "true") == {
        .Ok(_) -> {
            print("spaced-table:accepted")
        }
        .Err(_) -> {
            print("spaced-table:rejected")
        }
    }
    if db.policy("tasks", "owner != user") == {
        .Ok(_) -> {
            print("other-expr:accepted")
        }
        .Err(_) -> {
            print("other-expr:rejected")
        }
    }
    padded :: db.policy("  tasks  ", "owner == user") ?? panic("padded policy")
    scoped := conn.with_policy(padded, "alice")
    _created :: db.migrate(scoped, "tasks-v1", [SQL{"CREATE TABLE tasks (owner TEXT, title TEXT)"}]) ?? panic("create")
    owner :: "bob"
    title :: "one"
    _one :: scoped.execute(
        SQL{"INSERT INTO tasks (owner, title) VALUES ({owner}, {title})"}
    ) ?? panic("insert")
    rows :: scoped.query(SQL{"SELECT title FROM tasks"}) ?? panic("query")
    print("padded-rows:{rows.len()}")
    _closed :: scoped.close()
}
"#;

const INVALID_POLICY_EXPECTED: &str =
    "digit-table:rejected\nspaced-table:rejected\nother-expr:rejected\npadded-rows:1\n";

#[test]
fn invalid_row_policy_is_denied_the_same_way_aot() {
    let (code, stdout) = build_and_run("db_policy_invalid", INVALID_POLICY_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, INVALID_POLICY_EXPECTED);
}

#[test]
fn invalid_row_policy_is_denied_the_same_way_default() {
    let (code, stdout, stderr) = run_default_multi(
        "db_policy_invalid_jit",
        "main.jet",
        &[("main.jet", INVALID_POLICY_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, INVALID_POLICY_EXPECTED);
}
