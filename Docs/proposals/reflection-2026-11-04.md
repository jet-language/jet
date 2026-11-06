# Reflection: everything Jai's editor demo does, and further

Status: proposal for owner review (Owner inquiry 1, 2026-11-04). Nothing here
is ratified by this document. Facts about today's language are marked
**today** and come from dev `15098e62a5` (2026-11-04). Probes ran on the
admitted compiler `7c672809`, which is four commits behind and still spells
markers `marker Name($sites: …)` and pastes with `$`.

Code blocks carry one of two labels in their first line:

- `// runs today (probe pN)` means it ran on the admitted compiler, as shown;
- `// ratified, not built: …` means it uses ratified spellings that no
  compiler accepts yet, and names what is missing.

The ratified future spellings are:

- struct markers with `impl Marker` and `expand(self, target: Target)`;
- derives as `impl Trait.Derive { fn expand(target: Target) }`;
- plain-value reads such as `target.fields` and `T.reflect()`;
- `#{…}` pastes.

Source: "How Reflection Removes Game Engine Busywork", Jonathan Blow Clips,
<https://www.youtube.com/watch?v=tSv-872TXpQ> (607 s). It is an excerpt of the
Reboot Develop 2017 talk
(<https://youtube.com/watch?v=De0Am_QcZiQ&t=1936s>). Mining run
`jai-reflection-tSv-2026-11-04`; the receipt is in section 15.

## 1. Verdict

Jet already plans every capability the video shows. Ten of the twenty-six
mined claims are built in whole or in part, and the other sixteen are owned by
named card criteria. The plan is stronger than Jai's in four ways:

- reads are typed, and run-time facts exist only for what a program asks
  about;
- library markers are typed values instead of string notes;
- code generation uses hygienic templates instead of string pastes;
- generated code is ordinary code that users can inspect.

The real gaps are narrow:

1. The typed field facts (`type`, `default`, `offset`) and the typed marker
   lookup are ratified but unbuilt. #6039 [10] has been reopened for the
   lookup.
2. **Today** `T.reflect()` works only inside derive templates. In ordinary
   code, `Rock.reflect()` is E0311.
3. Run-time reflection gives a one-level display-text snapshot, not the
   ratified typed value.
4. No library can yet list every use of its marker. That is how Jai's editor
   finds entity types.
5. A compile-time loop inside a generated method body unrolls today, but it
   cannot paste the loop's field into an expression (`self.$fname` is E0302).
   Jai's editor code depends on that per-field paste.
6. Two ratified records name a reflected field's type differently, `type` and
   `ty`. That is the one owner ballot (section 12).
7. No paired cost cells prove the zero-cost claim (section 8).

This proposal adds no new syntax and no new build value. It gives Main exact
criteria for existing cards (section 11), one new work item (section 10), one
owner ballot (section 12), and record corrections (section 13).

Ideas deliberately not proposed, with reasons:

- A global type table: retention is per requested root (section 8).
- `using`-based subclass tests: markers, traits and `implements` cover them.
- A dynamic `Any`: it was rejected when `reflect.of` was ratified.
- String notes on fields: typed field markers replace them.
- String-built code: typed templates replace it, and raw code text needs the
  ratified raw-code power.
- A Core write through reflection: ratified run-time reflection is read only.
  Edits come from code a marker writes (section 6).
- SoA pointer packing: `Columns<T>` replaces it.

## 2. What the video shows

Jai builds a table of every type. At run time, `type_info(T)` points to a
record whose tag gives the kind (C1). Integer records carry signedness and
size, pointer records the pointee, procedure records the argument and return
types, and struct records the name and members (C2). The same facts exist
while compiling and at run time (C3).

On top of that table:

- `print` prints any struct with no per-type code and no format specifiers
  (C4, C5, C24).
- The editor lists and edits any entity's fields, with no per-member
  registration (C8, C25, C26).
- After all structs compile, a metaprogram:
  - walks the table and keeps the types that include `Entity`;
  - sorts them;
  - generates a count, a name table, per-type buckets, index lookups and
    default constructors from string templates (C10, C11, C13).
- Generated code can be saved to a file for debugging (C12).
- Per-type buckets improve locality (C14).
- Defaults drive undo and redo (C15, C22), and also greying of default fields
  in the editor (C21).
- Console commands bind from the same metadata (C16).
- Each entity is one text file in source control (C17). Loading is
  positional, not by name (C18), and is version-aware (C19).
- Field notes such as `DoNotDisplay` filter the editor (C23).
- A new struct that includes `Entity` appears in the editor with no other
  edit (C20).

## 3. State report

### 3.1 Built today (exercised)

| Capability | Evidence |
|---|---|
| Generic print of any struct, type-checked interpolation | probe `p9_today.jet` prints `Rock { size: 1.0, name: rock }` |
| `T.reflect()` inside derive templates: fields (name, type as text in `ty`, `has_marker`, `is_pub`, span), methods (text signatures), markers (strings), layout size and field offsets | `Compiler/JetSema/Source/Sema/Comptime/Reflection.jet:800-818`, `:868-898`, `:1358-1407`; probe `p2_fields.jet` |
| A compile-time loop and a compile-time `if` inside a generated method body | probe `p12_bodyloop2.jet` prints `size name`, skipping the hidden field |
| Struct-form markers whose `expand(self, target: Target)` writes impls at the site | `Examples/features/reflection/user_rule_body.jet:10-24` (dev HEAD) |
| Built-in and user field markers on fields | `Compiler/Prelude/Markers.jet:372-437`; probe `p4_userfield.jet` |
| Program-wide type list in `fn build` with string `implements("…")` | probe `p3_catalog.jet` |
| `b.generate` writes reviewable files under `.jet/generated`, and `jet inspect expand` shows expansions | `Compiler/JetSema/Source/Sema/Calls/Build.jet:158-168`; `Compiler/JetCli/Source/Cli/Expand.jet:205-207` |
| Typed undo data: `#Patchable` gives `T.Patch`, `apply`, `diff` and `merge` | `Examples/features/types/patchable.jet:1-19`; `Compiler/Prelude/Markers.jet:307-311`; probe `p9_today.jet` |
| `#Codable` and typed decode, RFC 6902 `json.patch` | `Compiler/Prelude/Markers.jet:230-233`; `Examples/features/serde/json_patch.jet:26,68` |
| Run-time `reflect.of(x)`: `type_name`, `path`, `display`, and one level of `fields()` with `name()` and `value()` | `Core/Reflect/Reflect.jet:12-22` (`type_name`, `display`); `Compiler/JetSema/Source/Sema/Calls/Reflection.jet:229-230` (field `name`, `value`); `Compiler/JetIR/Source/BuildRoutes.jet:4976-5021`; probe `p10_reads.jet` |
| Secret-bearing types refused by reflection | registered E0112 |

### 3.2 Ratified, not built

| Capability | Record | Today |
|---|---|---|
| `FieldInfo.type: TypeId`, `default: ?DefaultInfo`, `offset` | D-META-REFLECT2 | `fields[i].default` is E0956 (probe `p5_default.jet`); the type is text in `ty` |
| `T.reflect()` outside templates | D-META-HASH1 ("Elsewhere, `T.reflect()` returns the same kind of value") | `Rock.reflect()` in a constant is E0311 (probe `p10_reads.jet`, first version) |
| Per-field paste in a body: `self.#{f}` | D-META-HASH1 (`self.#{field}`; "e is a String, Type or reflected item") | `self.$fname` is E0302, and an unrolled local repeats as E0118 (probe `p11_bodyloop.jet`) |
| Typed `ValueInfo` / `ValueHandle`; `Field.value()` returns a typed `Value` | D-META-REFLECT2, D-ENUM-CASEMETA1 | display-text snapshot (`BuildRoutes.jet:4976-5021`) |
| Typed marker lookup `target.marker(T)` | D-MARKER-REFLECT1 | only string `has_marker`, `implements`, `has_method` (`Sema/Calls/Reflection.jet:232-255`; `Registration/Derives.jet:245-249`) |
| A marker lists all its uses | D-MARKER-USES1 | `Editable.#uses` is E0003/E0107 (probe `p7_marker_uses.jet`) |
| `#` for compiler action, reads as values | D-META-HASH1 | examples still paste with `$` |
| A `settled` compiler stage | D-JAI-EVENTS1 → #5009 | absent |
| Compile-time-initialized globals | D-JAI-COMPTIME1 → #5029 | uppercase bindings fold; no general form |
| `Rows<T>` and `Columns<T>` | D-COLUMNS1, D-SOA-SITE1 → #2889, #6314 | absent |
| Code-site facts (`#line`, `#file`, `#package`) | D-JAI-SITE1 → #5010; #3520 [24] | partial |
| Run-time type-info retention as a build setting | D-JAI-BUILDOPT1 → #5005 [7] | absent |

### 3.3 In flight

- #4276: the `$` → `#` cutover ([22] spelling, [23] hygiene, [24] cutover).
- #6039: unified markers. [2] covers user markers on every site; [3] says
  markers, derives and `fn build` read the same reflection types; [10] covers
  struct markers and `target.marker(T)`; [12]–[14] cover the cutovers.
- #3520: the reflection programme. [12] callable info; [13] every declaration
  family; [14] typed run-time views; [15] command registry; [16] inspector,
  codecs and migrations; [17] entity buckets; [24] source location.
- #5023: code values and insertion.
- #6129: effect rows on extension points.

### 3.4 A gap that stays unproposed

**Today** `b.generate` expands its body in an empty compile-time scope
(`Sema/Calls/Build.jet:158-168`). A `fn build` cannot hand a computed list to
it: probe `p6_gen.jet` gives E0956 and E0003. Entity discovery does not need
it. A marker's uses, its expand and the `settled` stage cover the same need
without a build step. The related unification of `generate {}` with inline
`b.generate` is already #4275 [95].

## 4. Capability matrix

One row per mined claim. "Planned" names the card and criterion that already
own it. "Proof" points to a proof addition in section 11.

| Claim | Jai mechanism | Jet end state | Disposition |
|---|---|---|---|
| C1 | global `Type_Info` table, `type_info(T)` | `T.reflect()` at compile time; `reflect.of(x)` at run time, retained per requested root | built in part; planned #3520 [14], #5005 [7]; proof B9 |
| C2 | kinds with size, signedness, pointee, procedure types, members | `TypeInfo` with `TypeForm`, `FieldInfo`, callable info, layout | planned #3520 [12],[13]; proof B1 |
| C3 | same facts at compile and run time | one schema, typed `Value` at run time | planned #6039 [3], #3520 [14]; proof B2 |
| C4 | `print` any struct | auto `Display`/`Debug` | built (p9) |
| C5 | type-safe print with no specifier | `"{x}"` interpolation | built (p9) |
| C6 | C++ RTTI is too thin | full typed schema | planned D-META-REFLECT2 via #3520 [13] |
| C7 | syntax is temporary | `#` marks compiler action; reads are values | planned #4276 [22] |
| C24 | print-per-frame debugging | `print`, `reflect.of(x).display()` | built (p9, p10) |
| C8 | editor edits any entity | inspector from field facts; edits through code the marker writes | planned #3520 [16]; proof B3, B8 |
| C25 | manual member registration | none needed: `target.fields` | built (p2) |
| C26 | macros touch several places per field | one declaration | built |
| C9 | header parsers misread code | the compiler is the tool library | planned D-ONE-TOOLS1 |
| C10 | walk table, keep `Entity` structs, sort, generate | `Entity.reflect().uses`, sorted at compile time | planned #6039 (D-MARKER-USES1), #3520 [17]; proof B4, B7 |
| C11 | here-string templates with `%` | typed `#{…}` templates; raw code text only by the raw-code power | planned #4276 [22], #5023 |
| C12 | save generated code to a file | `jet inspect expand`, `.jet/generated` | built; planned #6039 (D-MARKER-EXPAND1) |
| C13 | count, names, buckets, lookups, constructors | constants from uses, `Rows<T>` per type, defaults | planned #3520 [17]; proof B4 |
| C14 | same-type allocation for locality | `Rows<T>` / `Columns<T>` | planned #2889, #6314; cost cell P4 |
| C15 | default state per type, diff, undo | field defaults plus `#Patchable` | built in part (p9); planned #3520 [13]; proof B1 |
| C16 | console commands from metadata | command registry | planned #3520 [15] |
| C17 | one text file per entity | `#Codable` and the codec | built; planned #3520 [16] |
| C18 | names written, positional load | generated positional decode | planned #3520 [16]; proof B3; cost cell P3 |
| C19 | VERSION constant, per-field version notes | library field marker `Since(version)` and codec migrations | planned #3520 [16]; proof B3 |
| C20 | new struct appears with no other edit | marker uses, complete after checking | planned #3520 [17]; proof B4 |
| C21 | grey fields equal to default | compare each field with the default-built value | planned #3520 [16]; proof B3 |
| C22 | undo, redo, cut and paste from metadata | `T.Patch`, `diff`, `merge`, codec | built (p9) |
| C23 | `DoNotDisplay` note | typed field marker read with `f.marker(Hidden)` | planned #6039 [2],[10]; proof B6 |

## 5. The surface

### 5.1 Compile time

```jet
// ratified, not built: Derive, #{…} pastes, f.marker(T), statement-level self.#{f}
// A library field marker.
struct Hidden { impl Marker { sites :: [.Field] } }

// A derivable trait: the trait itself implements Derive.
trait Describe {
    fn describe(self) -> String
}

impl Describe.Derive {
    fn expand(target: Target) {
        impl #{target}.Describe {
            fn describe(self) -> String {
                parts := [String]{}
                comptime loop f in target.fields {
                    comptime if f.marker(Hidden) == None {
                        &parts.push("{f.name}={self.#{f}}")
                    }
                }
                parts.join(" ")
            }
        }
    }
}

#Describe
struct Rock {
    size: Float{1.0}
    #Hidden cache: Int{0}
}
```

- Inside `expand`, `target` is the same value `T.reflect()` returns
  elsewhere. It has `name`, `fields`, `methods`, `traits`, `layout`,
  `generic_bindings` and `constants`.
- Each field has `name`, its type, `visibility`, `default`, `offset` and its
  markers. The type field is written `f.type` here; ballot FIELDTYPE1 settles
  `type` against `ty`.
- `f.marker(Hidden)` returns `Hidden?`.
- `#{x}` pastes a name, a type or a reflected item. A compile-time value used
  in added code, such as `f.name` in the string, is copied in.

The same body runs today in the old spelling, except for the per-field paste:

```jet
// runs today (probe p12_bodyloop2): prints "size name"
marker Hidden($sites: [.Field])

derive T.Describe {
    fn describe(self) -> String {
        parts := [String]{}
        comptime loop field in T.reflect().fields {
            comptime if !field.has_marker("Hidden") {
                &parts.push(field.name)
            }
        }
        parts.join(" ")
    }
}

#Describe
struct Rock {
    size: Float{1.0}
    #Hidden cache: Float{0.0}
    name: String{"rock"}
}

fn run() {
    rock := Rock{}
    print(rock.describe())
}
```

### 5.2 Run time

```jet
// runs today (probe p10_reads): prints "Rock", "size = 1.0", "name = rock"
use reflect :: core.reflect

struct Rock {
    size: Float{1.0}
    name: String{"rock"}
}

fn run() {
    v :: reflect.of(Rock{})
    print(v.type_name())
    loop f in v.fields() {
        print("{f.name()} = {f.value().display()}")
    }
}
```

Run-time reflection reads values and never writes. That rule is ratified
(D-ENUM-CASEMETA1 A: "no new get, read, cast, write-by-name, or constructor
operation"). Run-time facts exist only for types reached by a `reflect.of`
call, unless a build setting widens them (section 8).

### 5.3 The split

| Need | Compile time | Run time |
|---|---|---|
| Field list, types, defaults, offsets | `target.fields`, `T.reflect()` | `reflect.of(x).fields()` |
| Marker on a field or type | `f.marker(Hidden)`, `Rock.reflect().marker(Entity)` | the constants that expand wrote |
| All uses of a marker | `Entity.reflect().uses` | the folded constant list |
| Generated code | templates in `expand` | none |
| Values | none | typed `Value`, read only |

The rule is: **decide at compile time, carry only constants to run time.**
Everything the editor needs is folded into ordinary constants and methods by
`expand`. `reflect.of` is the exception, for code that is truly generic over
values.

## 6. Safety and privacy

- **Read only; edits come from markers.** Reflection never writes, casts or
  constructs by name (D-ENUM-CASEMETA1 A). An editor edits a field through
  ordinary code that a library marker's `expand` wrote. Package markers "may
  read target facts, add ordinary checked items, and reject code" (#4276 [13]).
  Only types that carry the marker can be edited by name, and each write is
  typed code for one field.
- **Secrets.** A type that holds a secret cannot be reflected (E0112).
- **Owner package.** Only the package that declares a marker can list its
  uses (D-MARKER-USES1). Other packages get a registered refusal, so a
  library cannot enumerate another library's types.
- **Privacy.** Field visibility is part of `FieldInfo`. Templates follow the
  ordinary visibility rules of the site they expand at.
- **What `expand` may do.** Compile-time eligibility follows the effect model
  and keeps the pure, recorded and `#Impure`-plus-permission tiers (#4276 [2]).
  Whether an extension-point row narrows that for `expand`, for example
  forbidding an `embed_file` inside a marker, is #6129's open question
  ([36]). The effect name `Compiler.Read` used there is proposed, not
  ratified.
- **Completeness.** A marker's uses list is complete once checking has
  finished, so it folds to a constant. Jai's table walk gives no such
  guarantee.

## 7. Diagnostics

**Today**:

| Code | Text | When |
|---|---|---|
| E0956 | "A derive template binding is not a known-time value" | reading `fields[i].default` (p5); passing build values into `b.generate` (p6) |
| E0311 | "`Rock` has no matching static method `reflect`" | `Rock.reflect()` outside a template (p10) |
| E0302 | "`Rock` has no field `$fname`" | per-field paste in a body (p11) |
| E0112 | "secret-bearing `T` cannot be reflected" | reflecting a secret type |
| E0003 / E0107 | parse and name errors | `Editable.#uses` (p7) |

End state: each refusal has a registered code with what/why/fix text and a
UI snapshot (I4). The owning cards register them.

- **Uses read outside the owner package.** Owned by #6039 (D-MARKER-USES1);
  criterion B7.
- **Run-time reflect when `type_info` is `none`.** Owned by #5005 [7];
  criterion B9.
- **Metadata not available for a type**, for example an incomplete or foreign
  type, gives `MetadataError`. Owned by #3520 [13] (D-META-REFLECT2).
- **`f.marker(T)` where `T` is not a marker.** Owned by #6039 [10];
  criterion B6.

## 8. Binary size and performance

Policy:

- A program that never reflects carries no reflection table (#3147 [12]).
- Run-time facts are kept only for requested roots: "Dead-code elimination
  must retain explicitly requested metadata without retaining unrelated
  metadata by default" (D-META-REFLECT2 A).
- D-JAI-BUILDOPT1 A lists the setting's values as "full, reachable or none"
  and maps them to Jai's `runtime_storageless_type_info` and
  `#type_info_none`. It defines neither `reachable` nor a default. The only
  reading consistent with D-META-REFLECT2 is: `reachable` is the default and
  keeps the types reachable from run-time reflect calls and explicit retain
  roots; `full` keeps every type; `none` keeps nothing. Criterion B9 records
  this; section 13 lists it as a definition gap, not an owner choice.
- Compile-time reflection costs nothing at run time, because it folds to
  constants and ordinary methods.

AGENTS requires a paired two-program cell for each surface used for
performance. Each cell runs on O0, O1 and O2, and on web where it applies.

| Cell | Program A (reflection) | Program B (plain) | Pass | Home |
|---|---|---|---|---|
| P1 print | `print(rock)` through auto `Display` | hand-written `Display` | A within noise of B | N1 |
| P2 inspector set | marker-written `set(field, text)` | hand-written per-field setter | A within noise of B | N1 |
| P3 decode | generated positional decode | by-name decode | A faster than B (the video's claim) | N1 |
| P4 storage | `Rows<T>` per entity type | one mixed list of a sum type | A faster than B for iterate-one-type | N1 |
| P5 size, none | program with no reflection | same program with the reflect import removed | byte-identical output | #3147 [12] (B5) |
| P6 size, one root | `reflect.of(rock)` only | same, plus `Door` declared and used | no `Door` record in A | #3147 [12] (B5) |
| P7 compile time | 200 entity types with the marker | 200 hand-registered types | compile time within 10% | N1 |

## 9. One example per capability

Each example is the end-state program unless its label says it runs today.
The engine package `engine` declares `Entity`, `Hidden`, `Since` and `Edit`.

```jet
// runs today (probe p9_today): C4, C5, C24 print anything
struct Rock {
    size: Float{1.0}
    name: String{"rock"}
}

fn run() {
    print(Rock{})                      // Rock { size: 1.0, name: rock }
}
```

```jet
// ratified, not built: C1, C2, C3 Rock.reflect() outside templates, typed field type
use reflect :: core.reflect

SIZE :: Rock.reflect().layout.size                  // folded constant

fn run() {
    print(Rock.reflect().fields[0].type.name)       // Float
    print(reflect.of(Rock{}).fields()[0].value().display())   // 1.0
}
```

```jet
// ratified, not built: C8, C25, C26 any entity's field names, no registration
fn field_names<T>() -> [String] {
    T.reflect().fields.map(.name)
}
```

```jet
// ratified, not built: C10, C13, C20 discover entity types from the marker's uses
#engine.Entity
struct Rock {
    size: Float{1.0}
}

#engine.Entity
struct Door {
    open: Bool{false}
}

// package engine
struct Entity { impl Marker { sites :: [.Type] } }

ENTITY_COUNT :: Entity.reflect().uses.len()          // 2
ENTITY_NAMES :: comptime {
    names := Entity.reflect().uses.map(.site.name)
    &names.sort()
    names                                            // ["Door", "Rock"]
}
```

```jet
// ratified, not built: C11, C12 a typed template; `jet inspect expand` prints its output
struct Kind {
    impl Marker {
        sites :: [.Type]
        fn expand(self, target: Target) {
            impl #{target} {
                fn kind(self) -> String { target.name }   // copied in
            }
        }
    }
}
```

```jet
// ratified, not built: C14 per-type storage (spelling owned by #3520 [17] and #2889)
rocks :: Rows<Rock>{}
doors :: Rows<Door>{}
```

```jet
// runs today (probe p9_today): C15, C22 undo through typed patches
#Patchable
struct Rock {
    size: Float{1.0}
    name: String{"rock"}
}

fn undo(before: Rock, after: Rock) -> Rock {
    back :: Rock.diff(before.copy(), after.copy())
    Rock{after.apply(back)}
}
```

```jet
// ratified, not built: C21 grey the fields that equal the type's default
// (written in the Entity marker's expand; needs statement-level self.#{f})
impl #{target} {
    fn greyed(self) -> [String] {
        base :: #{target}{}
        out := [String]{}
        comptime loop f in target.fields {
            if self.#{f} == base.#{f} {
                &out.push(f.name)
            }
        }
        out
    }
}
```

```jet
// ratified, not built: C8 the edit path, written by the Entity marker's expand
// package engine
pub trait Edit {
    fn set(&self, field: String, text: String) -> Bool
}

struct Entity {
    impl Marker {
        sites :: [.Type]
        fn expand(self, target: Target) {
            impl #{target}.Edit {
                fn set(&self, field: String, text: String) -> Bool {
                    comptime loop f in target.fields {
                        if field == f.name {
                            self.#{f} = #{f.type}.parse(text) ?? return false
                            return true
                        }
                    }
                    false
                }
            }
        }
    }
}

pub fn edit<T: Edit>(value: &T, field: String, text: String) -> Bool {
    &value.set(field, text)
}

// app
fn run() {
    rock := Rock{}
    engine.edit(&rock, "size", "2.0")
    print(rock.size)                                 // 2.0
}
```

```jet
// ratified, not built: C16 console commands (spelling owned by #3520 [15])
#cli.Command
fn spawn(kind: String, x: Float, y: Float) { … }   // the registry lists it
```

```jet
// ratified, not built: C17, C18, C19 one file per entity, positional load, versions
#[engine.Entity, Codable]
struct Door {
    open: Bool{false}
    #engine.Since(3) locked: Bool{false}
}
// the decoder the marker writes reads fields by position for the current
// version, and runs the migration for files written before version 3
```

```jet
// ratified, not built: C23 a field the editor hides
#engine.Entity
struct Rock {
    size: Float{1.0}
    #engine.Hidden cache: Int{0}
}
// in the inspector template: comptime if f.marker(Hidden) == None { … }
```

## 10. New work (list a)

**N1. Reflection cost receipts.** Record cells P1–P4 and P7 (section 8) in
the canonical performance manifest. Each cell is a pair of programs with the
same output. P5 and P6 belong to #3147 [12] (B5), not to N1.

Acceptance:

1. Each N1 cell has a golden output on O0, O1 and O2, and on web where the
   features exist.
2. P3 shows positional decode beating by-name decode, and P4 shows `Rows<T>`
   beating a mixed list for iterate-one-type. A loss stays recorded on a card,
   as AGENTS requires.
3. Cells whose features are unbuilt wait on their owners: #3520 [16] and
   [17], #2889 and #6314.

No card owns these cells today. #6314 [4] covers allocator and SoA surfaces
only.

## 11. Criteria to add to existing cards (list b)

Exact text for Main. "Proof addition" means the existing criterion already
states the requirement; only the named proof is new.

- **B1 → #3520 [13]** (proof addition): "Proof: a golden reads a field's
  type, `default` and `offset` (D-META-REFLECT2 `FieldInfo`) in marker and
  derive `expand`, in `T.reflect()` from ordinary code and in `fn build`, for
  a struct with a defaulted field. Today the default read is E0956 and
  `Rock.reflect()` outside a template is E0311. `TypeInfo` lists associated
  constants with their values and generic bindings, and numeric forms report
  signedness and bit width. String `implements("X")` reads are replaced by the
  typed `traits` list."
- **B2 → #3520 [14]** (proof addition): "Proof: a golden walks a three-level
  nested value through `reflect.of(x).fields()[i].value()` on O0, O1 and O2.
  The one-level display-text record (`BuildRoutes.jet:4976-5021`) is gone."
- **B3 → #3520 [16]** (proof addition): "The reflected_inspector witness
  reproduces the 2017 Jai editor demo:
  - it lists the fields of any selected entity;
  - it hides fields with a library field marker;
  - it greys fields equal to the default-built value;
  - it edits through an `Edit` impl that the entity marker's expand wrote;
  - it undoes through `Patch` and `diff`;
  - it saves each entity as one text file that round-trips.

  A library `Since(version)` field marker drives a generated positional
  decoder and migrations for older files."
- **B4 → #3520 [17]** (proof addition): "Proof: an engine-style library marks
  entity types with its own marker and derives the count, the sorted names,
  per-type `Rows<T>` storage and default constructors from
  `Entity.reflect().uses`, with no hand-kept list. A golden shows that adding
  one marked struct is the only edit needed for it to appear."
- **B5 → #3147 [12]** (proof addition): "Proof: paired size cells in the
  canonical manifest. A program with no reflection has byte-identical output
  to the same program with the reflect import removed. A program reflecting
  only `Rock` carries no record for a declared and used `Door`."
- **B6 → #6039 [10]** (amend): add "`target.marker(T)` works on every site
  kind, including fields, and on named types as `Rock.reflect().marker(T)`. A
  non-marker argument is a registered refusal with a UI snapshot. String
  `has_marker("X")` reads (`Sema/Calls/Reflection.jet:232-255`,
  `Registration/Derives.jet:245-249`) are retired by the `jet fix` rewrite
  D-MARKER-REFLECT1 names."
- **B7 → #6039** (new criterion): "`Entity.reflect().uses` lists every use
  of a marker with its typed marker record and site (D-MARKER-USES1). It is
  refused outside the marker's own package with a registered code and UI
  snapshot, and folds to a constant after checking. Today `Editable.#uses` is
  E0003/E0107."
- **B8 → #4276 [22]** (proof addition): "Proof: a `comptime loop f in
  target.fields` inside a generated method body pastes `self.#{f}` in
  expression and assignment positions, and each unrolled iteration's locals
  are private. Today `self.$fname` is E0302 and a repeated loop local is E0118
  (research probe p11_bodyloop)."
- **B9 → #5005 [7]** (proof addition): "Runtime type-info retention is the
  `type_info` option with values `full`, `reachable` and `none`. `reachable`
  is the default and keeps only the types reachable from run-time reflect
  calls and explicit retain roots, as D-META-REFLECT2 requires. `full` keeps
  every type. `none` refuses `reflect.of` at compile time with a registered
  code and UI snapshot."
- **B10 → #5021** (new criterion): "`Docs/audits/jai-parity-2026-10-16.md`
  row C08 is corrected to partial in a new dated audit entry. It cites
  `BuildRoutes.jet:4976-5021` and names #3520 [14] as the owner."
- **B11 → #6129 [36]** (proof addition): "The published types for marker
  expansion and derive are the D-DECL-TYPES1 methods,
  `Marker.expand(self, target: Target)` and `Derive.expand(target: Target)`,
  with template bodies (D-CTCODEGEN1). The 2026-10-29 effect-types proposal,
  section 9.1, is amended to match, and the effect name for their row is
  settled by this card."

## 12. Contradictions for the owner (list c)

### 12.1 Is a reflected field's type `f.type` or `f.ty`?

Ballot: `$JET_SCRATCH_ROOT/lanes/Main/ballots/D-REFLECT-FIELDTYPE1.json`.

- **Side 1, D-META-REFLECT2 A** (2026-09-21): "FieldInfo has identity:
  DeclarationHandle, name: String, type: TypeId, …". `ParameterInfo`,
  `CaptureInfo`, `SettingInfo` and `ValueInfo` also use `type`, and
  D-MARKER-LADDER1 A writes `p.type` for parameters.
- **Side 2, D-META-HASH1 A** (2026-10-30): "Inside expand, `target.fields`,
  `target.name` and `field.ty` are plain field reads". **D-ENUM-CASEMETA1 A**
  (2026-09-12): "FieldInfo.ty is the payload field's Type handle".

Why they conflict: both names are in ratified text for the same field.
**Today** the compiler's record uses `ty`, holding type text. No example or
test reads a reflected field's type by either name.

Options:

- **A (recommended):** `type` on every reflected record; amend the two `ty`
  mentions.
- **B:** `ty` on every reflected record; amend the schema.

## 13. Record corrections (no owner choice)

AGENTS says that code conflicting with an approved requirement is a defect
and must be recorded in Tower. These entries have one ratified meaning, so
they need no ballot.

- **Stale `#` reads in two records.** D-MARKER-USES1 A writes `Route.#uses`
  and D-MARKER-REFLECT1 A writes `Order.#marker(Table)`. Both defer their
  spelling to D-META-HASH1 A: "The read is written in D-META-HASH1 A's
  spelling without presuming it", and "Spelled with D-META-HASH1 A …; under
  another spelling only the marks change". HASH1 A maps type-level reads to
  `T.reflect()`. So the ratified spellings are `Entity.reflect().uses` and
  `Rock.reflect().marker(Entity)`. Main should note this on both records.
- **The effect-types proposal's extension-point table.**
  `Docs/proposals/effect-types-2026-10-29.md:367-368` types marker expansion
  as `fn(Site) -[Compiler.Read]> Syntax` and derive as
  `fn(TypeInfo) -[Compiler.Read]> Syntax`. The ratified shape is
  D-DECL-TYPES1 A rules 6 and 9 (`expand(self, target: Target)` and
  `fn expand(target: Target)`, template bodies) and D-CTCODEGEN1 (no injected
  tree). The proposal is unratified and is corrected; criterion B11 carries
  it.
- **`reachable` has no definition.** D-JAI-BUILDOPT1 A names it without
  defining it or a default. Section 8 gives the one reading consistent with
  D-META-REFLECT2; criterion B9 carries it.
- **#6039 [10] was marked met while `target.marker(T)` was absent.** Main
  has reopened it; criterion B6 lists the additions.
- **Run-time `reflect.of` is a display snapshot**, short of the ratified
  typed `Value`. #3520 [14] owns it; criterion B2 adds the proof.
- **The Jai parity audit (2026-10-16) marks C08 as shipped.** #5021 owns the
  audit; criterion B10 corrects it.

## 14. Owner-question matrix

| Owner question | Answer | Where |
|---|---|---|
| Does Jet cover every capability in the video? | Yes, once the planned work lands. 8 of 26 claims are built, 2 are built in part, and 16 are owned by named card criteria. None needs new syntax. | sections 3 and 4 |
| Where does Jet go further than Jai? | Typed reads and markers; complete, owner-only marker uses; hygienic templates; read-only values; secrets refused; per-root retention; columnar storage. | sections 1, 5 and 6 |
| Is it zero cost unless used? | Ratified yes. The proof is cells P5 and P6 on #3147 [12]. | section 8 |
| How does an editor write a field? | Through typed code a library marker writes. Reflection stays read only. | sections 6 and 9 |
| What needs the owner? | One ballot, FIELDTYPE1. | section 12 |
| What is new work? | N1, the reflection cost receipts. | section 10 |
| What do existing cards gain? | B1–B11. | section 11 |

## 15. Coverage and confidence

**Run directory:**
`$JET_SCRATCH_ROOT/lanes/Main/research/mine/jai-reflection-tSv-2026-11-04/`.
It holds `coverage.json`, `ledger.json`, `claims.json`, `recall-audit.json`
and `coverage-receipt.txt`.

**Digest identity:**

- HEAD `15098e62`;
- status sha256 `21856f7c…d349`;
- `tower.json` sha256 `6970a10f…7d62`. Card criteria cited in sections 11
  and 13 were re-read from live Tower on 2026-11-05.

**Checker** (`check_coverage.mjs`):

```text
source tSv-872TXpQ (video, 10.1 min, 3 segments): ledger 35 (3.46/min), claims 26 (2.57/min), micro 20 / macro 6
  tSv-872TXpQ/01 00:00-03:53: ledger 13 (3.35/min), claims 8, floor 4 - ok
  tSv-872TXpQ/02 03:53-07:58: ledger 12 (2.94/min), claims 10, floor 5 - ok
  tSv-872TXpQ/03 07:58-10:07: ledger 10 (4.65/min), claims 8, floor 3 - ok
PASS
```

**Recall audit:** seed `prep-2026-11-05`, run by Prep, who did not author the
run. It sampled segments 01 and 02 and re-mined them from the transcript and
frame f118 before reading the ledger. Outcome: no misses, no disputes.

**Fresh-context review:** Prep, 2026-11-05, findings F1–F22. All are
resolved in this revision.

**Limits:**

- The captions are automatic, and frames were used to corroborate them.
- The audience-corrections lane did not run, because the clip has no Q&A.
- The probes ran on the admitted compiler `7c672809`, not on HEAD. Blocks
  marked "ratified, not built" were not run.

**Strongest unverified assumption:** that a statement-level compile-time
loop with per-field pastes is meant to be in #4276 [22]'s scope. B8 makes it
an explicit proof.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| R1 typed field facts and `T.reflect()` outside templates (B1) | card | #3520 |
| R2 run-time view is a display snapshot (B2) | card | #3520 |
| R3 inspector, versions, greying and positional decode (B3) | card | #3520 |
| R4 entity discovery and buckets (B4) | card | #3520 |
| R5 typed marker lookup (B6) | card | #6039 |
| R6 marker uses read (B7) | card | #6039 |
| R7 per-field paste in generated bodies (B8) | card | #4276 |
| R8 run-time type-info default and `reachable` (B9) | card | #5005 |
| R9 no-table size proof, cells P5 and P6 (B5) | card | #3147 |
| R10 published `expand` type and effect-types table (B11) | card | #6129 |
| R11 parity audit overstates C08 (B10) | card | #5021 |
| R12 editing through reflection | decision | D-ENUM-CASEMETA1=A |
| R13 spelling of the uses and named-type marker reads | decision | D-META-HASH1=A |
| R14 build values into `b.generate` | card | #4275 |
| R15 field type named `type` or `ty` | no-action | owner ballot D-REFLECT-FIELDTYPE1 drafted; Main posts it |
| R16 reflection cost cells P1–P4 and P7 | no-action | new work N1 proposed here; card creation is Main's |
<!-- /audit-dispositions -->
