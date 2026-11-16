# Jet Documentation

Generated from the checked semantic graph.

## Effect Roles

required effects: IO; granted effects: Exec.Args, IO, Mem.Alloc; denied effects: none; authority: application default

## Modules

- `c06_doc_heading` — `c06_doc_heading.jet` ([source](../c06_doc_heading.jet#L1))

## API

### `add_one`

Adds one.

```jet
fn add_one(x: Int) -> Int
failure: Int (implicit default Err!)
```

[Source](../c06_doc_heading.jet#L11)

### `run`

Undocumented public item.

```jet
fn run()
failure: Err! (implicit default Err!)
```

[Source](../c06_doc_heading.jet#L13)

## Doctests

### `c06_doc_heading.jet` line 8

```jet
add_one(1) // => 2
```

[Source](../c06_doc_heading.jet#L8)

