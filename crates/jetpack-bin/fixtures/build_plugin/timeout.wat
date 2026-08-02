(component
  (core module $m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 4096))
    (func $realloc (export "cabi_realloc")
      (param $ptr i32) (param $old i32) (param $align i32) (param $new i32)
      (result i32)
      (local $p i32)
      (local.set $p (global.get $heap))
      (local.set $p
        (i32.and
          (i32.add (local.get $p) (i32.sub (local.get $align) (i32.const 1)))
          (i32.xor (i32.sub (local.get $align) (i32.const 1)) (i32.const -1))))
      (global.set $heap (i32.add (local.get $p) (local.get $new)))
      (local.get $p))
    (func $build (export "build") (param i32 i32) (result i32)
      (loop
        (br 0))))
  (core instance $i (instantiate $m))
  (type $t (func (param "request" (list u8)) (result (list u8))))
  (func $build (type $t)
    (canon lift
      (core func $i "build")
      (memory $i "memory")
      (realloc (func $i "cabi_realloc"))))
  (export "build" (func $build)))
