(component
  (core module $m
    (func $spin (export "spin") (result i64)
      (loop
        (br 0))
      (i64.const 0)))
  (core instance $i (instantiate $m))
  (type $t (func (result s64)))
  (func $spin (type $t)
    (canon lift (core func $i "spin")))
  (export "spin" (func $spin)))
