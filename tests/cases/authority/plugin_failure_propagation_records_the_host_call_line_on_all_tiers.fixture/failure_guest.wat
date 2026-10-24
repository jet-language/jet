;; D-PLUGIN-FAILURE1=A: four real Component exports and one scoped host import.
;; wasm-tools parse failure_guest.wat -o failure_guest.wasm
(component
  (type $read-type (func (param "path" string) (result s64)))
  (import "read" (func $read (type $read-type)))
  (core module $memory-module
    (memory (export "memory") 1)
    (data (i32.const 0) "/etc/passwd"))
  (core instance $memory-instance (instantiate $memory-module))
  (alias core export $memory-instance "memory" (core memory $memory))
  (core func $read-lowered (canon lower (func $read) (memory $memory)))
  (core module $failure_guest
    (import "host" "read" (func $read (param i32 i32) (result i64)))
    (import "host" "memory" (memory 1))
    (func $trap (export "trap") (result i64) unreachable)
    (func $read-folder (export "read") (result i64)
      (call $read (i32.const 0) (i32.const 11)))
    (func $spin (export "spin") (result i64)
      (loop (br 0)) (i64.const 0))
    (func $normal (export "normal") (result i64) (i64.const 42)))
  (core instance $guest (instantiate $failure_guest
    (with "host" (instance
      (export "read" (func $read-lowered))
      (export "memory" (memory $memory))))))
  (type $result (func (result s64)))
  (func $trap (type $result) (canon lift (core func $guest "trap")))
  (func $read-folder (type $result) (canon lift (core func $guest "read")))
  (func $spin (type $result) (canon lift (core func $guest "spin")))
  (func $normal (type $result) (canon lift (core func $guest "normal")))
  (export "trap" (func $trap))
  (export "read" (func $read-folder))
  (export "spin" (func $spin))
  (export "normal" (func $normal)))
