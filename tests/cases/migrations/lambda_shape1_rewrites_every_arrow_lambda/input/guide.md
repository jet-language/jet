# Keeping the even numbers

`filter` keeps the elements its lambda accepts; prose such as `x -> y` stays as written.

```jet
evens: [1, 2, 3, 4].filter(n -> n % 2 == 0)
```

Write `xs.map((n) -> n * 2)` or `xs.map(n -> n + 1)` to transform; `(Int) -> Int` is no lambda.

```text
`xs.map((n) -> n * 2)` in another fence stays as written.
```
