# v5 usage gallery — Jet vs Rust, side by side

Companion to [memory-greenfield.md](memory-greenfield.md) (model v5, D-MEM1).
Jet samples use current Jet syntax plus the owner decrees of 2026-07-03 (card
#188): `fn run`, `String`, `[T]`/`[K, V]` collections with `[]` as the universal
empty literal (D-EMPTYLIT1), `Val(x)`/`None` options (D-OPT-SPELL1), bare lambdas
where inferable (D-LAMBDA-INFER1), `(a, b) := f()` tuple destructuring
(D-TUPLE-DESTRUCT1), `core.files` + `.write` (D-FILES-WRITE1), `.after(sep)`
string views (D-STR-AFTER1), `ok`/`err`/`??`/`?` failures, `::`/`:=` bindings,
`take(x)` spawn captures. v5 changes applied: bare = read, `&` = write, `^` = take
(glyph assignment is ballot axis A/B/C). `Pool<T>`, `Id<T>`, `Shared<T>`, and
`policy` are the proposed new pieces and are marked. Rust samples are idiomatic,
not strawmen.

---

## 1 · Python-style scripting — word count

```rust
// Rust
use std::collections::HashMap;
use std::fs;

fn main() {
    let text = fs::read_to_string("notes.txt").unwrap();
    let mut counts: HashMap<&str, i32> = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1;      // entry API + deref
    }
    for (word, n) in &counts {
        println!("{word}: {n}");
    }
}
```

```jet
// Jet
use core.files as files

fn run() {
    text :: files.read("notes.txt")?
    counts: [String, Int] := []
    loop word in text.split(" ") {
        counts[word] = ((counts.get(word) ?? 0) + 1)
    }
    loop key, count in counts {
        print("{key}: {count}")
    }
}
```

Gone: `let mut`, `&str` keys borrowing into `text` (a lifetime relationship in
line 6 of a beginner script), `entry`/`or_insert`/`*` deref, `unwrap`. Words are
view values — zero-copy, no borrow coupling.

## 2 · Everyday structs & methods

```rust
// Rust
struct Player { name: String, hp: i32 }

impl Player {
    fn new(name: &str) -> Self { Self { name: name.to_string(), hp: 100 } }
    fn heal(&mut self, amount: i32) { self.hp += amount; }
    fn is_alive(&self) -> bool { self.hp > 0 }
}

let mut kai = Player::new("Kai");   // mut required to ever heal
kai.heal(10);
```

```jet
// Jet
struct Player {
    name: String
    hp: Int

    fn heal(&self, amount: Int) { self.hp += amount }
    fn is_alive(self) -> Bool { return self.hp > 0 }
}

kai := Player.{ name: "Kai", hp: 100 }
kai.heal(10)
```

Reader/writer split kept (`self` vs `&self`) — one glyph, on the method that
writes. No `&str`/`to_string()` constructor dance, no `let mut` ripple.

## 3 · Passing things around — read / write / take

```rust
// Rust
fn total_hp(party: &Party) -> i32 { party.members.iter().map(|m| m.hp).sum() }
fn heal_all(party: &mut Party)    { for m in party.members.iter_mut() { m.hp += 5; } }
fn disband(party: Party)          { /* consumed */ }

total_hp(&party);
heal_all(&mut party);
let backup = party.clone();       // duplicate to survive the move
disband(party);            // move — no marker at the call
```

```jet
// Jet
fn total_hp(party: Party) -> Int {
    sum := 0
    loop m in party.members { sum += m.hp }
    return sum
}
fn heal_all(party: &Party) {
    loop m in &party.members { m.hp += 5 }
}
fn disband(party: ^Party) { }

total_hp(party)            // read is the default — no sigil either end
heal_all(&party)
backup :: copy party       // explicit duplicate — the one copy spelling (D-CAP2)
disband(^party)            // move — visible at the call
print(backup.members[0].name)   // the copy is yours
```

Same three access modes as Rust. Reading (the overwhelming majority) costs
zero glyphs; the move is written where it happens; `copy x` is the one way to
duplicate heap data (scalars and small POD copy freely on their own). Rule:
`^` marks giving away a *named binding* — temporaries (literals, call results,
`copy x`) pass without it, since nothing survives to be used-after.

## 4 · Collections & nested data

```rust
// Rust
if let Some(m) = party.members.get_mut(0) { m.hp += 10; }
party.members.retain(|m| m.hp > 0);
let names: Vec<String> =
    party.members.iter().map(|m| m.name.clone()).collect();   // clone or fight
```

```jet
// Jet
party.members[0].hp += 10
party.members = party.members.filter(m => m.hp > 0)
names :: party.members.map(m => m.name)   // views — no clone decision
```

Place paths write through nested data directly. The `.clone()`-to-make-it-
compile reflex has nothing to attach to.

## 5 · Strings & text — the `String`/`&str` tax

```rust
// Rust — two string types, one lifetime lesson
fn domain<'a>(email: &'a str) -> &'a str {
    &email[email.find('@').map(|i| i + 1).unwrap_or(0)..]
}
let input = String::from("  nate@jet.dev ");
let d = domain(input.trim());     // &str borrowing into input
let kept = d.to_string();         // must re-own to store it
```

```jet
// Jet — one string type
fn domain(email: String) -> String {
    return email.after("@")
}
input :: "  nate@jet.dev "
d :: domain(input.trim())         // view value: zero-copy AND storable
```

One `String`. Slicing/splitting returns counted view values — cheap like
`&str`, storable like Rust's `String`, and no lifetime parameter exists
anywhere in the language.

## 6 · Game dev — one world, many entities *(Pool/Id: proposed)*

```rust
// Rust — real projects reach for a crate (slotmap/hecs); std gives you Rc<RefCell>
use slotmap::{SlotMap, DefaultKey};
let mut world: SlotMap<DefaultKey, Player> = SlotMap::new();
let kai = world.insert(Player::new("Kai"));
let rem = world.insert(Player::new("Rem"));
world[kai].target = Some(rem);

fn tick(world: &mut SlotMap<DefaultKey, Player>) {
    let keys: Vec<_> = world.keys().collect();     // snapshot keys to iterate+mutate
    for k in keys {
        if let Some(t) = world[k].target {
            let dmg = world[k].attack;
            world[t].hp -= dmg;                    // disjoint access: your problem
        }
    }
}
```

```jet
// Jet — pools are first-class (proposed)
world := Pool<Player>.new()
kai :: world.add(Player.{ name: "Kai", hp: 100 })
rem :: world.add(Player.{ name: "Rem", hp: 100 })
world[kai].target = rem                    // Id<Player> is plain data

fn tick(world: &Pool<Player>) {
    loop id in world.ids() {
        t :: world[id].target
        dmg :: world[id].attack
        world[t].hp -= dmg                 // one writer per statement — checked
    }
}
```

One kai, referenced from anywhere, via `Id` — the pattern every shipped Rust
game converges on, without the crate hunt. Stale ids are caught accesses, not
corruption.

## 7 · Game dev — the hot loop *(policy: proposed)*

```rust
// Rust — zero-cost by default, ceremony everywhere in the program
fn integrate(world: &mut World, dt: f64) {
    for e in world.entities.iter_mut() { e.pos += e.vel * dt; }
}
```

```jet
// Jet — same machine code; strictness claimed where it matters
policy no_alloc                            // this module: no heap, enforced

fn integrate(world: &World, dt: F64) {
    loop e in &world.entities { e.pos += (e.vel * dt) }
}
```

Identical semantics and codegen (`&` → `&mut`, `iter_mut` equivalent). The
`policy` line is the module claiming embedded-grade guarantees; the rest of
the game doesn't carry them.

## 8 · Web dev — handlers, state, tasks *(Shared: proposed)*

```rust
// Rust
struct App { db: Db, sessions: Mutex<HashMap<Token, User>> }

fn handle(req: &Request, app: &Arc<App>) -> Response {
    let user = app.sessions.lock().unwrap().get(&req.token).cloned();
    match user {
        Some(u) => Response::ok(render(&u)),
        None    => Response::redirect("/login"),
    }
}

let app = Arc::new(App::new());
for conn in listener.incoming() {
    let app = Arc::clone(&app);                  // the clone-per-thread ritual
    thread::spawn(move || serve(conn.unwrap(), app));
}
```

```jet
// Jet
fn handle(req: Request, state: Shared<App>) -> Response {
    user :: state.read(s => s.sessions.get(req.token))
    if user == Val(u) {
        return Response.ok(render(u))
    }
    return Response.redirect("/login")
}

state :: Shared.new(App.new())
loop conn in listener.incoming() {
    tasks.spawn(take(conn) () => serve(conn, state))   // Shared: a copyable door
}
```

`Arc` + `Mutex` + `clone` + `lock` + `unwrap` collapse into one named concept.
Spawn captures use the existing S53 `take(x)` capture list; borrows can't cross
`spawn` at all, so the races Rust prevents are prevented — without `move`
closures or `Send` bounds surfacing to users.

## 9 · Concurrency — channels & workers

```rust
// Rust
let (tx, rx) = mpsc::channel();
thread::spawn(move || {
    for i in 0..10 {
        let job = make_job(i);
        tx.send(job).unwrap();      // job moves — invisibly
    }
});
for job in rx { process(&job); }
```

```jet
// Jet (D-TUPLE-DESTRUCT1 + v5 visible move)
use core.tasks as tasks

(tx, rx) := tasks.channel<Job>()
tasks.spawn(take(tx) () => {
    loop i in 0..10 {
        job :: make_job(i)
        tx.send(^job)               // giving away a named binding is written
    }
})
loop i in 0..10 {
    job :: rx.receive() ?? panic("channel closed")
    process(job)
}
```

Same model (values move through channels; no shared mutable state). The only
delta is visibility: every transfer is spelled at the site.

## 10 · Graphs & back-references *(Pool/Id: proposed)*

```rust
// Rust — the famous interview question
use std::rc::{Rc, Weak};
use std::cell::RefCell;

struct Node { parent: Option<Weak<RefCell<Node>>>, children: Vec<Rc<RefCell<Node>>> }

let root  = Rc::new(RefCell::new(Node::default()));
let child = Rc::new(RefCell::new(Node::default()));
child.borrow_mut().parent = Some(Rc::downgrade(&root));
root.borrow_mut().children.push(Rc::clone(&child));
// upgrade().unwrap() at every parent access; runtime borrow panics lurking
```

```jet
// Jet
struct Node {
    parent: Id<Node>?
    children: [Id<Node>]
}

tree := Pool<Node>.new()
root  :: tree.add(Node.{ parent: None, children: [] })
child :: tree.add(Node.{ parent: Val(root), children: [] })
tree[root].children.push(child)
```

Parent pointers are fields (`Id<Node>?` — Jet's existing option type). No
`Rc`/`Weak`/`RefCell` assembly, no `borrow_mut()` panics, no
`upgrade().unwrap()`.

## 11 · Small math types

```rust
// Rust
#[derive(Clone, Copy, Debug, PartialEq)]
struct Vec2 { x: f64, y: f64 }
let b = a;              // copies because you remembered the derive
```

```jet
// Jet
struct Vec2 { x: F64, y: F64 }
b :: a                  // small plain data copies; both usable — it's just data
```

The `Copy`-derive ritual and the "is this type move-or-copy?" lookup disappear;
small POD behaves like the numbers it's made of.

## 12 · Resources & cleanup

```rust
// Rust
let mut f = File::create("save.txt")?;
writeln!(f, "{data}")?;
drop(f);                       // early close: fine, but silent
// writeln!(f, "x")?;          // E0382 — mentions "move", not "closed"
```

```jet
// Jet (current files API — RAII on every exit path already ships)
save :: files.create("save.txt")?
save.write("{data}")?
save.close()                   // fn close(^self) — consumption is the API
save.write("x")?          // error: save was given away to `close` (line 3)
```

Both are RAII with compile-time use-after-close. Jet's version names the event
in the error ("given away to `close`"), because the `^` is in the signature.

## 13 · When you hit the wall — the error IS the product

```text
Rust (E0502):
error[E0502]: cannot borrow `party.members` as mutable because it is also
              borrowed as immutable
  --> src/main.rs:12:9
   = note: immutable borrow occurs here... mutable borrow occurs here...
```

```text
Jet (same program):
error[E0xxx]: party.members is being walked by this loop and can't also grow
what: adding to a list while looping over it would shift the ground under the loop
why:  one writer at a time — the loop counts as a writer of the list's shape
fix:  collect first:  fallen :: party.members.filter(m => m.hp == 0)
      then push after the loop ends
```

Same rule, same rejection. One explains the checker; the other teaches the fix.
Every diagnostic in this model ships what/why/fix and is snapshot-pinned (I4).

---

## The pattern across all 13

| | Rust | Jet v5 |
|---|---|---|
| read access | `&x` / `&self`, everywhere | unmarked — the default |
| write access | `&mut x`, `let mut`, `iter_mut` | `&x`, one glyph |
| moves | silent at call sites | `^x`, always written |
| lifetimes | user-facing syntax + inference rules | cannot exist (second-class borrows + view values) |
| two string types | `String` / `&str` | one `String` |
| shared state | `Arc<Mutex<T>>` + clone/lock/unwrap | `Shared<T>` |
| graphs/identity | `Rc<RefCell>`/`Weak` or crates | `Pool<T>` + `Id<T>`, first-class |
| strictness | global, mandatory | same core rules + scoped `policy` claims |
| errors | describe the checker | teach the fix |
