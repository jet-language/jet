# Jai-style declaration sample

This folder shows four large Core files rewritten so that every declaration uses
the Jai-style head `NAME :: kind`. It is a reading sample for the owner, not
compiled code; the live sources are unchanged.

|Sample|Source on dev|Lines|Declarations rewritten|
|---|---|---|---|
|`Math.jet`|`Core/Math/Math.jet`|2272|406 fn, 25 struct, 1 trait|
|`Data.jet`|`Core/Data/Data.jet`|1998|112 fn, 43 struct, 19 enum, 5 module|
|`Server.jet`|`Core/Http/Server.jet`|1442|95 fn, 17 struct, 3 enum, 7 marker|
|`Time.jet`|`Core/Time/Time.jet`|1923|314 fn, 16 struct, 1 enum|

## The rule

Every named declaration puts its name first, then `::`, then what it is.
Generic parameters move with the kind, so they sit next to the thing they
parameterize. Everything else is untouched: bodies, attributes, markers on
declarations, effect arrows, `impl` blocks and comments.

|Today|Jai style|
|---|---|
|`fn abs(n: Int) -> Int Never! { … }`|`abs :: fn(n: Int) -> Int Never! { … }`|
|`fn json<T: Encode>(status: Int, body: T) -> …`|`json :: fn<T: Encode>(status: Int, body: T) -> …`|
|`struct DataSnapshot<T> { … }`|`DataSnapshot :: struct<T> { … }`|
|`enum DataFormat { … }`|`DataFormat :: enum { … }`|
|`trait Shape { … }`|`Shape :: trait { … }`|
|`module csv { … }`|`csv :: module { … }`|
|`marker Get(path: String, $sites: [.Method])`|`Get :: marker(path: String, $sites: [.Method])`|
|`fn elapsed_millis(self) -[Time]> Int` (method in an impl)|`elapsed_millis :: fn(self) -[Time]> Int`|

Constants and comptime values already use this form (`LIMIT :: 64`,
`NAME :: comptime { … }`), so with this change every named thing in a file
reads the same way: scan the left edge to find any name.

## What stays the same

- `impl Type.Trait { … }` keeps its form: it names no new item.
- Function types without a name (`fn(String, Stage) -> R`) are unchanged.
- Attributes such as `#Test("…")`, `#Comparable` and `#FX(...)` stay in front
  of the declaration, before the name.

## How the sample was made

A mechanical rewrite of declaration heads only (script kept outside the repo).
No declaration was missed: no line in the samples still starts with `fn`,
`struct`, `enum`, `union`, `trait`, `module`, `marker` or `effect` followed by a
name.
