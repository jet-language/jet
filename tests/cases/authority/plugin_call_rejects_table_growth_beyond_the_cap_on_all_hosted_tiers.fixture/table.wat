(component
  (core module $m
    (table 1 20000 funcref)
    (func $grow (export "grow") (result i64)
      (local $old i32)
      (local.set $old (table.grow (ref.null func) (i32.const 10000)))
      (if
        (i32.eq (local.get $old) (i32.const -1))
        (then (unreachable)))
      (i64.const 7)))
  (core instance $i (instantiate $m))
  (type $t (func (result s64)))
  (func $grow (type $t)
    (canon lift (core func $i "grow")))
  (export "grow" (func $grow)))
