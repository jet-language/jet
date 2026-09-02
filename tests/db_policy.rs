//! Focused proof that one explicit DB scope enforces row policy on every row path.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{assert_tiers_agree, build_and_run, run_default_multi};

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

/// Hostile values, malformed placeholders, driver errors, policy denial, and
/// rollback all cross the same SQL carrier and preserve one policy boundary.
const HOSTILE_SOURCE: &str = r#"
use app as application
use core.db as db

fn run() {
    conn := db.open_memory()
    policy :: db.policy("tasks", "true") ?? panic("policy")
    scoped := conn.with_policy(policy, "attacker")
    _created :: db.migrate(scoped, "tasks-v1", [
        SQL{"CREATE TABLE tasks (id INTEGER PRIMARY KEY, body TEXT)"}
    ]) ?? panic("create")

    payload :: "x'); DROP TABLE tasks; --"
    _inserted :: scoped.execute(
        SQL{"INSERT INTO tasks (id, body) VALUES (1, {payload})"}
    ) ?? panic("insert")
    row :: scoped.query_one(SQL{"SELECT body FROM tasks"}) ?? panic("query")
    body :: db.row_text(row, "body") ?? panic("missing body")
    print("interpolation:{body}")

    duplicate :: scoped.query(
        SQL{"SELECT id FROM tasks WHERE body = {payload} OR body = {payload}"}
    ) ?? panic("duplicate")
    print("duplicate:{duplicate.len()}")

    scoped.query(SQL{"SELECT body FROM tasks WHERE body = ?"}) ? _rows -> {
        print("placeholder:accepted")
    } ! _error -> {
        print("placeholder:rejected")
    }
    scoped.query(SQL{"SELECT no_such_column FROM tasks"}) ? _rows -> {
        print("driver:accepted")
    } ! _error -> {
        print("driver:rejected")
    }
    scoped.query(SQL{"SELECT body FROM other"}) ? _rows -> {
        print("authority:accepted")
    } ! _error -> {
        print("authority:rejected")
    }

    live :: scoped.live(SQL{"SELECT body FROM tasks"}) ?? panic("live")
    live_before :: application.live_get(live)
    print("live-before:{live_before.contains(payload)}")
    updated :: "updated"
    _updated :: scoped.execute(SQL{"UPDATE tasks SET body = {updated}"}) ?? panic("update")
    _invalidated :: application.invalidate("tasks")
    live_after :: application.live_get(live)
    print("live-after:{live_after.contains(updated)}")

    tx_body :: "rolled back"
    tx_count :: db.transaction(scoped, "rollback", [
        SQL{"INSERT INTO tasks (id, body) VALUES (2, {tx_body})"},
        SQL{"INSERT INTO tasks (missing_column) VALUES (2)"}
    ]) ?? -1
    if {
        tx_count < 0 -> print("rollback:rejected")
        else -> print("rollback:accepted")
    }
    rollback_rows :: scoped.query(SQL{"SELECT id FROM tasks"}) ?? panic("rollback query")
    print("rollback-rows:{rollback_rows.len()}")
    migration_count :: db.migrate(scoped, "broken", [
        SQL{"CREATE TABLE migration_marker (id INTEGER)"},
        SQL{"INSERT INTO tasks (missing_column) VALUES (3)"}
    ]) ?? -1
    if {
        migration_count < 0 -> print("migration:rejected")
        else -> print("migration:accepted")
    }
    retry_count :: db.migrate(scoped, "broken", [
        SQL{"CREATE TABLE migration_marker (id INTEGER)"}
    ]) ?? -1
    if {
        retry_count == 1 -> print("migration-retry:1")
        else -> print("migration-retry:rejected")
    }
    _closed :: scoped.close()
}
"#;

const HOSTILE_EXPECTED: &str = "interpolation:x'); DROP TABLE tasks; --\nduplicate:1\nplaceholder:rejected\ndriver:rejected\nauthority:rejected\nlive-before:true\nlive-after:true\nrollback:rejected\nrollback-rows:1\nmigration:rejected\nmigration-retry:1\n";

#[test]
fn typed_sql_hostile_operations_keep_policy_and_provenance_across_tiers() {
    assert_tiers_agree("db_policy_hostile", HOSTILE_SOURCE, HOSTILE_EXPECTED);
}

#[test]
fn typed_sql_canonical_scope_example_has_aot_default_and_interpreter_parity() {
    assert_tiers_agree(
        "db_policy_scope_all_tiers",
        SOURCE,
        "audit:DBPolicy(table=tasks, user=alice, expr=owner == user, predicate=owner = ?)\nschema:rejected\nrows:2\nlimit:1\nquery-one:ok\ntransaction:ok\nbypass:rejected\ncross:1:2\nblank-user:rejected\nlive:ok\ncomment:rejected\nblock:rejected\njoin:rejected\nsubquery:rejected\nupsert:rejected\nreplace:rejected\n",
    );
}

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
const BLOB_SOURCE: &str = r#"
use core.db as db

fn run() {
    conn := db.open_memory()
    policy :: db.policy("blobs", "true") ?? panic("policy")
    scoped := conn.with_policy(policy, "alice")
    _created :: db.migrate(scoped, "blobs-v1", [
        SQL{"CREATE TABLE blobs (payload BLOB, explicit_payload BLOB)"}
    ]) ?? panic("create")
    payload :: [U8]{0, 1, 2, 127, 128, 255}
    explicit_payload :: [U8]{0, 1, 2, 127, 128, 255}
    bound :: DBValue.Blob(explicit_payload)
    _inserted :: scoped.execute(
        SQL{"INSERT INTO blobs (payload, explicit_payload) VALUES ({payload}, {bound})"}
    ) ?? panic("insert")
    row :: scoped.query_one(
        SQL{"SELECT payload, explicit_payload FROM blobs"}
    ) ?? panic("query")
    direct :: db.row_value(row, "payload") ?? panic("direct row value")
    explicit :: db.row_value(row, "explicit_payload") ?? panic("explicit row value")
    direct_bytes :: direct.blob() ?? panic("direct blob")
    explicit_bytes :: explicit.blob() ?? panic("explicit blob")
    print("direct:{direct_bytes}")
    print("explicit:{explicit_bytes}")
    print("equal:{direct_bytes == payload && explicit_bytes == explicit_payload}")
}
"#;

#[test]
fn sqlite_blob_round_trip_preserves_bytes_across_tiers() {
    assert_tiers_agree(
        "db_policy_blob",
        BLOB_SOURCE,
        "direct:[0, 1, 2, 127, 128, 255]\nexplicit:[0, 1, 2, 127, 128, 255]\nequal:true\n",
    );
}
