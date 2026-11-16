(component
  (core module $m
    (func $scale (export "scale") (param i64) (result i64)
      (i64.const 7)))
  (core instance $i (instantiate $m))
  (type $t (func (param "a0" s64) (result s64)))
  (func $scale (type $t)
    (canon lift (core func $i "scale")))
  (export "scale" (func $scale)))
