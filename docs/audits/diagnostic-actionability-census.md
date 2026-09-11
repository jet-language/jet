# Diagnostic actionability census

This is a static, registry-driven census. It does not run the compiler or rewrite diagnostic text.

## Command

```text
node scripts/agent/diagnostic-actionability-census.mjs --write
```

Source of truth: `crates/jet-codegen/src/Prelude/Diagnostics.jet` (943 registered rows; 805 active).
Registry digest: `sha256:08a1c88d843de401feef3db20dba65654ad269318f6718d92c026871e4c3c107`

## Rule

A Fix is actionable when it names a concrete source edit, a runnable command, or a repository/system path. Concrete edits use an imperative operation with a named code fragment, declaration, value, or other object; structured source_edit, suggested_source_edit, replace, remove, and generated markers count as executable edits. A command must name a recognized executable and subcommand (or a REPL command). A path must name a literal path or filename. Placeholders, generic advice, inspection-only advice, and prose such as `follow the guidance`, `check the definition`, or `nothing to fix` are not actionable.

A tier/toolchain diagnostic is actionable only when its What or Why also names the command the user ran; a command that appears only in Fix is a remedy, not the invoking command.

The check treats retired and reserved rows as dormant: they remain in the exhaustive table, but only active rows can fail the actionability gate.

## Counts

| measure | count |
| --- | ---: |
| registered rows | 943 |
| active rows | 805 |
| dormant rows | 138 |
| actionable active Fixes | 805 |
| non-actionable active Fixes | 0 |
| tier/toolchain command gaps | 0 |
| active rows with violations | 0 |
| malformed source rows | 0 |

The known-failure baseline records 0 current violations as a regression floor, not acceptance. `--check` fails when the violation count grows or a new code/requirement appears; repair rows may lower the count without adding baseline entries.

The JSON and TSV files are the exhaustive machine/diff views. Every row below is emitted from the registry; no diagnostic code list is copied into this report by hand.

## Failures

Each failure includes the registered code, the exact Fix text, and the requirement it misses. Fix text is shown verbatim.

| code | line | Fix | missed requirement |
| --- | ---: | --- | --- |
| (none) |  |  |  |

## Exhaustive row table

| code | status | stage | fix kind | actionable | tier/toolchain | invoking command | violations | Fix |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `E0001` | active | jet | source_edit | true | false |  |  | Remove the stray character or complete the escape or brace. |
| `E0002` | active | jet | source_edit | true | false |  |  | Close the unterminated text literal, interpolation, or block comment. |
| `E0003` | active | parse | source_edit | true | false |  |  | Write the complete statement form named by the diagnostic |
| `E0004` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0004 |
| `E0005` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0005 |
| `E0006` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0006 |
| `E0007` | retired | jet | source_edit | true | false |  |  | Do not use E0007 |
| `E0008` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0008 |
| `E0009` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0009 |
| `E0010` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0010 |
| `E0011` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E0011 |
| `E0012` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0012 |
| `E0013` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0013 |
| `E0014` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0014 |
| `E0015` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0015 |
| `E0016` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0016 |
| `E0017` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0017 |
| `E0018` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0018 |
| `E0019` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0019 |
| `E0020` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0020 |
| `E0021` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0021 |
| `E0022` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0022 |
| `E0023` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0023 |
| `E0024` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0024 |
| `E0025` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0025 |
| `E0026` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0026 |
| `E0027` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0027 |
| `E0028` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0028 |
| `E0029` | active | parse | source_edit | true | false |  |  | Keep exactly one access marker on the parameter. |
| `E0030` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0030 |
| `E0031` | active | parse | source_edit | true | false |  |  | Use `extern rust` for foreign functions and place the call behind its required gate. |
| `E0032` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0032 |
| `E0033` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0033 |
| `E0034` | active | parse | source_edit | true | false |  |  | Write generic arguments as `Type<Args>`. |
| `E0035` | active | sema | source_edit | true | false |  |  | Use a literal, a same-file `@` binding, or another comptime expression that produces an in-range integer. |
| `E0036` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0036 |
| `E0037` | active | sema | source_edit | true | false |  |  | Replace `println!`/`eprintln!` with `print`/`io.eprint`. |
| `E0038` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E0038 |
| `E0039` | active | sema | source_edit | true | false |  |  | Use `env.get` instead of `os.environ` or `getenv`. |
| `E0040` | active | sema | source_edit | true | false |  |  | Use `core.tasks as tasks` and call `tasks.spawn(() -> work())`. |
| `E0041` | active | sema | source_edit | true | false |  |  | Create `channel<T>()` and send owned values, or write `shared value` and edit it through `guard_edit()` (a guard can `wait` on a `Condition`). |
| `E0042` | active | lex | source_edit | true | false |  |  | Move `#!...` to line 1, or delete it |
| `E0043` | active | jet | exact_command | true | false | jet fetch |  | Run `jet fetch` to install all dependencies listed in package.jet |
| `E0044` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0044 |
| `E0045` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0045 |
| `E0046` | active | parse | source_edit | true | false |  |  | Handle the optional value before calling its method; `?.` reaches fields, not methods. |
| `E0047` | active | type | source_edit | true | false |  |  | Make the left operand optional (`T?`) before using `?.`. |
| `E0048` | active | parse | source_edit | true | false |  |  | Use named tuple members instead of positional tuple access. |
| `E0049` | active | parse | source_edit | true | false |  |  | Use named tuple members instead of `.0` field access. |
| `E0050` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0050 |
| `E0051` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0051 |
| `E0052` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0052 |
| `E0053` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0053 |
| `E0054` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0054 |
| `E0055` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0055 |
| `E0056` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0056 |
| `E0057` | retired | parse | source_edit | true | false |  |  | Remove `take(...)` and use the captured names directly. |
| `E0058` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0058 |
| `E0059` | active | parse | source_edit | true | false |  |  | Write `#Scrub(Tag) fn` instead of a bare `sanitizer fn`. |
| `E0060` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0060 |
| `E0062` | retired | retired | non_actionable | false | false |  |  | Follow the guidance for E0062 |
| `E0063` | active | parse | source_edit | true | false |  |  | Use `#` for an applied rule and reserve `@` for compile-time or fact reads. |
| `E0064` | active | parse | source_edit | true | false |  |  | Give the foreign function one triple-quoted raw foreign-source string body. |
| `E0065` | retired | parse | exact_command | true | false |  |  | Replace the marker with `->`; `jet fmt` applies this fix. |
| `E0066` | retired | parse | source_edit | true | false |  |  | Replace it with `-[Effects]>`, or write `-[]>` for an empty effect ceiling. |
| `E0067` | reserved | lex | non_actionable | false | false |  |  | Follow the guidance for E0067 |
| `E0068` | retired | parse | source_edit | true | false |  |  | Write `fn name(...) Type -> body`, or `fn name(...) Type -[Effects]> { … }`. |
| `E0070` | retired | parse | source_edit | true | false |  |  | Replace the retired arrow with `->`. For an effect ceiling, write `-[Effects]>`. |
| `E0071` | retired | parse/sema | non_actionable | false | false |  |  | Follow the current one-line body rule |
| `E0072` | active | sema | source_edit | true | false |  |  | Remove `->`, or iterate a finite source. Return one final value from an ordinary loop with `break value`. |
| `E0073` | active | sema | source_edit | true | false |  |  | Return a value on this path, or use `next` to omit the item. Remove `->` if the loop only performs effects. |
| `E0074` | active | sema | source_edit | true | false |  |  | Convert the items to one type, or split the operations into separate loops. |
| `E0075` | active | sema | source_edit | true | false |  |  | Write `break` to return the accumulated list, or return one final value from an ordinary loop. |
| `E0076` | active | sema | source_edit | true | false |  |  | Add the missing payload and make every payload the same type, or target an inner effect-only loop. |
| `E0077` | retired | parse | source_edit | true | false |  |  | Write `#FX(grant: FS, Net) { ... }`. |
| `E0078` | active | sema | source_edit | true | false |  |  | Add `?? fallback` after the closing `}`, or write a labeled search such as `found :: loop { ... break(found, value) }`. |
| `E0079` | active | sema | source_edit | true | false |  |  | Bind the loop with `::`, or remove the payload. |
| `E0080` | active | parse | source_edit | true | false |  |  | Write `fn name(...) Type -> { ... }`; keep `fn name(...) { ... }` bare for a unit result. |
| `E0101` | active | sema | source_edit | true | false |  |  | Add `fn run() { ... }` to the entry file |
| `E0102` | active | sema | source_edit | true | false |  |  | Did you mean `print`? |
| `E0103` | active | sema | source_edit | true | false |  |  | Add an argument to `print`, for example `print("hello")`. |
| `E0104` | active | sema | source_edit | true | false |  |  | Change the `show` call to pass exactly one argument. |
| `E0105` | active | sema | source_edit | true | false |  |  | Rename or remove one of the definitions |
| `E0106` | active | sema | source_edit | true | false |  |  | Rename the declaration so it does not shadow a built-in. |
| `E0107` | active | sema | source_edit | true | false |  |  | Did you mean `score`? |
| `E0108` | active | sema | source_edit | true | false |  |  | Put the value in text with interpolation: "{x}" |
| `E0109` | active | sema | source_edit | true | false |  |  | Use a supported operand pair, call a named method, or implement the operator's hook trait on your own type for this pair |
| `E0110` | active | sema | source_edit | true | false |  |  | Compare the value to something, e.g. `x > 0` or `name == "ok"` |
| `E0111` | active | sema | source_edit | true | false |  |  | Declare it with `x := ...` instead |
| `E0112` | active | sema | source_edit | true | false |  |  | Use a value of the expected type, or convert it before passing or interpolating it |
| `E0113` | active | sema | source_edit | true | false |  |  | Return a value matching the enclosing function's declared result. |
| `E0114` | active | sema | source_edit | true | false |  |  | Put one unadorned expression last, or use `return ...` for an early exit. |
| `E0115` | active | sema | source_edit | true | false |  |  | Use `break` or `next` only inside a loop. |
| `E0116` | active | sema | source_edit | true | false |  |  | Use a value-producing call here, or discard the valueless call as a statement. |
| `E0118` | active | sema | source_edit | true | false |  |  | Rename the binding or remove the existing declaration. |
| `E0119` | active | sema | source_edit | true | false |  |  | Use one of the known effect names, or remove it from the list |
| `E0120` | active | sema | source_edit | true | false |  |  | Write `~name` for an owning copy, add `^` to the parameter when it should take ownership, or remove `copies: .Explicit` for the default materialization |
| `E0121` | active | sema | source_edit | true | false |  |  | Move `{name}` only to its consuming use `{consumer}`, or write `~{name}` before reusing it. |
| `E0123` | active | sema/runtime | source_edit | true | false |  |  | Use a positive `Int` literal or expression for the loop stride. |
| `E0124` | active | sema | source_edit | true | false |  |  | Make every `if` expression branch produce the same type. |
| `E0126` | active | sema | source_edit | true | false |  |  | Move the referenced parameter before the parameter that uses it as a default. |
| `E0127` | active | sema | source_edit | true | false |  |  | Use `#Numeric`, an explicit unit conversion, or one commensurable numeric type. |
| `E0128` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E0128 |
| `E0129` | active | sema | source_edit | true | false |  |  | Base the distinct type on a primitive or other non-distinct type. |
| `E0131` | active | sema | source_edit | true | false |  |  | `Float` and `Decimal` cannot be mixed; convert the operands to one numeric type. |
| `E0133` | active | sema | source_edit | true | false |  |  | Use a supported `Decimal` operator or convert before operating. |
| `E0134` | active | sema | source_edit | true | false |  |  | Declare the suffix in an in-scope `#UnitFamily` or remove it. |
| `E0135` | active | sema | source_edit | true | false |  |  | Use a comptime integer within the range type's declared bounds. |
| `E0136` | active | sema | source_edit | true | false |  |  | Use the fallible `?` form when constructing the range from a runtime value. |
| `E0137` | active | parse | source_edit | true | false |  |  | Declare range bounds with `lo <= hi`. |
| `E0138` | active | sema | source_edit | true | false |  |  | Grant the required operation bundle on the distinct type. |
| `E0139` | active | sema | source_edit | true | false |  |  | Keep `#Pre` and `#Post` conditions free of effects. |
| `E0140` | active | sema | source_edit | true | false |  |  | Consume the `#SingleUse` value before scope end. |
| `E0141` | active | sema | source_edit | true | false |  |  | Consume the `#SingleUse` value on every control-flow branch. |
| `E0142` | active | sema | source_edit | true | false |  |  | Move the `#SingleUse` value instead of lending or sharing it. |
| `E0143` | active | sema | source_edit | true | false |  |  | Put the deliberate `consume` inside a `#Unsafe("reason")` region or function. |
| `E0144` | active | sema | source_edit | true | false |  |  | Use `result` in a `#Post` condition, not in `#Pre`. |
| `E0145` | active | parse | source_edit | true | false |  |  | Apply `#Persist` only to a module-level binding. |
| `E0146` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0146 |
| `E0147` | active | parse | source_edit | true | false |  |  | Put literal text between adjacent `{}` holes in the string pattern. |
| `E0148` | active | sema | source_edit | true | false |  |  | Add an `else` arm to cover values that do not match the str pattern. |
| `E0149` | active | sema | source_edit | true | false |  |  | Use `Type.from(text)` to construct the checked text value before using it. |
| `E0150` | active | sema | source_edit | true | false |  |  | Transition the value into `{required}` first — call the transition that reaches it (e.g. `pay` to reach `Confirmed`). |
| `E0151` | active | sema | source_edit | true | false |  |  | Correct the spelling, or add `{state}` inside `struct {type} { state { … } }`. |
| `E0152` | active | sema | source_edit | true | false |  |  | Use a valid typed `Regex` literal without interpolation. |
| `E0153` | active | sema | exact_command | true | false |  |  | Correct the `protocol {name}` declaration, then run `jet check`; report the generated handle fragment if it still fails. |
| `E0154` | active | parse | source_edit | true | false |  |  | Write `client: Message(…)` when the client sends, or `server: Message(…)` when the server sends. |
| `E0155` | active | sema | source_edit | true | false |  |  | Write a valid typed literal with holes only in URL or Path components; write `DateTime{"…"}` without holes, or parse a runtime String explicitly |
| `E0156` | active | sema | source_edit | true | false |  |  | Write `{gate}(value)` at this boundary |
| `E0157` | active | parse | source_edit | true | false |  |  | Move the states into the owner: write `struct {type}` with `state {states}` inside its body |
| `E0158` | active | parse | source_edit | true | false |  |  | Move `state { … }` into the named struct body |
| `E0159` | active | sema | source_edit | true | false |  |  | Add one `state { Name, … }` section inside `struct {type}`, or remove the typestate marker |
| `E0160` | active | sema | source_edit | true | false |  |  | Use a `:=` binding and write `name += 1` / `name -= 1`. |
| `E0161` | active | sema | source_edit | true | false |  |  | Declare with `:=` or mark the parameter with the write-access marker `&` if the function should change it. |
| `E0162` | active | sema | source_edit | true | false |  |  | On `Float`, use `+= 1.0` / `-= 1.0`; otherwise use `+= 1` / `-= 1` on an integer binding. |
| `E0163` | active | sema | source_edit | true | false |  |  | Write a total update, such as `map[key] = (map.get(key) ?? 0) {op} 1` |
| `E0164` | active | sema | source_edit | true | false |  |  | Write a total update, such as `map[key] = (map.get(key) ?? 0) {op} 1` |
| `E0165` | active | parse | source_edit | true | false |  |  | Bind the value with `:=`, then write `name {op} value` |
| `E0166` | active | sema | source_edit | true | false |  |  | Keep one declaration of `{state}` in the struct's `state { … }` section |
| `E0167` | active | sema | source_edit | true | false |  |  | Rename the state or the member so `Type.State.{state}` is unique |
| `E0168` | active | parse | source_edit | true | false |  |  | Merge the state names into one `state { … }` section |
| `E0169` | active | sema | source_edit | true | false |  |  | Add at least one state name, or remove the section |
| `E0201` | active | sema | source_edit | true | false |  |  | Add the move marker `^` at the consuming call site. |
| `E0202` | active | sema | source_edit | true | false |  |  | Add the write-access marker `&` at the call site. |
| `E0203` | active | sema | source_edit | true | false |  |  | Use `^` only for a consuming parameter. |
| `E0204` | active | sema | source_edit | true | false |  |  | End the active `mut` borrow before using the value again. |
| `E0205` | active | sema | source_edit | true | false |  |  | Add `&` to the receiver before assigning to `self.field`. |
| `E0206` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E0206 |
| `E0207` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E0207 |
| `E0208` | active | sema | source_edit | true | false |  |  | Place the raw pointer operation inside an audited `#Unsafe` region. |
| `E0209` | active | sema | source_edit | true | false |  |  | Add `^` to the move argument or use a non-consuming constructor. |
| `E0210` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0210 |
| `E0211` | active | sema | source_edit | true | false |  |  | Use a copyable value with `~`, or move the value with `^` instead. |
| `E0212` | active | sema | source_edit | true | false |  |  | Finish using the view before changing the owner, narrow the view's scope, or make an owned copy. |
| `E0213` | active | sema | source_edit | true | false |  |  | Bind the call or temporary to a name first, then take the window from that name. |
| `E0214` | retired | sema | source_edit | true | false |  |  | Replace `value.view(a..b)` with `value[a..b]`. |
| `E0215` | active | sema | source_edit | true | false |  |  | Use a direct stored-field projection such as `guard.map(value -> value.field)`. |
| `E0216` | active | sema | source_edit | true | false |  |  | Project two stored sibling fields, such as `value.left` and `value.right`. |
| `E0217` | active | sema | source_edit | true | false |  |  | Keep the guard in a local name or a tuple, and use `.map(...)` or `.split(...)` for projections. |
| `E0218` | active | sema | source_edit | true | false |  |  | Write `mem.pin(&place)` with the write-access marker `&`. |
| `E0219` | active | sema | source_edit | true | false |  |  | Finish using the pin before changing the place, or narrow the pin's scope. |
| `E0220` | active | sema | source_edit | true | false |  |  | Read or edit through the live window name instead of the owner. |
| `E0221` | active | sema | source_edit | true | false |  |  | Use `Shared.Weak<T>` for intentional back-edges, or store an id instead of a strong handle. |
| `E0222` | active | sema | source_edit | true | false |  |  | Move or take `{name}` as a whole, or update it atomically. |
| `E0223` | active | sema | source_edit | true | false |  |  | Compute an owned replacement, then assign the persistent or static binding as a whole instead of passing `{name}` with `&` or using a mutating receiver. |
| `E0301` | active | sema | source_edit | true | false |  |  | Call a named method, or implement `Mul` on a struct or distinct type of your own (an operator hook on `Int` belongs to the package that declares the other operand type, D-OPMIX1). |
| `E0302` | active | sema | source_edit | true | false |  |  | Use the suggested field name or declare the intended field. |
| `E0303` | active | sema | source_edit | true | false |  |  | Correct the named construction fields to match the struct or variant. |
| `E0304` | active | sema | source_edit | true | false |  |  | Use the suggested variant spelling or a declared variant. |
| `E0305` | active | sema | source_edit | true | false |  |  | Make the pattern's shape and types match the value. |
| `E0306` | active | sema | source_edit | true | false |  |  | Make the pattern bind exactly the number of names the value provides. |
| `E0307` | active | sema | source_edit | true | false |  |  | Add the missing dispatch arms or provide an `else` arm. |
| `E0308` | active | sema | source_edit | true | false |  |  | Give `None` a known optional type such as `Int?`. |
| `E0309` | active | sema | source_edit | true | false |  |  | Use one optional layer or define a distinct wrapper instead of `T??`. |
| `E0310` | active | sema | source_edit | true | false |  |  | Handle or unwrap the optional value before using it as plain `T`. |
| `E0311` | active | sema | source_edit | true | false |  |  | Use the receiver form named by the diagnostic, or correct the method name |
| `E0312` | active | sema | source_edit | true | false |  |  | Compare values with a supported equality operation or compare their fields. |
| `E0313` | active | sema | source_edit | true | false |  |  | Make the destructuring target match the value’s shape. |
| `E0315` | active | sema | source_edit | true | false |  |  | Make the list pattern length match the known list length. |
| `E0316` | active | sema | source_edit | true | false |  |  | Use an integer field and a non-empty range with `lo <= hi`. |
| `E0317` | active | sema | source_edit | true | false |  |  | Make every or-pattern alternative bind the same names with the same types. |
| `E0318` | active | parse | source_edit | true | false |  |  | Write `lo..hi` — it already means "lo through hi inclusive." |
| `E0319` | active | parse | source_edit | true | false |  |  | Remove the stride; to match only multiples of N, use a full condition: `subject >= lo && subject <= hi && subject % n == 0 ->`. |
| `E0320` | active | parse | source_edit | true | false |  |  | Remove the dot before `{`, for example write `Point{ x: 1 }`, `[Int]{ 1, 2 }`, or `{ x: 1 }`. |
| `E0321` | active | parse | source_edit | true | false |  |  | Replace `impl Type: Trait` with `impl Type.Trait`. |
| `E0322` | active | parse | source_edit | true | false |  |  | Use `==` for comparison in the `if` condition. |
| `E0323` | active | parse | source_edit | true | false |  |  | Use `module name { }` instead of the unsupported `namespace` keyword. |
| `E0324` | active | sema | source_edit | true | false |  |  | Use `struct` for a distinct primitive name, or add type parameters to the alias. |
| `E0325` | active | parse | source_edit | true | false |  |  | Replace the external method `~~` connector with `.`. |
| `E0326` | active | sema | source_edit | true | false |  |  | Add a trailing `..` to a partial struct destructure, or name every field. |
| `E0327` | active | sema | source_edit | true | false |  |  | Remove the redundant `..` when every field is already named. |
| `E0328` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0328 |
| `E0329` | active | parse | source_edit | true | false |  |  | Give the arm table a named subject before nesting braceless arms. |
| `E0330` | active | sema | source_edit | true | false |  |  | Provide a type context for the leading-dot enum variant. |
| `E0331` | active | parse | source_edit | true | false |  |  | Remove the payload from the variant group name. |
| `E0332` | active | sema | source_edit | true | false |  |  | Use a concrete variant instead of the variant group name as a value. |
| `E0333` | active | parse | source_edit | true | false |  |  | Split the comparison or keep its direction consistent. |
| `E0334` | reserved | sema | non_actionable | false | false |  |  | Follow the guidance for E0334 |
| `E0335` | active | parse | source_edit | true | false |  |  | Write `callee(() -> { … })`. Put each statement on its own line inside the block. |
| `E0336` | active | sema | source_edit | true | false |  |  | Remove `#Patchable` from the generic struct or make the struct non-generic. |
| `E0337` | active | sema | source_edit | true | false |  |  | Remove the function-typed field or remove `#Patchable` from the struct. |
| `E0338` | active | sema | source_edit | true | false |  |  | Break the computed-field dependency cycle, including any self-reference. |
| `E0339` | active | sema | source_edit | true | false |  |  | Remove the computed field from the struct literal or direct assignment. |
| `E0340` | active | sema | source_edit | true | false |  |  | Use `Path.from(p).walk()` instead of `read_dir`. |
| `E0341` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E0341 |
| `E0342` | active | parse | source_edit | true | false |  |  | Move it inside a function, or remove it from the declaration. |
| `E0343` | active | parse | source_edit | true | false |  |  | Put the marker before the statement. |
| `E0344` | active | parse | source_edit | true | false |  |  | Keep one marker: `#Off <statement>` or `#DebugOnly <statement>`. |
| `E0345` | active | sema | source_edit | true | false |  |  | Remove the unknown `#Meta` field or replace it with a supported field. |
| `E0346` | active | sema | source_edit | true | false |  |  | Keep each `#Meta` field name unique. |
| `E0347` | active | sema | source_edit | true | false |  |  | Use a plain quoted string for the `#Meta` category. |
| `E0348` | active | sema | source_edit | true | false |  |  | Set the `#Meta` category to non-empty text. |
| `E0349` | active | parse | source_edit | true | false |  |  | Place `#Meta` on a binding or function declaration. |
| `E0350` | active | sema | source_edit | true | false |  |  | Replace `Any` with the specific mechanism for this value. |
| `E0351` | retired | sema | source_edit | true | false |  |  | Write `DataTree` instead of `Data`. |
| `E0352` | active | sema | source_edit | true | false |  |  | Write `maturity: .Experimental`, `.Tested`, or `.Hardened`. |
| `E0353` | active | sema | source_edit | true | false |  |  | Write each validation statement as `check(cond, at: field, "msg")`. |
| `E0354` | active | sema | source_edit | true | false |  |  | Set `at:` to a field declared by the struct. |
| `E0355` | active | parse/sema | source_edit | true | false |  |  | Use `#Policy(gc)`, `#Policy(explicit_units)`, `#Policy(copies: .Explicit)`, or `#Policy(sentries: .Off)`; write memory floors as `!Mem.*` effect denials. |
| `E0356` | active | sema | source_edit | true | false |  |  | Add a type annotation or write the full `Type.new(...)` form |
| `E0357` | active | sema | source_edit | true | true | jet check |  | Rename the identifier to the exact spelling shown by the diagnostic. Foreign names inside FFI binding modules are exempt. |
| `E0358` | retired | sema | source_edit | true | false |  |  | Write the capitalized spelling shown by the diagnostic. |
| `E0359` | active | sema | source_edit | true | false |  |  | Use matching dimensions, or use `*` or `/` to derive a new dimension |
| `E0360` | active | sema | source_edit | true | true | jet check |  | Implement the named `Type.Trait` hook, or call a named method instead. |
| `E0361` | active | sema | source_edit | true | false |  |  | Combine the value's fields directly, or call a different named helper inside the hook. |
| `E0362` | active | sema | source_edit | true | false |  |  | Bind the inner value, update it, then assign the whole inner value back. |
| `E0363` | active | sema | source_edit | true | false |  |  | Use a named enum when a member needs an open shape. |
| `E0364` | active | sema | source_edit | true | false |  |  | Write `loop (i, item) in xs` — or `loop i in xs.indexes()` — or `0..<xs.len()`. |
| `E0365` | active | sema | source_edit | true | false |  |  | Remove this arm or merge it with the one above. |
| `E0366` | active | parse | source_edit | true | false |  |  | Write `if subject == { … }` for pattern arms, or use a Bool head. |
| `E0367` | active | parse/sema | source_edit | true | false |  |  | Write `.{name}` or `.{name}(…)`. |
| `E0368` | active | parse | source_edit | true | false |  |  | Write one or more entries between `@[` and `]@`. |
| `E0369` | active | parse | source_edit | true | false |  |  | Remove the second name or give it a different name. |
| `E0370` | active | parse | source_edit | true | false |  |  | Give every fence the same number of entries. |
| `E0371` | active | parse | source_edit | true | false |  |  | Move the fence to a binding target or a complete expression statement, or fix the entry. |
| `E0372` | active | parse | exact_command | true | false |  |  | Write `-> statement` for one statement, or wrap the body in `{ ... }`; `jet fmt` applies this fix. |
| `E0373` | active | parse | exact_command | true | false |  |  | Replace the semicolon with a line break when code follows it, or remove it at the end of a line; `jet fmt` applies this fix. |
| `E0374` | retired | parse | source_edit | true | false |  |  | Remove the keyword for ordinary code, or replace it with `@` when failure to compute now must stop the build. |
| `E0375` | retired | sema | source_edit | true | false |  |  | Write `field: T{{expr}}` instead of `#Default(expr)`. |
| `E0376` | retired | parse | source_edit | true | false |  |  | Write `loop i in 0..<n { … }` or `loop i in 0..n, 2 { … }`; keep `loop name := value, condition { … }` for mutable state. |
| `E0377` | retired | parse | source_edit | true | false |  |  | Write `@name :: …` for a binding, `@if <condition> { … }` for a compile-time branch, and `@ { … }` for a compile-time block. |
| `E0378` | retired | parse | source_edit | true | false |  |  | Replace `=` with `::`. |
| `E0379` | active | parse | source_edit | true | false |  |  | Remove the comma before `if`. |
| `E0380` | active | sema | source_edit | true | false |  |  | Write `loop item in source -> ...`; give nested loops named bindings. |
| `E0381` | active | parse | source_edit | true | false |  |  | Write `marker Name(args..., @sites: [...], @repeatable: true, ...)`. |
| `E0382` | active | sema | source_edit | true | false |  |  | Write `#Memo` before `{field}: T -> expr` without arguments. |
| `E0383` | retired | parse | source_edit | true | false |  |  | Replace the comma after the binding with `in`, for example `loop item in items { … }`. |
| `E0384` | retired | parse | source_edit | true | false |  |  | Write `xs.contains(x)` (the `.contains(x)` method) instead of `x in xs`. |
| `E0385` | retired | parse | source_edit | true | false |  |  | Write `name: Type{{value}}` for a declaration default, or keep `=` for reassignment and extern slot fill. |
| `E0386` | active | parse | source_edit | true | false |  |  | Write `(atom \| atom) && guard` or `(atom \| atom) \|\| guard`. |
| `E0387` | active | sema | source_edit | true | false |  |  | Wrap the call in `#Unsafe("reason") { … }`, or use `Type.from(text)` for validation |
| `E0401` | active | sema | source_edit | true | false |  |  | Handle the fallible result or use it in a fallible context. |
| `E0402` | active | sema | source_edit | true | false |  |  | Handle, propagate, or explicitly discard the fallible result. |
| `E0403` | active | sema | source_edit | true | false |  |  | Handle the failure locally, or declare a compatible failure domain in the enclosing return type. |
| `E0404` | active | sema | source_edit | true | false |  |  | Use `ok` or `err` only inside a fallible function or expression. |
| `E0405` | active | sema | source_edit | true | false |  |  | Make the fallback type match the return type and fallible value. |
| `E0406` | active | parse | source_edit | true | false |  |  | Write `?T !E`, `T !(E1 \| E2)`, or omit the contract for implicit `Err` |
| `E0407` | active | sema | source_edit | true | false |  |  | Provide a valid reason string to `.drop()`. |
| `E0408` | active | sema | source_edit | true | false |  |  | Remove `err`, or use a fallible result when the fallback needs the failure |
| `E0409` | active | sema | source_edit | true | false |  |  | Remove `err`, or return an explicit value from the miss route |
| `E0410` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0410 |
| `E0411` | active | parse | source_edit | true | false |  |  | Use `pub(package)` or another supported visibility qualifier. |
| `E0412` | active | parse | source_edit | true | false |  |  | Use `priv` instead of `private` inside a `#PubFile`. |
| `E0413` | active | parse | source_edit | true | false |  |  | Use `priv` only in a `#PubFile`, or remove the visibility keyword. |
| `E0414` | active | parse | source_edit | true | false |  |  | Remove the redundant `pub` from the `#PubFile` item. |
| `E0415` | active | parse | source_edit | true | false |  |  | Remove the rejected section visibility label and mark each item instead. |
| `E0416` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0416 |
| `E0417` | active | parse | source_edit | true | false |  |  | Choose either `pub` or `priv`, not both, on the item. |
| `E0418` | active | parse | source_edit | true | false |  |  | Replace `#PublicFile` with `#PubFile`. |
| `E0419` | active | sema | source_edit | true | false |  |  | Use the `#MustUse` result, bind it, or explicitly discard it. |
| `E0420` | active | sema | source_edit | true | false |  |  | Write to `{name}` on every path before reading it (e.g. fill it via `mut {name}`). |
| `E0421` | active | parse | source_edit | true | false |  |  | Write `` `{name} := <Type>{ uninit }` ``, e.g. `` `buffer := [U8#4096]{ uninit }` ``. |
| `E0422` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0422 |
| `E0423` | active | sema | source_edit | true | false |  |  | Use plain data — a number, `Bool`, `Char`, `U8`, or a fixed array of those (e.g. `[4096]U8`). |
| `E0424` | active | sema | source_edit | true | true | jet check |  | Add `use core.mem` at the top of this file to opt in. |
| `E0425` | reserved | sema | non_actionable | false | true |  |  | Follow the guidance for E0425 |
| `E0426` | retired | parse | source_edit | true | false |  |  | Write `` `{name} := <Type>{ uninit }` ``. |
| `E0427` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0427 |
| `E0428` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0428 |
| `E0429` | active | sema | source_edit | true | false |  |  | Write a qualified Core call, or remove `#NoPrelude`. |
| `E0430` | active | parse | source_edit | true | false |  |  | Write `#Shield { … }`. |
| `E0431` | retired | parse | source_edit | true | false |  |  | Replace `Void` with `()`. |
| `E0432` | retired | parse | source_edit | true | false |  |  | Replace `Error` with `Err`. |
| `E0501` | active | sema | source_edit | true | false |  |  | Give the empty list a context type. |
| `E0502` | active | sema | source_edit | true | false |  |  | Use an eligible scalar, enum, tuple, or struct key |
| `E0503` | active | sema | source_edit | true | false |  |  | Use a supported string operation instead of indexing with `[ ]`. |
| `E0504` | active | sema | source_edit | true | false |  |  | Make the collection literal elements share one type. |
| `E0505` | active | sema | source_edit | true | false |  |  | Use the correct index or key type and slice a valid target. |
| `E0506` | active | sema | source_edit | true | false |  |  | Use a hashable element type for `Set<T>`. |
| `E0507` | active | sema | source_edit | true | false |  |  | Move the collection mutation outside the `for` loop, or iterate over a snapshot. |
| `E0510` | active | sema | source_edit | true | false |  |  | Import `core.crypto.expert` and use the required `#Unsafe` gate, or use `crypto.seal`/`open`. |
| `E0511` | active | sema | source_edit | true | false |  |  | Use fallible `Expiring.get(clock)` instead of `Expiring.force`. |
| `E0601` | active | sema | source_edit | true | false | jet test |  | Add `#Test("describes what this checks") { assert(condition) }`, or add a doctest. |
| `E0602` | active | jet | source_edit | true | false |  |  | Keep the `use` path inside the project entry tree. |
| `E0603` | active | jet | source_edit | true | false |  |  | Correct the `use` target path or module declaration. |
| `E0604` | active | jet | source_edit | true | false |  |  | Break the `use` cycle. |
| `E0605` | active | sema | source_edit | true | false |  |  | Make the item public in its defining file or stop importing it. |
| `E0606` | active | jet | source_edit | true | false |  |  | Rename one module or remove the duplicate module path so only one matching path remains. |
| `E0607` | active | jet | source_edit | true | false |  |  | Add the matching `module name;` file declaration. |
| `E0608` | active | sema | source_edit | true | false |  |  | Define the referenced function in the inline code module. |
| `E0609` | active | sema | source_edit | true | false |  |  | Make the imported item public or import an exported item. |
| `E0610` | active | sema | source_edit | true | false |  |  | Change `use alias.item` to alias a module before importing its item. |
| `E0611` | active | sema | source_edit | true | false |  |  | Change `use alias.item` to import an item the aliased module defines. |
| `E0612` | active | jet | source_edit | true | false |  |  | Replace `use math.*` with explicit imports such as `use math.sqrt`. |
| `E0613` | active | sema | source_edit | true | false |  |  | Use only a property-test parameter type the runner can generate. |
| `E0614` | active | sema | source_edit | true | false |  |  | Use one of the listed members, or remove the block. |
| `E0615` | active | sema | source_edit | true | false |  |  | Move it inside a `#Test("…") { … }` block, or write an ordinary statement. |
| `E0616` | active | sema | source_edit | true | false |  |  | Move `.setup { … }` to the top of the block. |
| `E0617` | active | sema | source_edit | true | false |  |  | Match the member's shape, e.g. `.timeout(500ms) { … }`, `.timeout(wait) { … }`, or `.skip("reason") { … }`. |
| `E0618` | active | sema | source_edit | true | false |  |  | Move the member out to the top level of the block. |
| `E0619` | active | jet | exact_path | true | false |  |  | Remove the import, or remove/update boundary rule `{rule}` in `package.jet` |
| `E0620` | active | sema | source_edit | true | false |  |  | Move the statements into the entry file's `fn run`, or import a declaration-only file. |
| `E0621` | active | sema | exact_command | true | false |  |  | Run `jet fix` to move the loose statements into `fn run`, or remove the explicit function. |
| `E0631` | active | sema | source_edit | true | false |  |  | Keep the view inside the arena's region, or copy what you need out with `~` before it leaves. |
| `E0632` | active | sema | source_edit | true | false |  |  | Use the view before `reset`, or re-`alloc` after to get a fresh value. |
| `E0701` | active | sema | source_edit | true | false |  |  | Add an `@version` pin to the non-`std` `extern rust` crate. |
| `E0702` | active | sema | source_edit | true | false |  |  | Use an FFI-safe scalar or capability type, or put the boundary in an audited `#Unsafe` region. |
| `E0703` | active | jet | source_edit | true | true | jet build |  | Install `cargo` or provide it on `PATH`. |
| `E0704` | active | jet | source_edit | true | true | jet build |  | Inspect the cargo error, then fix the foreign crate and rerun the build. |
| `E0705` | active | jet | source_edit | true | false |  |  | Make the Rust path and Jet signature agree. |
| `E0711` | active | sema | source_edit | true | false |  |  | Use the handle only inside the `#FX` block, or perform the work that needs it there. |
| `E0712` | active | sema | source_edit | true | false |  |  | Add the named effect to the `#FX(…)` list, or move that work outside the region. |
| `E0721` | active | sema | source_edit | true | false |  |  | Remove the destination use, change the declaration if its policy is wrong, or pass the value through `#Scrub({tag})`. |
| `E0722` | active | sema | source_edit | true | false |  |  | Log a non-secret field, or pass the value through a matching `#Scrub(Credential)` function. |
| `E0725` | active | sema | source_edit | true | false |  |  | Inject a deterministic clock/RNG or mockable input, pass recorded data in, or move the ambient effect outside the replayable function. |
| `E0731` | active | sema | source_edit | true | false |  |  | Declare `{tag}` as a `trait` with the method(s) it should provide. |
| `E0732` | active | sema | source_edit | true | false |  |  | Make `{tag}` a `trait` if `{method}` should dispatch, or remove the method to keep `{tag}` a marker tag. |
| `E0733` | active | sema | source_edit | true | false |  |  | Declare it with `tag {tag} { deny: [Effect] }`, check the spelling, or use the suggested tag. |
| `E0734` | active | parse | source_edit | true | false |  |  | Write `tag {tag} { deny: [Effect], from: [source.path] }`; omit `from` when it is not needed. |
| `E0735` | active | sema | source_edit | true | false |  |  | Correct the path using the suggested effect, sink, or declared function. |
| `E0736` | active | sema | source_edit | true | false |  |  | Accept a `#{tag} T` parameter and return the untagged result. |
| `E0740` | active | sema | source_edit | true | false |  |  | Add the named effect to the `-[…]>` list, or stop using it (drop the Core call that introduces it, or move it out of this function). |
| `E0741` | retired | sema | source_edit | true | false |  |  | Fix the E0712 diagnostic, then add the effect to the `#FX(…)` list or move the work outside the region. |
| `E0742` | active | sema | source_edit | true | false |  |  | Remove the offending work from the impl, or widen the bound on the trait method. |
| `E0743` | active | sema | source_edit | true | false |  |  | Declare an effect row on the trait method, such as `-[]>` for pure dispatch, or move the dynamic call outside the bounded function. |
| `E0745` | retired | retired | source_edit | true | false |  |  | Use one canonical effect arrow: `-[]>` for an empty row or `-[Effects]>` for a bounded row. |
| `E0746` | active | sema | source_edit | true | false |  |  | Move the call after the block, register it with `<handle>.on_commit(() -> { … })`, or declare `#Undo(inverse)` on the foreign binding. |
| `E0747` | active | sema | source_edit | true | false |  |  | Pass a callback within the bound (a `fn … -[]>` for a pure parameter), or widen the parameter's effect bound. |
| `E0748` | active | sema | source_edit | true | false |  |  | Point `via` at a function-typed parameter, or drop the `-[via …]>` annotation. |
| `E0749` | active | sema | source_edit | true | false |  |  | Remove the offending effect, return a fallible result, add facts or a `#Pre`/refinement proof, or drop the prohibition |
| `E0750` | active | sema | source_edit | true | false |  |  | Use the suggested declared leaf, add an `effect {effect}` declaration, or use the bare root. |
| `E0751` | active | sema | source_edit | true | false |  |  | Drop `Panic` from the list, or write `-[!Panic]>` |
| `E0760` | active | parser | source_edit | true | false |  |  | Use `:` rather than `=` for a `#Context` field. |
| `E0761` | active | parser | source_edit | true | false |  |  | Use only the supported `#Context` fields: `allocator`, `logger`, and `deadline`. |
| `E0762` | active | sema | source_edit | true | false |  |  | Give each `#Context` field its required type. |
| `E0763` | active | parser | source_edit | true | false |  |  | Use `/` and `*` once in the declared parameter zones and in the required order. |
| `E0764` | active | sema | source_edit | true | false |  |  | Pass only labels declared by the callee. |
| `E0765` | active | sema | source_edit | true | false |  |  | Pass each parameter label at most once. |
| `E0766` | active | sema | source_edit | true | false |  |  | Provide an argument or a default for every required parameter. |
| `E0767` | active | sema | source_edit | true | false |  |  | Pass positional-only parameters without labels. |
| `E0768` | active | sema | source_edit | true | false |  |  | Place positional arguments before the first labelled argument. |
| `E0769` | active | sema | source_edit | true | false |  |  | Pass the label-only parameter by name. |
| `E0770` | active | parser | source_edit | true | false |  |  | Give each parameter a unique public call label. |
| `E0771` | active | sema | source_edit | true | false |  |  | Match the function value's call labels and parameter zones to the expected function type. |
| `E0772` | active | sema | source_edit | true | false |  |  | Name the arguments to choose one candidate: {rewrite} |
| `E0801` | active | sema | source_edit | true | false |  |  | Add a type annotation to the lambda parameter, for example `value: Int`. |
| `E0803` | active | sema | source_edit | true | false |  |  | Call a function-valued expression, or stop calling this value. |
| `E0804` | active | sema | source_edit | true | false |  |  | Use a named function or a non-recursive lambda binding for recursion. |
| `E0805` | active | sema | source_edit | true | false |  |  | Declare the function as `Stream<T>` before using `yield`. |
| `E0806` | active | sema | source_edit | true | false |  |  | Remove the value from a generator `return`. |
| `E0807` | active | sema | source_edit | true | false |  |  | Change the `yield` expression to the stream's declared element type. |
| `E0850` | active | sema | source_edit | true | false |  |  | Make the module alias name a visible module in scope. |
| `E0851` | active | sema | source_edit | true | false |  |  | Pass the module alias the declared number of type and value arguments. |
| `E0852` | active | sema | source_edit | true | false |  |  | Pass a type argument that satisfies the module alias bound. |
| `E0853` | active | sema | source_edit | true | false |  |  | Pass a value argument with the expected type. |
| `E0855` | active | sema | source_edit | true | false |  |  | Break the circular module alias instantiation. |
| `E0856` | active | sema | source_edit | true | true | jet check |  | Use a Tier-0 type for the generic-module value parameter. |
| `E0857` | active | sema | source_edit | true | false |  |  | Pass a closed compile-time value as the generic-module argument. |
| `E0859` | active | compiler | exact_command | true | true | jet check |  | Record the two distinct generic-module keys and rerun `jet check`; report the keys if the collision repeats. |
| `E0901` | active | sema | source_edit | true | false |  |  | Add the required generic bound to the method. |
| `E0902` | active | sema | source_edit | true | false |  |  | Define the type or trait locally before declaring the `impl`. |
| `E0904` | active | sema | source_edit | true | false |  |  | Provide the type argument explicitly or add enough context to infer it. |
| `E0905` | active | sema | source_edit | true | false |  |  | Implement the required trait for the type or choose a type that has it. |
| `E0906` | active | sema | source_edit | true | false |  |  | Implement every method required by the trait. |
| `E0907` | active | sema | source_edit | true | false |  |  | Write `fn mul(self, rhs: Money) Money`, matching the `Mul` declaration exactly, including `self`, types, labels, return type, and effects. |
| `E0908` | active | sema | source_edit | true | false |  |  | Remove the duplicate trait implementation. |
| `E0909` | active | sema | source_edit | true | false |  |  | Simplify the generic nesting or split the instantiation into smaller bounds. |
| `E0910` | active | sema | source_edit | true | false |  |  | Restore a compatible published shape or declare a valid migration. |
| `E0911` | active | parse | source_edit | true | false |  |  | Use the supported migration verb, such as `remove`, and omit `reorder`. |
| `E0912` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E0912 |
| `E0913` | active | sema | source_edit | true | false |  |  | Add the missing associated type to the trait implementation. |
| `E0914` | active | parse | source_edit | true | false |  |  | Use `:Debug`, `:Pretty`, `:Fixed(n)`, `:Grouped(n)`, `:Hex(n)`, `:Pad(n[, "fill"])`, `:PadLeft(n[, "fill"])`, `:Sci(n)`, `:Percent(n)`, `:Bin`, `:Oct`, `:Unit(name)`, or `:Unit(bare)`; bare interpolation is also valid. |
| `E0915` | active | sema | source_edit | true | false |  |  | Implement `Display` for the type or use an explicit formatter. |
| `E0916` | active | sema | source_edit | true | false |  |  | Implement `Debug` for the field or provide a redacting formatter. |
| `E0917` | active | sema | source_edit | true | false |  |  | Remove recursion from the `#Inline(Always)` function or relax the marker. |
| `E0918` | active | sema | source_edit | true | false |  |  | Call the `#Inline(Always)` function directly or relax the marker before taking its address. |
| `E0919` | active | sema | source_edit | true | false |  |  | Reduce the `#Inline(Always)` body below the checked statement ceiling or relax the marker. |
| `E0920` | retired | retired | non_actionable | false | false |  |  | Follow the guidance for E0920 |
| `E0921` | active | sema | source_edit | true | false |  |  | Grant the required memory right or move the call outside the denied boundary. |
| `E0922` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E0922 |
| `E0925` | active | parse | source_edit | true | false |  |  | Place `#Job` on a function and attach `#Every(...)` only to that job. |
| `E0926` | active | sema | source_edit | true | false |  |  | Use a positive, in-range duration or canonical `HH:MM` schedule. |
| `E0927` | retired | parse/sema | exact_command | true | false |  |  | Write `#Test("name") { .measure { … } }` and run `jet test --measure` |
| `E0928` | reserved | sema | non_actionable | false | false |  |  | Follow the guidance for E0928 |
| `E0929` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0929 |
| `E0930` | active | parse | source_edit | true | false |  |  | Pass marker arguments matching the shared typed marker signature. |
| `E0931` | active | parse | source_edit | true | false |  |  | Use `!` only with a structurally auto-derived trait marker. |
| `E0932` | active | sema | source_edit | true | false |  |  | Write `pub` on the item, or remove `#Deprecated` |
| `E0938` | active | sema | source_edit | true | false |  |  | Make the `#Memo` function pure. |
| `E0939` | active | sema | source_edit | true | false |  |  | Use hashable `#Memo` arguments. |
| `E0940` | active | sema | source_edit | true | false |  |  | Materialize the value or remove `#Memo` from the lazy `Iter`. |
| `E0951` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E0951 |
| `E0952` | active | sema | source_edit | true | false |  |  | Reduce the comptime work or split it so it fits the fuel budget. |
| `E0953` | active | sema | source_edit | true | false |  |  | Remove the `@panic` or change the condition that raises the authored compile error. |
| `E0954` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0954 |
| `E0955` | active | sema | source_edit | true | false |  |  | Provide a readable UTF-8 file at the comptime path. |
| `E0956` | active | sema | exact_command | true | false |  |  | Use a simpler form, or use `jet build` for the full evaluator |
| `E0957` | active | sema | source_edit | true | false |  |  | Use a literal relative path or glob that stays inside the project. |
| `E0958` | retired | sema | non_actionable | false | true |  |  | Follow the guidance for E0958 |
| `E0959` | active | tooling | source_edit | true | true | jet check |  | Read `kind`, `target`, `guarantee`, and `source`, or add an owner-approved physical layout declaration such as `#Layout(c)`. |
| `E0960` | reserved | parse | non_actionable | false | false |  |  | Follow the guidance for E0960 |
| `E0961` | active | parse | source_edit | true | true | jet check |  | Write bare names like `default.[cargo, ripgrep]`. |
| `E0963` | active | sema | source_edit | true | false |  |  | Use a literal, a same-file `@` binding, or another comptime expression that produces an in-range integer; a destructuring pattern must then name exactly that many elements. |
| `E0964` | active | sema | source_edit | true | false |  |  | If you need a growable list, bind it with `:=` (e.g. `r := [...]`) so its length can change. |
| `E0965` | active | sema | source_edit | true | false |  |  | Use an index in the valid range, widen to `[T]` for runtime checking, or tighten the range. |
| `E0966` | active | jetpack | source_edit | true | false |  |  | Wrap the value in the matching type, e.g. `Env { … }`. |
| `E0967` | active | jetpack | source_edit | true | false |  |  | Make every declaration of the source agree, or rename one of them. |
| `E0968` | active | jetpack | exact_path | true | true | jet build |  | Write `default: owner/repo/rev@github`, `default: channel@nixpkgs`, or a bare local path. |
| `E0969` | active | jetpack | exact_path | true | false |  |  | Write `imports: find("./modules")`, or use a recognized typed integration call. |
| `E0970` | active | jetpack | source_edit | true | false |  |  | Create the directory, or fix the path so it points at your modules folder. |
| `E0971` | active | jetpack | exact_path | true | false |  |  | Remove the `imports:` from the discovered module; declare all `find(…)` directives in the top-level env.jet. |
| `E0972` | active | jetpack | source_edit | true | false |  |  | Remove the field, or use one of the known fields named in the error. |
| `E0973` | active | jetpack | source_edit | true | false |  |  | Write `target: linux.x64` or `target: linux.arm64`. |
| `E0974` | active | jetpack | source_edit | true | false |  |  | Add `target: linux.x64` (or `linux.arm64`). |
| `E0975` | active | jetpack | source_edit | true | false |  |  | Add `enable: true` (or `false`) to the service. |
| `E0976` | active | jetpack | source_edit | true | true | jet image |  | Use `kind: .Oci` for an active image, or keep disk-image notes in the jetos research appendix. |
| `E0977` | active | jetpack | source_edit | true | true | jet image |  | Add `from: packages.<name>` for an `.Oci` image, or remove fields inherited from frozen `system.*` research. |
| `E0978` | active | jetpack | source_edit | true | true | jet image |  | Use `from: packages.<name>` for an `.Oci` image, or keep the system image as research capture. |
| `E0979` | active | jetpack | exact_command | true | false | jet os |  | Write `jet os switch laptop` or `jet os switch laptop@../machines`. |
| `E0980` | active | jetpack | source_edit | true | false | jet os |  | Define `system.<host>: { … }`, or select one of the systems the config already defines. |
| `E0981` | active | jetpack | exact_command | true | false | jet os |  | Create it with `jet os init <host>`, or pass an external root as `host@root`. |
| `E0982` | active | jetpack | exact_path | true | false |  |  | Remove the `use`, and run the executable's binary instead; or, if you meant to import its code, change the package to `library` in `package.jet`. |
| `E0983` | active | jetpack | exact_command | true | true | jet build, jet run |  | Run `jetpack env --prep` to realize the library into the hangar, or rerun `jet run` without `--no-prepare` from the declaring project. |
| `E0984` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0984 |
| `E0985` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0985 |
| `E0986` | active | parse | source_edit | true | false |  |  | Move the marker or opening brace onto the same logical line as the closing `)`. |
| `E0987` | active | sema | source_edit | true | false |  |  | Correct the name, or add `name ::` before the intended enclosing loop. |
| `E0988` | retired | parse/sema | source_edit | true | false |  |  | Replace the dot or `@` form with the matching target-argument exit. Keep the declaration as `name :: loop`. |
| `E0989` | active | sema | source_edit | true | false |  |  | Make the `@if` condition a comptime expression. |
| `E0990` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0990 |
| `E0991` | active | parse | source_edit | true | false |  |  | Replace the old `copy` keyword with the `~` sigil. |
| `E0992` | active | parse | source_edit | true | false |  |  | Write `if subject == { value -> body }`. |
| `E0993` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0993 |
| `E0994` | active | parse | source_edit | true | false |  |  | Remove the redundant subject operation from the arm head. |
| `E0995` | active | compile | exact_path | true | false |  |  | Write `module workspace { members: find("./packages") }` (or an explicit list) in `workspace.jet`. |
| `E0996` | active | compile | exact_path | true | false |  |  | Use `find("./packages")` or a list literal like `["./packages/hello", "./packages/ranker"]`. |
| `E0997` | active | compile | source_edit | true | false |  |  | Create the directory or correct the path in `members: find("…")`. |
| `E0998` | retired | parse | non_actionable | false | false |  |  | Follow the guidance for E0998 |
| `E0999` | active | parse | source_edit | true | false |  |  | Combine stacked rules into one `#[A, B]` list or a lone `#A`. |
| `E1001` | active | jet | source_edit | true | false |  |  | Select a declared core module. |
| `E1002` | reserved | jet | non_actionable | false | false |  |  | Follow the guidance for E1002 |
| `E1003` | active | sema | source_edit | true | false |  |  | Use an integer literal that fits the declared width. |
| `E1004` | active | sema | source_edit | true | false |  |  | Use a declared item from the core module. |
| `E1005` | active | sema | source_edit | true | false |  |  | Apply the overflow opt-in to exactly one integer operation. |
| `E1006` | active | sema | source_edit | true | false |  |  | Replace the `use core.*` import or helper with one below the package `runtime:` ceiling. |
| `E1007` | active | parse | source_edit | true | false |  |  | Use a width `U<1..64>[be\|le]` or the `...` hole form. |
| `E1008` | active | parse | source_edit | true | false |  |  | Add `be` or `le` to multi-byte reads and remove it from single-byte or non-byte-aligned reads. |
| `E1009` | active | parse | source_edit | true | false |  |  | Move the binary rest capture to the final pattern position. |
| `E1010` | active | sema | source_edit | true | false |  |  | Match the binary pattern against a `[U8]` subject. |
| `E1011` | active | sema | source_edit | true | false |  |  | Align the fixed bytes and rest capture to byte boundaries. |
| `E1101` | active | sema | source_edit | true | false |  |  | Give it with `^`, freeze it with `freeze`, or share it with `shared` |
| `E1102` | active | sema | source_edit | true | false |  |  | Send plain owned data, make an ordinary owned copy when permitted, or use `Shared<T>` for deliberate shared state. |
| `E1103` | active | sema | source_edit | true | false |  |  | Fix the E1102 error at the spawn site first; once the task only holds owned data, `.detach()` is safe. |
| `E1104` | active | sema | source_edit | true | false |  |  | Use a fixed-size array `[T#N]` instead, or remove `#Layout(c)` if C interop is not required. |
| `E1105` | reserved | sema | source_edit | true | false |  |  | Use `#Layout(c)`, `#Layout(c, align(64))`, or `#Layout(columnar)`. |
| `E1106` | active | sema | source_edit | true | false |  |  | Pass an owned `copy`, or a `Shared<T>` handle, to the task instead of a `view`. |
| `E1107` | reserved | sema | source_edit | true | false |  |  | Put `#Layout(columnar)` on the `struct` declaration instead. |
| `E1108` | active | sema | source_edit | true | false |  |  | Drop `#Layout(columnar)` from the struct to use the full list API, or rewrite the operation with indexing and a loop. |
| `E1109` | active | sema | source_edit | true | false |  |  | Write `#Layout(columnar)` to convert the whole struct. |
| `E1110` | active | sema | source_edit | true | false |  |  | Write `task work()` inside the active group, or pass that handle directly to `fn helper(group: Group)`; do not store or capture it. |
| `E1111` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E1111 |
| `E1112` | active | sema | source_edit | true | false |  |  | Write {method} {{ work() }} with one or more branches |
| `E1113` | active | sema | source_edit | true | false |  |  | Create a new value, freeze a new snapshot, or use `Shared`/`Cell` when shared mutation is intentional |
| `E1114` | active | sema | source_edit | true | false |  |  | Use `^` to give the owned value to the task, use `Shared`/`Cell` for deliberate shared state, or rebuild a plain owned value |
| `E1115` | retired | parse | source_edit | true | false |  |  | Write `shared <value>` instead of `Shared.new(<value>)`. |
| `E1116` | retired | sema | source_edit | true | false |  |  | Read the field directly (`config.name`) and write it directly (`config.hits += 1`); group several steps under `#Transact {{ … }}`, or take an expert guard with `.guard_edit()`. |
| `E1117` | active | parse | source_edit | true | false |  |  | Use `task.all { first: work(), second: other() }`, or remove every branch label. |
| `E1118` | active | sema | source_edit | true | false |  |  | Replace `#Align(64)` with `#Layout(c, align(64))` on the struct declaration. |
| `E1119` | active | sema | source_edit | true | false |  |  | Write `#Layout(c, align({suggestion}))` with a positive power-of-two byte value no larger than 1048576 bytes. |
| `E1130` | retired | sema/parse | non_actionable | false | false |  |  | Follow the guidance for E1130 |
| `E1201` | active | jet | exact_command | true | false |  |  | Select one compatible package version in `package.jet`, then run `jet fetch`. |
| `E1202` | active | jet | exact_command | true | false |  |  | Run `jet fetch` to regenerate `.jet/lock` from `package.jet`. |
| `E1203` | active | jet | source_edit | true | false |  |  | Install `git` or provide it on `PATH`. |
| `E1204` | active | jet | source_edit | true | false |  |  | Restore the store entry from a trusted source and rerun verification. |
| `E1205` | active | jet | source_edit | true | false |  |  | Delete `{path}` and rerun the command to regenerate it. |
| `E1206` | active | jet | source_edit | true | false |  |  | Replace `{code}` with `{name}` in `policy.lints.deny`. |
| `E1207` | active | jet | source_edit | true | false |  |  | Resolve the registry dependency or choose a source artifact that verifies. |
| `E1208` | active | jet | exact_path | true | true | jet build |  | Set a compatible `jet:` toolchain pin in `package.jet`. |
| `E1209` | reserved | jet | non_actionable | false | false |  |  | Follow the guidance for E1209 |
| `E1210` | reserved | jet | non_actionable | false | false |  |  | Follow the guidance for E1210 |
| `E1211` | active | jet | source_edit | true | false |  |  | Replace the removed `kind:` field with `targets:`. |
| `E1212` | active | jet | source_edit | true | false |  |  | Add the declared `module <name>` to the package source tree. |
| `E1213` | active | jet | source_edit | true | false |  |  | Keep exactly one `module <name>` declaration in the package source tree. |
| `E1214` | active | jet | source_edit | true | true | jet build |  | Use a Jet toolchain that supports version `{version}`, or remove the lock only after confirming its dependencies and rerun an unlocked build to create version `{supported}`. |
| `E1216` | active | jet | source_edit | true | false |  |  | Remove the unknown `targets:` field or replace it with a supported field. |
| `E1217` | active | jet | exact_command | true | false | jet registry publish, jet registry |  | Run `jet fetch` to resolve and pin `{dep}`, then commit the lockfile. |
| `E1218` | active | jet | source_edit | true | false |  |  | Bump to `{next_major}.0.0` (a major release), or restore `{item}` (a deprecated shim counts). Use `--force` to publish anyway with an explicit warning banner. |
| `E1219` | active | jet | exact_path | true | false |  |  | Use `--release` for the release profile, `--profile=debug` for debug, `--profile=ci` for CI, or add `{name}: Build{ optimize: full }` (or `none`/`basic`) to the `build { }` block in `package.jet`. |
| `E1220` | active | jet | source_edit | true | false |  |  | For an ordinary effect, add `{effect}` to `authority.holds.allow` or grant it to `{dep}` in `authority.grants`; for deny-only `Panic`, return a fallible result or add facts/`#Pre`/refinement proof. |
| `E1221` | active | jet | exact_path | true | false |  |  | Fix the authority field or right name; see docs/spec/syntax-decisions.md D-AUTHORITY-MANIFEST1. |
| `E1225` | retired | jet | exact_path | true | true |  |  | Move `[repo]` `name`/`version` to `package.jet`, move `[sources]` entries to `env.jet` `sources: { … }`, then delete `jetpack.toml`. |
| `E1226` | retired | jet | exact_path | true | false |  |  | Rename `{name}` to `package.jet`. |
| `E1227` | active | jet | source_edit | true | true | jet os, jet image |  | Use matching `jet`/`{engine}` versions — reinstall the toolchain so both binaries come from the same release. |
| `E1228` | active | jet | source_edit | true | true | jet os, jet image |  | Install the matching Jet toolchain; the `{engine}` binary ships alongside `jet`. |
| `E1229` | retired | jet | source_edit | true | false |  |  | Write `module {ns}.{role} { … }` and move the contribution's fields up to the module body. |
| `E1230` | active | jet | exact_path | true | false |  |  | Address one member by its relative path (e.g. `infra/logging`), or use `package@source`. |
| `E1231` | active | jet | source_edit | true | false |  |  | Use one of the listed members (a did-you-mean is offered), fix the name, or add the package to `members:`. |
| `E1232` | active | jet | exact_command | true | false |  |  | Correct the source URL and revision in `package.jet`, then run `jet fetch` again. |
| `E1233` | active | jet | exact_path | true | false |  |  | Add the dependency to the source repo's `workspace.jet` `members:`, or depend on it as an external `package@source` ref. |
| `E1234` | active | jet | exact_command | true | false |  |  | Bump the version in `package.jet` and publish again, or `jet registry yank {version}` the existing one first if it was a mistake (yanking hides it from new resolution; it does not free the version number for reuse). |
| `E1235` | active | jet | source_edit | true | false |  |  | Remove credentials and URL parameters, configure the host Git credential provider for the registry path, then check network access or set `JET_REGISTRY_URL` to a reachable mirror. |
| `E1236` | active | jet | exact_command | true | false |  |  | Add the source hash, remove URL credentials, or vendor the source with `jet registry vendor`. |
| `E1237` | active | jet | source_edit | true | false |  |  | Install into a path under the output root (no `..`, no absolute paths). |
| `E1238` | active | jet | source_edit | true | false |  |  | Realize every named tool, declare every input and authority fact, fix the output/dependency graph, and retry the stage. |
| `E1239` | active | jet | exact_path | true | false |  |  | Keep one declaration (conventionally in `workspace.jet`) and delete the others. |
| `E1240` | active | jet | exact_command | true | true | jet build |  | Run `jet update jet` to realize the pinned toolchain, or install Nix so the bridge builds through the compatibility provider. |
| `E1241` | active | jet | exact_command | true | true | jet build |  | The build falls back to the compiler-embedded `core.{ring}`; to ship the staged artifact, realize a toolchain object built for this platform (`jet update jet`). |
| `E1242` | active | jet | source_edit | true | false |  |  | Define captured `system.{system}: { … }`, or point the host at an existing captured system. |
| `E1243` | active | jet | exact_command | true | true | jet push |  | Until the jetos realization tier lands, `jet push` captures and validates fleets without deploying them. |
| `E1244` | active | jet | source_edit | true | false |  |  | Remove `{field}`; captured fleets use `hosts: { … }`. |
| `E1245` | active | jet | source_edit | true | false |  |  | Add `hosts: { web1: system.<name> }` if this is research capture. |
| `E1246` | active | jet | exact_command | true | false |  |  | Do not use this version. Re-run `jet fetch` after clearing the store entry; if the problem persists, report it — this should never happen for an untampered registry. |
| `E1247` | active | jet | exact_command | true | false |  |  | Use a different registry, or ask the package author to publish a signed release (`jet registry publish` auto-signs by default — they likely used `--no-sign`). |
| `E1248` | active | jet | exact_command | true | false | jet registry keygen, jet registry |  | Use `jet registry keygen --force` if you're sure (e.g. the old key was compromised), or back it up first with `jet registry key backup`. |
| `E1249` | active | jet | source_edit | true | true | jet build |  | Write a channel: `jet: 0.4` (track the 0.4 series), `jet: 0.4.2` (exact), or a named channel like `jet: main`. |
| `E1250` | active | jet | exact_command | true | true | jet build |  | Run `jet update jet` to resolve `{channel}` to an exact version, then commit `.jet/lock`. |
| `E1251` | active | jet | exact_command | true | true | jet build |  | Move the pin with `jet update jet <channel>` to a toolchain your platform has, or install the pinned toolchain from the release page. |
| `E1252` | active | jet | exact_command | true | true | jet init |  | Edit the existing manifest, or run `jet init` in an empty directory. |
| `E1253` | active | jet | exact_command | true | false |  |  | Commit a copy at `.jet/inline-deps/{name}/<version>/`, or run `jet init` and depend on `{name}` through `package.jet` once you have a real source for it. |
| `E1254` | active | jet | source_edit | true | false | jet dev |  | Add `fn dev() { … }` (a custom dev command) or `fn run() { … }` (the default) to the entry file. |
| `E1255` | active | jet | exact_command | true | false | jet dev |  | Pass `--trust` for this one run, or pre-authorize with `jet config trust add <pattern>`. |
| `E1256` | active | jet | exact_command | true | false |  |  | Use the supported literal devShell fields, run `jet os bridge flake` for the loss report, or declare the environment in `env.*`. |
| `E1257` | active | jet | source_edit | true | false |  |  | Restore the removed/changed export, or accept this as an intentional breaking change (delete the stale snapshot to re-freeze). |
| `E1258` | active | jet | source_edit | true | false |  |  | Remove the effectful call, or move it out of the sandbox into the host program that loads it. |
| `E1259` | active | jet | exact_command | true | true | rustc --target wasm32-unknown-unknown |  | Make sure `rustc` supports `wasm32-unknown-unknown` and `wasm-tools` is on PATH (both ship in the project's `nix develop` shell). |
| `E1260` | active | jet | source_edit | true | false |  |  | Narrow the signature to one homogeneous scalar shape, or drop `pub` if this function isn't meant to be called across the sandbox boundary. |
| `E1261` | active | jet | exact_command | true | true | jet dev, jetpack services up, jetpack services |  | Check `jetpack services logs {name}` for what the process printed, confirm its `run`/`ready` declarations are correct, or raise the timeout isn't configurable yet — fix the service itself. |
| `E1262` | active | jet | source_edit | true | true | jetpack services |  | Rename `{field}` to one of the recognized keys (`enable`, `ports`, `run`, `shutdown`, `data_dir`, `ready`, `after`, `before_start`, `sockets`, `restart`, `watch`), or remove it. |
| `E1263` | active | jetpack | exact_command | true | true | jetpack secrets get {name}, jetpack secrets |  | Set it first with `jetpack secrets set {name} <value>`, or check the spelling. |
| `E1264` | active | sema | source_edit | true | false |  |  | Add `-[Secret]>` to `{fn}`'s signature, or add `Secret` to its existing effect ceiling. |
| `E1265` | active | comptime | source_edit | true | false |  |  | Move the secret read out of comptime or module-field evaluation and into ordinary runtime code. |
| `E1266` | active | jet | source_edit | true | true | jet image |  | Write `kind: .Oci` for active Jetpack images, or keep `.Iso` only as research capture. |
| `E1267` | active | jet | exact_path | true | false |  |  | Declare `{package}: executable` in `package.jet`, or point `from:` at an existing executable package. |
| `E1268` | active | jetpack | exact_path | true | false | jet image <name>, jet image |  | Use `--push file:///path/to/layout`, or configure a verified registry transport. |
| `E1269` | active | jet | source_edit | true | false |  |  | Rewrite the field to match its documented shape. |
| `E1270` | active | jetpack | exact_command | true | true | jetpack env |  | Change `Pkg.adapt(...)` to use source `"./vendor/tool"` and a supported recipe such as `Recipe.copy()`, then rerun `jetpack env`. |
| `E1271` | active | jetpack | exact_command | true | true | jetpack update |  | Run `jetpack update {name}` with network or fixture metadata, then commit `.jet/lock`. |
| `E1272` | active | jetpack | exact_command | true | true | jetpack add |  | Provide a pinned fixture or verified Hangar output, or replace the listed refs with native sources/adapters; `jetpack add <ref> --adapt` drafts an adapter snippet. |
| `E1273` | reserved | jetpack | source_edit | true | false |  |  | Run `jet logs <pkg>` for the full per-step log, or rerun the build with `--shell-on-fail` to debug inside the preserved scratch. |
| `E1274` | active | jetpack | exact_command | true | true | jet explain |  | Run `jet build <ref>` first; for diagnostic-code help, keep using `jet explain E1234`. |
| `E1275` | active | jetpack | source_edit | true | true | jet build |  | Provide a trusted substitute or approved remote builder, or enable the native sandbox, then retry. |
| `E1276` | active | jetpack | source_edit | true | false |  |  | Drop `--offline` for this command, or realize/fetch the needed object before going offline. |
| `E1277` | retired | jetpack | source_edit | true | false |  |  | Rename the option namespace, for example `net.hostName` becomes `network.hostName`. |
| `E1278` | active | jetpack | exact_command | true | false | jet os switch, jet os |  | Run `jet os switch` to regenerate the proof artifacts, or delete the hand-edited generation before retrying. |
| `E1279` | active | jetpack | exact_command | true | false |  |  | Realize or expose the required tools, then rerun `jet os vm prove <host> --disk <disk>`. |
| `E1280` | active | jetpack | source_edit | true | false |  |  | Declare a first-party source that provides `cachyos-kernel`, or select a different ratified kernel. |
| `E1281` | active | jetpack | source_edit | true | false |  |  | Declare a first-party source that provides `systemd`, or select a ratified init override. |
| `E1282` | active | jetpack | exact_path | true | false |  |  | Add `boot/vmlinuz-cachyos` and `boot/initrd-cachyos` with real boot payloads, or select a different ratified kernel. |
| `E1283` | active | jetpack | exact_path | true | false |  |  | Add `bin/systemd`, `lib/systemd/systemd`, or `sbin/init` to the package output, or select a ratified init override. |
| `E1284` | active | jetpack | exact_path | true | false |  |  | Add `source/recipe.jet`, `source/build.sh`, `source/config`, `source/patches.manifest`, and `source/initrd-inputs.manifest` to the package output. |
| `E1285` | active | jetpack | exact_command | true | false | jet os vm prove, jet os |  | Inspect the VM run logs, fix the boot/install path, then rerun `jet os vm prove` to capture a guest proof marker. |
| `E1286` | active | jetpack | exact_command | true | false |  |  | Check the first-party `cachyos-kernel` source recipe and rerun `jet os build`. |
| `E1287` | active | jetpack | exact_command | true | false | jet os vm run, jet os vm prove, jet os |  | Run `jet os vm prove <host> --disk <disk>` first, then rerun `jet os vm run`. |
| `E1288` | active | jetpack | source_edit | true | false |  |  | Declare first-party packages for `gdm`, `gnome-session`, and `gnome-shell`, or select a ratified non-GNOME desktop profile. |
| `E1289` | active | jetpack | exact_path | true | false |  |  | Pass a flake/root with `jetos-import-facts.json`, rerun with `--facts-only` for an audited scan draft, or choose a fresh `--out` path when writing. |
| `E1290` | active | jetpack | source_edit | true | false |  |  | Rerun without `--real` for plumbing tests, or put real QEMU/image/media tools on PATH before claiming replacement proof. |
| `E1291` | active | jetpack | source_edit | true | true | jet os |  | Rename or drop the unmapped keys/packages/services, or map them to the nearest supported real-tier option (see the option/service/package mapping table for `--real`). |
| `E1292` | active | jet | source_edit | true | false |  |  | Retry as a new operation on a supported host; no key files were created. |
| `E1293` | active | jet | source_edit | true | false |  |  | Use the underlying lint fix ({fix}), or remove `{name}` from `policy.lints.deny` if this team no longer wants the wall. |
| `E1294` | active | jet | source_edit | true | false | jet run <entry> -- <name>, jet run |  | Mark a function `#Job`, or check the spelling; the diagnostic lists declared jobs. |
| `E1295` | active | jetpack | source_edit | true | false |  |  | Pass a real branch, tag, or commit (a did-you-mean is offered when a close match exists). |
| `E1296` | active | jetpack | source_edit | true | false |  |  | Use `-p <member>` (repeatable) or `--affected` / `--affected-since <ref>`. |
| `E1297` | active | jetpack | exact_command | true | true | jetpack tool install, jetpack tool |  | Install under a different bin name with `jetpack tool install <ref> --as <other>`, or use `jetpack use <ref> -- <command>`. |
| `E1298` | active | jetpack | exact_path | true | true | jetpack tool |  | Use a built-in ref (`name@nixpkgs`, `owner/repo@github`) or a bare local path, or wait for the `{source}` provider to land. |
| `E1299` | reserved | jetpack | source_edit | true | false |  |  | Rename the entry to a portable store path with no reserved names, no trailing `.`/` `, and no case-fold collision with a sibling. |
| `E1300` | retired | jetpack | source_edit | true | false |  |  | Select the composition with `--preset <name>`, declared under `presets:`. |
| `E1301` | active | sema | source_edit | true | false |  |  | Pass exactly two strings: the flag name and a help description, e.g. `.flag("verbose", "enable verbose output")`. |
| `E1302` | active | sema | source_edit | true | false |  |  | Pass three strings: the option name, a help description, and a placeholder like `FILE`, e.g. `.option("output", "write to FILE", "FILE")`. |
| `E1303` | active | sema | source_edit | true | false |  |  | Pass exactly two strings: the positional name and a help description, e.g. `.positional("input", "file to process")`. |
| `E1304` | active | sema | source_edit | true | false |  |  | Pass exactly one argument: the argv list, e.g. `spec.parse(io.args())`. |
| `E1305` | active | sema | source_edit | true | false |  |  | Change the field to a supported type, or drop it from the `#CLI` struct. |
| `E1306` | active | sema | source_edit | true | false |  |  | Rename the colliding field or choose a non-reserved flag name. |
| `E1308` | active | sema | source_edit | true | false |  |  | Mark the program struct `#CLI`, or use a direct scalar-parameter `fn run` entry. |
| `E1309` | active | sema | source_edit | true | false |  |  | Remove `#Flag`, or make the field a required scalar without a `{expr}` default. |
| `E1310` | active | parse/sema | source_edit | true | false |  |  | Move `name: ...T` to the end of the parameter list and remove any `{…}` default. |
| `E1311` | active | sema | source_edit | true | false |  |  | Spread a list value, or build the list without spread. |
| `E1312` | active | sema | source_edit | true | false |  |  | Pass arguments individually, or call a function whose last parameter is variadic. |
| `E1313` | active | sema | source_edit | true | false |  |  | Implement `{Trait}` for the argument's type, or drop the value from this call. |
| `E1314` | active | sema | source_edit | true | false |  |  | Iterate it with `loop x in {name} { … }` — that's the only supported use in v1. |
| `E1315` | active | jetpack | source_edit | true | false |  |  | Re-run realization or ingest against a stable tree; if the error names executable lease state, repair the lease service state and retry without deleting the last good Hangar object; inspect the structured reproducibility report and fix the nondeterminism. |
| `E1316` | active | jetpack | source_edit | true | false |  |  | Add a `variant_map`, pin one candidate, or make the need more specific on the named axis. |
| `E1317` | active | jetpack | source_edit | true | true | jetpack env |  | Write `target@provider`, a bare local path, or rewrite `package@nixpkgs` exactly as `package@jetpack`. |
| `E1318` | active | sema | source_edit | true | false |  |  | Use one letter and give colliding fields different values. |
| `E1319` | active | sema | source_edit | true | false |  |  | Remove the marker, mark the command-input struct `#CLI`, or move `#Env` to a value field. |
| `E1320` | active | jetpack | source_edit | true | true | jetpack hangar |  | Re-read the hangar root and retry the mutation against its current etag. |
| `E1321` | active | sema | source_edit | true | false |  |  | Use a ratified kind and fields, point `entry:` at one visible safe function with the role's exact signature, or select one of the listed Executables explicitly. |
| `E1322` | active | jetpack | source_edit | true | false |  |  | Use a relative path below the workspace root and remove escaping symlinks. |
| `E1323` | active | jetpack | source_edit | true | false |  |  | Remove the nested `members:` field from the named member manifest and declare those paths at the workspace root. |
| `E1324` | active | jetpack | source_edit | true | false |  |  | Keep one member path for the directory. |
| `E1325` | active | jetpack | source_edit | true | false |  |  | Rename one Package or remove the duplicate member. |
| `E1326` | active | jetpack | source_edit | true | false |  |  | Use a project-relative destination and a valid `source`/`content`, mode, and permission record. |
| `E1327` | active | jetpack | source_edit | true | false |  |  | Declare the environment or change `from:` to an existing one. |
| `E1328` | active | sema | source_edit | true | false |  |  | Use the documented `exec`, `http`, `notify`, or `tcp` shape and a project-relative path. |
| `E1329` | active | jetpack | source_edit | true | false |  |  | Set `trusted: true` after review and approve the changed environment. |
| `E1330` | active | sema/jetpack | source_edit | true | false |  |  | Use the typed metadata shape and project-relative paths without `..`. |
| `E1331` | active | sema | source_edit | true | false |  |  | Use a relative import directory without `..` or an escaping symlink. |
| `E1332` | active | sema | source_edit | true | false |  |  | Merge equal facts or give them different names. |
| `E1333` | active | sema/jetpack | source_edit | true | false |  |  | Fix the language selection/catalog fact, use a project-relative file and `Dotenv{ file, allow, secrets }` with valid variable names, or create an in-project directory for `git_hooks_path`. |
| `E1334` | active | jetpack | exact_path | true | false |  |  | Create `package.jet`, correct the path, or use `find("./packages")`. |
| `E1335` | active | sema/jetpack | source_edit | true | true | jetpack env |  | Merge the declarations, use a supported package ref, or re-realize the package so Jetpack can hand only its verified lease to the child. |
| `E1336` | active | jetpack | exact_command | true | true | jet image |  | Run the declared service through `jetpack services`, realize the environment package for the image target, or choose a regular project-relative non-secret extra file and run `jet image` again. |
| `E1337` | active | compile | source_edit | true | false |  |  | Select one of the declared module names, or omit `--env` to use `dev`, then `default`, then lexical order. |
| `E1338` | active | jet | source_edit | true | true | jet run |  | Rebuild the library with the loading program's Jet version, or install a matching Jet toolchain. |
| `E1339` | active | jet | source_edit | true | false |  |  | Widen the grant at the load site to include `{effect}`, or remove the effect from the library. |
| `E1340` | active | jetpack | source_edit | true | true | jetpack env |  | Correct the named problem, then run the command again. |
| `E1341` | active | jet | source_edit | true | true | jet build |  | Select the Library output directly, use `c`, `python`, or `swift`, and give native bindings `native: true`; for a loadable artifact, rebuild it with the matching compiler and export surface. |
| `E1342` | retired | jetpack | source_edit | true | false |  |  | Select the module with `--env <name>`. |
| `E1343` | retired | jet | source_edit | true | false |  |  | Use `--gate impure=allow`. |
| `E1344` | active | sema | source_edit | true | false |  |  | Rename the command or root field so every command word is unique |
| `E1345` | active | sema | source_edit | true | false |  |  | Bind a visible scalar-parameter function, or use a method on the program struct |
| `E1346` | active | sema | source_edit | true | false |  |  | Split the command program struct from the Codable data struct |
| `E1347` | active | sema | source_edit | true | false |  |  | Change the receiver to read-only `self` |
| `E1348` | active | jetpack | source_edit | true | true | jetpack env |  | Refresh the signed index or use a covered locked nixpkgs input. |
| `E1349` | active | jetpack | source_edit | true | true | jetpack env |  | Use a covered attr or a supported native provider, or resolve the input through an explicit compatibility path. |
| `E1350` | active | jetpack | source_edit | true | false |  |  | Repair the cache metadata or network response, then retry the admission. |
| `E1351` | active | jetpack | source_edit | true | true | jetpack env |  | Use a Linux host with unprivileged user and mount namespaces, or choose a native provider for this package. |
| `E1352` | active | jetpack | source_edit | true | false |  |  | Declare `{name}` with `#latest` for manual movement or `#auto` for automatic movement. |
| `E1353` | retired | jetpack | non_actionable | false | false |  |  | {fix} |
| `E1354` | retired | jetpack | non_actionable | false | false |  |  | {fix} |
| `E1355` | active | jet | exact_command | true | true | jet run |  | For `jet run`, remove `--no-prepare` and retry `{command}`; for another verb, run `jetpack env -- {command}`. |
| `E1356` | active | jet | source_edit | true | true | jet run |  | Resolve the Jetpack installation or environment problem, then retry `{command}`. |
| `E1360` | active | jet | source_edit | true | false |  |  | Move the `package { … }` block before every other top-level declaration. |
| `E1361` | active | jet | source_edit | true | false |  |  | Keep one leading `package { … }` block and remove the duplicate. |
| `E1362` | active | jet | source_edit | true | false |  |  | Close the block and fix its structure, then let the Package field diagnostic identify any invalid field. |
| `E1363` | active | jet | exact_path | true | false |  |  | Remove the inline block or remove `package.jet`, then keep the remaining Package declaration canonical. |
| `E1402` | active | jet | source_edit | true | false |  |  | Fix the component, or unset `JET_COMPILER_EXTENSION` to skip the extension. |
| `E1403` | active | compiler | source_edit | true | true | jet check |  | Pull inner parts out into named bindings or helper functions. |
| `E1404` | active | sema | source_edit | true | false |  |  | write `devtools.publish(field: .<state_field>, value: <expr>)` |
| `E1410` | active | sema | source_edit | true | false |  |  | keep one marked public `panel` function for this package |
| `E1411` | active | sema | source_edit | true | false |  |  | write one valid `#DevPanel` function with the required signature and state |
| `E1412` | active | sema | source_edit | true | false |  |  | write `field: .{field}` using a field from the package's marked `panel` state |
| `E1413` | active | sema | source_edit | true | false |  |  | publish a value with type `{expected}` or change the state field |
| `E1414` | active | sema | source_edit | true | false |  |  | publish `field: .{field}` with the field's declared type |
| `E1801` | active | repl | exact_command | true | true | jet repl |  | Check any loops for a condition that never becomes false. Use `:run` to allow unbounded execution (compiles and runs instead of interpreting). |
| `E1802` | active | repl | exact_command | true | true | jet repl |  | Run `jet run <file.jet>` or `jet build <file.jet>` to use the full compiler. |
| `E1803` | active | repl | exact_path | true | false |  |  | Add `allow: [IO, Mem.Alloc, Exec]` under `authority.holds` in `package.jet` (use every value in this report's `undecided_effects` list); otherwise deny effects deliberately, or approve the exact operation once or for the project in an interactive terminal. |
| `E2001` | active | jet | exact_command | true | false |  |  | Upgrade with `jet self upgrade`, or set `edition: "2026"` in `package.jet`. |
| `E2002` | active | jet | exact_command | true | false |  |  | use the named replacement, or run `jet fix` to migrate automatically. |
| `E2101` | retired | jet | source_edit | true | false |  |  | Moved bare form: ``run `jet {group} {cmd} {args}` ``. Invalid nested form: ``run `jet {group} help` ``. Human output renders these as Error/Why/Fix lines; JSON uses these exact message, why, and fix strings with control characters, quotes, and backslashes escaped. |
| `E2102` | active | jet | exact_command | true | false |  |  | Did you mean `{closest}`? Run `jet help` to see the flags. |
| `E2103` | active | jet | exact_command | true | true | jet self completions, jet self |  | Run `jet self completions` again after rebuilding `{program}` with this Jet toolchain. |
| `E2104` | active | jet | source_edit | true | false |  |  | Correct the named argument or input, then run the command again. |
| `E2105` | active | jet | source_edit | true | false |  |  | Correct the named problem, then run the command again. |
| `E2106` | active | jet | exact_command | true | false |  |  | Did you mean `{closest}`? Run `jet explain {closest}`. |
| `E2110` | active | jet | exact_command | true | false |  |  | check the trace path and retry with a smaller workload; for reports, run `jet run --gc-trace <file.jet>` before `jet gc report`. Exits 1 (user error). |
| `E2111` | active | sema | source_edit | true | false |  |  | add `#Policy(gc)` to the receiving function or convert the graph to ordinary ownership before the boundary. |
| `E2112` | active | jet | exact_path | true | false | jet fix memory, jet fix |  | Run the relevant workload once to create the ledger, or repair the configured `.jet/memory/ledger-v1.jsonl` path, then retry. |
| `E2201` | active | interp | exact_command | true | true | jet dev |  | Run `jet build` then the binary, or `jet run <file>` to compile and run it; `jet dev` keeps showing checks live. Opt in with `jet dev <file> --try-anyway` to attempt execution past the boundary, with no guarantees (D-DEV1). |
| `E2202` | active | interp | exact_command | true | true | jet dev |  | check the loop near the pointed-at line for a condition that never ends; `jet run` executes the real build with no step limit. |
| `E2203` | active | interp | exact_command | true | true | jet debug |  | use the native `jet debug <file>` path with LLDB installed, or use `jet run <file>` when native debugging is unavailable. |
| `E2204` | active | interp | exact_command | true | true | jet debug |  | Run `jet debug <file>` again and use `continue` (or `c`) to run to the end, or `jet run <file>` to run it without the debugger. |
| `E2210` | active | interp | exact_command | true | true | jet dev |  | Nothing to fix — `jet dev` restarted with the new types; this note just explains why the swap became a restart. Type-stable edits (function bodies, statements) swap without a restart. |
| `E2211` | retired | jit | exact_command | true | true | jet run |  | use `jet run --trace-tiers` to see per-function tier, reason, and timing. |
| `E2301` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E2301 |
| `E2302` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E2302 |
| `E2303` | active | sema | source_edit | true | false |  |  | Send plain owned data, or rebuild the value as an owned copy (`~x`) before crossing. |
| `E2304` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E2304 |
| `E2305` | active | sema | source_edit | true | false |  |  | Keep every possible owner alive, preserve the declared view boundary, or remove `copies: .Explicit` / write `~view` when an owning destination is intended. |
| `E2306` | retired | sema | non_actionable | false | false |  |  | Follow the guidance for E2306 |
| `E2307` | active | sema | source_edit | true | false |  |  | Keep every possible owner alive, use a proven `View<str>` boundary, write `~view` at the highlighted owning expression (the checker grades that edit), or remove `copies: .Explicit` for the default owning copy. |
| `E2389` | active | compile | exact_command | true | false |  |  | Fix the selected project entry, then run jet check again. |
| `E2390` | active | compile | exact_command | true | false |  |  | Restore the package or workspace context and fix the graph, then run jet check again. |
| `E2391` | active | compile | exact_command | true | false |  |  | Repair the Core route or compiler defect named by the proof, then run jet check again. |
| `E2392` | active | compile | exact_command | true | true | jet check |  | Repair the source or compiler route named by the proof, then run jet check again. |
| `E2393` | active | compile | exact_command | true | false |  |  | Run jet check from the package or workspace, or replace the import with the canonical file-relative form when the dependency is local to this file. |
| `E2401` | active | compile | source_edit | true | false |  |  | Implement `impl FieldType.Trait` on the field's type, or choose a different field that does implement `Trait`. If the field doesn't exist, add `{field}: FieldType` to the struct. |
| `E2402` | active | compile | source_edit | true | false |  |  | Add `impl {err} -> Err { … }` before this function, or change the enclosing return type to `T !{err}`. |
| `E2403` | active | compile | source_edit | true | false |  |  | Introduce a local `name :: …;` before the struct literal, or write the long form `Type { field_name: value }`. |
| `E2404` | active | compile | source_edit | true | false |  |  | Declare the caller with `{Source}` as its failure domain, or add `impl {Source} -> {Target} { … }`; otherwise handle `{callee}` locally with `??` |
| `E2405` | active | compile | source_edit | true | false |  |  | remove one of the two `impl … -> …` blocks. |
| `E2406` | active | compile | source_edit | true | false |  |  | Define one of these types locally, or convert the foreign source into `Err`. |
| `E2407` | active | compile | source_edit | true | false |  |  | Pass one quoted string — `#Rename("wire_name")`. |
| `E2408` | active | compile | source_edit | true | false |  |  | Give `{field}` a `#Codable` struct type, or drop `#Flatten`. |
| `E2409` | active | compile | source_edit | true | false |  |  | Pick one of `camel` / `snake` / `pascal` / `kebab` / `screaming`. |
| `E2410` | active | compile | source_edit | true | false |  |  | Mark the field optional (`T?`), give it a declaration default (`field: T{{expr}}`), or fix the input so the key is present. Compose with `??` to supply a fallback. |
| `E2411` | active | compile | source_edit | true | false |  |  | add a compiler-derived codec, use a configured tagged enum or `#CodableAsBase` distinct type for a union member, or remove the unsupported type from the encoded value (for example, with `#Skip`). |
| `E2412` | active | compile | source_edit | true | false |  |  | remove `#DenyUnknownFields` to ignore extra keys (the lenient default), add the field, or fix the producer. |
| `E2413` | retired | compile | non_actionable | false | false |  |  | — |
| `E2414` | active | compile | source_edit | true | true | jet check |  | use a literal or a `comptime`-evaluable expression, e.g. `port: Int{8080}`, `env: String{"prod"}`, or `ports: [Int]{[80, 443]}`. |
| `E2415` | active | compile | source_edit | true | false |  |  | Use a named enum with an explicit tag, or change the members so each has a distinct wire shape. |
| `E2416` | active | runtime | source_edit | true | false |  |  | Fix the named environment variable or field path, or adjust `prefix:`, `file:`, and `allow:` so the intended source reaches the typed record. |
| `E2417` | active | compile | source_edit | true | false |  |  | use a Core error, add `#Error` to a local error type, or remove the explicit error contract. |
| `E2418` | active | compile | source_edit | true | false |  |  | remove this edge or choose a target that is not already reachable back to the source. |
| `E2419` | active | compile | source_edit | true | false |  |  | remove the direct edge or remove one of the intermediate conversion edges. |
| `E2420` | active | sema | source_edit | true | false |  |  | Write one of the fields named by the row type |
| `E2421` | active | sema | source_edit | true | false |  |  | Make `{member}` public, or keep `{function}` non-public. |
| `E2422` | active | sema | source_edit | true | false |  |  | Move `Never` to a function return type, or use `()`/another value type. |
| `E2423` | active | sema | source_edit | true | false |  |  | End every path with `panic`, a `Never` call, or another diverging operation, or remove `Never`. |
| `E2424` | active | sema | source_edit | true | false |  |  | Remove the bare `return`, or make this function return a value type. |
| `E2425` | active | sema | source_edit | true | false |  |  | Remove the value and diverge, or change the return type. |
| `E2426` | active | sema | source_edit | true | false |  |  | Pass a function declared `Never`, or change the expected function type. |
| `E2473` | active | runtime | source_edit | true | false |  |  | Use the existing footprint for `{key}`, or choose a new key for the new dependency declaration. |
| `E2474` | active | sema | source_edit | true | false |  |  | Change `{action}` to accept `{form}` as its first parameter. |
| `E2475` | active | sema | source_edit | true | false |  |  | Rename the literal column name to an existing field on `{row}`, or declare `{column}` on the row struct. |
| `E2501` | active | compile | source_edit | true | false |  |  | use the correct handle type for the operation: `files.open` to read, `files.create`/`files.append` to write. |
| `E2502` | active | compile | source_edit | true | false |  |  | Iterate it directly: `loop line in handle.lines() { … }`. |
| `E2510` | retired | compile | source_edit | true | false |  |  | Pass a listed dot value, or use `v.sum()` / `v.product()` / `v.min()` / `v.max()`. |
| `E2511` | active | compile | source_edit | true | false |  |  | Match the operand types (`T.splat(x)` lifts a scalar into every lane), call a named method like `.dot()`/`.matmul()`, or implement the operator's hook trait on your own type for this pair. |
| `E2512` | active | compile | source_edit | true | false |  |  | use `Matrix<{left_rows}, {left_inner}> * Matrix<{left_inner}, {right_cols}>` |
| `E2520` | active | sema | source_edit | true | false |  |  | Use a `{expected}` point or displacement, or convert it with a checked transform. |
| `E2521` | active | sema | source_edit | true | false |  |  | Reverse or replace one transform so both middle spaces are the same. |
| `E2522` | active | runtime | source_edit | true | false |  |  | Use a non-singular transform or handle the failed inverse result. |
| `E2523` | active | runtime | source_edit | true | false |  |  | Re-read the point or use the transform created for `{value_frame}`. |
| `E2524` | active | sema | source_edit | true | false |  |  | Pass an explicit depth/plane, or call `.ray()` and intersect it with a known plane. |
| `E2601` | active | compile | source_edit | true | false |  |  | Bump to `{next_major}.0.0`, or restore `{item}` (a deprecated forwarding shim counts). Use `--force` to publish anyway with an explicit warning banner. |
| `E2602` | active | compile | source_edit | true | false |  |  | Upgrade or downgrade one of the conflicting dependents so their `{package}` constraints overlap, or ask the authors to release a version that satisfies both. |
| `E2603` | active | compile | exact_command | true | false | jet inspect audit, jet inspect |  | Upgrade to `>= {fixed_version}` (or the version listed in the advisory). Run `jet inspect audit --explain {advisory_id}` for details. |
| `E2604` | active | compile | exact_command | true | true | jet fetch |  | Re-run `jet fetch` after cleaning stale Jetpack hangar entries (`jet clean`). If the problem persists, the upstream source may have been altered; audit the change before proceeding. |
| `E2605` | active | compile | exact_command | true | false |  |  | Commit or stash all uncommitted changes (`git status` to list them), then run `jet registry publish` again. Use `--force` to bypass with an explicit warning banner. |
| `E2606` | active | compile | exact_command | true | false | jet registry yank, jet registry |  | Run `jet registry yank <version>`, e.g. `jet registry yank 1.2.3`. |
| `E2607` | active | compile | exact_path | true | false |  |  | Fix the malformed record and retry; use the parser contract in `spec.md` and UTF-8 text. |
| `E2608` | active | jet | source_edit | true | false |  |  | Choose a different package name than `{reference}`, remove the reserved suffix if present, then publish again. |
| `E2609` | active | compile | source_edit | true | false |  |  | Wait for the maturity window, or add a reviewed exact `{package}#{version}` exception with a reason, reviewer, and expiry. |
| `E2610` | active | compile | source_edit | true | false |  |  | Refresh the signed offline feed and trust root, or repair the lock provenance before retrying. |
| `E2611` | active | compile | exact_command | true | false | jet inspect audit, jet inspect |  | Provide the lock file and advisory database required by `jet inspect audit`, then rerun `jet inspect audit`. |
| `E2701` | active | runtime | source_edit | true | false |  |  | Fix the input at the location named, or validate it before parsing. |
| `E2702` | active | sema | source_edit | true | false |  |  | Replace the offending cryptographic argument with the concrete value or bound named in the diagnostic: {fix} |
| `E2703` | active | runtime | source_edit | true | false |  |  | Close the quoted literal, complete or correct the `%` escape, choose a supported token, or format a `ZonedDateTime` when the token needs a zone. |
| `E2704` | active | runtime | source_edit | true | false |  |  | Use seconds or a coarser precision, or handle the failed conversion explicitly. |
| `E2711` | active | compile | source_edit | true | false |  |  | Define `derive T.{Trait}` or `{Type}` in the entry module. |
| `E2712` | active | compile | source_edit | true | false |  |  | Fix the body so it satisfies the type's declared text grammar. |
| `E2714` | retired | compile | source_edit | true | false |  |  | Write `derive T.{Trait} { … }`. |
| `E2801` | active | compile | source_edit | true | false |  |  | check that the address is reachable and the port is not already in use. For `bind`, try a different port. For `connect`, verify the server is running. |
| `E2802` | active | compile | source_edit | true | false |  |  | Verify the server's certificate, ensure the system trust store is up-to-date, or use `core.net.tls.insecure_skip_verify()` in a test environment only. |
| `E2803` | active | compile | source_edit | true | false |  |  | Raise the `max_body` option passed to `http.serve`, or reject the request in your handler before reading the body. |
| `E2804` | active | compile | source_edit | true | false |  |  | remove one of the duplicate registrations, or make the patterns distinct (e.g. add a static prefix to one). |
| `E2805` | retired | compile | source_edit | true | false |  |  | use `:name` for one segment or final `*name` for a catch-all. Percent-encode a literal leading `:` or `*`; never encode `/`. |
| `E2806` | active | compile | source_edit | true | false |  |  | add `fn page()`, rename the file with a leading `_`, or remove it from the routes directory. |
| `E2807` | active | compile | source_edit | true | false |  |  | remove one registration, or rename the convention file. |
| `E2810` | active | compile | source_edit | true | false |  |  | Pass a named function, or declare `.mount(prefix, handler)` for dynamic subtrees. |
| `E2901` | active | compile | exact_command | true | false |  |  | Run `jet test --update-snapshots` to update the golden output, or fix the code to match the claimed output. |
| `E2902` | active | compile | source_edit | true | false |  |  | Replace `#Todo` with a real implementation. |
| `E2903` | active | sema | source_edit | true | false |  |  | use the one legal typed budget form named by the diagnostic. |
| `E2904` | active | sema | source_edit | true | false |  |  | remove one declaration or make their applicability disjoint. |
| `E2905` | active | sema | source_edit | true | false |  |  | Name one qualified scope, target, profile, or provider identity. |
| `E2906` | active | jet | source_edit | true | false |  |  | Correct the provider evidence, or bootstrap only when absent or stale evidence is eligible. |
| `E2907` | active | jet | source_edit | true | false |  |  | Improve the measured behavior, inspect the named evidence, or record an explicit exception. |
| `E2908` | active | jet | source_edit | true | false |  |  | Correct the named refusal and retry; there is no force bypass. |
| `E2910` | active | sema | source_edit | true | false |  |  | Write `reactive.derived(() -> … )` or `reactive.effect(() -> { … })`. |
| `E2911` | active | sema | source_edit | true | false |  |  | Drop the parameters: `reactive.{kind}(() -> { … })`. |
| `E2912` | active | sema | source_edit | true | false |  |  | return a value from the body, or use `reactive.effect(() -> { … })` for a side effect. |
| `E2913` | active | sema | source_edit | true | false |  |  | use a data value (number, text, list, struct, …); put behaviour in `reactive.effect`. |
| `E2914` | active | sema | source_edit | true | false |  |  | Remove the value return from the `#Reactive fn`. |
| `E2930` | active | sema | source_edit | true | false |  |  | Pass a real label, e.g. `ui.node_role("Submit", w, h, ui.aria_role_button())`. |
| `E2931` | active | sema | source_edit | true | false |  |  | Give each interactive node a distinct, descriptive label. |
| `E2932` | active | sema | source_edit | true | false |  |  | Compare or combine values from the same axis (a `LengthVar`, or a plain number, fits either axis). |
| `E2933` | active | sema | source_edit | true | false |  |  | Write a comparison, e.g. `label.width >= 80.0`. |
| `E2934` | active | sema | source_edit | true | false |  |  | remove the duplicate line, or change it if a different constraint was meant. |
| `E2935` | retired | parse | source_edit | true | false |  |  | Write `` `{name} :: Layout{{ … }}` ``. |
| `E2936` | retired | sema | source_edit | true | false |  |  | Write `Layout` instead of `LayoutHandle`. |
| `E2937` | active | sema | source_edit | true | true | jet check |  | Add `{capability}` to `authority.needs` in the package manifest, then rebuild. |
| `E2938` | active | sema | source_edit | true | true | jet check |  | Write a non-empty plain string literal, for example `.cmd("s")`; use `core.ui.host.shortcut` for a dynamic key. |
| `E2940` | active | compile | exact_command | true | false |  |  | Perform the producer-specific action named by `jet prove`, then run the same command again. |
| `E2941` | active | compile | exact_command | true | false | jet prove |  | use one exact value, for example `jet prove TARGET --lens tests`. |
| `E2950` | active | compile | source_edit | true | false |  |  | change the function or claim so every admitted input satisfies the postcondition. |
| `E2960` | active | compile | exact_path | true | false |  |  | Change the source or update `{rule}` in `package.jet`. |
| `E2967` | active | sema | source_edit | true | false |  |  | Preserve source order, declare the complete resource access, or provide the real completion event before requesting parallel execution. |
| `E3001` | active | runtime | source_edit | true | false |  |  | Fix the logic that led to the failure. Program-side stops exit 70 through the shared boundary; unhandled entry errors print their report and exit 1; exit 101 is reserved for Jet defects. |
| `E3002` | active | runtime | source_edit | true | false |  |  | Read the root failure above the trail first — it says what went wrong. Then read the trail from hop 1, the origin, down to the last hop at the entry. |
| `E3003` | active | runtime | source_edit | true | false |  |  | Raise the deadline budget, shorten the work before the wait point, or remove/adjust the ambient deadline for this scope. |
| `E3004` | active | runtime | source_edit | true | false |  |  | Handle `TaskFailure.Cancelled`, or use `#Shield` around a cancellation-sensitive wait. |
| `E3005` | active | runtime | source_edit | true | false |  |  | Fix the caller (a failed `#Pre` means an argument violated the function's stated contract) or the function body (a failed `#Post` means it broke its own promise about the result). |
| `E3010` | active | runtime | source_edit | true | false |  |  | Check the operands or bounds before the operation, or use a checked operation that returns an outcome. |
| `E3011` | active | runtime | source_edit | true | false |  |  | Implement the missing code before running the program. |
| `E3012` | active | runtime | source_edit | true | false |  |  | End the recursion or make progress toward a base case. |
| `E3013` | active | runtime | source_edit | true | false |  |  | Join each task or make its wait reachable before process exit. |
| `E3014` | active | runtime | source_edit | true | false |  |  | Fix the foreign function or handle its documented failure before calling it again. |
| `E3101` | active | sema | source_edit | true | false |  |  | wrap it: `#Unsafe("why this is safe") { … }`. |
| `E3102` | active | sema | source_edit | true | true | jet check |  | add `use core.mem;` at the top of the file. |
| `E3103` | active | sema | source_edit | true | false |  |  | Call it inside `#Unsafe("…") { … }`. |
| `E3104` | retired | sema | non_actionable | false | false |  |  | Follow E0121 and acquire a new allocator after close. |
| `E3105` | active | sema | source_edit | true | false |  |  | remove the operation or have the policy owner change the outer policy. |
| `E3106` | active | sema | source_edit | true | false |  |  | add `obligations: .Track`, or use `.Skip` only under a package `.PerSite` policy. |
| `E3107` | active | sema | source_edit | true | false |  |  | add `assert valid_ptr, aligned, no_alias`, reduced to the operation-specific required subset. |
| `E3108` | active | parse/sema | source_edit | true | false |  |  | use only `obligations: .Track`/`.Skip` and `valid_ptr`, `aligned`, `no_alias` inside `#Unsafe`. |
| `E3109` | active | load | source_edit | true | false |  |  | Fix `JET_ORG_UNSAFE_POLICY` and its manifest-shaped `policy: { unsafe: .Obligations, impure: .GateOnly, nondeterministic: .GateOnly }` file, or remove the variable. |
| `E3110` | active | sema | source_edit | true | false |  |  | use only the lanes defined for `{type}`. |
| `E3111` | active | sema | source_edit | true | false |  |  | Assign each lane once, e.g. `v.xy = …` instead of `v.xx = …`. |
| `E3112` | active | parse/sema | source_edit | true | false |  |  | add the reason: `#Unsafe("why this is safe") { … }` or `#Unsafe("why this is safe") fn ...`. |
| `E3201` | active | jet | exact_command | true | false |  |  | Install the system package (e.g. `pacman -S {lib}`), or declare it as `{lib}: c@system` in `deps:`. |
| `E3202` | active | sema | source_edit | true | false |  |  | Move the call inside `#Unsafe`, or change the type to a C-safe value type. |
| `E3203` | active | sema | source_edit | true | true | jet build |  | use scalars, `String`, or a struct with C layout; pointers only through the gated tier. |
| `E3204` | active | sema | source_edit | true | false |  |  | remove one line; keep the form that matches your workflow. |
| `E3205` | active | sema | source_edit | true | false |  |  | Match the generated signature, or rename your overlay function. |
| `E3206` | reserved | parse | source_edit | true | false |  |  | Drop `__bindgen__` from your module path, or use `#Import module c.{lib} { … }`. |
| `E3207` | active | parse | exact_command | true | false | jet inspect bind, jet inspect |  | Edit your overlay file with `#Import module`, or regenerate the cache with `jet inspect bind`. |
| `E3208` | active | jet | exact_command | true | false |  |  | Fix the header path, install dev headers, run `jet inspect bind` manually for details, or hand-write `#Import module c.{lib}`. |
| `E3209` | active | jet | source_edit | true | true | jet build |  | Declare it in `deps:` so Jet provisions it: `{lib}: c@system` (host pkg-config, else fetched from nixpkgs), or `{lib}: c@nixpkgs:<attr>` to pick the nixpkgs attribute, or install the system package. |
| `E3210` | active | jet | exact_command | true | true | nix build |  | check the attr exists (`nix build nixpkgs#{attr}`), or point at a local build with `{lib}: c@"<path>"`, or install it and use `system`. |
| `E3211` | active | sema | source_edit | true | false |  |  | remove the embedded NUL, or split the call so the C function only sees the part before it. |
| `E3212` | active | parse/sema | source_edit | true | false |  |  | use `system`, `cdecl`, `stdcall`, `fastcall`, `win64`, or `sysv64`, or remove `#ABI` from the `extern rust` function. |
| `E3213` | active | sema | source_edit | true | false |  |  | use the default C ABI or `system` for portable declarations. |
| `E3214` | active | sema | source_edit | true | false |  |  | remove `#ABI`, or use `#ABI(cdecl)` on Windows x86. |
| `E3215` | active | sema | source_edit | true | false |  |  | Wrap inline foreign code in an enclosing `#Unsafe("reason")` gate. |
| `E3220` | active | sema | source_edit | true | false |  |  | Use an inline foreign binder for `c`, `cpp`, or `asm`, or remove the unsupported language. |
| `E3222` | active | sema/build | source_edit | true | false |  |  | Make the inline body and signature use a supported scalar ABI. |
| `E3223` | active | sema | source_edit | true | false |  |  | Make the asm operands, return marker, clobbers, and target registers match the Jet signature. |
| `E3260` | active | loader/jet | source_edit | true | false |  |  | Generate, build, and run the COM module on a Windows host; use a non-COM boundary for other targets. |
| `E3261` | active | parse | source_edit | true | false |  |  | Write `#Import(c) fn name(args) Return = "symbol"`. |
| `E3301` | active | sema | source_edit | true | false |  |  | Embed the data at compile time with `@embed("file")`, or select a hosted target. |
| `E3302` | active | jet | exact_command | true | true | jet build, jet run |  | Run `jet self doctor --target=<triple>` to see what's missing, or `rustup target add <triple>` to install it; use `wasm32-wasip2` for a WASI Preview 2 server. |
| `E3303` | active | sema | source_edit | true | false |  |  | Select a typed allocator provider, or use heap-free Core operations. |
| `E3304` | active | sema | source_edit | true | true | jet build, jet run |  | Build for `wasm32-wasip2` or a supported native target, or remove the `{module}` import. |
| `E3305` | active | sema | source_edit | true | true | jet build, jet run |  | use a supported TCP/UDP operation, or build for a native target. |
| `E3306` | active | sema | source_edit | true | true | jet build, jet run |  | Build for a native target or `wasm32-wasip2`, or move plugin work behind a native/server boundary. |
| `E3310` | active | sema | source_edit | true | false |  |  | Select a typed target with `{required}` support, or remove `{api}` from the reachable closure. |
| `E3311` | active | sema | source_edit | true | false |  |  | Select a typed target profile with a `{capability}` provider, or remove the reachable operation. |
| `E3312` | active | sema | source_edit | true | false |  |  | Declare a nonempty provider identity and a `sha256:` digest for `{capability}`. |
| `E3313` | active | sema | source_edit | true | false |  |  | Declare a matching MMIO provider and region, then place the access inside `#Unsafe("reason")`. |
| `E3314` | active | sema | source_edit | true | false |  |  | Select a complete typed target profile and correct the `{fact}` declaration. |
| `E3401` | active | sema | source_edit | true | false | jet eval --pure, jet eval |  | Mark `{call}` as `fn … -[]>`, or remove the call from `{pure_fn}`; at compile time, compute the value at runtime instead. |
| `E3402` | active | sema | source_edit | true | false |  |  | Compute this value at compile time or pass it in as a parameter. |
| `E3403` | active | sema | source_edit | true | false |  |  | remove this call, or remove the enclosing function's explicit empty effect bound. |
| `E3410` | active | sema | source_edit | true | true | jet build, jet run |  | wrap the call in `#Impure("reading config") { … }`. |
| `E3411` | active | sema | exact_command | true | false |  |  | add `--gate impure=allow` to your `jet build` / `jet run` invocation. |
| `E3412` | active | sema | source_edit | true | true | jet build, jet run |  | use `core.net.fetch(url, sha256: "…")` for content-hash-pinned downloads. |
| `E3413` | active | sema | source_edit | true | false |  |  | Update the `sha256:` argument to match the actual content hash shown in the Why line, or verify the URL points to the correct file. |
| `E3414` | active | sema | exact_path | true | false |  |  | check the URL and arguments; use `file://` for local test paths. |
| `E3415` | active | sema | source_edit | true | false |  |  | remove the gate or change the owning policy to allow it. |
| `E3501` | active | build | source_edit | true | false | jet build |  | Write `fn build(b: BuildContext) BuildPlan`. |
| `E3502` | active | build | exact_command | true | false |  |  | Fix the named graph node or generated module; inspect it with `jet inspect graph` and `jet inspect explain-build`. |
| `E3503` | active | build | source_edit | true | false |  |  | Declare the effect, gate the ambient operation with `#Impure("reason")`, and grant the effect through CLI/package/workspace policy. |
| `E3504` | active | build | source_edit | true | false |  |  | Pass the named `--allow-<effect>` flag for a one-file build, or grant the effect in package/workspace policy. |
| `E3505` | active | build | source_edit | true | true | jet build |  | Fix the named command, probe, toolchain, input/output declaration, or enable a supported sandbox. |
| `E3510` | active | build | source_edit | true | false |  |  | rename the generated module, or delete the hand-written one. |
| `E3511` | active | build | source_edit | true | false |  |  | break the dependency between the named generators or paths. |
| `E3512` | active | build | source_edit | true | false |  |  | Rerun without `--locked` to review and record the new generated provenance. |
| `E3520` | active | build | source_edit | true | false |  |  | Keep one `fn build` and remove every other entry. |
| `E3521` | active | build | source_edit | true | false |  |  | Make the named writers agree, or move one value to a more explicit contribution layer. |
| `E3530` | reserved | build | source_edit | true | false |  |  | use a project prefix such as `ORG01`. |
| `E3540` | active | jet | source_edit | true | false |  |  | Keep one fn {command} in the package scope, or remove the other command override. |
| `E3620` | active | compile | source_edit | true | true | jet prove |  | Recapture with a compatible toolchain, or upgrade Jet. |
| `E3621` | active | compile | source_edit | true | true | jet prove |  | Recapture against this exact revision. |
| `E3622` | active | compile | source_edit | true | false |  |  | Pass an intact `.jetproof-replay` path. |
| `E3623` | active | compile | source_edit | true | false |  |  | Recapture with `--capture`, then replay the same target identity. |
| `E3624` | active | compile | source_edit | true | false |  |  | Select one runnable file or package target. |
| `E3625` | active | compile | source_edit | true | false |  |  | Route the operation through a supported deterministic input, or remove it from the captured target. |
| `E3626` | active | compile | source_edit | true | false |  |  | add the exact existing authority or change the target before capture. |
| `E3627` | active | compile | source_edit | true | false |  |  | use `--capture` for Time-only, or run `--capture-sensitive` interactively. |
| `E3628` | active | compile | source_edit | true | false |  |  | Reduce the captured target or recapture a bounded artifact. |
| `E3629` | active | compile | source_edit | true | false |  |  | Fix the path and retry; differing existing bytes are never overwritten. |
| `E4201` | active | sema | source_edit | true | false |  |  | Verify the URL points at an HTTPS server, not plain HTTP. For local tests, start the TLS fixture server. |
| `E4202` | active | sema | source_edit | true | false |  |  | use a certificate whose subject matches the host and chains to a trusted root. For tests, trust the local fixture CA explicitly. |
| `E4203` | active | sema | source_edit | true | false |  |  | Install the system certificate bundle (for example `ca-certificates`) or run in an image that includes it. |
| `E-APP-TARGET-FEATURE` | active | sema | source_edit | true | true | jet check |  | Choose a build target that supports `.{feature}` (for example `#Target(JS)` for browser features or a native target for server features), or remove that feature. |
| `E-CALL-VALUE` | active | parse | source_edit | true | false |  |  | Write `callee.call(…)` |
| `E-ERR-DEFAULT` | retired | parse | source_edit | true | false |  |  | remove `!`; failure is implicit, or name an explicit domain such as `!IOError`. |
| `E-ERR-PROPAGATE` | retired | parse | source_edit | true | false |  |  | remove the trailing `?`, or write `?(text)` to add one context frame. |
| `E-ERR-SIGIL` | retired | parse | source_edit | true | false |  |  | Write `?Entry !StoreError`, `Int !(DbError \| TimeoutError)`, or `!IOError`; remove a bare `!` contract because failure is implicit. |
| `E-ERR-SUFFIX` | retired | parse | source_edit | true | false |  |  | Write `!Error`, or write `?Success !Error` when the success can be absent. |
| `E-MODEL-LOAD` | active | jet | source_edit | true | false |  |  | Make the declared artifact available in the package, or choose a supported provider and declared limit. |
| `E-MODEL-PROVENANCE` | active | jet | source_edit | true | false |  |  | Add the artifact hash, tokenizer hash, package version, and license to the model package manifest. |
| `E-MODEL-SIGNATURE` | active | jet | source_edit | true | false |  |  | Make the tensor name, dtype, and shape match the package declaration. |
| `E-OSTARGET-BUILD-CONTEXT` | active | sema | source_edit | true | false |  |  | Write `@if @build.os == { .Linux -> … .MacOS -> … .Windows -> … }`, or use a plain runtime `if` for a value that isn't known at compile time. |
| `E-OSTARGET-DISPATCH-ARM` | active | sema | source_edit | true | false |  |  | Write `.Linux -> …`, `.MacOS -> …`, or `.Windows -> …` (add an `else -> …` for a shared fallback). |
| `E-OSTARGET-DISPATCH-EXHAUSTIVE` | active | sema | source_edit | true | false |  |  | add an arm for each missing OS ({list}), or an `else -> …` catch-all. |
| `E-OSTARGET-MIXED-AXIS` | active | sema | source_edit | true | true | jet check |  | Pick one axis: remove the `#Target(OS.{os})` marker or the web-axis marker. |
| `E-OSTARGET-UNMATCHED-CALL` | active | sema | source_edit | true | true | jet check |  | Only use `{gated_type}` from inside an `impl` already gated to `#Target(OS.{os})`, or move `{caller}`'s body into one. |
| `E-SUBJECT-CALL` | active | sema | source_edit | true | false |  |  | Write a value of type {expected}, or use `(subject: {expected}) -> subject{chain}` |
| `E-SUBJECT-CALL-ARITY` | active | sema | source_edit | true | false |  |  | Write the full lambda, for example `(first, second) -> first{chain}` |
| `E-WEB-ABI-TYPE` | active | sema | source_edit | true | true | jet build --target=web, jet build |  | use a scalar, `String`, a `List`/`Map` of ABI-safe values, or a `#Codable` struct/enum whose fields are ABI-safe (D-JSBIND1) |
| `E-WEB-CROSS-PARTITION` | active | sema | source_edit | true | true | jet build --target=web, jet build |  | Move the call behind a generated bridge, colocate both functions in the same bucket, or adjust their `#Target(Wasm\|JS)` markers |
| `E-WEB-RUN` | active | driver | exact_command | true | true | jet run |  | Use `jet dev <file.jet>` for the live browser loop, or `jet build --target=web <file.jet>` for web artifacts |
| `E-WEB-TARGET-BROWSER` | active | sema | source_edit | true | true | jet build --target=web, jet build |  | remove the `#Target(Wasm)` pin, move browser work into a `#Target(JS)` function, or drop the browser API calls |
| `E-WEB-TIR-UNSUPPORTED` | active | driver | source_edit | true | true | jet build --target=web, jet build |  | Move the unsupported work behind a Wasm export that uses covered Jet constructs, or simplify this function for the web target. |
| `JT0101` | active | jet | source_edit | true | false |  |  | Implement `{construct}` in Jet at `{source}`, or review the generated target `{target}` before rerunning the import. |
| `JT0198` | active | jet | source_edit | true | false |  |  | Check the source path and write permissions, then rerun |
| `JT0199` | active | jet | source_edit | true | false |  |  | Reconcile the files, then rerun with `--update` |
| `L0101` | active | sema | source_edit | true | false |  |  | Remove the binding when its initializer is side-effect free, or rename `{name}` to `_{name}` when the value must be computed. |
| `L0102` | active | sema | source_edit | true | false |  |  | Use `{name}`, rename it to `_{name}` when it is intentionally unused, or remove it from the function contract. |
| `L0103` | active | sema | source_edit | true | false |  |  | Remove the import, or rename `{name}` to `_{name}` when the import is intentionally kept. |
| `L0104` | active | sema | source_edit | true | false |  |  | Remove the function, or rename `{name}` to `_{name}` when it is intentional. |
| `L0105` | active | sema | source_edit | true | false |  |  | Remove the export, or rename `{name}` to `_{name}` when it is intentional. |
| `L0152` | active | sema | source_edit | true | false |  |  | Bring both paths to the same state before they meet, or do the work that needs the state inside the path that reaches it. |
| `L0153` | active | sema | source_edit | true | false |  |  | Add an entry or transition path to `{state}`, or remove it from the declaration. |
| `L0202` | active | sema | source_edit | true | false |  |  | Move the `Shared` clone before the loop, or replace the loop with a blocking `Condition` wait. |
| `L0203` | active | jet | exact_command | true | false | jet fetch --lock, jet fetch |  | Write the exact version Jet resolved (`use {name}#<major.minor.patch>;`), or run `jet fetch --lock` to pin it in `<script>.lock`. |
| `L0204` | active | jet | source_edit | true | false | jet os bridge flake, jet os |  | Review the generated shim and add `{field}`'s effect by hand if you need it — the shim is a starting point, not a full translation. |
| `L0205` | active | jetpack | source_edit | true | true | jetpack env |  | Provide a trusted substitute or approved remote builder, or enable the native sandbox. |
| `L0206` | active | sema | source_edit | true | false |  |  | Move the guarded work into a smaller block. When nesting guards is necessary, acquire them in one stable order. |
| `L0207` | active | sema | source_edit | true | false |  |  | Wait on a `Condition` through an edit guard, or receive from a channel; an empty plain-field predicate can use that wait directly |
| `L0301` | active | sema | source_edit | true | false |  |  | Remove the unreachable dispatch arm or make its pattern reachable. |
| `L0302` | active | sema | source_edit | true | false |  |  | Give the closed-enum arm table a named subject. |
| `L0501` | active | sema | source_edit | true | false |  |  | Move the slice copy out of the loop or use a view. |
| `L0502` | active | sema | source_edit | true | false |  |  | Compare floats with an epsilon or use an exact representation. |
| `L0503` | active | sema | source_edit | true | false |  |  | Write `{place} {op} …` |
| `L0504` | active | sema | source_edit | true | false |  |  | Use `Decimal` for money-like values. |
| `L0505` | active | sema | source_edit | true | false |  |  | Use an arena or another deliberate allocator for loop allocations. |
| `L0506` | active | sema | source_edit | true | false |  |  | Declare an allocator in `#Context` before allocating. |
| `L0507` | active | parse | source_edit | true | false |  |  | Write `if { condition -> body else -> body }` |
| `L0508` | active | sema | source_edit | true | false |  |  | Bind the loop with `::` to collect its values, or write `loop v in &values -> v *= 2` so it takes write access (&) for items. |
| `L0509` | reserved | jet | exact_path | true | false |  |  | Write `policy: { lints: { deny: [auto_derive] } }` in `package.jet` |
| `L0510` | active | sema | source_edit | true | false |  |  | Keep the declaration, or rename it to use the prelude alias |
| `L0511` | active | sema | source_edit | true | false |  |  | Rename the binding, or use it outside a fallback |
| `L0512` | active | sema | source_edit | true | false |  |  | Rewrite the inner shorthand with a named binding such as `(item) -> item.member` |
| `L0513` | active | sema | source_edit | true | false |  |  | Replace the table and fallback with an explicit `else` arm |
| `L0514` | active | sema | exact_path | true | false |  |  | Rewrite as `if {subject} == { … }` and group aliases with `\|`; see `examples/suites/dispatch.jet` |
| `L0515` | active | sema | source_edit | true | false |  |  | Replace process.argv().skip(1) with process.args() |
| `L0516` | active | sema | source_edit | true | false |  |  | Replace body().text(default_limit) with text() |
| `L0517` | active | sema | source_edit | true | false |  |  | Replace the string comparison with Path.is_within() |
| `L0518` | active | sema | source_edit | true | false |  |  | Delete `.replace(",", "")` from the `Fixed(n)` projection |
| `L0519` | active | sema | source_edit | true | false |  |  | Use `unit * scalar`, `scalar * unit`, or `unit / scalar` directly |
| `L0520` | active | sema | source_edit | true | false |  |  | Replace `{value}` with `{{{value}:Debug}}`. |
| `L0521` | active | sema | source_edit | true | false |  |  | Replace the complete ladder with value.to_ascii_lower() or value.to_ascii_upper() |
| `L0522` | active | sema | source_edit | true | false |  |  | Replace fs.walk with fs.walk_files |
| `L0523` | active | sema | source_edit | true | false |  |  | Write `[{type}]{{{{…}}, {{…}}}}` and remove the repeated element heads |
| `L0524` | active | sema | source_edit | true | false |  |  | Wrap the literal in `HTML{…}` and type any trusted fragment holes as `HTML` |
| `L0525` | active | sema | source_edit | true | false |  |  | Remove the unreachable statement, or add a reachable `break` |
| `L0526` | active | sema | source_edit | true | false |  |  | For `true`, write `loop { … }`; for `false`, remove the loop or change the condition |
| `L0527` | active | sema | source_edit | true | false |  |  | Use the value, bind it with `::`, or remove the expression |
| `L0528` | active | sema | source_edit | true | false |  |  | Declare it with `::` instead of `:=` |
| `L0529` | active | sema | source_edit | true | false |  |  | Use `loop line in io.stdin().lines()` for repeated reads, or stop from the body with `if test { break }` |
| `L0601` | active | sema | source_edit | true | false |  |  | Rename the soft-public item or expose it through its supported public API. |
| `L0619` | active | jet | source_edit | true | false |  |  | Remove the unused rule, or update it to match the intended import edge |
| `L1101` | active | sema | source_edit | true | false |  |  | Join it with `.join()`, use its result, or write `.detach()` to let it go free. |
| `L1141` | active | sema | source_edit | true | false |  |  | Bind the autodiff derivative to a name before calling it. |
| `L1401` | active | jet | source_edit | true | false |  |  | Fix compiler-extension rule `{rule}` in the configured component, or unset `JET_COMPILER_EXTENSION`. |
| `L2001` | active | jet | exact_command | true | false |  |  | Use the named replacement, or run `jet fix` to migrate automatically. |
| `L2101` | active | jet | exact_command | true | true | jet self doctor, jet self |  | Apply the fix printed on the advisory line; for a missing cache or store directory, run `jet self doctor --fix`. |
| `L2201` | active | jet | exact_path | true | false |  |  | Add a `///` summary before `{name}`, or remove `pub` when it is not part of the public API. |
| `L2401` | active | compile | source_edit | true | false |  |  | Callers can use `{param}: true` to document intent; or give the parameter a default value so it can be omitted. No action required — this is advisory. |
| `L2421` | active | compile | source_edit | true | false |  |  | Add `Never` to the return type, for example `fn {function}() Never`. |
| `L2501` | reserved | compile | source_edit | true | false |  |  | Use `files.open(path)?` and `loop line in handle.lines() { … }` to stream line-by-line. Not emitted yet. |
| `L2510` | active | sema | source_edit | true | false |  |  | Insert `{fix}` before the highlighted expression to make the copy explicit. |
| `L2608` | active | jet | source_edit | true | false |  |  | Choose a distinct package name instead of `{reference}` before publishing; the closer block band and reserved suffix rules reject stronger matches. |
| `L2801` | active | compile | source_edit | true | false |  |  | Wrap the handler body in `tasks.spawn(() -> …)` so each connection runs in its own task. |
| `L2901` | active | compile | source_edit | true | false |  |  | Add at least one assertion, or remove the test if it only exercises compilation. |
| `L2902` | active | compile | source_edit | true | false |  |  | Split the function, reduce nesting, or replace mixed control flow with a focused helper. |
| `L3102` | active | sema | source_edit | true | false |  |  | Add the reason: `#Impure("reading build config") { … }`. |
| `R0801` | active | runtime | source_edit | true | false |  |  | Bound the raw access before it reaches storage — obligation `{obligation}` was not met on this run. |
| `R0802` | active | runtime | source_edit | true | false |  |  | Do not use the pointer after release or frame exit — obligation `{obligation}` was not met on this run. |
| `R0803` | active | runtime | source_edit | true | false |  |  | Align the raw access before it reaches storage — obligation `{obligation}` was not met on this run. |
| `W0410` | reserved | sema | non_actionable | false | false |  |  | Follow the guidance for W0410 |

## Evidence boundary

This census is the actionability check. UI snapshot coverage and diagnostic text repairs remain the diagnostics snapshot/repair work; the check reports those rows rather than inventing text or evidence.

