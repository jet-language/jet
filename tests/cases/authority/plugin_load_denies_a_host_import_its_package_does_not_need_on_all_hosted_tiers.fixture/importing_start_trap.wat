;; The same trapping core start function behind an FS.Read host import. The
;; package does not need FS.Read, so the load is refused before the start
;; function runs and the trap never happens.
(component
  (type $read-type (func (param "path" string) (result s64)))
  (import "read" (func $read (type $read-type)))
  (core module $memory-module
    (memory (export "memory") 1))
  (core instance $memory-instance (instantiate $memory-module))
  (alias core export $memory-instance "memory" (core memory $memory))
  (core func $read-lowered (canon lower (func $read) (memory $memory)))
  (core module $m
    (import "host" "read" (func $read (param i32 i32) (result i64)))
    (import "host" "memory" (memory 1))
    (func $boom unreachable)
    (start $boom)
    (func $seven (export "seven") (result i64)
      (i64.const 7)))
  (core instance $i (instantiate $m
    (with "host" (instance
      (export "read" (func $read-lowered))
      (export "memory" (memory $memory))))))
  (type $t (func (result s64)))
  (func $seven (type $t)
    (canon lift (core func $i "seven")))
  (export "seven" (func $seven)))
