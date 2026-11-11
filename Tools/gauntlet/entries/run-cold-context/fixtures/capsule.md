# Fixture capsule — the smallest capsule the evaluator accepts

## 1. Program shape

A program is a file with `run: fn() { … }`.

## 2. Dot rule and common calls

`value.copy()` makes an owned copy.

## 3. Memory and ownership verbs

`&T` borrows to write, `&&T` hands over.

## 4. Effects and outcomes

An effect row is written `-[IO]>`. A result is `Ok(value)` or `Err(error)`; an
absent value is `None`, and `??` supplies a fallback.

## 5. Forty common library verbs

1. `term.print`  2. `term.eprint`  3. `term.confirm`  4. `term.readline`
5. `term.read_all_input`  6. `files.read`  7. `files.write`
8. `files.read_bytes`  9. `files.write_bytes`  10. `files.exists`
11. `files.list_dir`  12. `files.walk`  13. `process.argv`  14. `process.run`
15. `process.cmd`  16. `sys.get`  17. `sys.set`  18. `json.parse`
19. `json.decode`  20. `json.to_string`  21. `csv.parse`  22. `csv.to_string`
23. `text.trim`  24. `text.lower`  25. `text.upper`  26. `text.splitn`
27. `regex.compile`  28. `regex.replace`  29. `math.abs`  30. `math.floor`
31. `math.ceil`  32. `math.round`  33. `math.sqrt`  34. `time.now`
35. `time.sleep`  36. `net.tcp_listen`  37. `net.tcp_connect`  38. `http.get`
39. `http.post`  40. `http.serve`

## 6. Ten runnable canonical programs

### 1. Hello

```jet
run: fn() { print("Hello, Jet") }
```

### 2. Arithmetic

```jet
run: fn() { print("{1 + 2}") }
```

### 3. Function call

```jet
double: fn(n: Int) -> Int { n * 2 }
run: fn() { print("{double(21)}") }
```

### 4. Branching

```jet
run: fn() { print(if 2 > 1 -> "yes" else -> "no") }
```

### 5. Loop and mutation

```jet
run: fn() {
    total := 0
    loop n in 1..=3 -> total += n
    print("{total}")
}
```

### 6. List ordering

```jet
run: fn() { print("sorted") }
```

### 7. Optional fallback

```jet
run: fn() { print(core.sys.get("NAME") ?? "nobody") }
```

### 8. Command-line argument

```jet
run: fn() { print("argument") }
```

### 9. File input

```jet
run: fn() { print("file") }
```

### 10. HTTP health endpoint

```jet
run: fn() { print("serve") }
```

## 7. Cold-agent rules

Return one complete Jet source file — no Markdown.
