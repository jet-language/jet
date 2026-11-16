(component
  (core module $m
    (func $zero (export "zero") (result i64)
      (i64.const 7)))
  (core instance $i (instantiate $m))
  (type $t (func (result s64)))
  (func $zero (type $t)
    (canon lift (core func $i "zero")))
  (export "zero" (func $zero)))
