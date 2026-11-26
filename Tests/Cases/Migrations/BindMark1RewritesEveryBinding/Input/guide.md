# Changeable names

A changeable name is written `total := 0`, and `w: ^xs[0]` opens a window.
Go's own binding stays as written in a `go` fence.

```jet
run: fn() {
    total := 0
    loop v in ^items -> total += v
}
```

```go
n := 1
```
