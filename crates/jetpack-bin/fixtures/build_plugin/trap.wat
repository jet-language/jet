(component
  (core module $m
    (memory (export "memory") 1)
    (func $realloc (export "cabi_realloc")
      (param i32 i32 i32 i32)
      (result i32)
      (i32.const 16))
    (func $build (export "build") (param i32 i32) (result i32)
      (unreachable)))
  (core instance $i (instantiate $m))
  (type $t (func (param "request" (list u8)) (result (list u8))))
  (func $build (type $t)
    (canon lift
      (core func $i "build")
      (memory $i "memory")
      (realloc (func $i "cabi_realloc"))))
  (export "build" (func $build)))
