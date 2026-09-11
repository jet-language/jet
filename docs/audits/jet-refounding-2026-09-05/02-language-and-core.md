# Make types and ordinary functions do more of the work

[Executive report](index.md) · [First principles](01-first-principles.md) · [Tools and systems](03-tools-and-systems.md)

## How to read the code

**Current** samples quote or closely isolate an inspected repository example. **Proposed** samples specify an API that is not implemented by this audit. A proposed sample is not a claim that the current parser, checker, or runtime accepts it. Where a sample is an excerpt, its surrounding inputs are named explicitly; an ellipsis is not used as a substitute for an implementation claim.

The recurring Jet notation is small enough to explain once:

| Notation | Read it as |
|---|---|
| `name :: value` | Bind a name to a value that is not reassigned |
| `name := value` | Bind a name that can be changed |
| `struct Sale { ... }` | Define the fields of a sale |
| `[Sale]` | A list whose elements are sales |
| `?Sale` | A sale may be absent |
| `Value !Error` | The operation returns a value or a stated error |
| `value ?? alternative` | Use the alternative when the ordinary value is absent or the operation fails, according to the checked carrier |
| `sale -> sale.cents` | A small function: given a sale, return its cents field |
| `&value` | Give exclusive write access for the call |
| `^value` | Transfer ownership |
| `~value` | Request a semantic copy |
| `.North` | The `North` variant of the type expected at this position |
| `loop item in items` | Visit each item; the protected loop decision is unchanged |

The new power below uses these ordinary forms. No `live` binding keyword, query comprehension syntax, schema language, dependency-injection annotation, or generic “graph” syntax is required.

## F01 · An endpoint is a function plus its transport mapping

### The job

You have a function that looks up a ticket. You want an HTTP endpoint and a client that calls it. The request should be checked before the function runs. The client should know the result and possible failure types. Renaming a field should not leave an unrelated schema or client silently stale.

An **endpoint** is one operation exposed outside a program. Its **transport mapping** says how an HTTP path, query, or body supplies the function's inputs, and how its result becomes a response. It does not decide the business rule or grant authority.

### What Jet already has

Jet has typed functions, argument contracts, generated codecs, accumulated field validation, web app graphs, HTTP serving, and OpenAPI-related source. D-WEBAPP-SERVE1 already chooses automatic serving of the app graph under `jet run` and `jet dev`. D-SHAPE-ONE1 and D-SHAPE-PROJECT1 already require shared field facts and generated argument decoding.

The [web battery](../../../examples/features/web/battery/run.jet) still shows manual route-parameter conversion and form validation beside the app graph. The [backend battery](../../../examples/features/net/backend_battery/run.jet) separately constructs a router and checks its OpenAPI document. These are source observations, not proof that every high-level route is missing. The proposed increment is a **complete checked function-to-endpoint-to-client relationship**, not another HTTP server or a claim that Jet lacks web routing.

### The proposed ordinary form

The following is a declaration excerpt. `lookup_ticket` is the application's existing function with input `GetTicket`, output `Ticket`, and its declared lookup error. Its implementation remains ordinary application code.

```jet
struct GetTicket {
    id: Int(1..)
}

// Proposed endpoint declaration; handler implementation is unchanged.
endpoint :: web.get("/tickets/{id}", lookup_ticket)
```

The path placeholder `id` names a field of `GetTicket`. The compiler checks that relationship. It does not turn a typo into a runtime lookup. The request decoder uses the same type and validation rules as an ordinary typed decode. The server adapter calls `lookup_ticket`; the client adapter exposes the same typed operation.

The ordinary form has a strict, teachable binding rule:

1. Path placeholders bind matching top-level input fields.
2. For a GET operation, remaining fields come from query parameters.
3. For an operation with a body, the explicitly selected body model supplies the remaining fields.
4. A field supplied from two places is rejected, not silently resolved by an undocumented precedence.
5. Missing, repeated, malformed, or invalid values use the existing field-error model. Invalid is not treated as absent.
6. The operation is not published until it is explicitly added to the application's public graph.

A complex endpoint uses an explicit binding record over the same descriptor. Field-name literals in that record are checked against the input type; they are not arbitrary unverified strings. An author can rename an HTTP parameter without renaming an internal field, just as a public argument label can differ from its local parameter name.

### The generated client is not a second API definition

A generated client is a checked projection of the endpoint descriptor. It contains the request type, response type, failure cases, transport mapping, and endpoint identity. It does not scrape documentation or infer behavior from a sample response.

```jet
// Proposed client use; ticket_api is generated from the checked endpoint set.
request :: GetTicket{id: 42}
ticket :: ticket_api.lookup_ticket(request) ?? panic("lookup failed")
print(ticket.title)
```

Changing `GetTicket.id` or the public response type changes the descriptor and client together. A stale client artifact is rejected by its descriptor identity. The same change is visible to API comparison and generated documentation.

### One declaration does not mean one giant shared record

A database row, an internal domain object, and a public response often mean different things. A `User` may contain a credential hash; a public user response must not. The programmer still chooses a public response type or an explicit projection. Sharing facts is not permission to publish every field.

The compiler must refuse an undeclared exposure of secret or restricted data. It must not guess authentication, authorization, tenancy, or a business-error-to-status mapping. Those remain explicit endpoint/application policy. Generated binding removes mechanical work; it does not invent policy.

### Alternatives and the recommendation

| Option | Same ticket-lookup job | Gain | Real cost or loss |
|---|---|---|---|
| **A · Derive the endpoint contract from the function and explicit transport mapping** | Register `lookup_ticket` once and generate its adapters | Strong type continuity; one place to change the operation | The binding rule and descriptor become compiler/library obligations; transport policy must remain explicit |
| B · Keep separate request/schema/client definitions with a consistency checker | Write each form, then check their agreement | Maximum independent presentation control | The user still writes and updates the same mechanical information several times |
| C · Define a typed protocol first, then check its implementations | Make the protocol authoritative and bind `lookup_ticket` to it | Supports an external protocol, independent teams, or several implementations before one handler exists | A separate declaration remains; binding must diagnose disagreement with the handler |
| Rejected · Treat every public function as remotely callable | Export by visibility alone | Very little registration text | Visibility is not authority; accidental exposure and unclear transport semantics make this unacceptable |

**Recommend A for a Jet-owned handler.** Its checked function already supplies the operation's contract. C is the stronger alternative when an external protocol leads several independent implementations. It is not a worse spelling of A: the declaration has a different owner. The ballot presents that real choice. B remains the explicit manual boundary. Visibility-only exposure is rejected.

### Show, control, refuse

- **Show:** `jet inspect shapes` and the endpoint view show path/body binding, public fields, error mapping, generated client identity, and reachable rights.
- **Control:** write explicit field and error mappings; set body limits, deadlines, and authorization through the existing application policy mechanisms.
- **Refuse:** keep the function private; use the explicit HTTP API; reject generation when a type lacks a sound transport representation.

### Complete acceptance

The generated client must call a real running server, not a mock that repeats its inputs. A wrong path-field name must be a Jet diagnostic. A malformed request must not enter the handler. A private field must remain private. A handler error must reach the client through its declared mapping. Schema/client changes must invalidate together. Native server execution and each applicable client target must share the same codec and error semantics.

The implementation extends the existing web and shape owners. It must delete any replaced duplicate metadata rather than leave two authoritative endpoint descriptions.

## F02 · One typed query, ordinary values at the end

### The job

Group sales by region and total their amounts. Later, read the sales from a file. Later still, keep the result current. The calculation should not lose the meaning of “region” or “amount” at any of those steps.

A **query** is a description of the calculation, not the result. Calling `collect()` runs it and returns the ordinary list of results. A **materialized** result is simply an answer that has actually been computed and stored.

### Current Jet makes the family harder to carry across boundaries

The current [data example](../../../examples/features/tooling/data_analysis.jet) uses separate functions:

```jet
focused :: data.filter(tickets, t -> t.minutes >= 4.0)
sorted :: data.sort_by(focused, t -> t.team) ?? panic("sort")
joined :: data.inner_join(sorted, owners, t -> t.team, o -> o.team)
    ?? panic("join")
groups :: data.group_mean(sorted, t -> t.team, t -> t.minutes)
    ?? panic("group")
```

The reference also exposes `Table<T>`, `Series<T>`, `LazyFrame<T>`, `DataStream<T>`, `lazy_filter`, and `lazy_sort_by`. `DataGroup` has a `String` key and `Float` sum/mean. These are real distinctions in the current API, but some are wrappers or operation-family differences rather than different user questions.

Jet already has checked SQL. The [analytics example](../../../examples/features/serde/analytics_query.jet) uses:

```jet
query :: SQL{"SELECT * FROM sales WHERE qty > 1 ORDER BY qty DESC LIMIT 2"}
sales :: csv.query<Sale>(path, query)
```

The proposal retains that checked SQL door and D-SQL-SURFACE1. It does not add a rival SQL interpreter.

### The proposed ordinary program

```jet
use core.data as data

enum Region { North, South }

struct Sale {
    id: Int
    region: Region
    cents: Int
}

fn run() {
    sales :: [
        Sale{id: 1, region: .North, cents: 125},
        Sale{id: 2, region: .South, cents: 80},
        Sale{id: 3, region: .North, cents: 75}
    ]
    totals :: data.query(sales)
        .group_by(s -> s.region)
        .sum(s -> s.cents)
        .collect() ?? panic("total failed")
    loop total in totals -> print("{total.key}: {total.value}")
}
```

**Proposed result:** North has 200 cents; South has 80. The key remains `Region`; the amount remains `Int`. There is no conversion to a region-name string or a floating-point amount to satisfy the grouping interface.

`Group<K, V>` is an ordinary result record with `key: K` and `value: V`. For this query it is `Group<Region, Int>`. A count uses an integer value. A mean or variance must follow the existing numeric and unit laws; the query layer does not invent a universal Float conversion. A user-written reducer's result type is determined by that ordinary reducer.

### The proposed cutover is a simplification, not an additional facade

**Authority check:** D-DATA-SURFACE1, D-DATAFLOW1, D-DATA-PLOT1, and affected D-DATA-STATUS1 projections govern the current wrappers, queries, plots, and bridge/status contract. D-QUERY-RETAIN1 is a proposed amendment to those surfaces—not permission to delete them under an implementation card. Until the owner ratifies it, the existing forms remain canonical.

| Current public concept | Proposed home |
|---|---|
| Materialized `Table<T>` | Ordinary `[T]`; static schema comes from `T`, including for an empty list |
| Materialized `Series<T>` | Ordinary `[T]`; statistical functions operate on the element type |
| `LazyFrame<T>` | `Query<T>` |
| `lazy_filter` / `lazy_sort_by` | `Query.filter` / `Query.sort_by` |
| Fixed `DataGroup` | Generic `Group<K, V>` and typed reducer results |
| `DataStream<T>` | Retained as a one-shot, fallible source; its EOF/error/ownership contract is not erased |
| Checked SQL and typed builder paths | Two ratified ways to construct the same normalized typed query operations |
| Existing eager List methods | Unchanged; they remain eager under D-CORE-EAGER1/2 |

This is an owner-gated public API change. Every in-repo caller, plot adapter, example, schema projection, and test must migrate. The old wrappers and old lazy-specific functions do not remain as compatibility aliases.

### A small operation vocabulary with precise rules

| Operation | Type relationship | Observable rule |
|---|---|---|
| `filter(predicate)` | `Query<T>` to `Query<T>` | Preserves relative order of retained rows; predicate errors and effects cannot be silently reordered |
| `map(transform)` | `Query<T>` to `Query<U>` | `U` is the ordinary function result type; no untyped row dictionary |
| `sort_by(key)` | `Query<T>` to `Query<T>` | Stable ordering for equal keys; named memory/spill limits |
| `inner_join(other, left_key, right_key)` | Typed left/right rows and a checked common key relation | Preserves the adopted join multiplicity and deterministic ordering contract |
| `left_join(...)` | Right row is `?R` | Missing is ordinary absence, not an invented null sentinel |
| `group_by(key).sum(value)` | `Query<Group<K,V>>` under the numeric operation's valid rules | Retains key/value identity; group ordering is defined, not hash-table accident |
| `group_by(key).reduce(...)` | Result type comes from the declared reducer | Ordered reduction is the reference meaning unless a stronger algebraic law licenses reordering |
| `collect()` | `Query<T>` to `[T] !DataError` | Runs the plan once; ceilings and source errors remain visible |

A query over a one-shot reader cannot be collected twice by silently reopening or copying it. A reusable in-memory source may be queried again. These capabilities belong to the checked source type and ownership, not an undocumented runtime guess.

### The optimizer does not get to change error behavior

Suppose a CSV row contains an invalid value in a field that a later projection does not display. If the source contract validates that field before yielding the row, projection pushdown cannot skip the error merely because the field is unused later.

The same restriction applies to predicate pushdown, short-circuiting, callback fusion, and floating-point reassociation. The reference behavior includes validation, error order, callback effects, and output order. A transformation needs the facts that make those observations equivalent. “SQL engines commonly do this” is not a Jet proof.

A known-valid in-memory row is a different case from unvalidated bytes. Keeping that distinction gives the optimizer useful freedom without lying about the input contract.

### Alternatives and the recommendation

| Option | Same sales-total job | Gain | Real cost or loss |
|---|---|---|---|
| **A · One typed query with ordinary list results** | Build the calculation once, then collect or watch it | Fewer public wrappers; type-preserving composition; one plan for SQL and methods | The shared plan must retain failure and ordering rules; old data wrappers and callers migrate |
| B · Keep Table/Series/LazyFrame but standardize their methods | Similar fluent calls on three wrappers | Smaller internal API migration | Three result/container concepts remain where ordinary lists already carry the needed type |
| C · Keep separate operation families | Use current eager/lazy/data APIs | No new query contract | The same calculation still has several entry-point and result-shape rules |

**Recommend A.** Retain distinct source/resource types where they carry lifetime or consumption meaning. Do not replace every collection with `Query` or make a list implicitly deferred.

### Show, control, refuse

The ordinary user sees the result. An expert can inspect the typed operations, source validation, ordering, allocation, retained state, pushdown decisions, and refused rewrites. Memory and spill policy reuse `DataLimits` and the existing resource-policy path. Requiring no spill or a specific execution strategy must fail clearly when the plan cannot satisfy it.

Performance evidence must compare the same rows, types, errors, ordering, and precision against the incumbent. A fast stringly query is not a fair replacement for a typed, validated one; a Jet implementation also cannot claim victory by omitting work the peer performs.

## F03 · Make a query stay correct when its inputs change

### The job

A sale changes from 125 to 175 cents. A regional total should rise by 50. A dashboard should not show a new row with an old total, and an obsolete background calculation should not overwrite a newer answer.

Today a programmer often keeps a batch calculation and a separate update routine. The routines must agree on filtering, grouping, deletion, and correction. That is a second program whose only job is to keep the first program's answer current.

### Existing Jet pieces

Computed fields already retain an answer under `#Memo` and invalidate it when a dependency changes. Reactive values already track reads. Live database queries already track relevant reads and invalidation. D-SHARED-REVISION1 is now ratified and owns atomic snapshots and stale-result rejection.

F03 is the missing composition: **a typed query over an explicitly changing source can be maintained from changes to that source**, using the existing publication and lifecycle rules. It is not a new implicit reactive binding rule.

### Proposed program

This continues the `Sale` and `Region` definitions from F02. `initial` is the same three-sale list used there.

```jet
// Proposed changing source and watched query:
sales :: data.track(initial, key: s -> s.id) ?? panic("duplicate id")
totals :: data.query(sales)
    .group_by(s -> s.region)
    .sum(s -> s.cents)
    .watch() ?? panic("watch failed")

sales.replace(1, Sale{id: 1, region: .North, cents: 175})
    ?? panic("missing sale")
print(totals.get())
```

The total changes from North=200 to North=250. The query is the same query. The programmer did not write `old_total - old_sale + new_sale` and did not remember to invalidate a separate chart cache.

`data.track` returns an owned changing table with unique keys. It rejects duplicates rather than choosing an arbitrary winner. `replace` requires the key to exist and the replacement's key to agree. Insert, replace, and remove are distinct operations with explicit errors; “upsert” is not smuggled into `replace`.

### What “incremental” means

Incremental work uses the change rather than recalculating everything. For an exact integer sum, replacing 125 with 175 can subtract 125 and add 175. For a minimum, removing the smallest value may require finding the next smallest. For a join, changing one key may affect many matching rows.

The compiler/library must know the operation's rule. It cannot assume every calculation has a cheap inverse.

| Query operation | Useful maintained state | What a change must do |
|---|---|---|
| Filter | Membership of relevant keyed rows | Evaluate the changed row; handle entry into or exit from the result |
| Map | Dependency from source key to output | Replace the affected output while preserving order and errors |
| Exact sum/count | Per-key or per-group accumulator | Add/remove the exact contribution; remove an empty group correctly |
| Minimum/maximum | Ordered multiset or equivalent checked structure | Preserve duplicates and find the next value when one occurrence is removed |
| Join | Keyed arrangements of both inputs | Preserve duplicate-key multiplicity; produce every added/removed pair |
| Stable sort | Key plus stable source-order identity | Move changed rows without scrambling equal-key order |
| Opaque pure reducer | A valid recomputation path | Recompute the affected scope unless a checked incremental law is available |
| Effectful callback | Not automatically incrementalizable | Reject watched transformation or require an explicitly modeled effect boundary |

The public promise is correctness of the maintained result, not a universal constant-time update. A query can legitimately require work proportional to a large part of the data. The explanation must say which operations are maintained incrementally and which recompute.

### The publication rule prevents mixed revisions

A transaction of input edits publishes one new revision. Every derived result is attached to the input revision it used. A subscriber sees a consistent completed publication, not half of one batch. A result computed for revision 18 cannot replace revision 19.

For work still running, the result type must distinguish a current value, pending work, and an error. It must not return a stale value as though it were fresh. A retained last-good value may be useful, but its freshness is visible through the existing live-query status model. Dropping the final subscription detaches it; cancellation and source destruction terminate or invalidate dependent work according to the shared lifecycle contract.

The implementation should reuse D-SHARED-REVISION1 and the existing `LiveQuery` lifecycle. It must not invent a second epoch counter and publication check for this feature.

### Memory is part of the operation

Incremental computation keeps information to avoid repeating work. That retained information can be substantial. Joins, sorted results, and windows can keep indexes or history. A supposedly faster query that grows without bound is not an acceptable default.

The watched plan reports its retained structures and their ceilings. A ceiling breach is a typed error with the responsible operation and requested/allowed size. It does not silently discard old rows, switch to an approximate answer, or spill to an undeclared location. An explicit spill policy may permit a semantically identical disk-backed structure.

### Alternatives and the recommendation

| Option | Same live-total job | Gain | Real cost or loss |
|---|---|---|---|
| **A · Maintain eligible operations; recompute unsupported pure scopes honestly** | Write the query once and watch it | Removes a second algorithm; supports cheap updates where justified | Retained state and update scheduling; not every operation gets an incremental speedup |
| B · Re-run the whole query after every change | Write the query once and subscribe | Simple correctness baseline | Work scales with the full input even for a tiny edit |
| C · Require a user delta function | Write query plus update routine | Full expert control | Two algorithms can drift; beginner must learn maintenance rules before getting a live result |

**Recommend A**, with B as the reference meaning and explicit refusal of recomputation when the user requires an incremental plan. C is an expert extension only when its law can be checked against the same reference contract; it is not the ordinary path.

### The hard correctness cases

The proof corpus must include deletion, duplicate join keys, group-key changes, empty groups, ordering ties, cancellation, source destruction, stale publication, and budget exhaustion. A Float reduction must preserve the adopted numerical contract; subtracting an old rounded sum is not automatically equivalent to recomputing it. A query containing validation or failure cannot drop an old error without accounting for the same observation.

This is where the interesting research begins. The theory of incremental queries supplies a method; Jet must extend the method with its own type, effect, ownership, and failure rules. The research chapter gives the candidate theorem and a standalone exact-integer investigation, without presenting either as a shipped Jet engine.

## F04 · A position should say which space it belongs to

### The job

A mouse position is measured in window pixels. A game object position is measured in world coordinates. Both may be pairs of numbers. Adding them is usually nonsense, but ordinary numeric types cannot tell.

A **coordinate space** says where a position is measured from and which axes it uses. A **transform** converts a position from one space to another. A metre-versus-pixel distinction helps, but units alone do not distinguish two cameras or two world origins.

### Existing Jet is already partway there

Jet has exact unit families, dimension algebra, Point/Delta distinctions for affine quantities, vectors/matrices, and the carrier-plus-knowledge foundation. It should not get a second dimension system. The proposed increment is **nominal source/destination spaces for geometry and transform composition**, represented with ordinary types and the existing knowledge rules.

The precedent is concrete: [Euclid's typed geometry](https://docs.rs/euclid/latest/euclid/) uses a generic unit/space parameter to prevent screen-space values from mixing with world-space values. Jet can make that protection the normal domain-library path while reusing its stronger unit and ownership information.

### Current style and the proposed style

Current-style numeric code can only rely on names and comments:

```jet
// Two numeric pairs; the names carry the space distinction.
mouse_x :: 240.0
mouse_y :: 120.0
camera_x :: 1000.0
camera_y :: 500.0
world_x :: mouse_x + camera_x
world_y :: mouse_y + camera_y
```

That translation is valid only for a particular camera model and scale. Zoom, viewport scaling, rotation, or a second camera can invalidate it without changing the numeric types.

Proposed typed geometry makes the relationship explicit. `Screen` and `World` are ordinary nominal types; `screen_to_world` is a `Transform2<Float, Screen, World>` created from the application's camera state.

```jet
// Proposed stock name: ScreenPoint = Point2<Float, Screen>.
mouse :: ScreenPoint.new(240.0, 120.0)
world :: screen_to_world.point(mouse) ?? panic("invalid transform")

// Rejected: the transform expects Screen, not World.
wrong :: screen_to_world.point(world)
```

The ordinary game/UI library supplies meaningful stock types and camera operations, so a beginner does not write generic parameters for every point. The generic form remains available for custom coordinate systems. An untyped “unknown space” is not the default escape from a mismatch.

### Composition should read in application order

```jet
// Proposed: first world to view, then view to screen.
world_to_screen :: world_to_view.then(view_to_screen)
screen :: world_to_screen.point(player_position)
    ?? panic("projection failed")
```

The middle spaces must match. `World -> View` followed by `Screen -> Device` is a compile-time mismatch, not a numeric matrix multiplication that happens to have compatible dimensions.

The library distinguishes operations that are mathematically different:

| Operation | Rule |
|---|---|
| Point plus displacement in the same space | Produces a point in that space |
| Point minus point in the same space | Produces a displacement |
| Point plus point | Rejected unless an explicitly named domain operation supplies a meaning |
| Transform composition | Destination of the first matches source of the next |
| Inverse transform | Fallible when singular or otherwise non-invertible |
| Perspective screen point to world | Produces a ray or requires depth/plane information; it does not invent a unique point |
| Dynamic scene/frame identity | Checked at runtime when it cannot be established statically; not erased and called “zero cost” |

### Static spaces and runtime identity are different

`World` can name a coordinate convention. It does not automatically prove that a point from one dynamically loaded scene belongs to another scene with the same convention. Such values need an owner/frame identity where the application can mix instances. The existing provenance and generation machinery supplies that check.

Similarly, an event recorded before a viewport change cannot be interpreted using a new transform without saying so. A UI adapter should attach the relevant frame/revision identity and either use the matching transform or report a stale conversion. This prevents a class of “the click lands on the wrong object after resize” mistakes that memory safety alone does not address.
The public constructors make the distinction visible without a second syntax:

```jet
dynamic_mouse :: Point2<Float, Screen>.new(240.0, 120.0, viewport_frame)
world_ray :: screen_to_world.ray(dynamic_mouse) ?? panic("stale viewport")
world_at_depth :: screen_to_world.point_at_depth(dynamic_mouse, depth)
    ?? panic("stale viewport")
```

`Point2` and `Delta2` retain the supplied frame identity; stock `ScreenPoint`
and `ScreenDelta` constructors are static, frame-free carriers. A dynamic
operation returns `Result<..., TransformError>` so an old viewport or scene
cannot be hidden by an unchecked arithmetic result. Static aliases keep their
ordinary value representation and do not pay for a runtime tag.


### Alternatives and the recommendation

| Option | Same mouse-picking job | Gain | Real cost or loss |
|---|---|---|---|
| **A · Typed spaces and checked transforms** | Convert through `Transform<Screen,World>` | Rejects a wrong-space operation; composes with units and matrices | Public geometry types carry a space parameter; dynamic instance identity sometimes needs runtime data |
| B · Distinct named point structs for each application | Write manual conversion functions | Ordinary types can catch some mixing already | Repeats geometry operations and transform rules for every space |
| C · Raw vectors and naming conventions | Add camera numbers manually | Minimal visible types | The compiler cannot distinguish correct and incorrect spaces |

**Recommend A.** It is an ordinary library/type extension, not a new keyword or a new kind of number. Safe stock domain types keep the common path short. Explicit raw numerical escape remains subject to the existing audit and conversion rules, not an automatic erasure of space identity.

### What this prevents, and what it does not

It can reject using a screen displacement as a world displacement, composing transforms in the wrong space order, or using a stale dynamic frame handle. It cannot know that the artist chose the wrong camera, that the designer intended a different collision shape, or that a physically valid transform is the desired one. Those remain domain requirements and tests.

## F07 · Share columnar data through one checked lifetime

### The job

A database, Python library, or analytics engine already has a large table in memory. Jet should not need to convert every row into a dictionary and rebuild it merely to inspect or query the table. It also must not keep pointers after the producer releases the memory.

**Columnar** means values from the same field are stored together. **Zero-copy** means the data buffers are shared rather than duplicated. Neither phrase means “skip validation” or “ignore ownership.”

### The boundary should be a typed value, not raw C structures

The [Arrow C data interface](https://arrow.apache.org/docs/format/CDataInterface.html) is specifically designed for same-process sharing without depending on the Arrow software libraries. It describes arrays and schemas and defines release callbacks. Consumers release the base structure once, not each child independently; producers release their children. Exported data should be treated as immutable.

That fits Jet's ownership model. The raw ABI remains in a vetted boundary adapter. Ordinary code receives a proposed `ColumnBatch<Row>` handle carrying checked schema, buffer layout, and release ownership.

```jet
// Proposed; producer_result is a vetted foreign Arrow export.
batch :: data.import_arrow<Sale>(^producer_result)
    ?? panic("incompatible data")
totals :: data.query(batch)
    .group_by(s -> s.region)
    .sum(s -> s.cents)
    .collect() ?? panic("query failed")
```

The query is the F02 query. The buffer adapter does not get a separate grouping implementation or numeric policy.

### What must be checked before exposing a typed row

| Boundary fact | Required behavior |
|---|---|
| Schema field names and physical types | Match the requested row model or use an explicit checked conversion |
| Nullability | An absent cell requires `?T`; do not fabricate a value |
| Lengths, offsets, child counts, dictionaries | Validate according to the supported Arrow format contract before safe indexing |
| Buffer lifetime | The imported owner outlives every view and query using the buffers |
| Release callback | One owner releases the base once; children follow the Arrow producer rule |
| Mutability | Shared read-only data stays immutable; a writer requires exclusive ownership or an explicit copy |
| Units, nominal keys, and semantic tags | Preserve declared metadata where meaningful; do not infer dollars or trusted text from a matching numeric/string layout |
| Unknown producer behavior | Remains a foreign trust boundary; a schema match is not proof that arbitrary C code obeys its lifetime contract |

A required conversion is reported as a copy with its reason. An expert may require no-copy import and receive a typed refusal when layout or lifetime makes it impossible. The default may choose a safe copy, but never call it zero-copy.

### Native file formats are a separate promise

An Arrow C import does not read an Arrow IPC file or a Parquet file. The Arrow specification explicitly separates same-process interchange from storage/IPC. Jet must do the same.

The inspected `DataFlow` implementation recognizes Arrow/Parquet format names but returns bridge errors on the named paths. Card #2035 explicitly excluded Parquet. The expanded file-query design therefore needs a real reader, supported codecs, corruption limits, and a concrete dependency decision. It cannot close on a format enum or an adapter that always returns “unavailable.”

The intended user form is a typed source for the same query:

```jet
// Proposed file source; not a current Parquet implementation claim.
source :: data.scan<Sale>("sales.parquet") ?? panic("cannot open source")
totals :: data.query(source)
    .group_by(s -> s.region)
    .sum(s -> s.cents)
    .collect() ?? panic("query failed")
```

The reader's compressed bytes, decompressed bytes, row count, nesting, and retained-buffer ceilings are explicit. Predicate/projection pushdown must preserve F02's validation and error rules. The final decision slate separates the ABI-only import from the reader/dependency choice rather than hiding a new external dependency in an implementation card.

### Alternatives and the recommendation

| Option | Same foreign-table job | Gain | Real cost or loss |
|---|---|---|---|
| **A · Implement the stable Arrow ABI boundary and checked typed ownership** | Import a batch and use the ordinary query | Broad interchange with one lifetime model; no Arrow-library dependency required for the ABI itself | The boundary must validate layout and obey foreign release rules; not every semantic type maps without conversion |
| B · Write one adapter for each foreign runtime | Convert Python, R, and database values independently | Each adapter can use host-specific conveniences | Repeated lifetime, nullability, and conversion rules; more pairwise paths to verify |
| C · Always materialize owned rows | Copy into `[Row]` before use | Simple independent ownership | Avoidable memory and conversion cost for compatible immutable buffers |

**Recommend A**, with C as the honest safe-copy path when sharing is not justified. The native reader is a separately gated implementation over the same typed source/query interface, not a rival analytics engine.

## F11 · A Core API should be one complete declaration

### The maintainer's everyday job

Add an operation to a library. Its name, parameter types, result, effects, errors, and semantic implementation are one API. A maintainer should not have to remember which compiler tables, interpreter arms, JIT hosts, and web handlers need a matching entry.

The source currently exposes explicit Core type/member/signature routes. The existing D-TIER-ONEIR1 ruling already requires one Core declaration, generated symbol/marshalling rows, a finite MIR operation set, and total backend consumers. **The recommendation is to finish that ruling, not create a new registration scheme.**

### The desired authoring boundary

```text
One API declaration
  name + parameter contract + result + effects + applicability
  semantic body or vetted primitive boundary
                         |
              generated typed projections
             /           |             \
        checking       reflection    MIR call descriptor
                                        |
                            total marshalling adapters
```

An ordinary helper stays an ordinary Jet or Prelude helper. A true primitive has an explicitly declared primitive boundary. Moving all code into Jet for ideological purity is not the same task; self-hosting remains its separately evaluated choice. The adopted Prelude-home ruling and I9 remain in force.

### The deletion test

For a new Core operation, delete a generated row and the build must recreate it or fail its coverage check. A maintainer should not be able to add a public name while forgetting a backend's matching marshalling route. A backend cannot attach its own default or validation policy.

The ordinary `#MustUse` declaration should likewise own ignored-value obligations. The named Core exceptions in `core_must_use_type` are candidates for removal after equivalent consumer-visible behavior is established. Closed marker argument menus must follow the already-ratified ordinary-enum direction, not acquire another phantom type category.

This produces Go-like maintenance simplicity while preserving Jet's broader capabilities. It also reduces the proof surface: the proof concerns one semantic definition plus mechanical adapters, not several hand-written versions of a function.

## The lexical budget: deliberately boring

The preferred designs add library names and checked API contracts, not new punctuation. That is a decision, not an assertion that syntax can never improve.

| Surface | Preferred change | Lexical consequence |
|---|---|---|
| Endpoint declaration | Ordinary `web.get(...)` and checked binding data | New API names only; existing literal/path conventions |
| Query | `data.query(...).filter(...).collect()` | Ordinary calls and lambdas; no new SQL or comprehension syntax |
| Changing query | `data.track(...)` and `.watch()` | No reactive binding keyword; ordinary bindings remain ordinary |
| Geometry | Generic points/transforms and stock domain names | Ordinary types and constructors; no unit/space sigil |
| Arrow import | Owned typed boundary handle | Existing move/view rules; no raw-pointer beginner API |
| Core authoring | Complete declaration under the adopted generator contract | No second marker registry or public macro system |

New names must still be checked against the actual exported namespaces and I7 rules before implementation. A convenient name is not permission to shadow an existing method with a different meaning. The owner ballot fixes the public spelling; generated artifacts and the entire in-repo corpus then cut over together.
