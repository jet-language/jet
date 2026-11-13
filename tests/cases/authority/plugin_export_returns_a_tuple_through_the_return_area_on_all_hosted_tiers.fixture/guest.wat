;; Two lifted exports whose results flatten to more than one core value, so each
;; core function returns the address of its return area.
(component
  (core module $guest
    (memory (export "memory") 1)
    (data (i32.const 128) "hello")
    (func (export "pair") (result i32)
      (i64.store (i32.const 16) (i64.const 7))
      (i64.store (i32.const 24) (i64.const -9))
      (i32.const 16))
    (func (export "scored") (result i32)
      (i32.store (i32.const 64) (i32.const 128))
      (i32.store (i32.const 68) (i32.const 5))
      (f64.store (i32.const 72) (f64.const 2.5))
      (i32.const 64)))
  (core instance $instance (instantiate $guest))
  (alias core export $instance "memory" (core memory $memory))
  (type $pair-type (func (result (tuple s64 s64))))
  (type $scored-type (func (result (tuple string f64))))
  (func $pair (type $pair-type) (canon lift (core func $instance "pair") (memory $memory)))
  (func $scored (type $scored-type) (canon lift (core func $instance "scored") (memory $memory)))
  (export "pair" (func $pair))
  (export "scored" (func $scored)))
