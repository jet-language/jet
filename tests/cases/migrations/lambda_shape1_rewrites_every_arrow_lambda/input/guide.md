# Keeping the even numbers

`filter` keeps the elements its lambda accepts; prose such as `x -> y` stays as written.

```jet
evens: [1, 2, 3, 4].filter(n -> n % 2 == 0)
```
