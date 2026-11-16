# Bounds

A function returns two named values as a record:

```jet
bounds: fn(xs: [Int]) -> (min: Int, max: Int) {
    (min: xs[0], max: xs[xs.len() - 1])
}
```

The prose spelling `(min: Int, max: Int)` stays as written, and so does a
shell fence:

```sh
echo "(min: 1, max: 2)"
```
