(component
  (core module $m
    (memory (export "memory") 1 512)
    (func $grow (export "grow") (result i64)
      (local $old i32)
      (local.set $old (memory.grow (i32.const 256)))
      (if
        (i32.eq (local.get $old) (i32.const -1))
        (then (unreachable)))
      (i64.const 7)))
  (core instance $i (instantiate $m))
  (type $t (func (result s64)))
  (func $grow (type $t)
    (canon lift (core func $i "grow")))
  (export "grow" (func $grow)))
