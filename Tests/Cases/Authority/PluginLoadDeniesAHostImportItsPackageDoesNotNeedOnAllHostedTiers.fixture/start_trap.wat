;; An import-free component whose core start function traps, so loading it
;; runs that start function and the load fails.
(component
  (core module $m
    (func $boom unreachable)
    (start $boom)
    (func $seven (export "seven") (result i64)
      (i64.const 7)))
  (core instance $i (instantiate $m))
  (type $t (func (result s64)))
  (func $seven (type $t)
    (canon lift (core func $i "seven")))
  (export "seven" (func $seven)))
